# Milestone 1 scalar reference

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
6. Tone adjustments, still scene-linear.
7. Active-area crop / linear-light downsampling.
8. Output sigmoid, Rec.2020 to sRGB, gamut mapping, OETF and dither.

Other stages are explicitly not implemented in M1. Their default settings are
no-ops, including the contract's default lens corrections and detail sharpening.
Changing out-of-scope settings returns an error rather than silently pretending
they were applied. Embedded orientation is not applied here; geometry is a later
work package. HDR, soft proofing, DCP profiles/looks, curves and Auto WB are not
implemented. Native and Sigmoid select this same M1 display transform.

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
