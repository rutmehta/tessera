# LR-1 Point Color reference

Point Color now renders in the CPU color stage, before vibrance, saturation,
the HSL mixer and grading. `pipeline-adobe` delegates its final color stage to
this path. This is a deterministic Tessera approximation, **not verified Adobe
pixel equivalence**. Adobe's internal color space, shift curves, range-amount
curve and overlap behavior are not specified by the SDK; no Adobe render oracle
was available in this lane. The shared `lrcat_translation_diagnostics.PointColors` info list records this limitation via `diagnostics::push_approximate`.

## Representation and decoding

Existing `/settings/color/point_colors` entries still own `hue_shift`,
`saturation_shift`, `luminance_shift`, and `range`. One optional `selection`
field is added to each entry because neither an OkLCh sample nor one scalar
width represents Adobe's HSL sample plus twelve independent feather limits.
`selection` is omitted when absent, so old native points retain identical JSON.
The shared schema-v4 predicate registry requires v4 for any nonempty point list.
Absent/empty points leave schema-v3 recipe bytes unchanged; no dependency changes are made.

| Source | Recipe | Conversion |
| --- | --- | --- |
| SrcHue, SrcSat, SrcLum | selection.source_hsl | `[SrcHue * 60, SrcSat, SrcLum]` |
| HueShift | hue_shift | `* 60` degrees (reference-operator convention) |
| SatScale, LumScale | saturation_shift, luminance_shift | `* 100` |
| RangeAmount | range | `* 100` |
| HueRange | selection.hue | LowerNone, LowerFull, UpperFull, UpperNone |
| SatRange | selection.saturation | same order |
| LumRange | selection.luminance | same order |

