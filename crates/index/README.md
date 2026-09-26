# SQLite catalog index

`Index::open(path)` opens a per-user, rebuildable catalog with WAL,
`synchronous=NORMAL`, a 256 MiB mmap ceiling, foreign keys, versioned migrations,
FTS5 and R*Tree. Bundled SQLite supports R*Tree in this build; no fallback is
needed. The `gps` virtual table is kept in sync by image triggers.

The public API returns `engine_api::error::EngineResult`. Image IDs and selection
values use engine-api types without contract modifications.

## Persistent people (schema v9)

Index-local exports: `Person { id: String, name: Option<String>, medoid:
Option<Vec<f32>> }`, `FaceKey { image_id: ImageId, ordinal: u32 }`, and
`FaceAssignment { face: FaceKey, person_id: String, person_name: Option<String>,
confirmed: bool }`. No engine-api changes.

All methods return `EngineResult`:

- `create_person(id: &str, name: Option<&str>, medoid: Option<&[f32]>)`: caller
  supplies a stable nonblank unique ID; medoid must have 128 finite components.
- `people() -> Vec<Person>`: stable ID order, including empty clusters.
- `name_person(id: &str, name: Option<&str>)`: rename/clear. Names need not be
  unique; reads join the cluster, not copied face/image names.
- `assign_face(face: FaceKey, person_id: &str)`: requires both rows. Reassignment
  resets confirmation and clears both affected medoids atomically; idempotent
  same-person assignment preserves confirmation and the medoid.
- `confirm_face(face: FaceKey, confirmed: bool)`: requires an assignment.
- `face_assignments(image_id: ImageId) -> Vec<FaceAssignment>`: assigned faces,
  ordered by ordinal, with current names.
- `merge_people(target: &str, source: &str)`: atomic, distinct existing IDs;
  target name wins, source is removed, confirmations survive, target medoid clears.
- `split_person(source: &str, new_id: &str, faces: &[FaceKey])`: atomic creation
  of an unnamed cluster; nonempty unique selection must belong to source.
  Selected confirmations reset and both medoids clear. Source survives even empty.
- `images_with_person(person_id: &str, confirmed_only: bool, limit: usize,
  offset: usize) -> Vec<ImageId>`: indexed, deduplicated, stable image-ID order;
  zero limit means 100, unknown person returns empty. `Predicate::Person(name)`
  supports name-based search/facets and composition with other filters.

Composite face foreign keys cascade on face/image deletion. `replace_faces`
invalidates previous assignments even if ordinals are reused; failed replacement
rolls back assignments too. Names/confirmation persist across database reopen.
Merge/split clear stale representative descriptors rather than inventing medoids.
Migration v9 invalidates medoids on assignment deletion, including cascading
face re-detection and image removal. The next people job rebuilds them from members.
No face-table triggers reference people, so historical face-drop migration replay
remains supported; migration v8 uses idempotent table/index creation.

Run `cargo test -p index --test people`; opt-in synthetic 100k-image indexed
search budget (<50 ms for all 100,000 matching images) is exercised with
`cargo test -p index --release --test people -- --ignored --nocapture`.

## Scanning

`Scanner::new(&sidecars, &metadata).scan(&mut index, root)` (or `Index::scan`)
returns the number of inserted/refreshed images. Paths are canonicalized, symlink
entries are not followed, image size and nanosecond mtime detect source changes.
An unchanged second scan produces zero SQLite row changes. Image updates are
transactional. Provider and traversal errors propagate rather than silently
being recorded as empty metadata.

Embedded JPEG/TIFF/DNG metadata is read with kamadak-exif independently of the
optional MetadataProvider. Unsupported/corrupt EXIF does not prevent file
indexing. Providers can supply RAW metadata and override embedded values.
Capture dates use sortable ISO-like local timestamps (no invented timezone).

SidecarReader receives the image path. The scanner fingerprints both
`image.ext.xmp`, `image.xmp`, and `.edits/image.json` so sidecar-only changes
refresh the index. SidecarData carries caption, keywords, flattened values,
optional selection and optional recipe hash. An absent selection leaves existing
catalog selection intact. The no-op hooks permit indexing before sidecar/RAW
integration. IDs currently derive deterministically from canonical paths.
The scanner upserts files; it does not prune disappeared files or track renames.

## Queries

All filters are ANDed. `text` is an FTS5 MATCH expression (including its query
syntax), not interpolated SQL. Folder scope is a canonical absolute directory
and includes descendants, with literal wildcard characters escaped. Dates are
inclusive bounds using sortable strings. `limit=0` means 100, `offset=0` starts
at the first match; ordering is capture time then ImageId.

Keyword filters include descendants through the closure table. Keywords have
unique names; parent assignment is immutable, and missing parents/cycles are
rejected. Tagging also refreshes searchable keyword text. Facets ignore paging
and count camera/lens/decision and directly attached keyword values across all
matching images. Missing camera/lens values use the empty string bucket.

## Typed boolean predicates

`Query::predicate: Option<Predicate>` defaults to `None` in Rust and when absent
from old JSON. It is ANDed with all existing filters, including folder scope,
for search and every facet. Exhaustive Rust `Query` literals must add
`predicate: None`. `Predicate` and `Comparison` are exported, cloneable,
comparable, and serde-serializable.

Compose `All`, `Any`, and `Not` around `Text`, `Keyword`, `Camera`, `Lens`,
`Decision`, `Mark`, `DateFrom`, `DateBefore`, `Grade`, `Focus`, `Person`, and
`Ids` leaves. Numeric leaves take `(Comparison, f64)` with `Eq`, `Ne`, `Lt`,
`Le`, `Gt`, or `Ge`. `Text` retains FTS5 query syntax. Unlike legacy `date_to`,
`DateBefore` is exclusive; `DateFrom` is inclusive.

