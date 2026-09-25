# M2-17b interactive performance redesign

This section supersedes the M2-17 notes below where they conflict.

- **Whole-level resident rendering.** When a request covers a whole level
  (every Develop surface frame, full-level `render_region`) and the padded
  level has fewer than 2^24 pixels (operator parameters carry plane lengths as
  f32), the renderer runs each stage once on a level-sized tile instead of
  once per 256² tile: memoized WB level padded by the largest Detail halo (9,
  edges replicated exactly like `gather`), memoized developed (post-Detail)
  level, one fused point-chain dispatch, one surface + histogram dispatch.
  CPU readback crops the level into pyramid tiles on the GPU. Tests:
  `tests/level_mode.rs` (bit-identical to the per-tile resident path; CPU
  parity; one fused dispatch for warm point edits). Linear kernels accept rows
  of 65535 workgroups (`Batch::record`), so levels above 4.19 MP dispatch.
- **Texture/Clarity/Dehaze are resident** (`resident_tone.rs`,
  `presence.wgsl`) at every level whose packed level fits 1 GiB. Texture and
  Clarity run as two fused 16x16 workgroup-tile kernels (z → self-guided
  coefficients for every active scale; coefficient means → guided outputs →
  no-new-extrema presence written to the output). Dehaze runs separable
  min filters and box means (cached global reads). Box sums keep the scalar
  reference's order and clipped normalisation (no prefix sums). Exact
  airlight/confidence are order statistics on the host, computed once and
  cached (8 entries) under a key covering image, upstream chain, basic tone,
  Texture/Clarity and level extent — Dehaze-only drags never read back.
  Per-operator gate: `tests/local_tone_resident.rs`, max 4.9e-6 vs
  `tone_extra_image` (tolerance 1e-4); full-chain L0 renderer ≤ 2e-3 linear /
  1 display code. Dehaze's global statistics are ill-conditioned: a 4e-6
  upstream difference moved the X-Trans fixture's output by 2.9e-4.
- **Downsampled guidance (opt-in, off by default).**
  `RendererConfig::preview_approximations` computes the wide (r = 8) Clarity
  guided filter on a 1/2-resolution grid of block moments and upsamples its
  coefficients bilinearly (levels above 0 only). Measured max display error vs
  exact at L2: ≤ 2/255 on synthetic scenes and the CR3/RAF/ARW/DNG fixtures,
  but 5/255 (+100) and 8/255 (−100) on the Nikon NEF (14/255 with 1/4
  resolution), over the 4/255 target; a 1/4-grid Dehaze transmission measured
  8/255 on synthetic haze and was removed. The exact wide scale costs one
  extra slot in the fused kernels (~1–2 ms at L2), so exact is the default.
- **Effects constants map.** Vignette mask and grain value are amount-
  independent per-pixel functions of extent, crop frame, vignette shape and
  grain size/roughness. The resident fused kernel binds a GPU-resident f32
  (mask, value) map built by the same WGSL functions once per parameter key
  (one map retained, published only after successful completion), so
  vignette/grain drags skip the double-single `pow` and 64-bit hash work.
  Exactness: map vs CPU ≤ 1.2e-6 (`level_mode.rs`).
- **Detail** writes the halo-free interior directly (no strip pass), reads a
  planar Y + Oklab decomposition, and uses host-evaluated spatial weight
  tables computed exactly like the CPU `kernel()`. Inactive Detail on a
  halo-free tile shares the input buffer. (Workgroup-memory tiles for this
  kernel and 32x32 presence blocks measured slower on M4 than cached global
  reads / 16x16 blocks.)
- **Transient buffers** retired by a transaction (including capability probes
  and cancelled transactions) are recycled across transactions (≤ 1 GiB),
  avoiding per-frame allocation and wgpu zero-fill of level buffers.
- **Level-0 cache policy.** The output-demosaic checkpoint is not retained at
  level 0 (it is a crop of the memoized sensor demosaic); duplicating it
  evicted WB/Detail tiles so every Detail edit on an 18 MP frame re-decoded
  (`level_mode.rs::level_zero_detail_edits_do_not_redecode`).
- **Device limits** request up to the adapter's 16 storage bindings, 32 KiB
  workgroup memory and 1 GiB buffers. The legacy nonresident `tone_local`
  path keeps its 128 MiB ceiling.