The optional shift defaults (zero), range-amount default (0.5), source bounds
(0–6, 0–1, 0–1), and four ordered range limits follow the
[Adobe SDK documentation](https://lrc.mcor.dev/modules/LrDevelopController.html#LrDevelopController.addPointColorSwatch).
The SDK's 0–6 hue is interpreted as six sectors, not as radians; this and the
HueShift-to-degrees factor are explicit reference conventions, not a claim
that Adobe disclosed its transform. `source_lch` remains zero for these entries
and is ignored when `selection` exists; HSL data is never labeled OkLCh.

The shared sidecar codec accepts SDK-shaped RDF resources produced by the Lua
adapter and the comma-separated 19-number XMP sequence layout documented in
[JarvisArt's converter](https://github.com/LYL1015/JarvisArt/blob/main/data_scripts/format_conversion/utils/xmp_converter.py#L363).
The latter is an independent implementation, not an Adobe format specification.
Order: source H/S/L, three shifts, range amount, then the four H/S/L limits.
Lua accepts positional arrays and contiguous explicit numeric indices.
All samples, shifts and boundaries must be finite and in range; unordered
limits, partial samples, incomplete feather ranges, unknown fields, extra sequence
children, malformed numbers and mixed valid/invalid swatch lists fail atomically.
The exact original source is retained for these cases. Absent SDK feather tables
use `[0, 0.25, 0.75, 1]` independently for H/S/L. The SDK declares the tables
optional but does not publish their defaults; these are explicit Tessera
reference defaults. Complete 19-number all-`-1` placeholder records are skipped,
so valid swatches in the same list translate. Variance and future layouts remain
unsupported and retained. Catalog import requires Adobe PV3+; PV1/2 retains the
source and reports an unsupported warning before constructing recipe history.

Successful translation is classified as `approximate`: exact PointColors source
stays in `lrcat_develop_source`, alongside an appended info diagnostic with field
`/settings/color/point_colors` and lane `LR-1`. The shared import finish records
all lanes in one replayable Import edit. Empty/nil and native-extension-only
inputs keep the previous retention behavior. Export uses
the existing Tessera native RDF fields; it does not claim to generate Adobe
PointColors strings. Original source XMP remains available for unchanged exports.

## CPU reference math

For an imported point, clamp linear Rec.2020 working RGB to [0,1] for the
selection/adjustment coordinate, apply the sRGB transfer function per channel,
then convert that gamma-encoded RGB to ordinary HSL. This corrects the linear
versus gamma mismatch (encoded .5 is about .214 linear), but does **not** establish
Adobe's exact primaries/transfer function. The working primaries stay Rec.2020;
Adobe's internal color space remains unverified. Native OkLCh points retain their
separate unbounded working-space path.

For each point in recipe order:

1. Compute membership from the **original pixel entering the Point Color stage**,
   never the previous point's output. Hue membership is
   `0.5 + wrap_signed(h - source_hue)/360`; saturation and luminance are
   `0.5 + sat - source_sat` and `0.5 + lum - source_lum`.
2. Set width = `range / 50`. All three sample-relative coordinates are centered
   at .5; evaluate each feather at `.5 + (x-.5)/width`. Zero width selects only
   the source (coordinate tolerance 1e-7). The sample sits at the full-weight
   center for the default ranges, including high/low saturation and luminance.
3. Feather [a,b,c,d] is zero outside [a,d], one on [b,c], and cubic smoothstep
   `t*t*(3-2*t)` on either shoulder. Multiply H/S/L memberships for weight w.
4. Apply the weighted shifts to the current gamma-encoded pixel in recipe order:
   `h += w*hue_shift`, `s *= 1+w*saturation_shift/100`,
   `l *= 1+w*luminance_shift/100`; clamp s/l to [0,1], then convert back through
   RGB and the inverse transfer function.
5. Add back `input - clamp(input,0,1)` per channel. Signed/HDR residuals survive,
   and crossing 1.0 no longer switches the entire operation off. This is a
   continuous SDR reference extension, not an Adobe HDR implementation.

No-op and excluded points return the original floats without a roundtrip.
Validation precedes pixel mutation. Native OkLCh membership also uses the
original pixel: Euclidean distance in `[L,2C,h/180]` with circular hue distance,
smoothstep falloff from zero to `range/100`, and the same shift rules.
Only adjustment composition remains sequential; Adobe overlap semantics are
unverified.

GPU tile, whole-image and mixed-chain paths route Point Color to CPU; fused
chains reject it as a capability, and renderer resident capability returns false
so preview/export callers take the established nonresident path. GPU shader
parameter validation remains strict. The fallback color stage is bit-identical
to CPU; surrounding GPU stages retain their existing numerical tolerances.

## Hand-computed parity cases

The RGB triples below are gamma-encoded coordinates; tests apply the inverse
transfer to input and expected output. Tests use a per-channel absolute tolerance of **2e-6 in linear working RGB**.
This is parity with these independent arithmetic references, not Lightroom.

- RGB [.75,.25,.25] is HSL [0,.5,.5]. A full-weight +30 degree hue shift gives
  chroma .5, secondary component .25, base .25 => RGB [.75,.5,.25].
- RGB [.5625,.4375,.4375] has S=.125 and L=.5. Halfway through the saturation
  feather, smoothstep(.5)=.5, so the same point shifts only +15 degrees and
  produces [.5625,.46875,.4375].
- A -30 degree, -50% saturation, +20% luminance adjustment gives HSL
  [330,.25,.6], hence chroma .2, secondary .1, base .5 => [.7,.5,.6].
- A luminance-half-feather swatch [.1875,.0625,.0625] yields
  [.1875,.09375,.0625]. Cyan outside the hue window stays bit-identical.

Coverage also includes wrap at red, narrower range, invalid-range atomicity,
recipe JSON roundtrip, numeric-XMP/Lua equivalence, and synthetic SQLite catalog
import through the full CPU renderer, original-pixel selection, the 0.999/1.001
seam, and GPU fallback rendering. Exact Adobe pixel parity remains open.

## LR-1c ordering with monochrome

Point Color runs **before B&W conversion**, so selection sees the original
colour. With monochrome enabled, the shared stage split moves points into the
pre-curve colour stage: basic tone → Point Color → B&W → point/channel tone
curves → vibrance/HSL/grading. Point edits invalidate the tone-stage cache in
this configuration. With monochrome disabled, Point Color keeps its existing
post-curve position, before vibrance/HSL/grading, preserving colour-only renders.

The CPU scalar operator also applies points before B&W when passed a combined
colour block. The native CPU renderer, Adobe chain, tiled GPU fallback, and RGB
preview use the shared split. Resident/fused capability rejects nonempty point
lists; their callers take the CPU fallback rather than a shader that omits points.
No-op points with an inactive monochrome block avoid an unnecessary OkLab roundtrip.

Regression coverage includes the S=.9/L=.1 full-weight sample, selected negative
channel preservation, pre-B&W selection, tone-cache invalidation, mixed GPU
chains and resident rejection, depth export, and MCP preview.
