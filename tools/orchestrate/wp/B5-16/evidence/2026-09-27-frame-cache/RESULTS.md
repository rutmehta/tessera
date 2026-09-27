# Combined CPU frame cancellation and FFI image cache validation — 2026-09-27

Base `bb64b987ae6fed586db33dd19765af6fbf944711`. The integration branch imports only B product `render.rs` from `f518c03cdd4978fc2c0ba8a08d864030380cd075`, `filters.rs` from `755ac31e91d5a88a607366ec030667f486691673`, and their two B5-16 scope documents. Initial candidate `bb020485e2ff308b6e32b75de93b4bdd99ca91a3` was frozen before validation; corrected candidate `544e8a13b75ac4dab1f031cc7e1f4e95c0726494` was frozen after the narrow telemetry repair. Each snapshot contains exact source bytes and a SHA-256 manifest. No main merge occurred in this lane.

The initial candidate passed frame cancellation 5/5, image cache 7/7, preview cancellation 5/5, document filters 12/12 (20 MP benchmark ignored), and a small frame-ring test 1/1. The unchanged `frames_for_a_replaced_ring_are_dropped` test failed 0/1: obsolete work cancelled before the generation-drop branch, so no dropped render record was kept. The failure, exit 101, and all earlier passes remain in `bb020485/`. Strict Clippy was not run after that failure.

The correction carries the post-snapshot attempt record to the worker's final Signal owner decision. It records cancelled or rejected work exactly once as dropped, then invokes accepted callbacks without the Signal lock. It preserves the prior no-record behavior for pre-snapshot failures and ordinary non-cancel render errors. CPU and GPU elapsed stage fields are assigned before propagating checked failures. The source removed an unused `TileCoord` import. Two new deterministic tests cover cancelled work accounting and final-gate rejection/acceptance; the previously failing integration assertion was left unchanged.

The corrected `544e8a13/` snapshot passed:

| Gate | Result |
| --- | --- |
| `cargo test -p tessera-ffi --lib frame_cancellation_tests` | 7 passed, 0 failed |
| `cargo test -p tessera-ffi --lib image_cache_tests` | 7 passed, 0 failed |
| `cargo test -p tessera-ffi --lib request_cancellation_tests` | 5 passed, 0 failed |
| `cargo test -p tessera-ffi --test document_filters` | 12 passed, 0 failed; 1 ignored |
| `cargo test -p tessera-ffi --test document frames_are_coalesced_and_straight_alpha` | 1 passed |
| `cargo test -p tessera-ffi --test document_viewport frames_for_a_replaced_ring_are_dropped` | 1 passed; assertion unchanged |
| `cargo clippy -p tessera-ffi --all-targets -- -D warnings` | Exit 0 |
| `cargo fmt -p tessera-ffi -- --check`; `git diff --check` | Exit 0, source-only |

All process gates used two Cargo build jobs, two Rayon threads, the shared external target, a 600-second per-process timeout, and source/HEAD verification before each command. Each raw `.log` and `.json` is retained. Integration tests may use resident GPU for small readbacks; no large benchmark was run. The cache bounds only retained `Vec<f32>` payload capacity in `FilterState`, not concurrent work or global CPU/GPU allocation. The frame test validates ring publication and accounting, not full app GUI timing. A request after the Signal acceptance decision may precede its unlocked callback; that callback remains valid for the accepted frame, and the new request schedules a later frame.
