# Layered-editor filters (M5-06)

CPU reference and real WGSL/Metal implementations over the published
`compositor::raster::Raster` API. Smart-filter evaluation carries explicit
document colour context through the compositor.

## Compositor smart-filter adapter

Install `Arc::new(filters::CompositorFilters)` with
`Compositor::set_filter_evaluator`. The adapter uses the CPU halo-aware tiled
path (whole-image operators retain their global barrier). Identifiers are the
snake_case `Effect` names; distortions use their own names (`pinch`, `offset`,
etc.), and `gaussian_blur` aliases `gaussian`. Unknown names are `Unsupported`.
The compositor, not the adapter, handles enabled flags, filter blend options,
and the shared filter mask.

`SmartFilter.params` must be a JSON object of `FilterParams` fields. Unknown
fields, malformed types, nonfinite/overflowing values, and invalid domains
return errors. Omitted `amount` is **1** here, unlike standalone `FilterParams`.
For example: `{"radius": 1.5}` or
`{"adjust": {"exposure": {"stops": 1, "offset": 0, "gamma": 1}}}`.
Adjustment variants are externally tagged snake_case; unit variants use a
string, e.g. `{"adjust": "invert"}`. Distortion parameters are nested under
`distort` and retain their documented standalone defaults.

### Camera Raw (M5-28, exact CPU Develop memo; bounded resident coverage)

Default feature `camera-raw-filter` enables `camera_raw`. Parameters are
`{"settings": <engine_api::recipe::DevelopSettings JSON>, "amount": 1.0}`.
The old flat tone-only object is no longer accepted. Amount defaults to 1 and
must be finite in [0,1]. Settings are deserialized with the engine schema,
checked against engine CRS slider domains, and validated by the shared CPU
renderer. Unsupported engine controls fail explicitly. AI masks are rejected
even when amount is zero; this adapter never loads models. Without the feature,
the identifier returns `Unsupported`.

The boundary is nonempty F32 **straight RGBA in document-linear working space**.
Do not unpremultiply it. `FilterContext` supplies profile, native level and
canvas. Embedded RGB matrix-shaper ICC profiles are authoritative. color-mgmt
linearizes their TRCs and resolves the working-space conversion to/from linear
Rec.2020. Untagged documents mean linear sRGB. Unresolved profiles and ICC CLUT
profiles error instead of falling back to sRGB. Alpha is preserved, including
transparent pixels with nonzero RGB. Amount interpolates developed RGB against
the original in the document space. Zero amount is an exact CPU COW identity.

CPU evaluation uses `image_core::Renderer::render_rgb_linear`, a native RGB
Develop entry point with persistent **f32** stage checkpoints. It does not use
`run_m2` or its f16 upstream cache. Lens analysis/CA/defringe, WB plus profile
and manual vignette gains, Detail, Tone/presence/curves, Color, Locals, Effects
and composed lens/Geometry run in reference order. The public resolved CPU
optics entry point supplies the private lens operators with neutral creative
settings; detail, tone, colour and effects use image-core StageOp dispatch.
The final warp uses the calibration resolved from the original pixels, never
re-estimates a lens from developed pixels. Geometry output is placed at the
canvas origin, clipped/padded black to retain input extent; alpha is unchanged.

The adapter shares a 256 MiB payload LRU, capped at 64 checkpoints. Entries own
exact f32 planes and the small RGB calibration. Image-core exposes the same
configurable cache through Renderer; it is separate from its legacy tile cache.
Source identity hashes exact input bits, every tile revision, dimensions and
colour/level/canvas context because FilterContext has no layer ID. This permits
safe reuse between identical layer inputs and invalidates same-revision pixel
changes. Image-core callers must supply an immutable image ID plus revision.
Stage keys chain source identity and upstream settings. WB changes retain lens
analysis/alignment, colour changes retain Detail/Tone, and crop changes also
invalidate crop-anchored Effects. Lookup starts at the latest retained stage,
so evicted ancestors are not rebuilt for a cached descendant. Amount changes
reuse Develop; zero amount is an exact COW identity. Oversize checkpoints are
not retained. The budget bounds retained payload, not in-flight full-frame
scratch or output held by callers; evaluations sharing this CPU cache serialize.

CPU tests compare identical in-memory pixels with the image-core renderer and
with the independent scalar reference, including optics, signed/HDR samples,
profiles, alpha, slider edits, revisions, eviction and exact warm/cold results.

