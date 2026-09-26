# Explicit precise resident transform stage

## API and integration boundary

`ResidentRenderer::prepare_transform(&TransformOp, source: Extent, output: Extent, level: u8) -> EngineResult<TransformPlan>`

- Generates source pixel-center coordinates with `TransformOp::displacement`, in the same f64-to-f32 order as the CPU renderer. Coordinates are absolute, at the requested level; invalid geometry uses `[-1e20; 2]`.
- Uploads an RG32Float texture (8 bytes/output pixel), not reconstructed pixels or per-tap footprints.
- Compiles lazily per resolved kernel on the existing shared compositor device using `gpu_core::precise_compute_pipeline`. Requires `Precision::Ieee`; no relaxed backend fallback. Automatic uses TransformOp's level-aware chooser, shared with CPU evaluation.
- Free transforms additionally check source-rectangle projective poles, matching CPU `apply`. Warp, Perspective and Puppet delegate geometry to their existing displacement implementation. Content-aware scaling is not a geometry field and is rejected.
- Keep the plan while geometry, kernel, source/output dimensions and level remain unchanged. Source pixel changes do not invalidate it. Preparation does not wait for upload completion; subsequent submissions on the shared queue are ordered after the upload.

`ResidentRenderer::encode_transform_buffers(&mut CommandEncoder, &source: Buffer, &destination: Buffer, &TransformPlan, Option<ComputePassTimestampWrites>) -> EngineResult<()>`

- Caller-owned source/destination are distinct, same-device STORAGE buffers, tightly interleaved premultiplied f32 RGBA at offset zero. Caller supplies valid finite premultiplied pixels and uses the same device for the plan and encoder.
- Encodes only: no submission, wait, CPU pixel processing, pixel upload/readback or geometry regeneration. Destination allocation/lifetime and subsequent presentation/compositing belong to the caller.
- Checks dimensions, buffer sizes/usage and source/destination aliasing. Output must fit a 2D texture, both images must fit device storage bindings, and each canvas is capped at 100 MP.

`ResidentRenderer::encode_transform_level(&mut CommandEncoder, &destination: Buffer, &TransformPlan, Option<ComputePassTimestampWrites>) -> EngineResult<()>`

- Borrows the renderer's internal premultiplied level buffer directly, at the level selected by the plan. Requires the complete rendered level with matching source dimensions; rejects missing, partial, compact or invalid level state.
- Does not overwrite the level or change document/SmartObject behavior. This is an explicit callable resident stage, **not automatic Transform SmartFilter or SmartObject integration**. It can consume a child renderer's full composite explicitly, but does not replace the existing smart-page path.

`TransformPlan::precision() -> gpu_core::Precision` reports the mandatory IEEE compilation mode.

## Reconstruction

Nearest, bilinear, Catmull–Rom bicubic, and normalized Lanczos-3 follow `transform::sample`, including y-major/x-major accumulation. Out-of-image taps are transparent black. Lanczos normalization includes the complete support, including missing taps. No unpremultiplication, clamping, edge renormalization, hardware interpolation or f16 intermediates. Negative RGB/alpha lobes survive. Precise division uses gpu-core's `pdiv` helper; Metal safe math and contraction-off are mandatory. Transcendental Lanczos results are tolerance-compatible, not promised bit-identical.

## Verification

Real Apple M4 Metal execution, release builds:

- Four transform GPU tests pass; no adapter skips or CPU fallback. Affine/projective free transforms, curved warp, levels 0/1/2, wide-source fractional precision, half-pixel ties, far-outside geometry, single-pixel transparent edges and negative lobes are compared with CPU `TransformOp::apply`.
- Observed nearest/bilinear/bicubic/Automatic maximum absolute error: 0. Lanczos-3 maximum: `4.7683716e-7`, below `1e-4`.
- Explicit rendered-level test checks missing/partial rejection, direct resident input, parity and unchanged original composite. Validation tests cover malformed dimensions, version, singular/nonfinite/pole geometry, content-aware rejection, undersized/nonstorage buffers and aliased buffer clones.
- `cargo test -p compositor --release` passes, including existing resident, smart-resampling, viewport and output tests. Existing ignored benchmarks remain ignored.
- Strict `cargo clippy -p transform -p compositor --all-targets -- -D warnings` passes after integration cleanup.

Commands (run from repository root):

```sh
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-21 cargo test -p compositor --release --test transform_gpu -- --nocapture
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-21 cargo test -p compositor --release --test transform_gpu benchmark_36mp -- --ignored --nocapture
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-21 cargo test -p compositor --release
```

## 36 MP benchmark and unmet target

The ignored `benchmark_36mp` test uses a 6000×6000 premultiplied gradient, fractional rotated/scaled affine mapping and actual GPU reconstruction. Resident input and output are each 576,000,000 bytes; the displacement texture is 288,000,000 bytes, plus transient host map and upload storage. Interior result pixels are checked, not merely timed.

Latest measured run, Apple M4, release, 8×8 workgroups, kernel-specialized precise pipelines; five cached samples after warm-up:

| Kernel | GPU median ms | GPU range ms | Cached submit+wait median ms | End-to-end ms |
|---|---:|---:|---:|---:|
| Nearest | 17.295 | 17.102–19.218 | 17.618 | 176.803 |
| Bilinear | 17.015 | 16.693–19.048 | 17.284 | 113.603 |
| Bicubic | 15.946 | 15.681–16.819 | 16.246 | 112.358 |
| Lanczos-3 | 32.845 | 32.803–34.226 | 33.232 | 130.501 |

**The GPU-only <30 ms target passes for nearest/bilinear/bicubic but fails for Lanczos-3. No end-to-end case reaches 30 ms.** Benchmark success asserts pixel checks, not timing; each timing row explicitly reports whether its GPU median meets the target. Do not present cached GPU times as interactive geometry-change latency.

GPU timestamps surround only the compute pass. End-to-end starts before CPU map generation and includes allocation/upload of the map, encoding, queue submission and completion. It excludes pre-existing source/destination allocation, source upload, pipeline compilation and pixel readback. Geometry preparation is CPU work and remains significant. This measures free-transform geometry; warped map preparation latency was not benchmarked at 36 MP.

Timestamp queries are resolved in a **separate submission after pass completion**. Same-submission resolution on this Metal backend produced zero/stale first values and spuriously short cached samples; those measurements were discarded. Wall-clock submit+wait is reported independently as a cross-check.

Tuning retained constant kernel specialization and 8×8 groups. Ordinary division was slower than gpu-core's precise `pdiv` on this device and was reverted. Further optimization is needed for Lanczos and CPU-map/upload latency; there is no all-kernel sub-30-ms claim.
