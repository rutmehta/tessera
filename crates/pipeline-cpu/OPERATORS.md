# Milestone 1 and 2 scalar reference

All pixel arithmetic is scalar `f32`, with no explicit SIMD or fused multiply-add.
Colour matrices are composed/inverted with engine-api's `f64` algebra, then baked
once to `f32` coefficients. This is a CPU reference, not a fast preview renderer.
Floating-point transcendental functions may differ slightly across platforms;
the encoded regression tolerance is two 8-bit code values, not bit identity.

## Entry points, types and ordering

`render(&DevelopSettings, &RenderSource) -> EngineResult<Rgb8Image>` renders at
full resolution. `render_scaled(..., scale)` applies the same full-resolution
operators, crops the active sensor area, and box-averages linear RGB in `scale`
square blocks before display. Partial edge blocks use their actual sample count.
The golden suite uses scale 8. It never subsamples a CFA directly.

`RenderSource::Cfa { image: &raw_decode::CfaImage, metadata: &RawMetadata }` takes
linearized sensor samples and metadata retrieved after decode. `RenderSource::Rgb`
takes an `Image` already in scene-linear Rec.2020 / D65; it skips raw reconstruction,
demosaic and CameraProfile. AsShot is identity for this RGB input; explicit white
balance interprets its source illuminant in XYZ. `Rgb8Image` aliases `image::RgbImage`.

`Image` is a validated helper holding one or three planes and dimensions. Operators
use engine-api `Tile` planes. Point operators preserve halos; neighbourhood
operators return only the tile interior. `Image::tile` gathers neighbours across
tile boundaries, and pads image edges with the nearest sample of the same CFA
phase (period 2 for Bayer, 6 for X-Trans, ordinary clamp for RGB). Bayer images must
contain at least a complete 2x2 period, X-Trans at least 6x6.

The contract order is unchanged:

1. Decode and normalization in raw-decode.
2. Linearize-stage highlight handling on the CFA.
3. Demosaic to camera RGB.
4. CameraProfile to XYZ, then linear Rec.2020.
5. WhiteBalance as a working-space CAT16 transform.
6. Active-area extraction for M2, then Detail sharpening/manual NR.
7. Tone (basic controls, Texture/Clarity, Dehaze, parametric and point curves).
8. Color (OkLCh vibrance/saturation, HSL and grading).
9. Effects in final crop-relative coordinates, pulled back through straighten.
10. Geometry: single crop/straighten inverse map with Lanczos-3.
11. Linear-light box downsampling; Output sigmoid, sRGB gamut mapping/OETF/dither.

Unimplemented stages remain no-ops only at their defaults. Non-default unsupported
controls return errors: lens/Upright/orientation, AI denoise, locals, Point Color,
LUT, lens blur, calibration/DCP/looks, HDR/proofing and Auto WB. Native and Sigmoid
use the same existing output transform. B&W mix and configurable grain seed are
absent from the schema: see [MISSING_FIELDS.md](MISSING_FIELDS.md).

Native revision 2 applies default sharpening (40/1/25/0) and chroma NR (25/50/50).
Only zero amounts disable these operators; unchanged tuples have no special
bypass. CPU reference, image-core and GPU use the same activation rules.
The synchronous reference renderer distributes independent Detail tiles over
at most eight scoped workers (bounded by available parallelism). Every tile
reads the same immutable full-resolution source with real-neighbour halos;
only copying finished interiors is serialized. Pixel arithmetic, operation
order and downsampling are unchanged. A multi-tile/edge regression checks
bit-exact output against one worker, and the existing preview latency test
continues to enforce its three-second release limit without bypassing detail.

`image-core::StageOp` dispatches Detail, Tone, Color, Effects, Geometry and Output.
Its additive `run_image` method supplies whole-image ToneExtra/Geometry CPU
fallbacks because Tile interiors cannot exceed 256 pixels. Detail gathers real
halos from immutable source images; dehaze estimates airlight once per whole
image, never independently per tile. `PipelineGraph::m2()` marks the added stages
implemented; only upstream Demosaic/WB outputs are memoized. Effects uses crop
parameters but is not cached under an Effects-only hash. No later M2 output is
cached, so crop/rotation edits cannot reuse stale crop-relative effects.

