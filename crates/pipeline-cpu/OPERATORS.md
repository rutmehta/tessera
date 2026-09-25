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

Recipes without M2 controls retain the original M1 execution path byte-for-byte.
Default sharpening (40/1/25/0) and default chroma NR (25/50/50) independently bypass
to preserve existing goldens, despite their nonzero schema amounts. Changed tuples
use absolute amounts. This compatibility discontinuity is explicit, not a claim
that amount 40 means zero in Lightroom. See MISSING_FIELDS.md.

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
  `1+v/(1+C/(.25*max(abs(L),.05)))*(1-.7*skinWeight(h))`.
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

No engine-api schema, stage order, or process revision has been changed. This is
the initial M1 reference, replacing a placeholder, not a revision of a shipped
renderer. Future changes to its rendering semantics require the contract's normal
process-version review.

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

Take the first three rows of the XYZ-to-camera `cam_xyz` matrix, invert it to N,
and normalize each inverse row: `M[i,j] = N[i,j] * D65_XYZ[i] / sum_j(N[i,j])`.
Thus camera `[1,1,1]` maps to D65 with Y=1. Singular matrices and nonpositive row
sums are errors. CameraProfile applies `inverse(Rec2020_to_XYZ) * M` to the
unbalanced camera RGB. A single calibration matrix is used; dual-illuminant
selection can later consult WhiteBalanceSettings without reordering stages.

For AsShot, scene white is `M * [1/mul_R,1/mul_G,1/mul_B]`, converted to xy.
Positive finite multipliers and a valid positive XYZ white are required.
Let A be `ChromaticAdaptation::Cat16.matrix(scene_white, D65)` and W be
Rec2020_to_XYZ. WhiteBalance applies `inverse(W) * A * W`, preserving the source
white's Y while neutralizing its chromaticity. This is not a second diagonal WB
in camera space. A synthetic grey patch verifies CameraProfile then WhiteBalance.

Custom white uses the standard piecewise Planckian-locus xy polynomial over
1667–25000 K, converted to CIE 1960 uv, with source `v += tint*0.00005` for tint
in -150..150. Positive tint corrects toward magenta. This is a Duv-like axis,
not a calibrated perpendicular distance to the locus. Presets: Daylight/Flash
D55, Cloudy D65, Shade D75, Tungsten A, Fluorescent F2. Preset temperature/tint
fields are informational per the contract; Custom makes them authoritative.

## Scene tone formulas

Exposure is `RGB *= 2^EV`, EV clamped to -10..10. With all other controls zero,
this is the entire stage: +1 exactly doubles samples, and all-zero is bit-exact
identity, including negative values. Non-finite controls return errors.
Other sliders are clamped to -100..100; this is not Adobe's tone model.

Compute Rec.2020 luminance `Y = .2627*R + .6780*G + .0593*B` after exposure.
For Y<=0, retain RGB. For Y>0, let `z = ln(1+Y/.18)` and `p = ln(2)`.
Contrast with `c = 2^(contrast/100)` changes z to:

    zc = c*z + (1-c)*2*p*(1-exp(-z))

It fixes black and the .18 pivot. Its derivative is positive for c in [.5,2],
and remains bounded, avoiding overflow from exponentiating a power of log Y.

Define `S(t)=max(t,0)+ln(1+exp(-abs(t)))`. For each region centre k,
`U(z,k)=S(z-k)-S(-k)` and `L(z,k)=z-U(z,k)`. Apply simultaneously:

    zo = zc + .2 * (blacks/100 * L(zc,.25)
                  + shadows/100 * L(zc,.8)
                  + highlights/100 * U(zc,1.5)
                  + whites/100 * U(zc,2.5))
    Yo = .18 * (exp(zo)-1)
    RGB *= Yo/Y

Every region derivative is between zero and one, so the combined derivative is
at least .2 even at slider extremes. Operations are global and continuous, with
no local edge filter that could create spatial halos. Luminance rescaling retains
RGB ratios. Tests exercise all extreme slider combinations over the raw headroom
range at +10 EV and check finite, monotone output.

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
A missing fixtures/golden/<stem>.png is created once, with an sRGB tag and notice.
Existing files are never overwritten: dimensions/format must match and maximum
absolute channel error must be <=2/255. Commit newly reviewed goldens; do not
regenerate existing ones merely to make a failing regression pass.
