# Culling core

`CullSession::open(&index, folder.as_path())` opens a recursive folder queue.
`CullSession::open(&index, Query { .. })` opens a filtered queue. Ordering is the
index's capture-time / image-ID order. A zero query limit means **all matches**
for culling, not the index search API's default 100-row page. Explicit limit and
offset are respected after sidecar reconciliation. The queue is a snapshot:
changing a decision does not remove an image from the active review queue.

## Editing and history

- `decide(Decision::{Keep, Reject, Undecided})`, `grade(1..=3)`, and `mark(name)`
  immediately write the recipe and XMP through `sidecar`, then update SQLite.
  Grades imply Keep; changing away from Keep clears the grade. Empty mark clears it.
- `set_auto_advance(bool)` defaults to true. Only decisions advance, stopping at
  the last image. `current`, `position`, and `set_position` expose navigation.
- `undo` / `redo` share one session-global stack across images, marks, grades,
  basket changes, and whole-group decisions. A group action is one undo step.
  Redo restores the original post-action position. A new mutation clears redo.
- Recipe selection is authoritative when present; otherwise existing XMP or the
  index selection seeds a new recipe. Existing recipe history, XMP metadata,
  foreign XMP properties, and synchronization clocks are retained.
- Writes preflight the whole action, use atomic per-file replacements, and
  compensate completed writes on an I/O/SQLite failure. Undo entries and cursor
  advancement happen only after success. Failures during compensation are
  reported, not hidden. Undo refuses to overwrite externally changed selection.

This is not a distributed transaction: process death between file and SQLite
writes can leave the cache stale. Reopening reconciles index selection from the
recipe before filtering. Cross-process concurrent writers need higher-level
serialization; the conflict checks are not a filesystem lock.

The existing sidecar contract uses `.edits/<stem>.json`. Indexed RAW+JPEG pairs
with the same stem therefore cannot be edited independently yet. Culling rejects
colliding destinations before writing, including collisions outside the current
query, rather than corrupting either image's recipe. Recipe naming is not changed
here because it is shared with sidecar and index scanning.

## Host surface (used by the UniFFI bridge)

`CullSession<I>` holds its index as `I`: `CullSession::open(&index, ..)` borrows, and
`OwnedCullSession::open_owned(Index::open(db)?, ..)` owns a second connection (WAL) so a host can keep
the session across calls and threads (`OwnedCullSession: Send`). Files missing on disk when the queue
opens (deleted or moved since indexing) are excluded. `set_current(id)`, `selection(id)`,
`can_undo`/`can_redo` and `undo_images`/`redo_images` (what the next step would touch) support hosts
that mirror state. Batches never move the cursor and are one undo step each: `decide_images`,
`grade_images`, `mark_images`, `decide_each` (a decision per image, e.g. "choose this" in compare;
`keep_best_reject_rest` uses it), `set_basket(ids, add)` and `remove_from_album(album, ids)` (safe
delete: membership only). `derived_statuses(ids)` reads library.json once; `library()` returns it.

## Groups and best-frame selection