The resident evaluator uses the same device/queue and pipeline-gpu resident
operators, never a CPU readback/re-upload. It supports WB, detail, tone,
Texture/Clarity/Dehaze, curves, colour, procedural locals, vignette/grain,
manual lens distortion/vignetting, crop/straighten/manual transforms and amount.
GPU-side layout conversion bridges interleaved RGBA and planar RGB. Dehaze
airlight and confidence use exact GPU order statistics, recomputed within each
transaction rather than caching buffers from potentially abandoned batches.
Local linear/radial/brush/luminance/color masks use immutable pre-local RGB,
the engine's group composition rules and slider-strength semantics. Local
point/detail operators are row-tiled with real halos, including 24MP frames.
As in the CPU engine, local moire is a validated no-op and local defringe/color
overlay are errors. AI/depth inputs are not fabricated.

The following exclusions are intentional in this adapter's resident capability
contract. They return a capability miss or an explicit engine error; they are
never silently treated as identity. The compositor controls CPU fallback.
This documents bounded coverage, **not completion of the full resident M5-25
chain**. Select `lens.profile={"kind":"none"}` and
`lens.remove_chromatic_aberration=false` for resident recipes.

| Control | Reason and CPU behavior |
| --- | --- |
| Lens Auto / AutoCalibrated, automatic CA (including profile None + CA enabled) | **GPU engineering gap, valid on RGB.** The existing image-derived line/edge fitting and CA estimator consume CPU pixels. Porting those estimators requires resident analysis kernels, not a fake identity calibration or hidden readback. CPU runs the actual estimators and caches their result. Distortion/vignetting/CA scale controls work on CPU with that calibration. |
| Embedded lens profile | **Missing source data at this boundary.** Flattened RGBA carries no sensor opcodes/calibration. CPU errors when embedded calibration is requested but absent. RGB itself could carry metadata in a richer host interface. |
| Named database lens profile | **Missing host binding.** FilterContext supplies ICC, not lens database/capture metadata. Neither adapter invents a profile; CPU errors for an unsupplied named profile. |
| Purple/green defringe, including hue ranges | **GPU engineering gap, valid on RGB.** Edge-dependent hue suppression must precede WB; the resident lens planner rejects it. CPU supports it. |
| Upright Auto/Level/Vertical/Full/Guided and guides | **GPU engineering gap, valid on RGB.** Analysis/guided fitting and the composed inverse map are not exposed by the resident optics planner. CPU uses its supported Upright implementation. |
| Geometry orientation, constrain-crop | **Shared engine engineering gaps, not RGB limitations.** Both the CPU geometry implementation and resident planner reject these settings. Input file EXIF orientation is consumed at decode; this does not make an explicit recipe orientation a no-op. |
| Lens softness correction | **Shared engine engineering gap.** No PSF correction implementation; explicit error on both backends. |
| Lens blur, all focus/bokeh/model controls | **Missing depth/host interface plus integration gap.** An RGB image alone does not supply aligned depth; this adapter accepts no depth plane and does not run inference. Existing CPU/GPU depth-layer blur operators could support RGB given that input; engine schema validation rejects this effect here. |
| AI/depth local masks | **Missing host-supplied segmentation/depth.** No fabricated masks or model downloads; explicitly rejected. |
| Local defringe/color overlay; local moire | Defringe/overlay are shared operator gaps and error; moire is the shared validated no-op. These are not described as RGB impossibilities. |
| WB Auto | Shared engine estimator gap; execution errors. Presets and Custom are supported. |
| Point colors, LUT/style, retouch, nondefault camera profiles, unsupported display transforms | Shared native engine/host-resource gaps; schema validation rejects changed unsupported controls rather than approximating them. |
| Raw denoise, demosaic, highlight reconstruction | **RGB source limitation.** There is no CFA or pre-demosaic signal. Valid raw-domain recipe settings are retained but skipped, matching the RGB Develop reference; unsupported engine settings still fail validation. |
| Output HDR/proof/export controls | This filter returns document-linear RGB, before output encoding. Unsupported changed output controls fail shared validation; it does not apply a display/export transform inside a layer. |

Whole-frame buffers must fit device storage limits; the manual optics bridge
additionally retains the existing less-than-2^24-pixel operator limit. It is not
a constant-memory streaming implementation.

Parity tests cover tile boundaries, Display P3, signed/HDR colour, amount
endpoints and alpha. The 24MP ignored benchmark covers the supported subset,
not the missing full GPU chain:

    cargo test -p filters --release --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --nocapture

