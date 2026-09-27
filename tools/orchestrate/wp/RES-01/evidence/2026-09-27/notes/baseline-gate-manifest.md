# Render resource gate manifest

Checkout: `/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera`
Branch: `codex/render-resource-bounds`
Base: `3825d78e89173d5a01dce5f2ec935816a38b33b9` (`origin/main` at worktree creation)
Files changed: `crates/compositor/src/render/smart_filters.rs`, `crates/compositor/tests/smart_filter_deadlock.rs`
Source SHA-256 (before gate): `smart_filters.rs` 9ba4cbb60df0b99ade687f104c44c6177375236f6bef311315f9fcce95fae93f; `smart_filter_deadlock.rs` 56023cdcdd57edd057079378c10128e61a9bae51767e9f4842c9351c418caf21; `Cargo.lock` ea4bcc1d3e7c62749b1ca4efb9afb69b84032ca898f38180610299c27e680104.

Cached target selected: `/Volumes/betterSSD/tessera-cache/target/main`. It contains prior release `libcompositor` artifacts and the existing deadlock test binary; Cargo checks dependencies and rebuilds changed source. This is shared cache reuse, not proof prior binaries match this source. Run only after A heavy slot release.

Command 1 (expected green, 15-second child-process deadlock watchdog inside test; outer process-group timeout 600 seconds for compile + test):
```
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2 cargo test -p compositor --release --test smart_filter_deadlock -- --exact smart_filter_parallel_reentry_does_not_deadlock --nocapture
```
Log: `/tmp/tessera-resource-deadlock-gate.log`.

Command 2 (expected RED until production resource admission/preparation fix; same 2-worker and timeout controls):
```
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2 cargo test -p compositor --release --test smart_filter_deadlock -- --exact oversized_filtered_source_is_evaluated_once_per_frame --nocapture
```
Log: `/tmp/tessera-resource-oversize-red.log`.

The new full-level test uses a nonblocking counter. The existing two-party Barrier applies only to the direct two-`render_tile` race. No large fixture, app launch, or B test is included.

Baseline outcomes: existing `smart_filter_parallel_reentry_does_not_deadlock` exit 0 (1 passed, child-process watchdog); oversized full-level regression exit 101 as expected (`attempted_stacks` 2 vs 1 at test line 149). The latter is a resource RED, not a build error. Compiler was limited to 2 jobs, Rayon to 2 workers, process group to 600 seconds. Heavy slot was released to root after the red result.
After the first green gate, the red assertion was tightened to require successful rendering and one attempt, and one missing field doc was added. Final RED source SHA-256: `smart_filters.rs` a916899e67214788c24c41cfe70ed2249fa79b2fd68c89070aa5114b46567a46; `smart_filter_deadlock.rs` 7df8e8e59438e9d4378054359f39807aed0de69ae7590701e37968c38e5922b4; `Cargo.lock` unchanged.
