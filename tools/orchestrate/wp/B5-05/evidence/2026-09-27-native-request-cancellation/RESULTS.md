# Native preview and bake request cancellation — 2026-09-27

Candidate `cd07b43559d5cd7e59c7193c096e0fdb995d9231` combines the B-authored `filters.rs` product (`d9974d71f895058ee12d8d38d687d5f2b239ab35`, imported as `711f9ada`) with the current A CPU-region API. The B filter source is byte-identical to the earlier, unrun `711f9ada` freeze; compositor `render/mod.rs` gained the region API in this new candidate. No source changed during validation. The `manifest.json` hashes nine exact source files and records the Git head/tree; `source/` preserves their bytes.

| Gate | Result |
| --- | --- |
| `cargo test -p tessera-ffi --lib request_cancellation_tests -- --nocapture` | 5 passed, 0 failed |
| `cargo test -p tessera-ffi --test document_filters -- --nocapture` | 12 passed, 0 failed; 20 MP benchmark ignored |
| `cargo clippy -p tessera-ffi --all-targets -- -D warnings` | Exit 0 |
| `cargo fmt -p tessera-ffi -- --check`; `git diff --check origin/main...HEAD` | Exit 0, source-only |

`run_gate.py` used `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, and the shared `/Volumes/betterSSD/tessera-cache/target/main` target, verified clean HEAD and all source hashes before each gate, and imposed a 600-second timeout on each process. The runner is preserved here, and the portable log/JSON files preserve commands, exits, elapsed time, and complete output. All three processes exited 0 without timeout. The integration tests may use resident GPU through `read_presented_level`, so they are a small mixed-backend functional gate, not an all-CPU or performance claim.

Source review found no blocker in request-token propagation, atomic effect cancellation, queue supersession, identity-safe completion, or cancellation before native compositor publication. The five private tests cover cancellation during a first filter stage, pre-cancel/fresh retry, stale preview completion, bake supersession, and shutdown. This does not validate the separate document viewport/readback cancellation path, PSD handles, large-image latency, or total working-memory admission. The source comment describing these regressions as “UNRUN on B” refers to their pre-import state; this gate ran them on the integrated candidate.
