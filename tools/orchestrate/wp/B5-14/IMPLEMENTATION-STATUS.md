# B5-14 implementation status: viewport-only document render, snapshot rendering, resource policy

Branch `wp/B5-14` (base `wp/B5-10b`, ee11f55). Files touched: `crates/tessera-ffi/src/document.rs`,
`document/render.rs`, `document/filters.rs` (pressure registration only), `tests/document_viewport.rs` (new),
`tests/document_perf.rs` (new, ignored benches), `apps/mac/Sources/Tessera/Document/DocumentSelfTest.swift`,
this folder. crates/compositor and crates/jobs are unchanged (NEEDS.md has three asks). engine-api unchanged.
`io.rs` was not needed: the save snapshot is taken in document.rs and `io::save` receives it.

## P13: viewport-only composition

* `present_frame` calls `ResidentRenderer::render_viewport(doc, level, src, VIEWPORT_HALO)` with the visible
  source rect `resolve()` already computed. `VIEWPORT_HALO` = 16 level pixels (one resident block,
  COMPOSITOR.md §12.2). Pointwise programs need no halo; the halo keeps edge blocks warm for small pans.
* **Full-level fallback** (`resident.render`) only when `needs_full_halo`: a positive-radius
  Shadows/Highlights or HDR Toning anywhere in the layer tree (the resident spatial passes read the whole
  level, COMPOSITOR.md §4.3). Also when forced (`TESSERA_DOC_FULL_LEVEL`, or
  `DocumentSession::set_viewport_rendering(false)`, the parity reference). `read_level` (tests/export) still
  renders the full level.
* **CPU** path unchanged: layer styles (B5-07) and no Metal. The B5-07 fallback and the B5-10b shared font
  snapshot are untouched (`fonts::compositor` / `fonts::install`); covered by document_styles_ui and
  document_fonts, which pass.
* Every frame produces a `DocRenderRecord` (path, level, visible, requested, dispatched block union, blocks,
  epoch, superseded/dropped, spans). Path switches are always logged (`document: frames on the Viewport
  path (L0, requested …)`); every frame with `TESSERA_DOC_RENDER_LOG=1`; resources every 2 s with
  `TESSERA_DOC_PERF_LOG=1`. `DocumentSession::render_records()` exposes them to tests (not over UniFFI).
* Test `viewport_frames_composite_only_the_viewport_plus_halo` (20 MP, 100 layers, 3840×2160 at L0):
  requested = visible ± 16 clipped to the level, dispatched = exactly the block-aligned requested region
  (33 154 blocks, less than half the level), a 64 px pan dispatches only the new column strip, edge clipping.
* Test `viewport_frames_equal_full_level_frames`: two sessions of one document (viewport vs forced full
  level), byte-identical surfaces after the first frame, 5 pans, 2 resizes, a level change, 4 interactive
  opacity ticks and the commit, hide, adjustment add/drag, undo, a pan after off-screen edits, a spatial
  adjustment (both FullLevel), a layer style (both Cpu) and its undo.

## P14: rendering from an immutable snapshot

* `State::doc` / `State::scratch` are `CowDoc` (an `Arc<Document>`, `DerefMut` = `Arc::make_mut`), so the
  other document modules compile unchanged. `present_frame` holds the live-state lock only to take the
  snapshot (`live_shared()`, an `Arc` clone), the surface and view generation: 0.00–0.17 ms held in the
  records. Filter preparation (`filtering::presented`), composition (GPU or the CPU style fallback via
  `cpu_present`) and presentation run without it; the GPU path drops the snapshot before the GPU wait.
* Publication is generation-checked: a frame whose surface ring was replaced (`View::generation`, bumped when
  a surface leaves the ring) or whose session closed is dropped (`dropped` record, no `on_frame`) and the new
  ring gets a frame. A frame whose document changed meanwhile is still published (`superseded`) because the
  next frame is already queued; dropping those starved the display during continuous drags.
* `save` / `save_as`: commit the pending drag and snapshot under the lock, write the file without it, then
  record the saved node; `Shared::saving` orders concurrent saves. `read_level` (tests) renders a snapshot.
* Tests: `edits_do_not_wait_for_frames_in_flight` (styled CPU frames ≈ 330 ms; opacity/rename/visibility
  calls during them: median 0.006 ms), `frames_for_a_replaced_ring_are_dropped`,
  `save_render_undo_races_keep_history_and_surfaces_valid` (saves on one thread, viewport changes on another,
  60 interactive edits/commits/undos/redos on a third; history rows reachable, every saved file reopens with
  the same layers, a quiet save leaves the document clean, the final frame matches a fresh render within 1).

## P17: resource policy

* `render::Pressure` registers a document frame (for its duration) and each filter preview/bake
  (filters.rs worker, the only change there) with `jobs::pressure`, so photo export bands and checkpoints
  (`jobs::yield_to_interactive`) wait for them, bounded by `EXPORT_MAX_YIELD` (1 s) per band so export keeps
  progressing. GPU queues stay separate. `pressure::begin` is `pub(crate)`, so registration is a
  Viewport-priority hold job on a private 4-worker pool, recycled every 4096 holds (NEEDS.md 2).
