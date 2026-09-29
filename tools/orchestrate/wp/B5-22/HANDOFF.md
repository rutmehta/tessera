# B5-22: document-render frame starvation during continuous drags

## Regression
bb020485 ("Integrate bounded frame cancellation and FFI image retention") made `Signal::request_frame`
cancel the frame in flight on every `Renderer::request`. Drafts arrive about 16 ms apart and a 20 MP frame
takes about 143 ms, so during a continuous drag every frame was cancelled before it finished. The canvas
froze until the pointer paused. Vector self-test: `previews 60, frames 1, latency n 1 median ~1.25 s`.

## Policy: latest-wins coalescing (no budgets, no thresholds)
- A draft never cancels the frame in flight. `request_frame` only sets the single coalesced `frame` flag
  (and `since` for latency).
- While a frame renders, only the newest draft is kept. Drafts mutate the live state, and the next frame
  snapshots `live_shared()` when it starts, so extra requests collapse into one pending frame.
- When the frame completes, the worker loop sees `frame` pending and starts the next frame at once from
  the newest state.
- Cancellation is kept for real invalidations:
  - Document close: `Renderer::stop` (via `stop_frames`) cancels the frame in flight, as before.
  - Surface-ring generation change: `attach_surface`, when it replaces or rotates the ring (bumps
    `View::generation`), and `detach_surfaces` now call the new `Renderer::invalidate_frame`
    (`Signal::cancel_frame`). It never waits for the backend mutex. A replaced ring also requests its own
    frame, because the cancelled frame no longer reaches the publication gate's re-request.
  - The final owner gate (`finish_frame`) and the in-render `cancel.check()` points are unchanged, and so
    are the generation, closed and ring checks in `present_frame`. `begin_frame` still cancels a stray
    previous owner. In production that cancels nothing, because the single worker starts a frame only
    after the previous one finished.

Latest-wins fits every case bb020485 protected. The only one that depended on "request cancels" was the
first `attach_surface` after a ring resize (clear, then attach, then request). It is now covered by the
explicit generation invalidation.

## Commits (branch wp/B5-22, rebased on origin/main 8d3996f7)
- RED `4f819655` test(B5-22): extracts the scheduling loop into `run_frames` (behaviour preserving;
  `worker_loop` passes `present_frame` and the listener) so tests can inject a slow renderer. Two new
  tests fail on main:
  - `draft_request_keeps_in_flight_frame_and_coalesces_the_next`: `a new draft must not cancel the frame in flight`
  - `drafts_faster_than_frame_time_keep_publishing_latest_wins`:
    `frame starvation: 1 frames for 100 drafts over 733.8ms (want >= 18)`
- Fix `383fb31e` fix(B5-22): latest-wins `request_frame`; `Signal::cancel_frame` / `Renderer::invalidate_frame`;
  generation-change invalidation in `document.rs`. Tests that encoded "request cancels" now use explicit
  invalidation (`supersession…` asserts a draft does not cancel; `final_owner_gate…` and
  `cancelled_cpu_region…` use invalidation; `request_cancellation_does_not_wait…` became
  `invalidation_does_not_wait_for_render_backend_lock`). New
  `invalidation_and_stop_still_cancel_the_in_flight_frame_promptly`: a 60 s injected frame is aborted
  promptly by `invalidate_frame` and then by `stop`, nothing is published, and a draft during the frame
  does not end it.

Files: `crates/tessera-ffi/src/document/render.rs`, `crates/tessera-ffi/src/document.rs`. No engine crates,
Cargo.lock or board.json changes.

## Gates (on 383fb31e)
- `cargo fmt --all -- --check`: OK. `cargo clippy --release -p tessera-ffi --all-targets -- -D warnings`: OK.
- `cargo test --release -p tessera-ffi --no-fail-fast`: 46 binaries, 506 passed, 1 failed, 22 ignored. All 10
  `frame_cancellation_tests` and `document_viewport` (including `frames_for_a_replaced_ring_are_dropped` and
  `edits_do_not_wait_for_frames_in_flight`) pass.
  - The one failure has nothing to do with this change and already fails on origin/main 8d3996f7 (fails 2 of 2 there):
    `smart_preview_thumbnail::tests::hdr_saved_offline_recipe_keeps_policy_and_renders_sdr_thumbnail_without_mutation`
    returns `conflict: close the active Smart Preview editor before changing originals`, from the process-global
    `image_edit_admission` GATES table shared by parallel lib tests. It passes alone (3 of 3), and the whole lib
    binary passes with `--test-threads=1` (198 passed). This is a test-isolation issue for a separate package.
- `apps/mac/build-ffi.sh`: OK. `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (837 tests, 3 skipped, 0 failures).
- `apps/mac/Support/make-app.sh debug`: `Verified debug 383fb31e…`.

## Vector self-test (background, `run-background-selftest.sh vector`, debug app, frontmost app unchanged)
| build | fill-only affine drag, 20 MP | stroked-dashed drag | check "previews coalesce to frames" |
|---|---|---|---|
| before (4f819655, main behaviour) | previews 60, frames 1, latency n 1 median 1245.7 ms | n 1, median 1300.9 ms | FAIL 1 |
| after (383fb31e) | previews 60, **frames 9**, latency n 9 **median 253.5 ms**, p90 372.7 ms, max 372.7 ms | n 5, median 445.5 ms, p90 1142.7 ms | ok |

The drag is 60 steps at 60 Hz (1 s) plus a 0.5 s pause. With frames of about 110 to 160 ms rendered back to
back, 9 frames is the most that frame time allows. B5-11's "51 previews to 50 frames" came from a different,
slower on-screen drag cadence (median 517 ms latency then). A frame count on that order would need faster
frames, not different scheduling. Median latency is now half of the B5-11 figure.

Two failures appear in both runs and have nothing to do with this change: `11b-3 dash-offset slider found`
and `11b-6 fill colour well found` (inspector control lookup in the background host). Before: 3 failures;
after: 2.

## Review nits from Machine A (commit on top of e373b993, no rebase)
- `final_owner_gate…`: the post-acceptance call is `invalidate_frame()` again, so the test checks its
  original intent: a cancel after acceptance cannot revoke the accepted callback.
- New `ring_replacement_and_detach_cancel_the_in_flight_frame` (macOS) drives the real `DocumentSession`
  paths. The test holds the render backend lock so a frame stays in flight, then calls `attach_surface`
  with a different size (the ring is replaced) and, in a second round, `detach_surfaces`. Each time it
  asserts that the in-flight token is cancelled right away. It also asserts that the cancelled frame never
  reaches `on_frame`: frames publish only to the new ring, and nothing publishes after detach. The
  generation gate alone would still drop those frames, so `document_viewport::frames_for_a_replaced_ring_are_dropped`
  passes without the invalidation. This test does not. Checked locally by removing each call in turn
  (not committed): without the `attach_surface` call it fails with
  `attach_surface replacing the ring cancels the frame`, and without the `detach_surfaces` call it fails with
  `detach_surfaces cancels the frame`. With both calls in place it passed in 15 of 15 repeat runs.
- `drafts_faster_than_frame_time…`: the monotonic check uses `<=`, because a microsecond race can publish the
  last draft twice. The last published frame must still show the final draft.
- Gates: `cargo fmt --all --check` OK. `cargo clippy --release -p tessera-ffi --all-targets -D warnings` OK.
  `cargo test --release -p tessera-ffi` (full) had 0 failures: lib 199 passed, 3 ignored, and the
  `hdr_saved_offline_recipe…` flake did not trip in this run. Tests only, no Swift change.
