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

**Note.** `crates/filters/tests/adaptive_real_size.rs::a_cancel_stops_the_coarse_render`
still cancels from a timed thread. It was left alone (out of scope).