* `render_resources()` / `TESSERA_DOC_PERF_LOG` log GPU live pages and resident MiB, the CPU fallback cache,
  active render/filter registrations, `jobs::interactive_pending()` and frames per path. The app self-test
  logs the process footprint.

## Composite thumbnails (found while measuring; render.rs)

The app's input → frame for an opacity edit was 3.5 s on a 60-layer 18 MP document with the engine frame
at 40 ms: the Channels panel asks for `composite_thumbnail(48)` on the main thread per epoch, and every call
built a fresh CPU compositor over a fresh document key, re-reducing all 60 layers from level 0. Unstyled
composites now render the thumbnail level of the live document with the session's resident renderer and read
it back (a few KB); its mip pages are content-addressed, so edits and drags stay warm (test
`composite_thumbnails_reuse_mips_across_edits`: cold 341 ms, then ≈ 1 ms per edit and per drag tick). Styled
documents and the CPU backend use a persistent CPU compositor (warm while the document key is stable). The
thumbnail takes the backend lock, so it can wait for a GPU frame in flight (tens of ms, not a CPU frame:
styled documents never take this path).

## Before / after

Machine: Apple M4 Max (not the audit's M4), macOS 26, release builds. "Before" = ee11f55 engine with the
same bench/self-test code. Rust: `tests/document_perf.rs` (ignored; `--ignored --nocapture
--test-threads 1`), times to the completed frame (`on_frame` after the GPU wait, or `wait_idle`). App:
`Support/make-app.sh release`, `open -g -n --env TESSERA_DOC_PERF=1 … --document-selftest <dir>`, sample.dng
(5212×3468, 16-bit) duplicated to 60 layers in 8 modes at 50 %, 100 % zoom, viewport 3842×2238 device px
(the window cannot reach 2160 pt lines on this screen at 2×; one earlier pair ran at 3842×1930 and agrees),
input → frame = the controller call until the frame reaches the main thread (`frameObserver`). Every run
started after 20 s with no cargo/rustc/xcodebuild/swift or other agents' test binaries and load < 24; runs
under contention are excluded (listed below).

| Scenario | Before p50 / p95 (ms) | After p50 / p95 (ms) | Method |
| --- | --- | --- | --- |
| P13 4K L0 first frame (after the fit frame) | 137.8 (single) | 36.0 (single) | Rust, 20 MP/100 layers |
| P13 4K L0 opacity (incremental) | 25.02 / 25.73 | 12.35 / 13.63 | Rust, 40 frames |
| P13 4K L0 visibility toggle | 25.03 / 26.25 | 12.44 / 13.82 | Rust, 40 frames |
| P13 4K L0 pan (97/61 px steps) | 1.17 / 2.23 | 10.14 / 13.78 | Rust, 40 frames |
| P13 4K L0 resize (3584×2048 ↔ 3840×2160) | 1.16 / 2.59 | 8.91 / 13.05 | Rust, 40 frames |
| P13 4K L0 engine render_ms, all warm frames | 24.12 / 26.00 | 11.59 / 13.77 | Rust, 173 frames |
| P14 mutation calls during styled CPU frames (≈ 550 ms each) | 546.06 / 552.90 (max 595) | 0.02 / 0.07 (max 0.08) | Rust, 240 calls: opacity, rename, visibility, text draft |
| P17 input → frame, idle (L2 fit, 100 layers) | 6.30 / 11.42 | 6.42 / 12.49 | Rust, 120 edits at display rate |
| P17 input → frame during photo export | 2.72 / 5.53 (ratio 0.48) | 5.83 / 12.10 (ratio 0.97) | Rust, 240 edits; Web preset exports of sample.dng in a loop |
| P17 exports completed during those 240 edits | 20 (0.32–0.42 s each) | 5 (0.31–6.30 s each) | Rust |
| App 100 % opacity input → frame | 3520.66 / 3556.51 | 27.75 / 29.22 | app self-test, 60 edits |
| App 100 % engine render_ms | 31.09 / 49.84 | 14.14 / 24.71 | app self-test, 100 frames |
| App 100 % pan input → frame | 3.45 / 9.96 | 18.16 / 29.08 | app self-test, 40 pans |
| App input → frame during photo export | 3633.45 / 3839.92 (ratio 1.08) | 27.57 / 29.34 (ratio 1.00) | app self-test, 120 edits |
| App mutation calls during 4K GPU frames | 0.06 / 0.09 (max 0.51) | 0.06 / 0.08 (max 0.19) | app self-test, 150 calls |
| App footprint start → end | 3982 → 1576 MiB | 3850 → 3268 MiB | task_vm_info phys_footprint |

App rows are the 3842×2238 pair (runs f3). The earlier 3842×1930 pair (f1) agrees: opacity 3601 → 28.9 ms
p50, render_ms 40.2 → 10.3 ms p50; its pans were 35 ms in both builds.

Reading the table:

* P13 acceptance (4K p95 < 50 ms to the completed frame): met, p95 13.8 ms at worst; edits halve because
  only the viewport is composited. Pans and resizes get slower than a warm full level (1.2 → 10 ms): each new
  region reallocates the compact output (≈ 135 MB at 4K) and copies the overlap, where the old path only
  presented from an already complete level. They stay far under the target, and the first frame at 100 %
  and every edit no longer pay for invisible canvas (the audit's M4 figures were 197 ms full L0 vs 38 ms 4K).
* P14 acceptance (p95 < 2 ms, max < 8 ms with a slow style frame in flight): met (0.07 / 0.08 ms), from
  552.9 / 595 ms. The app's GPU-frame case was already fast before (the old GPU path released the lock before
  the GPU wait); styled 20 MP layers take minutes per CPU frame (audit hotspot 4, P15), so the slow-style case
  is the Rust bench's 768×512 styled document.
* P17 acceptance (during export p95 ≤ 1.25 × idle; export completes with bounded progress): met in every
  quiet run (0.97 Rust, 1.00 app). On this M4 Max the baseline also met it in quiet runs (0.48–0.59; 2.14 in
  one run with other builds going), because the export has its own device and the L2 document frame is 5 ms.
  The visible effect of the policy is on the export: it yields up to 1 s per band while frames keep coming,
  so during continuous interaction exports take up to 6.3 s instead of 0.4 s, and resume full speed when
  interaction stops.
* App pans: 3.5 → 18 ms p50 for the same reason as the Rust pans (the old path had the complete level).
  The app's end footprint is higher (3268 vs 1576 MiB). Not investigated; the likely cause is that the
  resident renderer now also keeps whole-document mip chains for the composite thumbnail (its page pool is
  bounded by the 2 GiB resident budget).
* App: the 3.5 s per edit was the composite thumbnail (above), not the renderer; after it the UI path is
  ≈ 28 ms (engine 14 ms + controller reloads + main-thread delivery).

## Runs

* Rust final runs: `final-bench-{after,baseline}-1.log` (quiet). Earlier runs with other agents' builds or test
  binaries running were discarded as timing (one baseline P17 at ratio 2.14, one after P13 at load 79).
* App: f3 pair (3842×2238, quiet) in the table; f1 pair (3842×1930, quiet) agrees. An intermediate after
  build (committed-document CPU thumbnails, reverted) is not reported.

## Tests

Gate (worktree root, `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-14 MACOSX_DEPLOYMENT_TARGET=15.0
CARGO_BUILD_JOBS=2`), after the last commit:

* `cargo test --locked --release -p compositor -p jobs -p tessera-ffi`: exit 0; 80 test binaries, 497 passed,
  0 failed, 24 ignored (the 3 new benches among them). New `tests/document_viewport.rs`: `test result: ok. 6
  passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`.
* `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean.
* `cargo fmt --all -- --check`: clean.
* `(cd apps/mac && ./build-ffi.sh && swift build --jobs 2 && swift test --jobs 2)`: `Executed 295 tests, with 0
  failures (0 unexpected)`; `Test run with 5 tests in 2 suites passed`.
* `xcodebuild -scheme Tessera -configuration Debug … -jobs 2 build`: `** BUILD SUCCEEDED **`.
* App self-test (perf mode, release, background): `document-selftest: done, 0 failure(s)` in both builds.
* Regenerated bindings: unchanged (no UniFFI surface change; the new session methods are Rust-only
  `#[doc(hidden)]` test hooks).

## Deviations and limits

* Machine A's P01 spans and `--nonactivating` (M2-53) are not on main (checked `main` and `origin/main`);
  equivalent spans live in render.rs (`DocRenderRecord`) and the app was launched with `open -g -n` only.
  The existing `NSApp.activate()` at launch is cooperative on macOS 26 and did not take focus; the
  perf mode skips the self-test's floating/order-front step. Screenshots: `screencapture -x -o -l <window>`
  of the test's own window only; the app was quit by its own PID (or `NSApp.terminate`).
* The "Rust bench" is ignored tests in `tests/document_perf.rs`: `benches/` needs a `[[bench]]` entry in
  Cargo.toml, which is outside the allowed paths.
* Races between an edit and a frame's CPU phase give the live document a new cache key (`Arc::make_mut`
  clones; `Document::clone` renews the key). The next frame then recomposites the viewport (or re-resamples
  smart pages) instead of only damaged blocks. Rare in practice (the snapshot is released before the GPU
  wait); NEEDS.md 1.
* The app scenario's "4K" viewport is 3842×2238 (screen-limited window), not exactly 3840×2160.
* A `DocumentController.run` shortcut (skip History reloads on live drag ticks) was tried and reverted: three
  Swift tests showed the History panel relies on the reload.
* `DocumentViewport.swift`, `DocumentController.swift` and `EngineDocumentBackend.swift` needed no change.