The reference evaluates M2 at full resolution before box downsampling. Interactive
image-core evaluates expensive M2 passes on the complete requested-level WB buffer
(like its existing nonneutral preview Tone path), then emits requested tiles.
This is a scalar correctness reference, not a memory-bounded streaming solution.
Preview neighbourhood radii are level pixels; preview grain is not an exact
area-filtered full-resolution rendition. At level 0 the two CPU paths agree.

## M2 formulas

Detailed constants, validation, halo support, matrices and test contracts:
[Tone](TONE_M2.md), [Color/Detail](COLOR_DETAIL_M2.md),
[Geometry/Effects](GEOMETRY_EFFECTS_M2.md). Summary below is normative for ordering.

- Log tone axis: `E(Y)=ln(1+max(Y,0)/.18)/ln(1+1/.18)`, inverse
  `D(z)=.18*expm1(z*ln(1+1/.18))`; positive HDR is not clipped to white.
- Guided filter: `a=cov(I,p)/(var(I)+.001)`, `b=mean(p)-a*mean(I)`,
  `G=mean(a)*I+mean(b)`. With `F=G1(z,z), M=G3(z,z), B=G8(z,z)`,
  Texture/Clarity use `delta=(texture/100)*(F-M) + (clarity/100)*4u(1-u)*(M-B)`.
  Constrain `z+delta` to source 3x3 extrema to avoid new halo extrema, then scale
  RGB by `D(z')/Y`. Texture excludes the finest noise band. Maximum local support
  is 16 pixels; the complete Tone path uses whole-image storage.
- Dehaze: radius-3 RGB dark channel; airlight is channel medians from top-dark-channel
  candidates excluding top-1% luminance outliers. Clamp its chromaticity near neutral.
  Low percentile contrast suppresses ambiguous white/snow scenes. Transmission
  `t=guided(clamp(1-.85*abs(dehaze)/100*confidence*dark(I/A),.15,1))`, clamped again.
  Positive amount gives `A+(I-A)/t`, negative gives `t*I+(1-t)*A`.
- Parametric: in each log-axis split interval `[a,b]`, `u=(z-a)/(b-a)` and
  `z'=z+3*s*(b-a)*u²*(1-u)²`. Four normalized region sliders are `s`; this has
  positive derivative even at extremes and joins with continuous first derivative.
  Point curves use Fritsch–Carlson monotone cubic Hermite interpolation directly,
  master RGB then channel RGB then luminance, all on the same log axis. Reject
  unordered/descending knots. Identity curves bypass; HDR tails continue linearly.
- Color: signed Rec.2020→OkLab matrix/cube-root conversion, `C=hypot(a,b)`,
  `h=atan2(b,a)`. Saturation scales C by `1+s`; Vibrance scales it by
  `1+v*muted*(1-shield)` with `S=C/(.25*max(abs(L),.05))`, `muted=1/(1+S²)` and
  `shield=.7*skinWeight(h)*(S/.3)²/(1+(S/.3)²)` (see COLOR_DETAIL_M2.md).
- HSL: centres `[25,55,95,145,195,255,295,335]` degrees. Raised-cosine weights
  on adjacent bands partition unity and wrap smoothly. Weighted normalized sliders
  rotate hue by up to 30 degrees, scale C by `1+sat`, L by `1+.5*lum`.
- Grading: normalized Gaussian weights around `[0,.5,1]` in balanced L;
  width `.15+.5*blending/100`, balance offset `.25*balance/100`. Wheels add
  `.2*sat/100*weight*min(abs(L),1)*(cos(h),sin(h))` to a/b and
  `.25*lum/100*weight` to L. Global weight is one.
- Sharpening: Gaussian radius/sigma, `r=Y-G(Y)`, Detail blends a bounded residual
  and `1.5*r`; Amount scales the result. A smoothed Sobel magnitude gates Masking.
- Luminance NR: bilateral radius 2, Gaussian spatial sigma 1.2, range width set by
  Luminance Detail; Contrast suppresses smoothing in high-variance regions.
  Chroma NR smooths a/b in OkLab using chroma and L range weights, radius 1–5
  from Smoothness. Amount blends residuals. Linear Y is restored on recombination.
- Vignette: superellipse exponent `p=2+3*(1-roundness/100)` in crop coordinates,
  midpoint `.05+.9*midpoint/100`, smoothstep feather. Highlight Priority multiplies
  linear RGB by `2^(2*a)` with highlight protection; Color Priority changes CIE Lab
  L retaining a/b; Paint Overlay blends toward black/white.