- **Not done / rejected.** Per-pixel 1D curve (4096) and 3D grading/HSL (33³)
  LUTs were not adopted: exact evaluation costs ≈0.3 ms (curves) and ≈0.8 ms
  (HSL/vibrance/grading) per L2 NEF frame, and LUT interpolation would break
  level-0 exactness; their constants stay host-prepared per parameter set.
  f16 intermediates were not adopted (M2-17 measured 3.2e-3 linear on f16
  checkpoints, over 2e-3). Preview-level downsampled NR was not added: exact
  resident NR measures ~8–9 ms per L2 NEF frame, inside the 12 ms target.
- **Diagnostics.** `TESSERA_GPU_PROFILE=1` prints per-dispatch GPU time
  (timestamped pass per dispatch) for resident transactions.

Benchmarks: `tests/interactive_performance.rs` (per-operator resident frame
p50/p90 at L2 and L0 on every fixture) and the tessera-ffi Develop session
benches; results in `tools/orchestrate/wp/M2-17b/`.

# M2-17 current status (partial, performance targets not established)

This section supersedes historical residency/cache descriptions below.

- Bayer and X-Trans Highlights/Demosaic now have a resident GPU path. X-Trans
  uses the existing CPU mean-filter oracle, not a new reconstruction algorithm.
  Tile-level nonresident X-Trans entry points still retain their CPU fallback.
- The renderer fuses Tone, curves, Color, Effects and optional Display in one
  WGSL dispatch per output tile. Detail precedes that pass at the render level.
  Point-chain batching also uses fusion. Geometry, locals and active presence
  settings retain the existing whole-image barriers.
- Curve coefficients, grading wheel directions and effects/crop constants use
  bounded last-parameter-set caches. These are exact-parameter equality caches,
  not per-image hashed LUTs or cached vignette pixel maps.
- Resident graph checkpoints now retain f32: the new creative-chain regression
  exceeded the documented 2e-3 linear tolerance with f16 checkpoints. Full f32
  bytes count against the unchanged cache budget. Tests enforce <=2e-3 linear
  and <=1 display code for the new fused renderer fixtures, with <=1e-4 for
  isolated X-Trans operators. This increases cache payload per retained tile.
- Local-tone separable means cooperatively load halos into workgroup memory.
  Accumulation order, clipped-edge normalization and CPU semantics are unchanged.
  Pipelines are cached lazily on GpuContext, with no global device cache or
  unsafe device identity lookup. Shared-memory loading alone did not demonstrate
  an end-to-end speedup. Downsampled guidance is not implemented.
- Local-tone dispatches now share one ordered compute pass between readbacks.
  Presence scales share radius-independent moments, and Clarity-only edits omit
  the unused fine-scale filter. Exact GPU-graph comparison and dispatch-count
  tests guard this reduction; current measurements are in
  `tools/orchestrate/wp/M2-17/presence-benchmark-results.md`.
  Host airlight uses exact order-statistic selection rather than full sorting,
  and default identity curves no longer introduce another pixel roundtrip after
  presence. Latest incremental NEF results are in `retry-benchmark-results.md`
  under the work-package directory; L2 presence still exceeds 16 ms.
- Develop's adaptive classification queries actual renderer residency, so
  newly fused curves/color/effects do not automatically start at the old heavy
  proxy level. Frame-time adaptation remains enabled on both backends.

The NEF benchmark reports 15 independent operators at L2, full L0 and a 1024²
L0 crop, before/after, including host transfers. It is not a resident slider
benchmark. Full L0 presence/geometry can still exceed GPU storage-buffer limits
and delegate to CPU. No <16ms screen or <100ms regional guarantee is claimed.
See `tools/orchestrate/wp/M2-17/validation.md` and `benchmark-results.md`.

# M2-15 managed ICC output

`GpuManagedOutput::new(context, settings, &mut OutputContext)` resolves the same
profile/proof identities as the CPU output adapter. A missing/mismatched proof
or HDR setting is an error. Intent, BPC, paper simulation and gamut threshold
come from the explicit context, without changing engine-api.

