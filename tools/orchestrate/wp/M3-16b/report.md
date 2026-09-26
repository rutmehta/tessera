# M3-16b implementation and verification

Implementation is present within the allowed scope. Host verification runs Metal successfully, including the new resident-versus-CPU parity tests. Full acceptance remains FAIL: the existing CFA training test lacks its Python environment/support files and real-model <12 ms latency is not established. The FFI thumbnail fallback timeout from the earlier run did not reproduce in the latest retry.

## Current verification retry

- Reproduced the training failure in isolation (`current-repro.log`), then personally ran the exact required chained command with the external `CARGO_TARGET_DIR` retained. It exited 101: 55 passed, 1 failed, 2 ignored before stopping at `trained_model_quality_tiling_and_partition_report` (`current-required.log`).
- Confirmed `TESSERA_TRAIN_PYTHON` is unset and `tools/orchestrate/wp/M3-16/requirements.txt` is absent. The test also requires the absent `tools/orchestrate/wp/M3-16/test_training.py`. Both support paths are outside the permitted write scope. The training/export scripts themselves are present. No tests were disabled or weakened.
- Independently reran release tests for pipeline-gpu and tessera-ffi: 197 passed, 0 failed, 15 ignored (`current-gpu-ffi.log`). Independently reran the required clippy and fmt commands: both exit 0 (`current-clippy.log`, `current-fmt.log`).
- Executed the ignored real-model benchmark: it printed `NO TIMINGS` because `TESSERA_CFA_BENCH_CALIBRATION` is missing (`current-benchmark.log`). The process success does not validate the <12 ms requirement.
- `git diff --check` passed, and a programmatic status allowlist check found no out-of-scope paths. No production code was changed during this retry. No Kanban task ID is available in this session, so no board lifecycle transition was possible.
- Acceptance remains FAIL pending upstream training support/environment and real-model fixture calibration/timing evidence.

## Previous independent retry