- Grain: fixed-seed smooth lattice noise, two octaves weighted by Roughness;
  Size maps to `.5+7.5*size/100` pixels, amplitude is
  `(0.025+.075*roughness/100)*amount/100`. Equal increments in RGB are achromatic.
- Geometry: output dimensions `max(1,round(crop_fraction*source_dimension))`.
  Inverse rotation around the crop centre plus crop translation/scaling is sampled
  once with separable normalized `sinc(x)*sinc(x/3)`, support 3. Outside-source
  centres are black; boundary taps clamp. No Upright/lens warp is implied.

M2-04b bumps the native process revision to 2 without changing schema or stage
order. Old serialized process versions remain intact and do not alias new cache
keys. The operator entry points take settings, not a revision: they are not an
implementation of a historical revision-1 renderer.

## Linearize / highlight handling

raw-decode computes `clamp((sample - black[channel]) / (white - black[channel]),
0, 1.2)`. `inverse_linearize(v,b,w) = v*(w-b)+b` is the inverse only on the
unclamped portion. It intentionally returns a float, without sensor quantization.

Clip mode applies `min(v, 1)`. ReconstructColor leaves unclipped samples unchanged.
For a clipped sample, its proxy is the mean of unclipped other-colour samples in
the surrounding 3x3 neighbourhood. Unclipped same-colour donors in a 7x7 window
supply ratios `donor / donor_proxy`, with the proxy measured in the donor's 3x3
neighbourhood. The output is `clamp(target_proxy * mean(ratios), 1, 4)`.
The input is frozen for the entire pass (no scan-order feedback); support is a
four-pixel halo. With no usable donor/proxy, including fully clipped regions,
output is 1. This is deliberately limited colour propagation, not recovery of
missing physical detail. ReconstructLch and Inpaint return errors.

## Demosaic

Bilinear averages the nearest samples of each missing colour in 3x3: four axial
greens at red/blue, four diagonal opposite-colour samples, and two horizontal or
vertical red/blue samples at green. Known samples are unchanged. Green channel 3
is treated as green channel 1 after decoder black-level normalization.

MHC is independently implemented from H. S. Malvar, L.-w. He and R. Cutler,
“High-quality linear interpolation for demosaicing of Bayer-patterned color
images,” ICASSP 2004, equations 2–5 and Figure 2.
DOI: https://doi.org/10.1109/ICASSP.2004.1326587
Paper: https://home.cis.rit.edu/~cnspci/references/dip/demosaicking/malvar2004.pdf

Let C be the known centre sample, H1/V1 the sums at horizontal/vertical distance
one, H2/V2 at distance two, and D the four diagonal samples at distance one.
Every expression below is divided by eight:

- Green at red/blue: `4*C + 2*(H1+V1) - H2 - V2`.
- Red at blue / blue at red: `6*C + 2*D - 1.5*(H2+V2)`.
- Red/blue at green, wanted colour in same row:
  `5*C + 4*H1 - H2 - D + 0.5*V2`.
- Wanted colour in same column: transpose H and V in the previous expression.

These are the paper's 5x5 gradient-corrected kernels, with alpha=1/2, beta=5/8,
gamma=3/4. Negative estimates and headroom remain floats; no clipping is done in
this stage. Tests cover every kernel tap for all Bayer phases, not just flat fields.
Bayer requires halo >=2. `DemosaicMethod::Auto` resolves to MHC for Bayer, and
`Bilinear` selects bilinear. The public `DemosaicAlgorithm::MalvarHeCutler` selects
MHC directly at the operator level, without adding an engine-api enum variant.
Other named methods are rejected, never relabeled as MHC.

X-Trans uses a documented placeholder regardless of the two supported methods:
retain known samples, average each missing colour within 3x3. If that colour is
absent, use its mean in 7x7 (halo >=3). A 6x6 pattern is indexed in full sensor
coordinates, including across 256-pixel tile seams. This is not Markesteijn.

Licensing: all pipeline implementation code is original Apache-2.0 code derived
from the mathematical paper, not copied from darktable, RawTherapee, RCD, AMaZE,
LMMSE or IPOL implementation source. LibRaw remains under the repository's CDDL
selection. This document does not offer a new legal opinion about patent status.

## CameraProfile and white balance

