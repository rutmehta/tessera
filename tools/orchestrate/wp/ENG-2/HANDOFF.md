# ENG-2 — Document state publication without the render lock

## ENG-2b review follow-up (2026-10-01)

Local commits on top of `a312c3c0`, with no rebase:

- `a5cc2eaf` — regression tests (M1/M2/M3 fail on the original implementation).
- `d6c788b6` — panic-unwind publication guard.
- `c9e903d5` — draft and frame publication cost reductions; final tested Rust source.
- The documentation commit containing this follow-up is the final ENG-2b tip.

Every ENG-2b commit carries the requested Claude Opus 5.5 co-author trailer.
The earlier ENG-2 report below is preserved as historical evidence.

- **M1:** `StateGuard::drop` publishes only when changed and not unwinding.
  The `catch_unwind` regression mutates a document under the edit guard, then
  panics before edit metadata completes. Readers keep the exact pre-edit state;
  the edit mutex remains poisoned. This prevents partial publication; it does
  not attempt to recover a poisoned editing session.
- **M2:** the live publication is `Arc<DocState>`, with no second mutation stamp.
  Draft ticks no longer clone a `Document` for publication, and thus do not copy
  its history map or damage deque. Committed history retains the original stamp
  and reuse logic. The regression observes historical-state strong counts across
  publication: the original code adds a history reference (3 -> 4), the final code
  adds none. This is a deterministic ownership check, not a drag latency benchmark.
- **M3:** reader preview geometry holds only viewport and first-surface dimensions.
  No published view owns IOSurface Arcs. Frame presentation advances its private
  ring cursor under the edit mutex without rebuilding the model publication.
  The regression renders an actual frame, checks publication Arc identity, and
  checks that only the local variable and live ring retain the surface.
- **Concurrent confirm:** 64 rounds overlap snapshot acquisition with a writer
  paused after document mutation and before commit publication. Snapshots retain
  the exact prior `Arc<DocState>` and its opacity 0.75 after the writer completes
  opacity 0.25. A temporary mutation restoring edit-lock acquisition fails on the
  250 ms deadline. The mutant was restored before final gates.
- Lock order remains edit -> publication only. Model readers still clone the
  published Arc without an edit/backend lock. Commit publication remains inside
  the edit guard's lifetime. Both duplicate-frame regressions remain unchanged.
- Only `tessera-ffi` Rust source is changed; no other engine crate was touched.
  No dependencies, Cargo.lock, board, apps/mac files, app launch, push, or merge.
  B5-48 continues to own the Swift confirm-time call-site work.

Validation: **correctness and static gates pass; performance gates remain non-green.**

- Full serial FFI aggregate: **exit 101; 570 passed, 1 failed, 31 ignored**
  across 51 result blocks including doc-tests. The sole failure is
  `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`: p95 **465.1 ms**
  versus 250 ms, median 306.5 ms. Develop frame delivery passes in this run.
  All six viewport tests pass. `evidence/eng2b-full.log`.
- Isolated serial Liquify retry: **exit 101**, p95 **608.6 ms**, median 363.3 ms,
  max 1048.6 ms. The 250 ms threshold is unchanged. This retry had no other
  ENG-2b build or test active; other host lanes were not stopped. Host contention
  is not established as the cause. `evidence/eng2b-liquify-isolated.log`.
- Isolated serial Develop retry: **exit 101**, **98/120 at L2** versus the
  required 108/120; render p90 7.2 ms, maximum 2978.6 ms. It passed in the
  aggregate. `evidence/eng2b-develop-isolated.log`. Both performance retries ran
  one at a time after the aggregate, with unchanged limits and `CI` unset.
  The failed retries are retained; neither is presented as a green gate.
- Focused correctness: 10 passed, one opt-in measurement ignored.
- Clippy, all `tessera-ffi` targets, `CARGO_INCREMENTAL=0`, `-D warnings`: exit 0.
- `cargo fmt --all -- --check` and `git diff --check`: exit 0.
- Fresh debug dylib + UniFFI CLI regeneration into `/tmp/eng2b-bindings`:
  Swift, C header, and modulemap all byte-identical to tracked app bindings.
  No generated output was copied into `apps/mac`.
- Evidence: `evidence/eng2b-{red,m1,m2,focused,concurrent-mutant,clippy,bindings}.log`.

Reproduction commands (export the environment from the original handoff below first):

```sh
cargo test --locked -p tessera-ffi --no-fail-fast -- --test-threads=1
cargo test --locked -p tessera-ffi --test document_liquify_ui brush_latency_on_a_20_megapixel_layer -- --exact --nocapture --test-threads=1
cargo test --locked -p tessera-ffi --test develop export_batch_does_not_starve_slider_drag -- --exact --nocapture --test-threads=1
CARGO_INCREMENTAL=0 cargo clippy --locked -p tessera-ffi --all-targets -- -D warnings
cargo fmt --all -- --check
```

