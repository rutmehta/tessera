# Adobe PV3–PV6 compatibility renderer (M2-18)

## Status and boundaries

This is an independent, approximate compatibility operator set, not Adobe code,
not an implementation of the private ACR algorithms, and not a measured claim of
Lightroom parity. No Lightroom reference exports were available. The tests prove
numerical behaviour and harness correctness, not perceptual matching to Adobe.
PV3–PV6 currently share these approximations; their proprietary version-specific
reconstruction differences are not simulated. Native edits continue to use the
native renderer. Neither engine-api nor image-core is changed.

`render_scaled(&DevelopSettings, &RenderSource, scale)` returns
`EngineResult<Rgb8Image>` (display sRGB). `render_linear_scaled` has the same
arguments and returns `EngineResult<Image>` in linear Rec.2020/D65 after edits,
including the profile tone curve, without sRGB encoding. The signatures mirror
pipeline-cpu, including integer area-reduction divisors and borrowed CFA/RGB
sources. A linear return is not a promise to omit nonlinear edits.

Settings do not contain a ProcessVersion. The caller selects the operator set.
CLI `render --process auto` reads the recipe version and selects this crate for
Adobe revisions 3 through 6 (`crs:15.4` is PV6). `--process adobe` explicitly
selects it; `--process native` explicitly selects the existing image-core path.
Standalone `_with_profile` variants add `Option<&DcpProfile>` as a fourth argument.
They require CFA data when a DCP is supplied. `render --process adobe --dcp file.dcp`
resolves an explicit user-supplied profile. No imported profile name is treated
as a filesystem path, and no licensed Adobe profile is bundled or extracted.

## Public sources and what they establish

1. [Adobe DNG 1.6 specification](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/dng_spec_1_6_0_0.pdf),
   camera profiles and colour rendering: TIFF tags/types, reciprocal-temperature
   matrix interpolation, ProPhoto HSV tables, table encodings and profile curves.
   See [DCP.md](DCP.md) for tag-by-tag details, page references and limits.