libraw-ffi and raw-decode expose `cam_xyz: [[f32;3];4]` and
`rgb_cam: [[f32;4];3]` as independent verbatim copies of the original LibRaw fields.
No matrix is reverse-engineered from the other. The legacy `camera_to_xyz` field
remains available for existing clients; this pipeline does not use it.

Take the first three rows of the calibrated XYZ-to-camera `cam_xyz` matrix and
invert it to M. Do not independently normalize XYZ rows: unbalanced camera
`[1,1,1]` is not a D65 neutral. That old normalization distorted chromaticities
and pushed fixture whites outside the tint domain. Singular/nonfinite matrices
are errors. CameraProfile applies `inverse(Rec2020_to_XYZ) * M` to the
unbalanced camera RGB. A single calibration matrix is used; dual-illuminant
selection can later consult WhiteBalanceSettings without reordering stages.

For AsShot, scene white is `M * [1/mul_R,1/mul_G,1/mul_B]`, converted to xy.
Positive finite multipliers and a valid positive XYZ white are required.
Let A be `ChromaticAdaptation::Cat16.matrix(scene_white, D65)` and W be
Rec2020_to_XYZ. WhiteBalance applies `inverse(W) * A * W`, preserving the source
white's Y while neutralizing its chromaticity. This is not a second diagonal WB
in camera space. A synthetic grey patch verifies CameraProfile then WhiteBalance.

Custom white uses the smooth Krystek (1985) rational Planckian-locus approximation
in CIE 1960 uv over 1667–25000 K. Tint is `3000 * Duv`: ±150 corresponds to
±0.05 perpendicular distance, not a vertical v offset. The signed unit normal
is `(dv/dT, -du/dT) / hypot(du/dT,dv/dT)` (green-positive). Positive tint therefore
corrects toward magenta. This chosen Lightroom-like scale is not Adobe parity.
`as_shot_temperature_tint` derives xy from the same matrix/multiplier white as
AsShot, then uses a Robertson-style isotherm search with 60 reciprocal-temperature
bisections. It returns unrounded f32 coordinates, rejecting out-of-domain whites
instead of clamping them and falsely promising identity. The locus approximation
is shared in both directions. Reference coefficients:
https://colour.readthedocs.io/en/master/_modules/colour/temperature/krystek1985.html

All five fixture whites fit the domain with the calibrated inverse; fixture
tests cover as-shot identity within 1e-4, first-touch continuity, monotonic
+500 K warming, and +50 tint toward magenta from each as-shot coordinate.
The FFI uses this same unrounded inverse for slider initialization. A first
single-slider edit from AsShot seeds the other coordinate from that inverse,
not the recipe's informational default. Its Basic adapter passes through
Texture, Clarity, Dehaze, Vibrance and Saturation.
Presets remain Daylight/Flash D55, Cloudy D65, Shade D75, Tungsten A, Fluorescent F2.
Preset temperature/tint fields are informational; Custom makes them authoritative.

## Scene tone formulas

Exposure is `RGB *= 2^EV`, EV clamped to -10..10. With all other controls zero,
this is the entire stage: +1 exactly doubles samples, and all-zero is bit-exact
identity, including negative values. Non-finite controls return errors.
Other sliders are clamped to -100..100; this is not Adobe's tone model.

Compute Rec.2020 luminance `Y = .2627*R + .6780*G + .0593*B` after exposure.
For Y<=0, retain RGB. For Y>0, work on the log axis `u = ln(Y/.18)` (one stop
is `ln 2`; `u=0` at 18% grey). The stage is a pure luminance gain
`RGB *= G(Y)` with `G = exp(shift + stops*ln 2)`, evaluated by
`tone_math::tone_gain` and its twin `tone_gain` in
`pipeline-gpu/src/operators.wgsl` (PL-1 calibration, 2026-10).

Contrast with `c = 2^(contrast/100)` and `K = 1.5*ln 2`:

    shift = (c-1)*K*tanh(u/K)
    u1 = u + shift

The log-slope at the pivot is exactly `c` (×2 at +100, ×0.5 at −100), the
slope returns to 1 away from it, and the far shadows/highlights move by at most
`(c-1)*K`: ∓1.5 stops at +100, ±0.75 at −100. Black stays black because the
gain is multiplicative.

Each region control adds a feathered number of stops to `u1`:

    stops = Σ amount/100 * range * w(u1)
    w(u1) = σ(±(u1 - centre*ln 2)/width),  σ(x) = 1/(1+exp(-x))

