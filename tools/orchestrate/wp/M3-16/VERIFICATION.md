# M3-16 verification and handoff

Status: partial implementation, NOT full acceptance. Required test/lint/fmt chain
passes. GPU-resident output feeding demosaic is still missing; the reference
implementation transfers host tensors and has mixed CoreML/CPU partitions.

## Retry verification

Latest independent rerun: the exact required command returned exit 0 with the
external target directory preserved. Programmatic log aggregation confirmed 192
passed, 0 failed, and 2 ignored across 49 result blocks. Both
`trained_model_quality_tiling_and_partition_report` and `raw_fixture_goldens`
passed; `git diff --check` also passed. No implementation change was made in
this rerun. In addition to the host copies described below,
`crates/image-core/src/render.rs:742` explicitly disables resident rendering
when denoise is active. The concrete GPU `ResidentBatch` implementation is in
`crates/pipeline-gpu/src/resident.rs`, outside this work package's write allowlist.
Resolving that integration needs expanded scope, not another rerun of these
passing CPU/reference gates.

The retry independently reran the exact requested release-test, clippy and
format-check command with the required external `CARGO_TARGET_DIR`. It returned
exit 0: 192 passed, 0 failed, 2 ignored across 49 result blocks. The fresh tiny
training run reported +6.038656 dB; the trained-model inference test and
`raw_fixture_goldens` passed. `git diff --check` also returned exit 0.

The LibRaw C++ warnings in the supplied failure excerpt are not the acceptance
failure: they do not fail this command chain. The remaining failure is the
resident integration requirement. Inspection confirms that
`crates/ml-runtime/src/session.rs` extracts inference results into host tensors,
and `crates/image-core/src/ml_cfa.rs` unpacks those into a CPU `Image`.
`ResidentBatch` in `crates/image-core/src/resident.rs` owns opaque backend storage,
not an implemented ML/device-buffer interoperability path. Passing the reference
tests does not establish that handoff. No GPU-residency claim is made.

The retry preserves the existing implementation and records **RESULT: FAIL**
for full M3-16 acceptance. Completing the resident path needs a concrete backend
interop implementation and verification, with scope approval if that touches
GPU backend paths outside this work package's allowlist. Engine API is unchanged.

## Executed on this worktree

Host reports Apple M4. All cargo commands retained
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M3-16`.
No target directory or model weights are included in source changes.

The exact requested command completed with exit 0:

    cargo test -p ml-enhance -p pipeline-cpu -p image-core --release && cargo clippy -p ml-enhance -p pipeline-cpu -p image-core --all-targets -- -D warnings && cargo fmt --check

Aggregated cargo test output: 192 passed, 0 failed, 2 existing ignored tests,
across 49 result blocks (including doc tests). `raw_fixture_goldens` and the
image-core fixture/reference comparisons passed. No goldens were regenerated.
Vendored LibRaw C++ warnings remain; Rust clippy with `-D warnings` passed.
Log: `artifacts/verification.log` (local, ignored).

Additional executed checks:

* ml-runtime registry_validation: all 4 tests pass, including local source digest
  verification, corrupt-weight rejection and no cache publication on failure.
* image-core with ml-denoise: CFA-stage cache invalidation/reuse and zero-amount
  lazy adapter tests pass.
* `ml_cfa_local -- --ignored` explicitly run: fixture-trained ONNX adapter preserves
  unselected sensor-mask bits for all four Bayer rotations on odd-sized images,
  while selected pixels actually change.
* image-core `--features ml-denoise --all-targets` clippy: exit 0.
* Python flat-noise-fit test: each shot/read parameter within 10%.
* Python export test: ONNX checker accepts opset-17 fp32/fp16 graphs and local
  registration. Full MIT license and exact tested package pins are included.
* `git diff --check`: exit 0.

## Training and actual inference evidence

Final default training: 1200 MPS steps on Bayer crops from the CC0 fixtures,
7.73 seconds reported training/data/evaluation time. Original raw samples are
noisy proxies, not clean laboratory targets. Held-out spatial regions with fresh
synthetic noise: 29.951 dB input, 39.045 dB output, **+9.094 dB**. This metric
measures removal of injected noise, not real-camera denoise quality. Full source
hashes and fitted coefficients: `training-report.json`.

The normal Rust `cfa_model` test trains from scratch on CPU and exports a separate
tiny model. Across 24 unseen procedural crops, ONNX inference measured:

| Variant | PSNR improvement | maximum whole/tiled difference |
|---|---:|---:|
| fp32 | 6.038656 dB | 0 |
| fp16 weights / fp32 IO | 6.038672 dB | 0 |

The latest explicit test run (training, noise-fit test, exports, inference,
partition report, seam comparison) finished in 5.38 seconds. It does not use
committed weights and fails rather than skipping when Python dependencies are
absent. The generated procedural crops are only a smoke-test distribution.

Both variants' executed-provider report contains two fused CoreML nodes and CPU
Resize and Slice nodes. ORT/CoreML additionally emitted unbounded-dimension
compilation diagnostics. Do not describe this as all-CoreML, ANE-confirmed,
or a GPU-resident handoff. Full report: `artifacts/cfa-test.log`.

## Delivered source

* Original MIT training/export scripts, four-plane conditioning, adjacent-pair
  self-supervision and flat-patch calibration estimates.
* Local source-kind registry support, actual generated SHA-256 version pins,
  fp32 and fp16-weight entries. Weights remain local in ignored `artifacts/`.
* CFA inference/rotation/packing/halo tiling, exact Amount-zero and sensor-mask
  blending, legacy CfaDenoise trait implementation.
* Raw-domain CPU barrier before unchanged MHC, X-Trans RGB fallback, opt-in lazy
  image-core adapter, F32 raw Denoise cache with calibration/mask/model keys.
* Reproduction instructions and qualification limits in
  `crates/ml-enhance/TRAINING.md`.

## Remaining acceptance gaps

1. GPU-resident runtime output -> demosaic is not implemented. Existing runtime
   `Session::run` returns a host Vec; current backend contracts do not expose the
   required shared device-buffer ownership. Implement/verify a resident runtime
   and GPU-backend contract before claiming full M3-16 completion. This would
   require coordination beyond the currently allowed GPU/engine-api paths.
2. This tiny fixture experiment is not a verified low-ISO capture corpus or a
   calibrated per-camera/per-ISO model. Texture-biased flat-patch estimates and
   the basic adjacent-pair loss are explicitly documented. A representative,
   independently captured validation set remains necessary for product rollout.
3. Masks are accepted as already sensor-sampled M2-08 rasters. Persistent recipe
   mask references are not added (engine-api was kept unchanged).

The source remains opt-in. It must not be enabled as a production default on
these measurements alone.