One local Metal run with Dehaze and a procedural exposure/saturation adjustment
measured CPU 33.406 s and GPU 3.203 s (cold pipelines included, upload/readback
excluded). The benchmark excludes the unsupported automatic lens stages and
manual optics. These are measurements, not latency gates.

## API and data contract

`Filter::apply(&Raster, &FilterParams, &AtomicBool) -> EngineResult<Raster>`
and `Filter::halo(&FilterParams) -> Halo` are the dispatch interface. Returning
`EngineResult` instead of an infallible raster is deliberate: cancellation,
invalid controls, unavailable processors, and unimplemented placeholders must
not appear to succeed. `Effect` implements the trait. `GpuFilters::new/apply`
selects the explicit GPU backend; it never silently substitutes CPU pixels.

`amount` is final effect opacity, 0..1, not a blur radius or distortion amount.
The default is zero, which returns an exact copy-on-write identity. Set amount
1 for a full-strength filter. `strength` controls sharpening/noise amplitude,
`angle` is radians for motion/spin and a scale fraction for radial zoom.
Distortion-specific controls are in `params.distort`; adjustment-specific
controls are in `params.adjust`. Every variant and parameter has Rust API docs.

Inputs are finite normalized U8/U16 or unbounded F32, one-channel masks, RGB,
or straight RGBA in linear Rec.2020. Output preserves extent, depth, and channel
count. U8/U16 output is quantized by Raster; F32 remains unclamped unless an
adjustment explicitly defines bounded values. Source tiles and revisions are
never changed. New output tiles use source max revision + 1. Failure or
cancellation never returns partial output.

Neighbourhood boundaries clamp at the **canvas**, not tile edges. Offset wraps.
Blurs process the straight channels independently, including alpha (they are
not premultiplied compositing operations). Geometric transforms resample RGBA.
Adjustments, detail, median, noise, stylize, render, and layered lens blur retain
input alpha. One-channel sources are temporarily replicated to RGB where a
colour/detail operator needs it; only channel zero is stored back.

## Inventory and reuse

- Gaussian: normalized separable 3-sigma support, sigma 0..250. Zero sigma is
  identity. Large sigma uses the reduction/reconstruction path below.
- Box: separable square, support ceil(radius).
- Motion: symmetric line gather with Lanczos-3. Radial spin/zoom: symmetric
  angular/scale gathers around canvas center; sample count 2*ceil(radius)+1.
- Lens blur: public `pipeline_cpu::lens_blur`, the depth-layer implementation
  used with ml-depth. Supply `DepthMap::near_to_far()` (or a normalized mask) in
  `params.depth`. No model inference or download. Radius is 0..128, 16 layers,
  circular aperture and `params.focus` near-to-far range. GPU uses the same
  layer membership, disk taps, far-to-near coverage and in-focus preservation.
- Surface blur: spatial/range Gaussian bilateral; threshold is range sigma.
- Unsharp: RGB residual with threshold and strength. High pass: 0.5 + residual.
- Smart sharpen / reduce noise: public `pipeline_cpu::detail` and
  `detail_halo`, gathering real neighbours through `pipeline_cpu::Image`.
  Smart sharpen uses its documented unsharp/deconvolution hybrid at detail
  100, radius clamped to 0.5..3, amount <=150. This is deconvolution-lite, not a
  new Richardson-Lucy solver. NR uses its luminance bilateral and Oklab chroma
  reduction; no neural denoiser is silently requested.
- Noise: counter-hash seeded by coordinates/channel; uniform [-1,1] or
  Box-Muller unit Gaussian, scaled by strength. Mono shares a sample across RGB.
- Median / dust & scratches: square RGB median; dust replaces only differences
  greater than threshold, retaining lower-amplitude detail/noise.
- Pinch, spherize, twirl, wave, ripple, polar conversions, offset: genuine
  inverse maps and normalized Lanczos-3. See `distort.rs` for formulas,
  invertibility constraints and polar seam/center conventions.
- Emboss: diagonal derivative around 0.5; find edges: Sobel magnitude / 4;
  solarize: invert channels at or above threshold.
- Clouds / difference clouds: five octaves of seeded gradient Perlin noise;
  radius is lattice scale in pixels. Use a scale larger than one for clouds
  rather than sampling only lattice vertices. Difference uses abs(input-cloud).
- Oil paint and lens flare: explicit placeholders, errors when enabled rather
  than fake no-op implementations.