with `+` for Highlights/Whites (the band is above the centre) and `−` for
Blacks/Shadows. The bands (`tone_math::REGIONS`, in evaluation order):

| control    | range (stops at ±100) | centre (stops from grey) | width (ln) |
|------------|----------------------:|-------------------------:|-----------:|
| Blacks     | 1.25                  | −3.5 (Y≈.016)            | .55        |
| Shadows    | 1.5                   | −2.0 (Y≈.045)            | .75        |
| Highlights | 1.6                   | +1.25 (Y≈.43)            | .55        |
| Whites     | 1.0                   | +2.75 (Y≈1.2)            | .6         |

Shadows +100 lifts Y=.0065 by 1.4 stops and Y=1 by .02; Highlights −100 drops
Y=1 by 1.3 stops and Y=.02 by .01; mid grey moves .2–.27 stop at ±100 of
either. Response is linear in the amount: +20 is a fifth of +100.

Both backends clamp the logistic argument to ±60 and evaluate tanh as
`(1-t)/(1+t)`, `t=exp(-2*min(abs(x),20))`, so the operation order and
rounding match. Clamp and divide each amount by 100 on the host. The
exposure-only/neutral and `Y <= 0` bypasses are the same on both sides. No
luminance floor is used; `G` is a bounded exponential of bounded terms, so
near-zero signed luminance never cancels.

Every feather derivative is bounded by `range/(4*width)`, and the bands are
spaced so the combined log-slope stays above .24 at every combination of
slider extremes (worst case Blacks/Shadows +100 with Highlights/Whites −100
and Contrast −100). Operations are global and continuous, with no local edge
filter that could create spatial halos. Luminance rescaling retains RGB
ratios. Tests exercise all extreme slider combinations over the raw headroom
range at +10 EV and check finite, monotone output; `tests/pl1_response.rs`
pins the measured response on the `sample.dng` fixture.

History: until PL-1 the regions were `.2*amount*L/U(z,k)` integrals on
`z=ln(1+Y/.18)` whose lower regions saturated to a constant offset above
their band, so Shadows ±100 moved the shadow quartile .19 stop and the
highlight quartile .27, Highlights ±100 moved highlights .15 stop and
Contrast ±100 changed global log-contrast by ×1.2/×.92. See
`docs/design/evidence/PL-1/`.

## Display

A generalized log-logistic sigmoid supplies contrast and skew:

    p = clamp(contrast,.25,4), q = 2^clamp(skew,-1,1)
    a = .18 * (.18^(-1/q)-1)^(1/p)
    f(x) = (1 + (a/x)^p)^(-q), x>0; f(x)=0 for x<=0

The implementation evaluates the logistic form in log space. It maps black to
zero, .18 to .18, is monotone, and asymptotes to 1. `SigmoidSettings` exposes these
kernel parameters for tests/future transforms. Because the current recipe has
no display-contrast/skew fields, `render` fixes them to contrast=1.5 and skew=0;
changing the standalone kernel does not introduce hidden recipe state. This is
inspired by sigmoid display rendering, not a port or exact darktable match.

Map Rec.2020 luminance with f, rescale RGB by f(Y)/Y, then apply the linear
Rec.2020-to-sRGB matrix. Perceptual gamut mapping compresses chroma along the ray
from the achromatic luminance to RGB until all three channels fit [0,1]. Clip mode
instead hard-clips. This limited RGB-ray mapper is not a full perceptual CMM.

The sRGB OETF is `12.92*x` below .0031308, else `1.055*x^(1/2.4)-.055`.
A fixed 4x4 Bayer ordered dither adds `(rank+.5)/16-.5` in 8-bit code units before
rounding and final clamping. It uses global output coordinates and the same noise
for all channels, so grey stays grey and tile seams are deterministic.

## Golden tests

Every CR3, ARW, NEF, RAF and DNG in fixtures/raw is decoded and rendered with
DevelopSettings::default at scale 8. The suite requires each of the five formats
when the directory exists. If the directory is absent, it prints a skip notice.
`PIPELINE_RAW_FIXTURES` can point at another fixture directory for testing this.
Regression tests never write goldens: dimensions/format and encoded bytes must
match exactly. The explicit revision-2 update command is
`cargo run -p pipeline-cpu --release --example regenerate_goldens`. It writes all
five scale-8 PNGs with sRGB tags. Do not regenerate for unrelated regressions.