2. [Adobe DNG Profile Editor documentation](http://www.gapalmer.co.uk/WBC/DNGProfile_EditorDocumentation.pdf):
   a profile's base tone curve is separate from the user point curve; a linear
   user curve does not remove the base curve. Adobe Standard is camera-dependent,
   and Camera Raw Default is a distinct selectable base. There is no universal
   exact Adobe Standard curve recoverable from a profile display name.
3. [Adobe tone-control procedure](https://helpx.adobe.com/ie/lightroom-classic/help/tone-control-adjustment.html):
   intended target regions, exposure/midtone behaviour, highlight/shadow recovery,
   and white/black endpoint controls. This is user-facing behaviour, not source
   code or a published internal evaluation order.
4. [Russell Cottrell, Process 2012](https://russellcottrell.com/photo/BTDZS/process2012.htm):
   measured qualitative histogram behaviours; Whites stretches toward the right
   with black fixed, Blacks acts at the opposite end. Uniform patches do not
   predict Adobe's image-adaptive response on natural scenes.
5. [Adobe colour and tone adjustments](https://helpx.adobe.com/lightroom-classic/help/tone-color.html):
   saturation/vibrance, selective HSL and colour grading semantics. This does not
   disclose Adobe's colour coordinates, skin protection or wheel kernels.
6. [Sharma, Wu and Dalal, CIEDE2000](https://hajim.rochester.edu/ece/sites/gsharma/ciede2000/):
   equations, hue-angle edge cases and published numerical reference pairs.

All coefficients below are chosen approximations unless the source explicitly
specifies them. UI slider order must not be mistaken for proof of ACR's private
execution order. No proprietary binaries or Adobe implementation source are used.

## Stage order and assumptions

1. Native raw normalization/reconstruction, full-resolution demosaic, lens optics
   and WB. Demosaicing is never performed on a subsampled CFA. Without an explicit
   DCP, the native camera matrix substitutes for a named Adobe camera profile.
2. With an explicit DCP, undo the native profile/WB matrix before detail to recover
   camera RGB. Interpolate DCP matrices by reciprocal Kelvin, Bradford-adapt to
   D50, and apply HueSatMap then LookTable in linear ProPhoto HSV. Convert to
   Rec.2020/D65. A residual native CAT16 matrix approximates tint. AsShot resolves
   temperature using the existing native estimator, not Adobe's DCP-neutral solver.
   Native lens spatial warps happen before this substitution. This is a standalone
   integration compromise, not the eventual image-core graph implementation.
3. Detail once, using native scalar sharpening and manual luminance/chroma NR.
4. Basic tone: Exposure → Contrast → Highlights → Shadows → Whites → Blacks,
   all before the DCP/default tone curve. This is an assumed compatibility order.
5. Explicit DCP ProfileToneCurve, if present, in linear ProPhoto, once. Missing
   curves in an explicit DCP mean identity. Native Texture/Clarity/Dehaze and
   parametric tone adjustments follow. Without DCP, apply the default S-curve
   described below after these local-contrast operations.
6. User master point curve, per-channel curves, then optional luminance curve,
   in ProPhoto on a gamma-2.2 axis, not the native logarithmic axis.
7. Native approximations for Vibrance/Saturation, eight HSL bands, colour grading,
   and local adjustments. Effects (vignette/grain) and crop/angle follow, then
   linear-light box reduction with partial edge blocks. The native lens transform
   is already done in preprocessing and is not applied twice.
8. Linear Rec.2020 → linear sRGB → sRGB OETF and byte rounding. No second native
   sigmoid. Out-of-gamut channels clip at encoding; no Adobe gamut compressor,
   monitor ICC transform, dithering or HDR rendering is claimed. Both native
   gamut-mapping selections currently share this compatibility clipping policy.

### Basic tone formulas

Let Y = .2627 R + .6780 G + .0593 B. Exposure multiplies channels by 2^EV,
clamping EV to [-10,10]. With other sliders at zero, +1 exactly doubles linear
midtones before curves. Nonpositive Y is unchanged after this gain.

Contrast maps Y to `.18 * (Y/.18)^(2^(.5*C/100))`. This pivots at 18% grey,
retains HDR headroom and is an approximation to a contrast curve, not the exact
PV2012 contrast algorithm. Each later regional slider recomputes its weight
from the preceding output Y and multiplies Y by `2^(amount/100 * weight)`:

- Highlights: smoothstep(.25, 1, Y).
- Shadows: 1 - smoothstep(.02, .25, Y).
- Whites: constant .5, a white-point gain with zero fixed.
- Blacks: 1 - smoothstep(0, .08, Y).

Regional amounts clamp to [-100,100]. RGB receives the resulting Y ratio, so
these operations preserve channel ratios. Highlights -100 reduces bright
patches and leaves dark patches untouched. These are global, not spatially
adaptive recovery algorithms. They cannot recreate lost detail in clipped RAW
channels. Blacks approaches zero multiplicatively rather than implementing
Adobe's exact clipping threshold. Tests sweep ramps at both slider extremes
and assert direction/region behaviour. Narrow negative Whites weighting was
specifically avoided because it can reverse a grey ramp.

### Base and user curves

Without supplied DCP, use an independently chosen monotone cubic S-curve with
linear knots `(0,0), (.02,.012), (.08,.07), (.18,.22), (.5,.62), (1,1)`.
This is **Adobe-Standard-inspired**, not a copy or exact numeric reconstruction
of Adobe Standard/Camera Raw Default. It has a toe and increased midtone
contrast. Negative/above-one inputs clamp for this SDR base curve.

DCP curves are natural cubics per the documented DNG convention. User curves
are bounded cubic Hermite interpolation with harmonic interior slopes, applied
to `max(channel,0)^(1/2.2)` and decoded with power 2.2. The gamma, independent
channel application, end slopes, and interpolation choice are assumptions.
Strictly increasing normalized x and finite normalized y are required. Descending
y is allowed for creative point curves. Empty/identity curves bypass arithmetic;
above the last knot an identity-slope HDR tail is retained. The optional
luminance curve scales RGB by the mapped Rec.2020-style luma ratio in this
ProPhoto stage, an approximation rather than exact ProPhoto photometry.

### Remaining operator approximations / native fallback

These controls intentionally reuse the independently implemented native operators,
not no-ops. Exact equations and parameter/radius ranges remain in
[OPERATORS.md](../pipeline-cpu/OPERATORS.md), [TONE_M2.md](../pipeline-cpu/TONE_M2.md),
[COLOR_DETAIL_M2.md](../pipeline-cpu/COLOR_DETAIL_M2.md), and
[GEOMETRY_EFFECTS_M2.md](../pipeline-cpu/GEOMETRY_EFFECTS_M2.md):

- Texture/Clarity: guided fine/mid-scale local contrast with bounded extrema.
- Dehaze: dark-channel/estimated-airlight model, opposite blend for negative values.
- Parametric curve: native log-axis four-region adjustment and imported splits.
- Saturation: OkLCh chroma scale. Vibrance: stronger low-chroma enhancement with
  a skin-hue protection weight. Adobe's exact skin selector is not public.
- HSL: eight raised-cosine OkLCh bands, with hue/chroma/lightness adjustments.
  These are not Adobe's unknown HSL band centres or internal colour space.
- Split toning/colour grading: importer-mapped shadow/highlight/midtone/global
  wheels, balance and blending, using native weighted OkLab offsets. Legacy
  split toning is handled by the existing crs→grading mapping.
- Sharpening: amount/radius/detail/masking using Gaussian residual/Sobel gating.
  NR: luminance bilateral filtering with detail/contrast controls and chroma
  smoothing with amount/detail/smoothness. Not Adobe's wavelet filters.
- Vignette: native crop-relative priority modes, amount/midpoint/roundness/feather
  and highlight protection. Grain: deterministic noise with amount/size/roughness.
- Crop and angle: native normalized crop and Lanczos resampling; final area
  reduction is after all effects. Imported camera orientation is handled by CLI.

Settings outside the compatibility overrides remain in native settings and use
native implementations/validation. Unsupported native operators fail explicitly
rather than pretending to render. Opaque unknown XMP is preserved by sidecar and
import-lrcat, but the typed DevelopSettings API cannot execute unknown XMP keys.
Named camera profiles without supplied bytes use the native matrix plus fallback
curve. Profile amount/calibration/creative looks remain unsupported nondefaults,
not silently interpreted as a DCP. No auto-download or profile-name path lookup.

## Fidelity harness

`tessera import lrcat catalog.lrcat --fidelity --reference-dir exports` performs a
read-only audit. Exports must be sRGB JPEGs named by catalog image ID, e.g.
`30.jpg`, preserving separate virtual-copy references. Use Lightroom export,
not the camera's embedded JPEG (which does not include Lightroom edits).
This package chooses the user-supplied-export branch of the requested harness;
it does not parse the proprietary Previews.lrdata cache.

The compat RAW render is evaluated at scale 4. References may already have that
exact extent, or may be full-sized with identical crop/orientation. Full references
are dimension-checked against a full render and triangle-downsampled to the
quarter extent. This is encoded-sRGB resampling, not Adobe export resampling;
filter/quantization discrepancies can contribute to the measured error. No image
registration, arbitrary resizing, or non-sRGB ICC conversion is done. Export
matching framing and sRGB colour explicitly. Missing files are skipped with a
reason; decode/settings/dimension errors produce failed rows without numeric
scores. `compared=0` is not a passing fidelity result. Each compared row reports
pixel count, mean and exact nearest-rank p95 ΔE2000, with unit weighting factors.

`pipeline_adobe::fidelity::compare` requires matching nonempty sRGB images. It
converts sRGB → XYZ D65 → Lab D65, computes the full CIEDE2000 formula including
hue wrap and achromatic cases, and sorts values for p95. Tests include published
Sharma pairs in both directions, an identical synthetic native-pipeline render
with zero error, known Lab anchors, dimension errors and percentile aggregation.
There is no real Lightroom fidelity score or asserted threshold in this package.

## Needed engine-api / future integration fields (not changed here)

- A content-addressed DCP handle/resolver and resolved camera-profile bytes;
  current CameraProfileRef carries only name/digest, not a path or byte payload.
- DCP per-image calibration/analog balance, explicit neutral/xy with tint, profile
  baseline exposure/default-black hints and profile identity/matching rules.
- Separate base-profile tone selection, renderer revision within Adobe family,
  and a fidelity provenance record identifying reference colour space/export size.
- B&W band mixer and grain seed if complete Lightroom round-trip coverage is needed.
- image-core process-version dispatch and profile context belong to the later
  package. This crate/CLI makes no changes to its concurrently edited graph.

## Verification

Run from the worktree with CARGO_TARGET_DIR outside the repository:

```
cargo test -p pipeline-adobe -p tessera-cli --release
cargo clippy -p pipeline-adobe -p tessera-cli --all-targets -- -D warnings
cargo fmt --check
```

Tests exercise both TIFF endiannesses, malformed/truncated/mutated profiles,
DCP matrix/table/curve order, CFA-to-final-render profile integration, the basic
operator directions and ramp order, per-channel user curves, native fallback
parameter plumbing, crop/angle/area-reduction order, CLI routing/flags, and the
fidelity metric. They do not replace comparison against real Lightroom exports.
