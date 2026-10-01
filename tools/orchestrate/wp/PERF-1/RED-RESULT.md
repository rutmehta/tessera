# PERF-1 observed RED — 2026-10-01

Exact source: ad4b7165. Command: `cargo test --locked --release -p compositor --lib render::effects::perf1_tests:: -- --nocapture --test-threads=1`. CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/codex-lr, jobs2, RAYON_NUM_THREADS2, TMPDIR=/Volumes/betterSSD/tmp/.

Exit101: 4 passed, 2 failed, 0 ignored. Independent multi-tile overlay, nested overlay, cross-call document isolation and same-pass smart-child context fixtures passed. Raster once-per-pass test failed with source_raster_build_calls6 versus1. Live-shape test passed its exact pixel oracle and live route guard, then failed with source calls2 versus1. Both style_render_calls assertions occur after the failed source assertion and were not reached; no measured style-call count is claimed.

Raw log: /Volumes/betterSSD/tessera-validation/perf1/red-ad4b7165-20261001T063556Z.log
SHA256: ae2327ff8b24df5e7b64fc463c16b9c1367cafbdbfa7eab3a048f0ceb5b5c5bd
Exit, command/head and hash sidecars share this prefix. Compiler warnings came from existing libraw sprintf use; compilation succeeded. This is observed RED of the intended work invariant, not an unexpected correctness acceptance failure or product completion.

Runtime coordination: batch17 had exited with Rust/Clippy/fmt0 and no active compiler/Swift/GPU process was observed before launch06:35:56Z. External batch18 started06:35:59Z, so compilation overlapped before detection. An identity-guarded cancellation attempt found the owned cargo had already exited; no process was signalled. No runtime timing/performance inference is made from this run. Batch18 now holds the lane, no retry launched. Published runtime-start status e0628ea5-75fb-4e47-90e1-10215853fd58 was not a verified peer reservation.

Next: implement the reviewed frame-local reuse with frozen blur/morphology oracle, budget/cancellation coverage; keep remaining limits explicit. Do not rerun while batch18 is active.
