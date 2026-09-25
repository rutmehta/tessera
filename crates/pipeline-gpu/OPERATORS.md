# Metal operators: M1 and M2 (full-suite acceptance blocked)

## M2 implementation status

The build-restoring CPU delegation is committed separately (`67a87cc`).
The following M2 operators now execute WGSL compute:

- Point curves: master RGB, individual channels and luminance, using direct
  monotone Hermite evaluation on the CPU reference's logarithmic axis. Host code
  validates knots and prepares slopes, but does not evaluate image pixels.
- Four-region parametric curves, including custom splits.
- Vibrance, saturation, all eight HSL bands and four grading wheels in Oklab /
  OkLCh, including balance and blending. Black explicitly avoids WGSL's undefined
  `atan2(0, 0)`. Neutral settings preserve input bits.

- Vignette: HighlightPriority, CIE Lab ColorPriority and PaintOverlay; crop-frame
  mapping, highlight protection and deterministic grain. WGSL emulates the CPU's
  wrapping 64-bit hash with u32 pairs and 16-bit partial products.
- Detail: unsharp mask, luminance bilateral NR and Oklab chroma NR. Two compute
  passes use an immutable decomposition buffer. Real-neighbour halos are gathered
  by the image dispatcher, and output halo samples remain unchanged.
- Texture/Clarity/Dehaze: whole-image guided filtering with separable clipped box
  sums. Pixel filtering and dark-channel morphology are compute passes; exact
  global percentile/airlight reduction remains on the host. Curves follow these
  passes, preserving CPU operator order.
- Crop/straighten: whole-image inverse map and normalized Lanczos-3 compute,
  including black outside-source centers, clamped taps and neutral bit identity.

`run_image` dispatches curve-only and color operations in batches of at most 16
tiles, so the renderer does not accidentally use the trait's CPU image barrier
for these ports. Tile halos are preserved. Tests assert actual GPU submissions,
CPU error <=1e-4, and backend repeatability. The fixture audit now also audits
whole-image calls, and its recipe activates all M2 operator families.

**CPU delegation retained:** X-Trans CFA operations; tile-level ToneExtra with
active presence controls (the renderer uses the GPU whole-image route); whole
images exceeding the GPU storage-buffer limit for local tone or geometry.
Tile-level Geometry remains an error, matching the CPU API.

**Acceptance blocker:** all five fixtures pass each same-input operator's linear
1e-4 gate and the final DeltaE2000 <=0.5 gate. The Nikon full-chain scene-linear
comparison fails at 8.587241e-4. Texture's luminance gain amplifies small upstream
rounding differences for mixed-sign RGB whose luminance nearly cancels to zero.
Routing ToneExtra to CPU does not fix this. A diagnostic shared presence guard
reduced the error to 8.702278e-6, but requires a matching CPU-reference semantic
change in `pipeline-cpu/src/tone_extra.rs`, outside this work package's allowed
paths. No guard or tolerance relaxation was applied. See the diagnostic logs in
`tools/orchestrate/wp/M2-05/`. M2-05 is not ready for acceptance.

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
safe. `GpuStageOp::stats()` reports pixel-buffer uploads, readbacks and submissions
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
operator parameters return errors. Matrix/Tone/Color/Effects preserve halos;
Detail updates only the interior while preserving halo samples. CFA neighbourhood
operators and Display return only the interior.

## Additive image-core batch contract

`StageOp::run` remains compatible. Two defaulted methods were added:

- `batch_size()`: defaults to 1, preserving CPU threaded execution. GPU returns 16.
- `run_chain_batch(chain, inputs, cancel)`: order-preserving application of a
  contiguous `[(StageId, Op)]` chain. Default implementation calls `run`, checking
  cancellation between operations. Empty chains are identity. GPU executes at
  most 16 tiles per resident point/CFA submission, uploads each once and reads
  back only the last buffer of that segment. Detail and Effects split resident
  chains at their dedicated compute paths, with one transfer pair per tile.
  `CountingStageOp` forwards batching and counts scheduled
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

Cancellation is checked between tiles and operations and before returning results.
Dedicated M2 modules check at the caller's entry/exit boundary, not between their
internal passes. Local tone and geometry can submit a whole image. An already
submitted Metal command buffer is not interrupted. CPU parallel workers take
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

| Fixture | L3 full-chain max linear error | L3 max DeltaE2000 | Display code error |
|---|---:|---:|---:|
| Sony ARW | 9.298325e-6 | 0.000826461 | 1 |
| Canon CR3 | 1.66893e-6 | 0.000527681 | 1 |
| Nikon NEF | **8.587241e-4 (FAIL)** | 0.0626712 | 1 |
| Fuji RAF | 4.529953e-6 | 0.000515072 | 1 |
| DNG | 1.5894184e-5 | 0.0247666 | 1 |

DeltaE uses raw scene-linear Rec.2020 -> XYZ -> CIELab D65, with negative XYZ
clamped to zero and no tone/display mapping. Its dependency-free CIEDE2000 helper
is tested against all 34 Sharma reference pairs in both directions.

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
| Sony ARW | 1230x819 | 8.26 | 10.63 |
| Canon CR3 | 1000x1000 | 7.32 | 11.21 |
| Nikon NEF | 1845x1231 | 17.27 | 22.70 |
| Fuji RAF | 1224x816 | 8.24 | 10.36 |
| DNG | 1303x867 | 9.39 | 11.22 |

The same ignored-test command also runs `bench_full_level2_m2_chain_gpu_vs_cpu`.
This uses three cold-cache renders per backend, activating presence, parametric
curves, color, sharpening, both NR controls, vignette, grain and straighten.
It includes upstream processing, transfers and CPU fallback work. It is not a
claim that the full chain is GPU-resident, and is not comparable to warm tone-edit
latency. Run with `--test-threads=1` to avoid benchmark cross-contention.

| Fixture | Hybrid GPU backend ms | CPU ms |
|---|---:|---:|
| Sony ARW | 471.40 | 1377.07 |
| Canon CR3 | 489.84 | 1378.38 |
| Nikon NEF | 1102.37 | 3075.60 |
| Fuji RAF | 827.15 | 1373.93 |
| DNG | 531.97 | 1552.57 |

Both benchmarks were executed locally on all five fixtures. These timings are
not acceptance evidence for the unresolved Nikon numerical regression. M2
pipelines are currently created per invocation (driver shader caching may help),
not cached explicitly. Dehaze uses one upload and three readbacks/submissions;
other local tone uses one. Exact percentile reduction stays on CPU.

Wall times vary with host/GPU contention. This implementation does not claim a
universal speedup or a <16ms full-frame guarantee. Buffers are allocated per
chain and host cache/gather/downsample boundaries remain; reuse and persistent
GPU caches are future optimizations, not hidden in these timings.
