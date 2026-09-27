# CPU region cancellation evidence — 2026-09-27

Product commit: `c7e4b6b525a4620a0898e65bad865e03fff70f29` on `codex/cpu-region-cancellation`, based on `dd44a467ea808f475477b1a8ec1a414bc4164ee9`. No main merge in this lane.

The RED snapshot (`red-source/`, `red-manifest.json`) contains the five new public API tests and unchanged compositor source. `cargo test -p compositor --test render_region` exited 101 at compilation with E0599 because `Compositor::render_region` did not exist; no tests ran. The exact compiler output and exit are retained as `red-public.log` and `red-public.json`.

The GREEN snapshot (`green-source/`, `green-manifest.json`) contains the implementation, tests, and exact `Cargo.lock` bytes. All commands used `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, and `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main`, with bounded process timeouts. Each JSON file records the command, exit, elapsed time, environment, and frozen source manifest. Results:

| Gate | Result |
| --- | --- |
| `cargo test -p compositor --test render_region` | 5 passed, 0 failed |
| `cargo test -p compositor --test smart_filter_deadlock` | 10 passed, 0 failed |
| `cargo test -p compositor --test transform_cancellation` | 4 passed, 0 failed |
| `cargo clippy -p compositor --all-targets -- -D warnings` | Exit 0 |
| `cargo fmt -p compositor -- --check`; `git diff --check` | Exit 0 |

The implementation clips signed half-open level coordinates before tile index conversion, returns full edge tiles in raster order, uses one per-request `FilterPass` for covered tiles, and forwards the caller's cancellation token. Filtered tile traversal stays serial. The tests cover signed/extreme clipping, level 1 ordering, off-region avoidance, oversized masked pass reuse, pre-cancel and in-stage cancellation, active-counter unwind, and a fresh successful retry. The gate does not validate B's document/FFI wiring, GPU cancellation, all compositor tests, or global working-memory admission. The resource pass limit remains a retained-result bound.

`MANIFEST.json` hashes every portable evidence file except itself. Raw logs are preserved even where vendor C/C++ warnings appear; strict Rust Clippy exited 0.