- Executed the requested release-test / clippy / fmt chain with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M3-16b`. It stopped with exit 101 in `trained_model_quality_tiling_and_partition_report`; prior result blocks contain 55 passed, 1 failed, 2 ignored (`retry-required.log`). `TESSERA_TRAIN_PYTHON` is unset and the default interpreter is absent.
- Inspected the test and tracked training assets. The mandatory test also invokes `tools/orchestrate/wp/M3-16/test_training.py`, which is absent along with `requirements.txt`. Those paths are outside this work package's write scope. No test was removed, ignored, or changed to pass without training.
- Independently ran `cargo test -p pipeline-gpu -p tessera-ffi --release`: 197 passed, 0 failed, 15 ignored (`retry-gpu-ffi.log`). This includes the CFA Metal tests and the previously failing thumbnail callback test, with its original deadline unchanged.
- Independently ran the requested clippy and fmt checks: both exit 0 (`retry-clippy.log`, `retry-fmt.log`). `git diff --check` passed. A programmatic git-status allowlist check found no out-of-scope paths.
- Executed the ignored real-fixture benchmark again. It reports `NO TIMINGS` because `TESSERA_CFA_BENCH_CALIBRATION` is unset (`retry-benchmark.log`). Its exit 0 does not establish latency. No performance or real-model parity claim is made.
- No production code changes in this retry. Remaining prerequisites are the missing upstream training support/environment and measured fixture calibration plus pinned model assets for real-model timing verification. Earlier run results below are historical, not the latest retry's results.

## Final host verification (supersedes sandbox limitations below)

- The parent ran the exact requested chained command. It stopped with exit 101 at `ml-enhance/tests/cfa_model.rs` because the training Python environment is absent. `git ls-files` also confirms that the referenced `tools/orchestrate/wp/M3-16/requirements.txt` and `test_training.py` are not tracked in this checkout; installing Python packages alone would not resolve that test.
- The parent then ran the full release suite with `--no-fail-fast`: **263 passed, 2 failed, 17 ignored**, captured in `verified-full.log`. Failures: `trained_model_quality_tiling_and_partition_report` (missing training setup) and `missing_jpeg_returns_pending_then_callback_and_cached_bytes` (callback timeout at `crates/tessera-ffi/tests/fallback.rs:56`). The latter also failed an isolated retry, recorded in `verified-fallback-retry.log`. Its root cause has not been established; it is not being dismissed as unrelated.
- New Metal CFA tests: **5 passed, 1 ignored** in `verified-gpu.log`, including rotation/halo/mask/endpoints, upload lifetime, CPU parity, inference reuse and managed export bands.
- A diagnostic retry of the thumbnail fallback with `CI=1` passed in 4.32 seconds (`verified-fallback-ci-diagnostic.log`). That flag changes the callback deadline from 3 to 120 seconds and disables the initial 100 ms assertion. This demonstrates eventual callback delivery, not compliance with the original release latency limits; the unmodified-command failure remains reported.
- The requested clippy command passed independently (`verified-clippy.log`). `cargo fmt --check` and `git diff --check` passed. A programmatic status check found **no paths outside the allowed scope**. All cargo commands retained the external target directory.
- The parent executed the ignored real-fixture benchmark (`verified-benchmark.log`). It reports **NO TIMINGS** because measured per-fixture calibration is absent. Its successful process exit is not performance evidence.

Earlier worker logs below include sandbox device failures. Those hardware-availability statements do not describe the final host run.

Implemented:

- Added `image_core::cfa::CfaDenoise`, validated `PackedCfa`, and `Renderer::with_cfa_denoise`. `MlCfaDenoise` retains the runtime-owned full-strength tensor without an additional output-buffer copy; host runtime internals remain unchanged.
- Added one fresh packed payload upload per sensor tile/band region, integer GPU unpack of the entire padded-image rotation, same-phase halo folding, and separate per-site mask coverage. Odd quarter turns swap dimensions. GPU Amount blend explicitly selects the original/full-strength value at its endpoints. CFA pages use `cache_exact` f32; existing `Storage::packed` still means f16, never Bayer.
- Inserted CFA between Highlights and Demosaic in both tile and band schedulers. Both resident capability gates enable only supported Bayer CFA with an injected capability. RGB/X-Trans retain CPU fallback; unsupported joint inference remains rejected.
- Separated full-strength identity from Amount/tone. Keys include image, upstream chain, sensor extent/pattern, pinned model digest, and adapter revision (noise, mask, runtime policy). Request snapshots share inference; the direct legacy adapter additionally memoizes by input content. Legacy CPU rendering also reuses full-strength inference across Amount edits.
- Managed export accepts the capability and preserves it and its inference memo across band-backend snapshots. Zero-budget exports retain Decode and Denoise pages transaction-locally, avoiding repeated packed uploads for overlapping dependencies.
- Added explicit FFI `configure_cfa_denoise` with caller-supplied registry, digest, calibration and optional sensor mask. Unconfigured sessions preserve their previous preview fallback and report denoise as ignored without changing the saved recipe. Stateless preview helpers retain their unconfigured behavior; configured session snapshots explicitly opt in. Detail crops retain full-sensor inference identity. The `ml-denoise` feature is compiled by default to expose the adapter to the existing FFI dependency; model loading and denoise activation remain explicit.

Actual RED evidence, captured before the corresponding implementation:

- `red-cpu.log`: Amount edit inferred twice: `left: 2`, `right: 1`, “Amount edits reuse full-strength inference”.
- `red-resident.log`: missing `image_core::cfa` capability/import.
- `red-handoff-copy.log`: full-strength output allocation differed from the runtime output allocation.
- `red-owned-tensor.log`: missing ownership-preserving `PackedCfa::from_tensors`.
- `red-scheduler.log`: CFA resident capability assertion failed.
- `red-ffi-guidance.log`: stateless guidance regression: `assertion failed: !pipeline_cpu::denoise_active(&renderable(&settings).denoise)`; corrected before the final 14-test FFI GREEN run.
- `red-review.log` and `red-backend-fork.log`: missing export-retention/session-fallback and backend-fork interfaces for the review regressions.

Verification commands all explicitly used `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M3-16b`:

- Targeted image-core/ML tests: **22 passed** (`green-cpu.log`). Covers all four rotations, odd extents, masks and exact endpoints, owned tensor retention, inference counts, image/upstream/model/adapter invalidation, resident scheduling, capability fallback, cancellation, and backend snapshots.
- FFI develop unit tests: **14 passed** (`green-ffi.log`), including configured preservation and unconfigured fallback.
- Export page-retention policy: **1 passed** (`green-export-cache.log`). WGSL parsing/validation passed (`green-shaders.log`).
- `cargo clippy -p ml-enhance -p image-core -p pipeline-gpu --all-targets -- -D warnings`: **passed** (`clippy.log`). `cargo fmt --check`: **passed** (`fmt-check.log`).
- Required `cargo test -p ml-enhance -p image-core -p pipeline-gpu -p tessera-ffi --release`: **exit 101**, stopped at missing M3-16 training assets (`full-test.log`). The additional `--no-fail-fast` run recorded **149 passed, 116 failed, 17 ignored** in `full-test-all.log`; failures are the missing training environment and unavailable Metal/IOSurface facilities. These are not GREEN acceptance logs.

Real-device tests remain enabled, not silently skipped. They exercise packed uploads, all rotations/halos, masks, exact blend endpoints, queue-write lifetime, resident-versus-CPU tolerance, bands, and managed exports. Earlier sandbox runs failed device creation with “Metal adapter: No suitable graphics adapter found” and “IOSurfaceCreate failed”; the final host Metal tests passed. The existing `cfa_model` test still requires the absent M3-16 Python environment/test assets. Vendored LibRaw C++ warnings are unrelated to the Rust clippy result.

The ignored benchmark `benchmark_real_cfa_first_frame_then_tone_only` uses real weights and each available Bayer RAW, printing first-render and subsequent tone-only measurements (including final pixel readback). Supply `TESSERA_CFA_BENCH_CALIBRATION` with filename plus four shot and four read coefficients per line, and optionally `TESSERA_CFA_MANIFEST` / `TESSERA_CFA_MODEL_ID`. Five RAW fixtures are present through the fixture symlink: Canon CR3, Fuji RAF, Nikon NEF, sample DNG, Sony ARW. The actual probe (`benchmark.log`) reports **NO TIMINGS** because measured calibration was not supplied. No fixtures or measurements were invented.

Limitations: inference still consumes and produces host tensors; first inference reconstructs Highlights on CPU, then downstream processing stays resident. Transfers use queue staging, not runtime/device shared-memory interoperability. Cancellation is checked around synchronous inference, not inside the unchanged runtime. The renderer's single-entry inference memo has a separate configured budget and request-local retention; the adapter retains its latest full tensor, while GPU page payloads count against the resident cache/scratch budgets. Real-model parity and viewport latency still require the missing model/calibration assets. Synthetic deterministic-backend Metal parity is verified, not equivalent to that real-model evidence.

No commits or board writes. Implementation was delegated within this worktree, then the parent reviewed changes and independently ran the requested command, full no-fail-fast suite, targeted Metal tests, clippy, formatting, scope validation and benchmark probe.
