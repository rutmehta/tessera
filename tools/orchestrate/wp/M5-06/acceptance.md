# M5-06 implementation handoff

Status: PASS. No commit or push performed.

## Scope

New `crates/filters` only, plus workspace registration in Cargo.toml/Cargo.lock
and this work package's evidence. Verified the modified/untracked path list
against the allow-list and ran `git diff --check`. Compositor and engine-api
are unchanged. All cargo invocations retained:

    CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M5-06

## Implemented

- Filter trait, validated immutable Raster evaluation, explicit halo/full-image
  scheduling, gathered-halo tiled execution, revision-preserving source COW.
- Requested CPU filter inventory, including every requested direct adjustment.
- Large Gaussian sigma through 250 using padded downsample/blur/upsample, on
  both CPU and GPU. Content-independent coefficient error analysis and tests.
- Real WGSL/Metal: all blur families, unsharp/high pass, noise, distortions and
  every adjustment variant. No CPU-rendered GPU fallback.
- Smart filter stack with enable/disable, parameters, and masks. Separate
  injected CameraRawProcessor interface for the full develop-pipeline case.
- Explicit oil paint/lens flare placeholders as requested.
- Ignored 20 MP L0/L2 per-filter benchmark and real GPU 20 MP memory test.

The API intentionally returns EngineResult<Raster>, not an infallible Raster,
so invalid controls, missing depth, missing Camera Raw processor, unsupported
GPU operations, and cancellation cannot be mistaken for a successful render.
See `crates/filters/README.md` for precise algorithm/parameter contracts,
alpha semantics, approximation bounds, and integration limitations.

## Required gate — executed by the implementation agent

    cargo test -p filters --release && cargo clippy -p filters --all-targets -- -D warnings && cargo fmt --check

Exit status 0. Evidence: `verification.log`.

Release suite: 49 passed, 0 failed, 2 intentionally ignored benchmark tests.
Includes 10 real-Metal test cases exercising all required GPU operator families
and every adjustment, max-absolute CPU/GPU tolerance 1e-4. Direct scalar-vs-
separable Gaussian tolerance 1e-5. Halo-vs-whole checks include integer and
float depths, all supported channel counts, and halos above MAX_HALO.

The vendored libraw dependency prints existing native compiler warnings.
The filters Rust clippy gate passes with -D warnings; no dependency files were
changed to suppress those warnings.

## Additional benchmark executions

Both normally ignored tests were explicitly executed and passed:

    cargo test -q -p filters --release --test bench -- --ignored --nocapture
    cargo test -q -p filters --release --test gpu_large_image -- --ignored --nocapture

`benchmark.log` contains 31 inventory dispatch rows at each level. Three at
each level are honestly marked unavailable: oil paint, lens flare, uninjected
Camera Raw. Active adjustments use exposure for the inventory timing; exhaustive
adjustment correctness is in the adjustment/GPU suites, not individual timings.

Selected observed CPU wall times (milliseconds, inclusive Raster conversion):

| Filter | L0 5000x4000 | L2 1250x1000 |
|---|---:|---:|
| Gaussian sigma 1 | 911.097 | 23.088 |
| Box radius 1 | 280.740 | 14.724 |
| Unsharp | 596.117 | 20.382 |
| Smart sharpen | 1469.326 | 47.798 |
| Noise reduction | 6674.086 | 229.093 |
| Radial zoom | 33537.430 | 816.427 |
| Ripple | 43490.426 | 1936.461 |

The scalar Lanczos/radial implementations are reference-quality kernels, not
interactive full-resolution CPU paths. The benchmark is not a latency gate.

`gpu-20mp.log`: real Metal exposure on 5000x4000 RGBA F32 completed in 377.288 ms,
including upload, dispatch, readback and Raster assembly. Adapter limits are
requested explicitly so the default 128 MB storage limit does not reject a
320 MB source. The last pixel and unchanged source are asserted.

## Integration notes

- Use FilterParams.amount=1 for an enabled full-strength filter; default zero
  is exact identity. Distortion amount is separately params.distort.amount.
- CameraRawFilter accepts a host-supplied processor. The bare enum entry errors
  without that context; it is not a pretend raw implementation.
- Smart sharpen and NR reuse pipeline-cpu's published detail operator. Lens
  blur reuses its published depth-layer function, consuming ml-depth's
  near-to-far output without model loading.
- Non-GPU inventory returns explicit errors from the GPU backend.
- No scene-graph schema, UI integration, model downloads, external publishing,
  or unrelated concurrent work-package edits were included.
