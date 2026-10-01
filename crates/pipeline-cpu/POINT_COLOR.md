# LR-1 Point Color reference

Point Color now renders in the CPU color stage, before vibrance, saturation,
the HSL mixer and grading. `pipeline-adobe` delegates its final color stage to
this path. This is a deterministic Tessera approximation, **not verified Adobe
pixel equivalence**. Adobe's internal color space, shift curves, range-amount
curve and overlap behavior are not specified by the SDK; no Adobe render oracle
was available in this lane. The import report explicitly says so.

## Representation and decoding

Existing `/settings/color/point_colors` entries still own `hue_shift`,
`saturation_shift`, `luminance_shift`, and `range`. One optional `selection`
field is added to each entry because neither an OkLCh sample nor one scalar
width represents Adobe's HSL sample plus twelve independent feather limits.
`selection` is omitted when absent, so old native points retain identical JSON.
No format/contract version or dependency changes are made.

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
limits, partial samples, missing feather ranges, unknown fields, extra sequence
children, malformed numbers and mixed valid/invalid swatch lists fail atomically.
The exact original source is retained for these cases. Absent SDK feather
ranges are not guessed. All-`-1` placeholder records, Variance, and
future layouts remain unsupported and retained.

After successful translation of a single nonempty PointColors property, only
that property's pending-source entry is removed. Other retained keys are
unchanged. Duplicate properties conservatively retain source. Empty/nil and
native-extension-only inputs keep the previous retention behavior. Export uses
the existing Tessera native RDF fields; it does not claim to generate Adobe
PointColors strings. Original source XMP remains available for unchanged exports.

## CPU reference math

For an imported point, convert SDR linear Rec.2020 RGB to ordinary HSL. Hue is
in degrees and saturation/luminance in [0,1]. Signed/HDR pixels outside [0,1]
and achromatic pixels are left unchanged by imported points. This restriction
avoids destructive clipping but means those pixels are not translated with
Adobe fidelity. Native OkLCh points have a separate unbounded working-space path.

For each point in recipe order:

1. Hue membership is `0.5 + wrap_signed(h - source_hue)/360`, with circular
   distance in [-180,180). Saturation/luminance membership uses their values.
2. Set width = `range / 50`. For each membership value x centered at source c,
   evaluate the feather at `c + (x-c)/width`. Hue's center is 0.5. Zero width
   selects only the source (absolute coordinate tolerance 1e-7).
3. Feather [a,b,c,d] is zero outside [a,d], one on [b,c], and cubic smoothstep
   `t*t*(3-2*t)` on either shoulder. Equal limits implement a hard boundary.
   Multiply the three memberships to obtain w.
4. Set `h += w*hue_shift`, `s *= 1+w*saturation_shift/100`,
   `l *= 1+w*luminance_shift/100`; clamp s/l to [0,1], then convert to RGB.

No-op and excluded points return the original floats without a color-space
roundtrip. Validation precedes all pixel mutation. Native OkLCh selection uses
Euclidean distance in `[L,2C,h/180]` with circular hue distance, smoothstep
falloff from zero to `range/100`, and the same hue/chroma/lightness shift rules.
Points are sequential, so a later point selects the output of the preceding
one. Adobe's overlap/compositing semantics are unverified.

## Hand-computed parity cases

Tests use a per-channel absolute tolerance of **2e-6 in linear working RGB**.
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
import through the full CPU renderer. GPU Point Color is outside this lane.
