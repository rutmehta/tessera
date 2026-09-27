# RES-01 next gate manifest (prepared, not run)

Checkout: `/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera` on `codex/render-resource-bounds`. HEAD checkpoint `c10b4eea653dcd63f7949eb06ce97d59717c3efa`. Two extra tests are currently uncommitted in `crates/compositor/tests/smart_filter_deadlock.rs`, frozen at `tools/orchestrate/wp/RES-01/evidence/2026-09-27/extra-cases/MANIFEST.txt` (test SHA-256 `401bb8638a6cc5db1154dcc95a5c782f6167e2753e5ca1aff29751f4b77d0dd2`). Before each gate, verify that hash and snapshot the exact source manifest. Do not build while another agent owns the shared compiler slot.

All commands below run in the checkout with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2`. Use an outer 600-second process-group timeout per command and save full stdout/stderr and exit code. The direct deadlock control deliberately creates local Rayon pools of 1, 2, and 4 workers; the environment value is not a strict worker maximum for that test.

1. **Extra focused cases:**
   `cargo test -p compositor --release --test smart_filter_deadlock -- --nocapture`
   Expected 8 integration cases, including same-frame cloned exact-key mask variants, concurrent independent full-level calls, and the existing child-process deadlock watchdog. Stop to inspect any failure; do not broaden a red gate.
2. **Relevant CPU compositor breadth, if step 1 passes:**
   `cargo test -p compositor --release --lib`
   `cargo test -p compositor --release --test smart_filters --test structure --test caching --test layer_styles --test live_render --test live_style_damage --test transform_content --test transforms`
   These exercise filter cache/mask edits, clipping and hidden layers, live-scene/style routing, and transform content without invoking the explicitly GPU or benchmark test binaries. Preserve per-command outcomes separately.
3. **Strict source validation, if the CPU gates pass:**
   `rustfmt --edition 2024 --check crates/compositor/tests/smart_filter_deadlock.rs`
   `git diff --check`
   `cargo clippy -p compositor --release --lib --test smart_filter_deadlock -- -D warnings`
   Strict Clippy may find pre-existing findings; report exact diagnostics and compare with the base rather than changing unrelated code merely to clear them. It can recompile due to lint settings, so run only in an allocated compiler slot.

No large image, full compositor matrix, GPU suite, Machine B process, or global memory-cap assertion is in this gate. Any GPU validation requires a separate scoped decision after CPU outcomes and A-host availability.
