# M1 Metal operators

`GpuContext::new()` requests a wgpu 30 Metal adapter and owns its device,
queue, and validated compute pipeline. Failure to acquire Metal is an error,
not a silently successful CPU render. Optional capabilities are reported as
`timestamp_query`, `shader_f16` (including f16 storage-buffer elements), and
`rgba16float_storage`. Supported timestamp-query / shader-f16 features are
requested on the device, but neither is required to render. In-flight samples
and shader math are always f32. The benchmark reports wall time, not GPU query
time. f16 is used only by image-core's existing host memo cache.

Construct `GpuStageOp::new(Arc::new(GpuContext::new()?))` and pass an Arc of
that operator to `Renderer::with_ops`, with a separate `TileCache` for that
backend. Do not share a memo cache between CPU and GPU renderers: memo keys do
not encode the backend, and numerical results are tolerance-equivalent rather
than bit-identical across backends. Device/queue and backend clones are thread
safe. `GpuStageOp::stats()` reports tile uploads, tile readbacks and submissions
(shared by clones of that backend), useful for detecting accidental transfers.

## Operator coverage

- Linearize: Clip and ReconstructColor highlight handling, including frozen
  donor neighbourhoods and the four-pixel reconstruction halo. Actual sensor
  black/white normalization remains in raw-decode, just as for CpuStageOp.
- Demosaic: bilinear and Malvar-He-Cutler for RGGB, GRBG, GBRG and BGGR.
  Decoder channel 3 is normalized to green. MHC preserves negative estimates
  and headroom. Coordinates, planar geometry and incoming phase-preserving
  gathered halos have the same meaning as for the CPU operators.
- CameraProfile and WhiteBalance: row-major 3x3 matrices, coefficients baked
  from f64 to f32. Existing pipeline-cpu metadata resolution / CAT16 matrix
  construction is reused by image-core; the pixels are transformed on GPU.
- Tone: exposure and all six Basic tone controls, preserving the scalar
  luminance/formula/accumulation order. WGSL log1p/expm1 helpers avoid shadow
  cancellation. The exposure-only path bypasses nonlinear math.
- Output: default sigmoid, Rec.2020-to-linear-sRGB matrix, Clip or Perceptual
  gamut mapping, sRGB OETF, absolute-coordinate ordered dither. The shader
  produces integral float code values, converted to U8 only after readback.
- X-Trans: Highlights and Demosaic explicitly execute CpuStageOp, including its
  documented placeholder interpolation. Later RGB stages still execute on GPU.

Parameter/layout validation mirrors the CPU scope. Unsupported highlight modes,
malformed CFA, insufficient halo, wrong channel count/sample format and non-finite
operator parameters return errors. Matrix/Tone preserve halos; neighbourhood
operators and Display return only the interior.

## Additive image-core batch contract

`StageOp::run` remains compatible. Two defaulted methods were added:

- `batch_size()`: defaults to 1, preserving CPU threaded execution. GPU returns 16.
- `run_chain_batch(chain, inputs, cancel)`: order-preserving application of a
  contiguous `[(StageId, Op)]` chain. Default implementation calls `run`, checking
  cancellation between operations. Empty chains are identity. GPU executes at
  most 16 tiles per submission, uploads each once and reads back only the last
  buffer of its chain. `CountingStageOp` forwards batching and counts scheduled
  invocations (including a submitted batch that fails later).

The renderer forms chains at its actual data boundaries:

1. Gathered CFA -> Highlights. The host must gather cross-tile demosaic halos.
2. Gathered reconstructed CFA -> Demosaic. When enabled, the demosaic host memo
   cache ends this chain.
3. CameraProfile -> WhiteBalance (no intermediate transfer). With demosaic
   memoization disabled, Demosaic joins this same resident chain.
4. Host crop / linear downsample and WB memo boundary.
5. Tone -> Display (or just Tone for scene-linear output).