Opening computes groups. `regroup(GroupingOptions)` changes the default inclusive
2-second burst gap or disables near-duplicate grouping. Missing/invalid capture
times never form burst edges. Capture times are converted by SQLite's date/time
parser, including fractional seconds and timezone offsets, and numeric Unix seconds (RAW scanners
store those; the index reads them with SQLite's `'auto'` modifier).

JPEG originals are decoded with `previews::Jpeg`; RAWs use
`raw_decode::RawSource::embedded_preview`, never full sensor decoding. `dhash`
resizes luminance to 9×8 and compares adjacent horizontal pixels to produce 64
bits. Hamming distance <= 6 adds a near-duplicate edge. Groups are connected
components of burst and duplicate edges, so transitive membership is intentional.
Unreadable previews are exposed by `preview_errors` and do not block manual
review. Missing embedded previews simply provide no duplicate signal.

`set_grouping_strategy(Box<dyn GroupingStrategy>)` installs an optional replacement
edge policy, effective on the next explicit `regroup`. The `Send + Sync` trait's
`related(&self, a: &ImageInfo, b: &ImageInfo, hash_a: Option<u64>,
hash_b: Option<u64>, options: GroupingOptions) -> bool` is called once per
unordered pair; implementations should be symmetric. Default time/hash edges
are not added when a strategy is installed. Hashes are `None` for unavailable
previews or when `near_duplicates` is false; preview errors remain observable.
The hook does not load embeddings or apply decisions. Tests demonstrate a
consumer-owned cosine >= 0.92 AND (time OR hash) policy, with conservative
time AND hash fallback when either vector is absent.

Group/member ordering follows the review queue. `next_group`, `prev_group`,
`next_in_group` and `prev_in_group` stop at boundaries; `group_of(id)` finds an image's group. `best_in_group` returns the suggested frame;
`keep_best_reject_rest(group_index)` explicitly applies the decision batch.
`set_scorer(Box<dyn Scorer>)` accepts explicit scorers, including `QualityScorer`
and `FaceScorer` snapshots. Without an override, ranking reads current index
signals: `quality * (0.5 + 0.5 * face_sharpness)` when both exist, or the available
signal alone. Missing scores rank below scored members. Only groups with no
signals fall back to `LargestFile` indexed byte size. Ties pick the first queue
member, and non-finite scores are errors. The five-landmark `eyes_open` proxy is
not a blink probability and is deliberately excluded from ranking. Hash
comparison currently uses an O(n²) in-memory pass.

## Library, basket, and status

Folder sessions default to `<folder>/library.json`. Query sessions explicitly
call `set_library(path)`. `set_basket_target(name)` chooses a target; `toggle_basket`
adds/removes the current image and returns whether it was added. No implicit
Quick Collection is created. Basket writes belong in the library, not sidecars.
New target albums are created on toggle and removed again by undo; existing album
order and unrelated fields are preserved. Outside changes to the target album
cause undo/redo to fail rather than overwrite them.

Minimal library schema (unknown library/album fields round-trip):

```json
{
  "albums": {
    "Portfolio": { "images": ["00000000000000000000000000000001"] }
  }
}
```

`derived_status(id)` returns `DerivedStatus { status, in_album }`. The phase is
`published > exported > edited > unedited`; edited means recipe history has at
least one entry, including history subsequently undone. Album names are an
orthogonal list, because an image can be published and in multiple albums.
Export and publish flags mean **ever** exported/published, not render freshness.

Index migration 004 adds `export_log` and the previously absent `score` table.
`Index::record_export(id, destination, published)` appends an event;
`export_status` reads historical flags. Export events currently live in SQLite,
not the portable library publish-state system planned for later milestones.

## Review-only defect sweep

`defect_sweep(&[Threshold::below("focus", 0.4),
Threshold::above("closed_eyes", 0.8)])` returns image IDs and structured reasons
including measured value, threshold, direction, and producing model. Signal names
and scales are producer-defined, so future per-face signals need no enum changes.
Equality is not a defect, missing signals are ignored, and no decision or undo
entry is generated. `Index::set_score` stores the latest finite value and model
version for each image/signal. Without ML-produced scores the result is empty.

## Assisted learning (M3-07)

`cull::learning` is a pure-Rust logistic learner with no additional dependencies.
The feature layout is 11 technical/context values plus 32 library-local PCA
components: sharpness, motion blur, mean RGB exposure, mean shadow/highlight
clipping, noise, face count (saturated at ten), minimum face focus, any low
eyes-open proxy, largest face/image area fraction, and burst sharpness percentile.
Continuous sharpness/blur/exposure/focus/rank values are centered on .5;
missing measurements contribute zero, not a synthetic defect. Unknown eyes do
not count as closed. Face-box area requires the analyzed preview's dimensions,
not the original RAW dimensions. Tied sharpness values share a midrank.

Host integration:

1. Load `Learner::open(app_support_root, stable_library_id)`. A missing model
   starts from technical priors. Corrupt, incompatible or wrong-library models
   return errors. Use one writer per library.
2. For a new library with embeddings, construct
   `Learner::with_embeddings(model_version, &library_vectors)` first. Supply
   vectors obtained from ml-embed's `VectorIndex::get` or concrete index `rows`
   methods. This avoids a cull → ml-embed → cull dependency cycle. PCA uses
   L2-normalized, centered inputs and covariance-free power iteration, up to
   32 orthogonal components, with zero padding for deficient rank. Projected
   values are clamped to [-1,1]. Fit off the UI thread on this library or a
   representative library sample. The basis stays frozen alongside weights;
   changing the embedding model or refitting requires a fresh learner.
3. Populate `ReviewContext` with embedding model/version, available per-image
   vectors, and face-coordinate dimensions. Quality and face measurements are
   read from the catalog. Without embeddings, technical-only learning works.
4. `session.review(&learner, &context, ReviewMode::Assisted)` returns an
   immutable `ReviewPlan`. Entries expose `p_keep`, the five largest nonzero
   signed feature contributions to log-odds, and technical quality. Call
   `reorder_review(&plan)` to apply navigation order while preserving cursor
   identity and undo/redo cursor identities. Likely keepers come first;
   undecided images below .5 are contiguous at the end and available through
   `plan.likely_rejects()` for bulk review.
5. `ReviewMode::Automated { reject_below: 0.2, keep_above: 0.8 }` supplies
   separate `SuggestedDecision` values only for currently undecided frames.
   Equality at a threshold does not suggest. Review, reordering, and best-frame
   queries write neither Selection nor sidecars. Only an explicit user action
   calls `confirm_suggestions(&plan, &accepted_ids, &mut learner)`. It preflights
   the entire batch, refuses stale selections, changed PCA bases, duplicate or
   absent IDs, missing originals and unsuggested frames, then writes through
   the existing one-step undoable batch path. Learning happens after success.
6. For manual decisions, `decide_with_learning(decision, &mut learner, &context)`
   adds a single label after a successful changed decision. No-ops and Undecided
   do not train. Low-level `observe(&features, Decision)` supports host replay of
   confirmed labels. Do not feed suggestions back as labels. Undo changes
   Selection, not historical training events; corrections add new labels. Hosts
   needing exact removal of training history can rebuild from final selections.
7. Explicitly `learner.save(app_support_root, stable_library_id)` after learning
   or before closing. It atomically replaces a versioned JSON file under
   `cull-learning/<hex-library-id>.json`, including the PCA basis. Surface save
   errors and retry; model saving is intentionally separate from authoritative
   photo decisions and cannot roll them back. Stable IDs are 1–100 UTF-8 bytes.

`plan.best_in_group(&session.groups()[group_index])` suggests the argmax of
`.7 * p_keep + .3 * technical_quality`, breaking ties by sharpness, then supplied
group order. The technical term uses stored quality and face-focus modulation,
or a formula from available quality components. It never uses file byte size.
This is an opt-in learned alternative to the existing technical-only
`CullSession::best_in_group`/`Scorer` path, which remains unchanged.

Cold-start priors penalize missed focus and low eyes-open proxies. The existing
five-landmark eyes proxy is **not a blink detector**: these are review suggestions,
not verified defect labels. No AI path silently changes a Decision. Per-face
chips and identity-resolved review filters are provided by `ml-faces`.

Tests cover 30 explicit labels learning a synthetic blur boundary (100/100
held-out classifications, versus 85/100 cold start), dominant blur explanation,
PCA diagonal/variance recovery and full rank, model isolation/corruption,
catalog features, queue reordering, safe confirmation, and sharpness tie breaks.

## Verification

Run with a target directory outside the repository:

```sh
cargo test -p cull -p index --release &&
cargo clippy -p cull -p index --all-targets -- -D warnings &&
cargo fmt --check
```

Tests cover a 50-image keyboard pass and sidecar reload, global undo/redo and
cursor restoration, >100-image queues, stale-cache selection filtering, burst
boundaries across midnight, generated JPEG brightness/blur variants, pluggable
best-frame selection, status/basket transitions, read-only synthetic score
sweeps, migration from v3, destination collisions, malformed XMP, and batch
rollback after an injected SQLite failure.


## People names and optional MWG export

`people::name_person(&index, library_path, person_id, name, &options)` renames a
stable indexed identity (`None` clears the name) and rebuilds the library's
sorted, unique, nonblank people display names. Assignments resolve current names
through index joins, including images outside the current review queue.

`NamePersonOptions::default()` performs **no sidecar I/O**. Set `write_sidecars`
to export all indexed detector faces for affected images as normalized MWG
center/size regions. `dimensions: HashMap<ImageId, (u32, u32)>` overrides persisted
`analysis_width` / `analysis_height` scores: these must describe the oriented
analysis preview, not RAW dimensions. Unassigned faces retain empty names.
`person_keywords` separately opts into append-only person keywords.

Existing full-name XMP wins; otherwise existing legacy stem XMP is edited in
place (shared legacy destinations are rejected). Unknown XML and non-face regions
survive, but replaced Face-entry extensions do not. All files are preflighted;
ordinary failures compensate previous writes and the indexed name. Atomicity is
per-file, not crash-atomic across files/SQLite. Serialize callers against other
writers. `Library::sync_people_names(&index)` is also available after merges or
splits; persist the modified library with `write`.