The compute kernel applies the CPU reference's luminance sigmoid, cached 33³
ICC/proof LUT, selected clipping or constant-luminance chroma compression, and
destination transfer encoding. Matrix/shaper display profiles are split into a
signed linear-destination 33³ LUT and 4097-point transfer curves to avoid losing
negative channels at the LUT nodes or interpolating across a gamma knee.
CLUT destinations retain their complete ICC transform, with a reversible output
shaper. Edge cells extrapolate rather than clipping working RGB before the CMM.
The preview is an approximation, not a universal accuracy bound for arbitrary
printer CLUTs or extreme signed scene colors; exports use the direct CPU CMM.

- `apply(&Tile)` returns float encoded pixels and separate monitor/proof masks,
  removing halos. `encode(encoder, buffer, layout)` returns RGB and two-bit mask
  buffers without a pixel upload/readback or submission.
- Preview warnings interpolate CMM round-trip DeltaE on an extended [-0.5,3.5]
  working-RGB lattice and threshold afterward. Tetrahedral interpolation preserves
  the neutral diagonal. Near a gamut boundary these are approximate; exact masks
  remain available through the CPU CMM. Out-of-domain warning coordinates clamp.
- `GpuManagedOutput::render_region` is the float/mask convenience path and includes
  scene readback and per-tile transfers. It validates and consumes proof settings
  before invoking legacy scene validation.
- `ManagedRenderer::new(Arc<GpuManagedOutput>, RendererConfig)` owns isolated
  output caches and wires the managed kernel into both batched and resident
  `Op::Display`. `render_region` returns encoded U8 tiles. `render_to_surface`
  writes directly through Metal to an IOSurface, with no pixel readback. It
  returns false for scene operators not supported by the existing resident graph.
  Surface output does not paint warning overlays; use the separate mask API.
- Recreate the immutable managed renderer when target/proof/output settings change.
  Recipe edits outside Output can reuse it. Caller-selected display ICC bytes are
  refreshed through `Registry::display_profile(display_id)`.

Real Metal gates: five built-in destinations against CPU managed output for
highlights, negative/saturated colors, both gamut modes and proof on/off (0.025
absolute RGB); whole Bayer frame vs CPU (0.03, including f16 scene caches and U8
quantization); synthetic smaller-gamut printer paper simulation vs CPU; neutral
warning regression; and exact IOSurface-vs-tile pixels with unchanged readback
counters. These do not claim a universal latency or DeltaE bound.

## Low-level raw LUT primitive

`GpuOutputLut::new(context, &nodes)` uploads exactly 33³ finite `[f32; 3]`
nodes once and compiles a reusable Metal compute pipeline. Nodes use red-fastest
index `(blue * 33 + green) * 33 + red`. All storage and interpolation are f32;
there is no f16 cache conversion, texture filtering approximation, Oklab
conversion, or CPU pixel-transform fallback. Normalized RGB inputs are clamped
to [0, 1] and trilinearly interpolated; LUT outputs are not clamped. The LUT
must already describe the desired output transfer function.

- `apply(&Tile) -> EngineResult<Tile>` accepts finite planar f32 RGB, includes
  halos, preserves layout/coordinates, and executes upload/compute/readback.
- `encode(&mut CommandEncoder, &Buffer) -> EngineResult<Buffer>` accepts a
  same-device planar f32 RGB storage buffer and returns a storage/copy-source
  buffer without submission or host pixel transfer. The caller guarantees
  finite samples and submits its encoder. This enables GPU-only composition.
- `from_lut(context, &transform.lut33())` accepts the typed `color_mgmt` LUT,
  validates its 33³ dimensions and finite values, and uploads it once per output
  transform. ICC parsing and profile ownership remain with `color_mgmt`.
- `render_region(&renderer, &image, &settings, level, rect)` is a raw LUT helper,
  not the complete managed output stage. Construct the transform with `LinearRec2020` as
  source and the desired monitor/proof profile as destination. It renders the
  scene-linear graph (including edits/geometry), bypasses legacy display, and
  returns destination-encoded planar f32 RGB tiles. Switching LUT contexts leaves
  scene caches reusable; no profile-dependent pixels are stored in those caches.
  This convenience path includes scene readback plus one upload/dispatch/readback
  per output tile; it is not fused resident/IOSurface presentation.

`tests/managed_output.rs` compares real ICC-generated sRGB and Display P3 LUTs
against `Lut3d::sample` (2e-6), and direct CMM transforms on off-grid interior
colors (0.01 absolute bound; observed maxima 0.001902 and 0.000933). Direct CMM
accuracy near gamut/transfer knees is not covered by that bound. Renderer tests
exercise profile switching and verify unchanged legacy display bytes.