- Adjust: levels, monotone cubic curves, HSL, brightness/contrast (legacy and
  midtone-preserving), exposure, vibrance, photo filter, channel mixer, gradient
  map, nine-range selective colour, threshold, posterize, invert, desaturate,
  six-band black & white, global Oklab mean/std match colour. Identity controls
  preserve HDR; alpha is untouched. No public ML colour-match function exists
  in this checkout, so Oklab statistics transfer is implemented here.

`SmartFilters` evaluates ordered `SmartFilter` nodes with editable parameters,
enabled flags and optional same-size one-channel masks, always from an immutable
source. It does not change the compositor scene schema. Camera Raw is a separate
full-image `CameraRawFilter` with an injected `CameraRawProcessor`: the host
supplies its configured develop pipeline. The bare `Effect::CameraRaw` tag
rejects execution without that host context. It is not simulated by a LUT.

## Tiled execution

`Halo::Radius(n)` is a finite support contract. `apply_tiled` gathers independent
256-pixel interiors with that halo and writes only the interior. Halos greater
than engine-api's tile allocation limit are gathered in crate-local scratch;
engine-api remains unchanged. Point operations need zero halo; Gaussian needs
ceil(3*sigma), motion ceil(radius)+3, box/median/bilateral ceil(radius),
detail up to 9, Sobel/emboss 1.

`Halo::WholeImage` is an explicit scheduling barrier for transforms, radial and
lens blurs, global statistics, large-radius multiscale blur, and coordinate-
anchored procedural operations. Tiled evaluation delegates these to the global
path rather than claiming a finite halo and introducing seams. Gaussian and
other local kernels have independently executed whole-vs-halo regression tests,
including 36-pixel halos and all supported depths/channel counts.

## Large-radius approximation and bound

For sigma >32, reduction factor is 2 below 64, 4 below 128, and 8 through 250.
Extend the source by clamp padding at least ceil(3*sigma)+2*factor, box-average
factor-square cells, blur with sigma/factor, then bilinearly reconstruct using
cell-center coordinates. Padding is essential: reducing only the original
canvas would average away its edge values before Gaussian edge extension.
The GPU performs **all** reduction, convolution, reconstruction and final
blending in WGSL using exactly the same lattice.

For a particular sigma/phase let k be the normalized exact 1-D kernel and a the
composed area/blur/linear-reconstruction weights on the original lattice.
For samples in [m,M], the 2-D absolute approximation error is bounded by
`(M-m) * max_phase(sum(abs(a-k)))` (plus floating accumulation roundoff):
1-D zero-sum coefficient error gives half the L1 bound; tensor-product
expansion adds the two axes. Edge clamping only merges coefficients and cannot
increase L1. This is content-independent, including adversarial impulses.

The coefficient test evaluates **every phase** at sigma 32.1..250.0 in 0.1
increments and requires L1 <0.035. Additional raster tests cover 32.01, branch
boundaries, edges, impulses, steps, irregular texture and constants. Thus 0.035
of input range is the tested regression envelope, not a claim that a finite
parameter sweep proves every real-valued sigma. HDR bounds scale with range.
This approximation envelope is separate from the much tighter CPU/GPU parity
limit: both backends use the same approximation.

## GPU and verification

WGSL coverage: every blur above, unsharp, high pass, both noise distributions,
all eight distortions, and every adjustment. Spline slopes, lens kernel tap
lists and global match statistics are parameter preparation on CPU, not CPU
pixel evaluation. Other inventory entries explicitly reject GPU dispatch.
Metal adapter/device errors and storage-buffer limits are reported. GPU tests
require a real adapter and do not silently skip or replace the GPU with CPU.

Per-operation CPU/GPU max absolute error is gated at 1e-4, per docs/11 §1.3.
Tests include non-neutral parameters for every GPU family and adjustment,
alpha preservation, HDR samples, tiny dimensions, tile boundaries, large
Gaussian sigma through 250, invalid controls and cancellation. Small Gaussian
is also compared against direct 2-D scalar convolution within 1e-5. Distortion
forward/inverse maps round-trip independently on grids, including signed
radial limits, excluding polar singularities.

Run from the workspace with the external CARGO_TARGET_DIR retained:

    cargo test -p filters --release
    cargo clippy -p filters --all-targets -- -D warnings
    cargo fmt --check

The ignored CPU benchmark prints milliseconds per inventory entry for a
5000x4000 L0 raster and its 1250x1000 L2 raster. Placeholders and the uninjected
Camera Raw tag are reported as unavailable, not claimed as rendered filters.
It is a reference throughput report, not an interactive-latency guarantee:

    cargo test -p filters --release --test bench -- --ignored --nocapture