All Cargo commands use `--locked`, the external ENG-2 target directory,
`CARGO_BUILD_JOBS=3`, and `RAYON_NUM_THREADS=3`.

---

**Status: implementation complete and locally committed; aggregate validation is non-green
on two performance gates. Coordinator review is required before integration.**

Branch: `wp/ENG-2-state-publication`. Local commits only; coordinator owns integration.

## Commits and provenance

- Actual starting HEAD: `87ff1ff173e4d6d2053a534b7cfd9343aef906e1`, the brief ownership
  update immediately above requested base `e6c3e5da`. No rebase or merge was performed.
- Initial RED: `79c462539efd06ba90b34cc8ad2a83f35d93a95d` — two 250 ms contention
  regressions fail; confirm-before/after-commit ordering already passes.
- Queue-window RED: `30b8a56bd061059b40ac2971b49a60fcbc5f0bcd` — deterministic request-before-snapshot test
  observes two frames instead of one. No performance assertion was weakened.
- Implementation: `ce2e4f12cbfe3315d731efaf366c563be3668bd6`.
- The documentation commit containing this handoff is the final lane tip; the
  implementation hash above identifies the tested Rust source.

## Scope and implementation

- `Shared` publishes an immutable read view behind a separate `std::sync::Mutex<Arc<_>>`.
  It contains committed and live document snapshots sharing their `Arc<DocState>`, plus
  matching history/session metadata. Readers hold this mutex only to clone an Arc.
- The edit mutex still serializes mutations. Its guard publishes after mutable access,
  before unlocking, including ordinary early-return paths. Consequently a reader after
  a completed edit/commit sees it; a concurrent reader can take the previous completed
  publication without waiting for the writer. No render, tree traversal, history clone,
  or old-view destruction runs under the publication mutex.
- Core info, layer/history/snapshot queries, surface planning, display ICC, channels,
  text/shape/vector-mask/style/transform/filter inspector queries, selection outlines,
  and Export Flat snapshot acquisition use the publication. Selection bounds and outline
  caches have independent locks rather than borrowing mutable session state.
- Publication copies history metadata only after document mutation and shares immutable states, rather than retaining
  the mutable `CowDoc` Arc. Retaining that Arc would force a new compositor cache namespace
  on every edit. A regression checks namespace stability and old-view immutability. A mutation stamp
  also tracks snapshot naming and history pruning (which need not change `DocState`),
  so view-only updates reuse published history without making it stale.
- `read_presented_level` now releases session state before filter preparation and backend
  rendering. The ordinary viewport renderer already released it before rendering on this
  base; the deterministic regression reproduces the synchronous readback contention path.
- The publication work exposed a queued-frame window: a request arriving after the
  worker claimed a frame but before it captured state produced a duplicate cached
  frame. Requests already represented by that snapshot are now consumed while the
  edit lock is still held. Later requests and cancelled-token replacement requests
  remain pending; layer/history notifications are not cleared. Deterministic tests
  cover both boundaries, and the unchanged six-test viewport suite passes.
- Existing Export Flat behavior for a pending interactive draft is preserved: it snapshots
  the live displayed state, with committed state maintained separately in the publication.
  Acquisition occurs inside `begin_export_flat`; later commits cannot alter that export.
  Public ABI, rendering math, import translation, and file formats are unchanged.
  Only `tessera-ffi` Rust source changes; the compositor already exposes immutable
  `Arc<DocState>` values and needs no engine-schema or dependency change.

## Machine B follow-up

Restore click-time snapshot acquisition in `apps/mac` after integrating this lane.
B5-40 currently acquires the menu export snapshot on a worker and therefore includes edits
committed between confirm and worker acquisition. This lane makes synchronous native
`begin_export_flat` acquisition independent of mutable session/render locks. It does not
change the Swift flow, cancellation reservations, progress throttling, or HUD implementation.
No Swift gates or app launch were performed.

## Limits

- This is model-read publication, not asynchronous rendering for APIs that explicitly
  produce pixels (thumbnails, sample/readback/export execution), nor nonblocking editing.
  Those operations may still render or serialize mutations. Hidden render diagnostic
  APIs such as GPU cache counters can still synchronize with their backend.
- A first selection-bounds scan, outline construction, large layer/history enumeration,
  or returned JSON serialization has its own computation cost. Microsecond measurements
  on the small synthetic fixture are not a constant-time claim for arbitrary documents,
  a release UI measurement, or proof of the B5-40 whole-main-thread <8 ms target.
- Publishing copies changed history/session metadata outside the publication lock;
  viewport/ring-only updates reuse immutable document snapshots. Pixel buffers
  remain shared. Very large history publication costs have not been benchmarked.
- No new Adobe feature is translated here; no unrepresentable recipe category changes.

## 29c compatibility and boundaries

