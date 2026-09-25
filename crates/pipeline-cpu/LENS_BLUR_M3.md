# CPU layered Lens Blur (M3)

## Public API

```rust,ignore
lens_blur(
    image: &Image,
    near_to_far_depth: &[f32],
    settings: &engine_api::recipe::settings::LensBlur,
    options: LensBlurOptions,
) -> engine_api::EngineResult<Image>

render_linear_scaled_with_depth(
    settings: &engine_api::recipe::DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &LensContext<'_>,
    depth: &[f32],
    options: LensBlurOptions,
) -> engine_api::EngineResult<Image>
```

Both are exported at the crate root. The operator takes finite scene-linear
Rec.2020 RGB and one row-major depth value per pixel, normalized **near=0, far=1**.
Inverse-depth inference outputs must be converted upstream. No model is loaded;
`LensBlur.depth_model` is provenance only. Engine-api is unchanged.

The explicit renderer requires full-resolution **active-area** depth: RGB source
size, or RAW `metadata.default_crop` size. Depth is aligned before recipe Geometry,
not to the cropped/rotated/warped/downsampled output. The order is Locals → Lens
Blur → vignette/grain → Geometry (including lens warp) → downsample. Display is
not applied. Caller-owned lens profiles remain supported via `LensContext`.
This entrypoint also supplies depth to local depth-range masks. A supplied depth plane is
validated even when blur is absent; blur options are unused when it is absent.
Legacy renderers/validation and tile-only effects still reject Lens Blur rather
than silently ignoring it. They do not gain implicit model inference.

## Controls and validation

- Recipe amount: finite 0..=100; focus range: ordered inclusive 0..=1.
- Bokeh IDs: `circle` (recipe default) or `disc`, `hexagon` (6 blades), `octagon`
  (8 blades). Unknown IDs are errors, including when amount is zero.
- `LensBlurOptions.max_radius`: finite 0..=128 input pixels, default 16.
- `LensBlurOptions.layers`: 2..=64 uniform depth bins, default 16.
- `LensBlurOptions.boost`: finite 0..=100 percent, default 0.
- `LensBlurOptions.cat_eye`: reserved placeholder, default 0. Nonzero and
  nonfinite values are rejected, not silently accepted as a rendered effect.
- Depth must have the exact image pixel count, with every sample finite in 0..=1.
  Monochrome/nonfinite RGB is rejected. Inputs are immutable; errors are atomic.
- Amount zero or maximum radius zero returns an exact clone **after validation**,
  preserving signed zero and HDR bits. The inclusive focus band is always copied
  bit-for-bit at this operator's output (later effects/Geometry may change it).

## Reference algorithm

For each sample let `d = max(focus_near-depth, depth-focus_far, 0)`. Focus samples
are excluded from blurred layers, then copied exactly to output. Remaining depth
samples are assigned to uniform bins (`min(floor(depth*layers), layers-1)`).
Each occupied layer uses radius

```
radius = max_radius * amount/100 * mean(d in layer)
```

The discrete kernel samples integer pixel offsets inside a disc or regular
6-/8-sided polygon with that circumradius. Polygon edges have outward normals
at angles `2*pi*k/blades`; apothem is `radius*cos(pi/blades)`. Kernels always
include the origin. Subpixel radii may therefore be identity kernels.

At each destination, layer color is divided by the count of same-layer taps,
not by the full aperture. Layer alpha is occupied taps divided by in-image
aperture taps; image boundaries use truncated, renormalized support. Process
layers **far-to-near**, compositing premultiplied color and alpha with `over`.
Divide final premultiplied color by final coverage. This avoids black fringes
from empty/occluded layer samples and dark image borders, without leaking sharp
foreground colors into a blurred background. It is not a single global blur
followed by a depth-mask blend; near blurred layers cover farther blurred layers.

Boost uses Rec.2020 linear luminance Y. Out-of-focus source RGB is multiplied by
`1 + boost/100 * (1 - 1/Y)` for Y>1, otherwise by 1. Focus samples are never boosted.
Accumulation/compositing is f64 to handle finite HDR safely; final output saturates
only at ±f32::MAX, not to display range [0,1].

## Limits and verification

This deterministic scalar full-image reference uses O(pixels) scratch and
O(pixels * occupied_layers * radius²) work. It is not a real-time full-resolution
implementation. It approximates depth with bins and a mean radius per bin; it
does not synthesize hidden background, antialias aperture edges, smoothly
interpolate depth bins, or simulate cat-eye optics. Exact focus protection takes
priority over physically correct foreground bokeh covering an in-focus pixel.
Tiles must be assembled before applying it, avoiding tile-edge seams.

`tests/lens_blur.rs` covers two-plane checkerboard background contrast reduction
>50% with exact focus; disabled signed-zero/HDR identity; invalid inputs/options;
distinct normalized aperture impulse footprints; boost and reserved cat-eye;
constant/HDR edge normalization and sharp-foreground isolation; analytical
far-to-near coverage order; inclusive focus boundaries, tiny images and cross-tile
support; explicit-render ordering, missing depth, and absent-blur compatibility.
Core behavior was developed through observed RED → GREEN tests before implementation.

```sh
CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-06 cargo test -p pipeline-cpu
CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-06 cargo clippy -p pipeline-cpu --all-targets --no-deps -- -D warnings
```