Leaves with absent/NULL values are false; `Not` takes their boolean complement,
including images with no selection row. Empty `All` is true; empty `Any` and
`Ids` are false. Missing selections are counted in the `undecided` facet bucket.
`Focus` reads the latest `score` value for signal `focus`. `Person` matches
current cluster names via indexed joins, plus legacy named keywords and their
descendants for compatibility. A face ordinal is not a person identity.

All user values are bound SQL parameters. ID scopes use catalog hex strings in
a single JSON parameter (avoiding SQLite's per-statement variable limit).
Facets ignore pagination and count the complete intersection. Semantic search's
API is unchanged; predicates filter candidate eligibility before ranking/paging.

`tests/predicates.rs` seeds the migrated SQLite schema directly, with no model
or image-fixture dependency. Run it with `cargo test -p index --test predicates`
when extending the compiler; include missing optional rows, nested negation,
empty scopes, bound hostile strings, and search/facet set agreement.

## Semantic and hybrid queries

`Query` is owned by `index`, not re-exported from engine-api. Its new
`semantic: Option<String>` defaults to `None`; old JSON and existing Rust
`..Default::default()` literals keep working. Exhaustive Rust struct literals
must add `semantic: None`.

Implement the dependency-inverted hook in the embedding crate or an application
newtype (index does not depend on ml-embed):

```rust,ignore
pub trait SemanticSearch {
    fn search_text(&mut self, query: &str, k: usize)
        -> EngineResult<Vec<(ImageId, f32)>>;
}
```

Call `index.search_with_semantic(&query, &mut provider)` for IDs, or
`index.facets_with_semantic(&query, &mut provider)` for unpaged counts. These
methods use the legacy behavior without calling the provider when semantic is
`None`. Plain `search`/`facets` reject semantic queries instead of silently
ignoring the request. Provider errors propagate.

Vector scores are higher-is-better similarities. Non-finite scores and unknown
catalog IDs are discarded; duplicates retain their best score. Vector-only
queries rank by similarity, breaking ties by ImageId. With both `text` and
`semantic`, FTS5 BM25 and vector ranks are fused over their **union** using equal
weight reciprocal rank fusion, `1 / (60 + one_based_rank)` per list. All other
catalog filters apply to both lists before ranks and offset/limit; fused ties
also use ImageId. Text-only queries keep their historical capture-time ordering.

For correctness with selective facets and stale vector entries, the hook is
called with `k = usize::MAX` to request all available candidates. This is an
upper bound, **not an allocation size**: adapters must clamp it to their stored
vector count before allocating or calling a backend with a bounded integer k.
This exact baseline materializes candidates and is not an ANN scalability claim.
Facet counts cover the complete filtered union, not just the displayed page.

## Faces (schema v5)

`index::FaceRecord` has `id: u32` (image-local ordinal),
`bbox: [f32; 4]` (image pixels, xywh), `landmarks5: [[f32; 2]; 5]`,
`confidence: f32`, `embedding: Option<Vec<f32>>`, `sharpness: f64`, and
`eyes_open: Option<f64>`.

`Index::replace_faces(&self, ImageId, &[FaceRecord]) -> EngineResult<()>`
atomically replaces one image's faces, all reserved `face/*` scores, and
`face_sharpness` / `eyes_open` image aggregates. Empty input clears these records;
unknown image IDs fail even for empty input. Other image scores are unchanged.
`Index::faces(&self, ImageId) -> EngineResult<Vec<FaceRecord>>` returns faces in
ascending ordinal order (an empty list when no records exist).

Geometry must be finite and nonnegative, with positive width/height and finite
right/bottom edges. Landmarks are image-pixel coordinates; they need not lie
inside the detected box. Image dimensions are not available here, so callers
must clip detections to image bounds. Confidence, normalized sharpness, and
optional eyes-open proxy must be in [0,1]. Embeddings must contain exactly 128
finite components; normalization is not imposed. Duplicate ordinals fail.

Per-face scores use `face/{ordinal}/sharpness` and optional
`face/{ordinal}/eyes_open`. Image aggregates are the minimum sharpness across
faces and the minimum eyes-open proxy across faces with that value. No faces
means no aggregates; no eyes-open values means no `eyes_open` aggregate.
All generated face rows and scores carry model provenance `yunet-sface-v1`.
The existing `score` schema and generic image score APIs are unchanged, so
`cull::defect_sweep` can use these named signals without changes. Callers should
reserve `face/*` and the two aggregate names for `replace_faces`, rather than
writing them individually with `set_score`.

**Five-point landmarks cannot measure eyelid closure.** `eyes_open` is an
optional confidence/proxy, not a reliable eye-state measurement; use `None` when
no defensible proxy is available. Do not reject photos solely on this heuristic.
Face and score rows cascade on image deletion. Older catalog tables such as
`selection` retain their existing non-cascading foreign keys and must still be
handled by the caller when deleting images.

## Verification

    cargo test -p index
    cargo clippy -p index --all-targets -- -D warnings
    cargo fmt --check
    cargo test -p index -- --ignored --nocapture

Keep CARGO_TARGET_DIR outside the repo as required by the work-package setup.
The ignored benchmark inserts 100,000 images, ten camera values, seven lens
values, caption variants, selections, and keyword links. Its text search matches
10,000 images and returns a 100-image page; its camera-filtered facets count
10,000 images. Both paths warm up before the <100 ms assertion. The dev profile
optimizes bundled SQLite's C library so the normal ignored-test command measures
an optimized SQLite build, without requiring a release build of the workspace.
RAW fixture coverage skips explicitly when `fixtures/raw` is absent.