No changes to `import-lrcat`, `lua_develop.rs`, `xmp.rs`, `engine-api` recipe structures,
retained `recipe.unknown["lrcat_develop_source"]` data, or recipe serialization. Existing
inputs retain identical translation/output behavior. All new fixtures are synthetic.
No private catalog access, dependency additions, Cargo.lock edits, board edits, mailbox,
push, app changes, Swift execution, or GUI launch.

## Validation environment and commands

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/ENG-2-state-publication"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
cargo test --locked -p import-lrcat -p engine-api -p tessera-ffi --no-fail-fast -- --test-threads=1
CARGO_INCREMENTAL=0 cargo clippy --locked -p import-lrcat -p engine-api -p tessera-ffi --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Final aggregate: **exit 101; 725 passed, 2 failed, 32 ignored** across 72 result
  blocks, including doc-tests. Two performance failures:
  Develop L2 frame delivery 107/120 versus required 108/120 (render p90 10.3 ms,
  maximum 1703.6 ms); Liquify brush latency p95 548.8 ms versus 250 ms
  (median 355.8 ms). All six viewport tests pass in this aggregate as well.
  `evidence/final-aggregate.log` retains suite results and failure diagnostics.
- Clippy: **exit 0**, all targets, `-D warnings` (81 seconds). Existing vendor LibRaw
  C++ deprecation diagnostics are emitted by its build script; no Rust clippy errors.
- Fmt: **exit 0**. `git diff --check` also passes.
- Focused ENG-2 correctness + opt-in measurement: **8 passed**, including both original
  contention bounds, confirm ordering, draft/commit/undo/redo/close and named-snapshot
  coherence, cache namespace preservation, pre-snapshot coalescing and post-snapshot /
  cancellation preservation. `evidence/final-green.log`.
- Full unchanged viewport suite: **6 passed** after the queue fix, including the
  previously failing dispatch assertion, pixel parity, cache reuse and save/edit races.
  `evidence/final-viewport.log`.
- The earlier expanded run completed compositor unit/integration tests with zero
  failures. Its FFI phase was stopped to rebuild final source; it is not reported as
  a successful aggregate invocation. `evidence/expanded-pre-ffi.log`.

### Intermediate failures and resolution

The final aggregate repeats the Develop and Liquify performance failures despite their
isolated passes below. Their cause remains unresolved; the isolated passes do not
establish that the final aggregate performance gates pass.

The earlier three-thread aggregate and continuation recorded three failures; their
logs are retained, not renamed green:

1. `develop::export_batch_does_not_starve_slider_drag`: 79/120 frames at L2 versus the
   required 90%; render p90 was 8.0 ms. Unchanged isolated retry passed: 120/120 at L2,
   render p90 11.7 ms. This is the separate Develop session path.
2. `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`: p95 393.4 ms versus
   the 250 ms limit. Unchanged isolated retry passed at p95 239.9 ms (median 169.9 ms).
3. `document_viewport::viewport_frames_composite_only_the_viewport_plus_halo`: the
   final record was a duplicate cached frame with empty dispatch. It also failed in
   isolation, while the pre-fix source passed three isolated runs. A temporary frame
   trace showed one correct full-region dispatch followed by that duplicate. The new
   deterministic queue-window RED test observed 2 frames instead of 1. The implementation
   consumes only requests represented by the captured snapshot, preserving later and
   cancelled-token replacement requests. The full six-test viewport suite then passed.

The baseline experiment temporarily used the pre-fix source, then restored all 11
implementation files byte-for-byte. No baseline source or temporary tracing was left
in the worktree. Tests and limits were not weakened; no `CI` skip was set. Host load
observations in `evidence/host.jsonl` are contextual, not matched to failure intervals
and not proof of causation for performance outliers.

## Measured publication latency

Final-source debug/test profile, synthetic 3 × 2 document, one pixel layer. These are
native Rust call timings, not Swift/UI timing or a release benchmark.

| Measurement | RED / before | GREEN / final source |
|---|---|---|
| Getter batch while backend is held by injected stalled render | Did not complete within 250 ms | 171.5 µs |
| Getter batch including export acquisition while edit lock is held | Did not complete within 250 ms | 582.583 µs |
| Warm `info()`, 1,000 samples with edit lock held | Not measured | median 0.666 µs; p95 0.667 µs; max 0.875 µs |
| Warm `begin_export_flat`, 1,000 samples with edit lock held | Not measured | median 0.208 µs; p95 0.250 µs; max 0.292 µs |

Reproduce the diagnostic (ignored by default; there is no flaky microsecond assertion):

```sh
cargo test --locked -p tessera-ffi --lib eng2_publication_latency -- --ignored --nocapture --test-threads=1
```

The renderer-blocking regressions retain their 250 ms wall-time limit. Scheduling on
a shared host can affect elapsed time; the injected backend/edit lock stays held until
the reader completes or the deadline expires, making lock dependence explicit.
