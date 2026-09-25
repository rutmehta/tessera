# SQLite catalog index

`Index::open(path)` opens a per-user, rebuildable catalog with WAL,
`synchronous=NORMAL`, a 256 MiB mmap ceiling, foreign keys, versioned migrations,
FTS5 and R*Tree. Bundled SQLite supports R*Tree in this build; no fallback is
needed. The `gps` virtual table is kept in sync by image triggers.

The public API returns `engine_api::error::EngineResult`. Image IDs and selection
values use engine-api types without contract modifications.

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
