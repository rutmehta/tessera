# B5-20d HANDOFF: deterministic AWA commit-cancel test

**Root cause.** `cancel_during_a_real_size_commit_stops_the_render_without_history`
(`crates/tessera-ffi/tests/document_adaptive_ui.rs`, from B5-20b) slept 300 ms
before cancelling. In `--release` the 5212 × 3468 commit finishes in about
0.31 s, so the cancel could land after the commit had already written history.

**Fix (test support only, no behaviour change).**
- `commit_adaptive_wide_angle` calls the existing B5-13 per-document checkpoint
  (`liquify::apply_checkpoint`) at a new site, `"adaptive:render"`, just before
  the full-resolution render of a pixel layer. It is a no-op unless a test installed a hook
  with `DocumentSession::set_apply_checkpoint_hook`.
- The test installs a hook that calls `cancel_adaptive_wide_angle` at that
  site, then asserts: the commit errors with "cancelled", the
  `"adaptive:render"` checkpoint fired, the `"write"` checkpoint (history write)
  never fired, history is unchanged, and the workspace is closed.

**Verification.** Target test 10/10 in `--release` (0.08 s each), 3/3 in debug
(about 2.2 s each); `cargo test --release -p tessera-ffi --test document_adaptive_ui`
12 passed, 2 ignored; clippy `--all-targets -D warnings -p tessera-ffi` clean;
`cargo fmt --all --check` clean. No Swift or binding change.

**Follow-up: `crates/filters/tests/adaptive_real_size.rs::a_cancel_stops_the_coarse_render`.**
It cancelled from a thread after a 200 ms sleep, so on a fast machine the
6000 × 4000 coarse render could finish first. `adaptive_lattice` has per-tile
`cancel.check()` calls but no hook to flip the token at the first tile, and
this change adds no product code. The test now cancels the token before it
starts and asserts `Err(EngineError::Cancelled)` (no raster) in two places:
(1) through `CompositorFilters::evaluate_with_cancel`, and (2) by solving the
coarse `Lattice` and calling `adaptive_lattice::render` directly. Case 2 skips
`evaluate`'s up-front check, so it is the render's own per-tile check that
stops it. Verification: 10/10 in `--release` (about 0.18 s each), 3/3 in debug
(about 4.2 s each); `cargo clippy -p filters --all-targets -- -D warnings`
clean; `cargo fmt --all --check` clean.
