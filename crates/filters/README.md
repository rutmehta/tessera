# Layered-editor filters (M5-06)

CPU reference and real WGSL/Metal implementations over the published
`compositor::raster::Raster` API. No compositor or engine-api changes.

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

Optional feature `camera-raw-filter` enables `camera_raw`, a **tone-only raster
stub**, not RAW decoding or demosaicing. Its flat parameter object accepts
`exposure` (-10..10), `contrast`, `highlights`, `shadows`, `whites`, `blacks`
(each -100..100), and `amount` (0..1, default 1). It invokes
`pipeline_cpu::tone` with `ToneSettings` on RGB tile data and preserves alpha.
Without the feature, `camera_raw` returns `Unsupported`.

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