This raw primitive is not an automatic replacement of `Op::Display`,
the resident renderer's fused display pass, or IOSurface presentation. Use
`GpuManagedOutput` / `ManagedRenderer` above for that integration. Raw LUT callers
must opt in before U8 quantization, and must not apply the old sRGB OETF again
to already encoded LUT output. HDR/extrapolation, gamut-warning overlays,
profile discovery, and soft-proof selection are not implemented by the raw
primitive. `GpuStageOp::stats` does not count standalone LUT calls.

`tests/output_lut.rs` executes real Metal and compares to an independent CPU
weighted-corner trilinear reference (absolute tolerance 2e-6), including axis
order, endpoints, off-grid values, halos and out-of-domain clamping. It also
chains two GPU transforms with no intermediate readback and rejects malformed
LUTs and input tiles. Metal absence is a test failure, not a silent skip.

# M2-08 local adjustment integration

`image-core` applies Locals to the complete requested pyramid level after Color
and before Effects/Geometry. Every group reads immutable pre-local RGB; masked
deltas are summed, rather than feeding one group's output into another group.
Procedural rasterization and adjustment operators use the CPU reference.
`StageOp::blend_local` dispatches linear-light alpha blending through a WGSL
kernel (three uploads, one submission, one readback). Oversized images exceeding
storage-buffer or dispatch limits use the CPU reference blend.

Any recipe containing local groups is explicitly excluded from direct resident
surface rendering. `render_surface` returns `None` and `render_to_surface`
returns `false`; callers must use normal tile delivery. The nonresident graph
still uses GPU global operators and local blend. No local setting is silently
ignored, and recipes without locals keep their previous resident path.

`Renderer::mask_cache()` exposes an independent byte-budgeted f32 raster LRU
with the same payload budget as its tile cache. Keys include procedural
components/composite inversion, pyramid level, upstream Color chain, dimensions,
exact RGB content for RGB-dependent masks/refinement, depth content and
refinement/color smoothness. Local
parameters, amount, id, name and enabled status do not identify alpha pixels.
Retained Arcs may outlive eviction; the byte budget counts cache ownership, not
in-flight callers. A zero budget computes without retaining rasters.

The recipe contract has no local curve, local Point Color, local grain, brush
density/auto-mask or general mask refinement fields. They are not invented here.
Depth/refinement are accepted by the standalone raster cache through runtime
`MaskOptions`; the RAW graph has no depth source and uses default options.

# M2-06 resident Bayer graph

The standard Bayer develop graph uses one compute pass and one queue submission
per requested level. It streams sensor dependencies in groups of 16, recycling
transient f32 buffers within the ordered pass. Sensor demosaic, output-level
demosaic, and WB checkpoints use packed f16 GPU buffers keyed by `MemoKey`. Raw
Decode sources retain f32 precision and count their full bytes against the same
LRU budget: f16 rounding before demosaic caused a verified display regression.
Crop/downsample
precedes both linear profile and WB matrices, reducing their work at preview
levels. Output-level demosaic keys include the crop and a separate domain so
level-zero crops cannot collide with sensor checkpoints. Cold and warm checkpoints
are rounded identically. Tests allow 0.005 absolute scene-linear error and two
8-bit display codes for this multi-checkpoint path.

`GpuStageOp::with_cache_budget` controls LRU payload bytes including pair
alignment. Transactions publish cache entries in access order only after successful
GPU completion. Transient allocations are tracked separately and reused within a
transaction; they are not included in the persistent memo budget.

CPU tile consumers use one final staging buffer/map. Surface presentation imports
an IOSurface through Metal's `newTextureWithDescriptor:iosurface:plane:` and
wgpu-hal `texture_from_raw`. Develop calls `Renderer::render_surface`, writes pixels
directly and reads back only a 4096-byte GPU histogram. Surface writes and
histogram accumulation share one compute dispatch per tile, tested against exact
pixels/counts and across repeated frames with partial tiles and nonzero origins.
It retains an immutable
recipe/level for on-demand saved preview/loupe pixels; it never unconditionally
reads back the frame or snapshots a mutable surface ring. Surface writes are
serialized per session and the ring advances only when a frame is published.
`Renderer::render_to_surface` also omits the histogram readback.

