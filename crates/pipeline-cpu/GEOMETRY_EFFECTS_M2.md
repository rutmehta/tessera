# M2 CPU geometry and effects

> M2-09 adds lens composition, Upright and manual transforms; see [LENS_M2.md](LENS_M2.md).
> The crop-only formulas and f32 sampling below remain unchanged. Extended optics/homography
> coordinates use f64. Historical exclusions of Upright/manual transforms below are superseded.

Implemented in `src/geometry_effects.rs`. Scalar f32 reference math throughout (maps, kernels, accumulation, Lab, vignette, and grain), not Adobe pixel matching. Public reference rendering and image-core StageOp assemble geometry's full source image and split its changed-size result into tiles.

## Contracts and stage ordering

- `geometry(&Image, &GeometrySettings) -> EngineResult<Image>` accepts one or three finite planes. The existing API is retained. No `geometry_tile` adapter is supplied: `engine-api::Tile::check_layout` rejects interior dimensions greater than `TILE_SIZE` (256), so a Tile cannot currently represent an arbitrary full image. Parent integration must assemble an `Image`, or first extend the Tile contract outside this module.
- `effects(&mut Tile, &EffectsSettings, Extent) -> EngineResult<()>` requires finite f32 linear Rec.2020 RGB. `Extent` is the **full level-0 image domain**, not the tile extent or the already-downsampled level extent.
- `StageId` specifies Effects (11), Geometry (12), Output (13). Both renderers preserve this ordering. `effects_in_crop(tile, settings, extent, crop)` pulls source pixel coordinates into the rotated crop frame before evaluating effects. For source displacement `(dx,dy)` from crop centre, `u=(cos*dx-sin*dy)/cropWidth+.5`, `v=(sin*dx+cos*dy)/cropHeight+.5`. Grain uses crop dimensions, not sensor dimensions. Geometry then resamples once. Thus the vignette follows the final crop without changing StageId order; discontinuous masks and grain are resampled along with the image. Never normalize each tile independently. The simpler `effects` entry point is a full-crop wrapper.
- Default geometry clones samples exactly, including signed zero. Zero vignette/grain bypass performs no mutable tile access and retains shared storage. Validation still occurs.
- docs/01 §2.11 describes effects; §2.12 actually describes calibration, not geometry. Calibration is outside this module.

## Geometry formulas

Rectangle edges are normalized source **edge** coordinates. Let source size be `(W,H)` and crop edges `(l,t,r,b)`:

```
cw = (r-l) W; ch = (b-t) H
outW = max(1, round(cw)); outH = max(1, round(ch))
c = ((l+r) W/2, (t+b) H/2)
d = ((x+0.5) cw/outW - cw/2, (y+0.5) ch/outH - ch/2)
sourceSample = c + [cos(theta) sin(theta); -sin(theta) cos(theta)] d - (0.5,0.5)
```

Positive angle rotates displayed content clockwise in top-left-origin coordinates. Rotation is about the crop center. Crop and straighten are composed **before a single resampling pass**; output dimensions do not expand with angle. Source-center coordinates outside `[-0.5,W-0.5) × [-0.5,H-0.5)` produce black. Within that domain, taps beyond source bounds clamp to the nearest edge sample.

Each destination uses a separable 6×6 Lanczos3 footprint, normalized by its total weight: `L(x)=sinc(x)*sinc(x/3)` for `|x|<3`, zero otherwise, `L(0)=1`. Accumulation, maps, and output all use f32. Negative/HDR values and normal Lanczos ringing are retained, not clipped to [0,1]; values outside finite f32 range saturate only at ±f32::MAX.

Limits:
- Rectangle must be strictly ordered and inside [0,1]; straighten finite and within ±45°.
- Crop aspect is a UI locking hint; nonzero entries are validated, but the explicit rectangle is authoritative.
- Nonidentity orientation, Upright/guides, manual transform, and constrain-crop return `Unsupported`, never silently disappear. Lens distortion is not part of this M2 API.
- No automatic rotation zoom, transparent border, perspective, output resize, or footprint widening for arbitrary downsampling. Full-image reference storage is used, not lazy tile warping.

## Effects coordinates and vignette