Thus 'once' means once per contiguous chain, not one upload for an entire raw
image across CPU halo gathering/resampling/cache boundaries. A chain cannot
supply a newly required halo itself; invalid chains fail, and Display must be
last because its public output format is U8. X-Trans fallback splits a chain
only at the actual CPU operations. GPU buffers/bind groups are retained by the
wgpu encoder, and passes provide dependencies between storage-buffer writes
and later reads. No unsafe code or unchecked shader access is used.

Cancellation is checked before encoding each tile/pass, before submission,
and before returning readbacks. An already submitted Metal command buffer is
not interrupted; the maximum submission is 16 tiles. CPU parallel workers take
ownership of their tiles, retaining in-place point processing without extra COW
copies. Renderer cache semantics, frame ordering and engine-api are unchanged.

## Verification

Tests require Metal. The fixture gate requires Sony ARW by default (no silent
skip). `PIPELINE_GPU_ALL_FIXTURES=1` requires one of each ARW/CR3/NEF/RAF/DNG.
`PIPELINE_RAW_FIXTURES` optionally overrides `fixtures/raw`.

- Synthetic tests cover all Bayer phases, both demosaicers, both highlight modes,
  CAT16 WB, tone controls, both display gamut modes, halo preservation, partial
  edge tiles, crop/seams and X-Trans fallback.
- Per-operator synthetic comparisons use <=1e-4 absolute linear error and <=1
  display code. Repeating each operator is checked bit-for-bit on the GPU.
- Batched tests verify order, repeated chain determinism, multi-submission tails,
  and exactly one upload/readback per tile. Renderer tests assert tone-only
  cache hits produce one transfer pair per tile, not one per stage.
- Edge cases include black/negative input, tiny shadows, fully clipped regions,
  exposure extremes, invalid inputs, empty batches and pre-cancellation.
- A CPU ownership regression test guards against copying each in-flight tile
  merely because the batch interface was introduced.
- The fixture gate audits every operator on the first actual tile in every
  renderer batch, then compares every final L3 pixel against a cold CPU Renderer.
  Both renderers use the same non-neutral tone settings. This distinguishes
  numerical operator tolerance from f16 cache rounding.

Measured on Apple M4 / Metal, wgpu 30.0.1:

| Fixture | L3 max linear error | L3 max display code error |
|---|---:|---:|
| Sony ARW | 3.5762787e-7 | 1 |
| Canon CR3 | 3.5762787e-7 | 1 |
| Nikon NEF | 1.4305115e-6 | 1 |
| Fuji RAF | 7.1525574e-7 | 1 |
| DNG | 2.861023e-6 | 1 |

These are measured regression gates, not an assertion of bit identity with CPU
libm or a uniform absolute bound over arbitrarily large scene-linear values.

## Ignored benchmark

`cargo test -p pipeline-gpu --release --test fixtures -- --ignored --nocapture`

Runs all five fixtures through image-core Renderer at full level 2, warms WB
caches and times five distinct tone edits. Demosaic memoization is disabled for
both backends so full-resolution demosaic buffers cannot evict the WB working
set. Transfer counters assert no upstream work runs during the timed edits.
The median includes encode, transfers, synchronization and result assembly.

Most recent local run (ms, median of five):

| Fixture | L2 extent | GPU | CPU |
|---|---|---:|---:|
| Sony ARW | 1230x819 | 16.34 | 15.46 |
| Canon CR3 | 1000x1000 | 14.79 | 19.42 |
| Nikon NEF | 1845x1231 | 24.52 | 36.97 |
| Fuji RAF | 1224x816 | 8.77 | 13.87 |
| DNG | 1303x867 | 10.48 | 13.55 |

Wall times vary with host/GPU contention. This implementation does not claim a
universal speedup or a <16ms full-frame guarantee. Buffers are allocated per
chain and host cache/gather/downsample boundaries remain; reuse and persistent
GPU caches are future optimizations, not hidden in these timings.