Host uploads use fresh buffers and `queue.write_buffer`. wgpu 30 opens a separate
Metal command buffer for every compute pass. A pass per dispatch exceeded Metal's
4096 outstanding-buffer limit on the NEF. Dispatches and compute clears are now
recorded in order and replayed in a single compute pass, followed by final copies.
Queue writes share the pending transfer encoder; successful renders retain one
explicit queue submission. Abandoned transactions
flush only their pending host transfers to release staging memory; cancelled
compute commands are never submitted. Queue-written buffers must never
be recycled for a later host upload in the same transaction. Failure diagnostics
include adapter, dispatches, payload allocation count/bytes, staging bytes and
captured device-loss reason. The host regression is `full_nef_transaction_keeps_device_alive`.

Automatic develop selection measures CPU and GPU first-frame/tone/WB wall times
at level 2 on the first opened RAW and chooses GPU only when both interactive
edit classes are faster. Cold-fill time is reported separately, not weighted as
if every edit were a cold render. Explicit `TESSERA_RENDER_BACKEND=cpu|gpu` skips
calibration. This is a one-image calibration, not a universal speed claim.

**Limits:** X-Trans and extended M2 image-level operators retain the existing
hybrid dispatch. They are not a whole-chain single-submission resident path.
Metal/IOSurface runtime checks were executed on the host Apple M4, including the
full NEF transaction, surface histogram/roundtrip tests, and forced-GPU develop
integration. See `tools/orchestrate/wp/M2-06/validation.md` for measurements and
coverage limitations. The measured NEF L2 path now meets both latency targets
(tone medians 4.0–4.3 ms, WB 8 ms in three M4 runs). Tone and display are fused
on this path; cold GPU frames remain slower than CPU.
The historical measurements below predate M2-06 and do not describe this change.

---

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

### Geometry capability boundary (M2-09c)

`GpuStageOp::run_image` uses WGSL only for crop/straighten with orientation 1,
Upright Off with no guides, default manual transform and constrain-crop disabled.
Every other geometry configuration delegates to `CpuStageOp`, including invalid
extended controls, so validation order and errors remain the CPU's responsibility.
In this checkout CPU supports manual transforms and Upright, but still rejects
nonidentity EXIF orientation and constrain-crop. Fallback does not add support
that the CPU does not have.

The image-level API accepts host `Image` storage, not a resident GPU handle.
`Renderer::supports_resident` excludes nondefault geometry; its M2 image barrier
collects upstream tiles into a host image (reading back any resident results).
CPU geometry therefore costs that synchronization/readback plus CPU resampling;
subsequent GPU operations upload the host result again. The fallback itself
submits no GPU work and adds no transfers to `GpuStats`. It is not zero-copy or
whole-chain GPU-resident. Crop/straighten compute retains one upload, submission
and readback when active.

`Op::Geometry` carries only `GeometrySettings`, with no lens calibration or
metadata. Lens distortion is composed separately in `pipeline-cpu::render` via
`geometry_mapped`; no lens map is accepted or silently ignored by this kernel.
The composed lens/Upright WGSL port is deferred, not claimed by this change.
Tests compare public geometry results/errors to CPU, enforce the per-operator
1e-4 linear tolerance, and check fallback transfer counts and cancellation.

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
time. (Before M2-06, f16 was used only by the host memo cache.)

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

## EDR presentation (M2-22)

`Op::Display { gamut, headroom: Some(h) }` is the viewport-only EDR transform
(`pipeline_cpu::display_linear`): the default display sigmoid rescaled to peak
at `h` (mid-grey stays 0.18), hue-preserving compression into `[0, h]`, F32
display-linear sRGB with no OETF, quantization or dither. `headroom: None` is
the unchanged SDR Output stage (bit-identical; `tests/hdr_surface.rs` pins
pre-M2-22 fingerprints). `Renderer::render_surface_as(.., RenderOutput::DisplayLinear(h))`
writes an `'RGhA'` RGBA16F IOSurface with `hdr_surface.wgsl`, fused with the
display histogram (bins of the SDR-clipped encoding). Surface format and output
must match: RGBA8 takes encoded Display, RGBA16F takes DisplayLinear.