At level `L`, use `E = extent.at_level(L)` (ceil division, matching engine-api). Global pixel is `(tile.x*256 + localX, tile.y*256 + localY)`. Halo coordinates outside the image clamp to its edge. Normalize pixel centers by `E`: `u=(gx+0.5)/E.width`, `v=(gy+0.5)/E.height`. Thus shared halo/interior pixels agree exactly and the vignette does not restart at seams.

All finite sliders clamp to their documented ranges. With `S(t)=clamp(t,0,1)^2*(3-2*clamp(t,0,1))`:

```
p = 2 + 3*(1-roundness/100)       # 8 (box-like) to 2 (ellipse)
rho = (|2u-1|^p + |2v-1|^p)^(1/p)
m = 0.05 + 0.9*midpoint/100
f = feather/100
mask = S((rho-m)/(f*(1.5-m)))     # f=0: hard step at m
Y = 0.2627 R + 0.6780 G + 0.0593 B
protection = 1 - highlights/100*S(Y)  # darkening only; otherwise 1
a = amount/100 * mask * protection
```

Styles:
- **Highlight Priority:** linear-light exposure, `RGB *= 2^(2*a)`; unbounded highlights survive. Highlights slider protects bright pixels from darkening.
- **Color Priority:** convert linear Rec.2020 → XYZ D65 → CIE Lab; retain a*, b* exactly in perceptual math, change normalized L* toward black for negative `a` or white for positive `a`, then invert. No gamut clipping; this may create negative RGB.
- **Paint Overlay:** `RGB*(1-|a|) + target*|a|`, target black/white by sign.

Highlight protection is explicitly supported for darkening in all three styles. Zero local mask skips the color transform to avoid neutral-region round-trip drift. Lab uses f32-rounded matrix coefficients. Extreme dot products use power-of-two scaling to avoid infinity cancellation; overflowing Lab intermediates saturate to ±f32::MAX before subsequent operations. This preserves finite output for extreme positive, negative, and mixed-sign RGB, but does not promise exact perceptual chroma preservation at numerical saturation.

## Grain

No recipe seed exists; the immutable implementation seed is `0x5445535345524132`. A stateless wrapping integer hash, cubic-smoothed bilinear lattice interpolation, and two octaves produce deterministic noise, independent of tile iteration order, thread order, and halo size.

```
sizePixels = 0.5 + 7.5*size/100
q = (u*extent.width, v*extent.height)/sizePixels
rough = roughness/100
noise = (N(q) + 0.5*rough*N(2*q+(19,7))) / (1+0.5*rough)
delta = noise * (0.025+0.075*rough) * amount/100
RGB += (delta,delta,delta)
```

Grain is achromatic additive luminance noise (equal channel increments), not independent RGB noise. Larger size interpolates across larger source-pixel distances; roughness adds fine structure and contrast. Reference coordinates keep the same continuous texture anchored across preview levels. Coarse levels sample this texture rather than analytically integrating it, so previews are **not** exact area-filtered versions of full-resolution grain. It is not a physical film-density model. Grain can produce negative values and does not clip HDR.

Lens blur returns `Unsupported` even if present with zero amount; it needs a depth model and is not falsely reported as rendered. Parameters, extent, channels, format, and finite input are checked before mutation; errors leave the tile untouched.

## Verification

Tests were run against initial `todo!` implementations and observed failing; basic bypass/crop then passed. Straighten/fractional sampling and active-effect tests were then observed failing before implementation. Nine final module tests pass, covering exact default storage, integer/fractional crop, rotation, black borders, tiny input, invalid settings, three distinct vignette styles and every vignette/grain field, repeatable grain, tile/halo seams, pyramid level normalization, finite extreme HDR, and atomic errors. The added scalar-f32 Lanczos regression was observed failing against the original implementation (0.76993394 versus 0.769934), then passing after conversion. A new extreme mixed-sign Lab regression also failed before overflow-safe f32 arithmetic was added.

The module is registered and its unit tests run under Cargo. Whole-render tests additionally verify crop-relative effects, default bit-identical goldens, cross-tile agreement, and finite extreme level-3 renders of the RAW fixtures. Run the module tests with:

```
CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-04 cargo test -p pipeline-cpu geometry_effects --lib
```
