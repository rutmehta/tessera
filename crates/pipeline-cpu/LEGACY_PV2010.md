# PV2003/PV2010 reference branch (LR-2e)

`ToneSettings::legacy_pv2010` selects dedicated legacy operators. Import populates
it only for the Adobe family, revisions 1–2. Legacy values take precedence when
a PV2010 row also carries modern spellings. The modern tone sliders,
Clarity2012, Texture, Dehaze, parametric sliders and PV2012 point curves
(including extended curves) are neutral in that branch. Their exact source is
retained with `diagnostics::push_ignored` (status `ignored`, no field). Native
revision 2 does not select it. Native and Adobe CPU renderers execute it. GPU sessions route legacy Tone through `cpu_fallback`;
resident/fused dispatch declines this operator, and Adobe compatibility already
uses a CPU barrier. GPU-session, batch and single-stage parity are tested.

The optional block and its six members default to absent in native recipes. A
fresh Adobe PV1/PV2 import always creates the block, even if all members are
absent. Rendering an older saved Adobe PV1/PV2 recipe without it fails with
"re-import needed" rather than using stale PV2012 values. Only present controls
are imported; no camera defaults are inferred. All-zero controls are neutral.
Brightness=50, Contrast=25 and Shadows=5 are original legacy operator values,
not offsets mapped to current sliders. In particular Shadows=5 becomes legacy
blacks=5 and modern blacks remains zero. HighlightRecovery precedes Recovery;
Shadows precedes Blacks. The shared `V4_FEATURE_PREDICATES` registry requires
schema v4 for `legacy_pv2010`, `curves_extended`, and monochrome when enabled
or when a disabled mixer is nonzero. Each has the shared bumped-only-when-present
test. LR-7 owns the first-lane LR-SCHEMA checklist changes; there is no lane-local
schema helper.

## Public semantic basis and exactness boundary

Primary sources checked 2026-10-01:

- [Adobe Lightroom tone controls](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/image-tone-color.html)
- [Adobe DNG SDK renderer, Adobe-authored source hosted by Android](https://android.googlesource.com/platform/external/dng_sdk/+/refs/heads/android-s-beta-4/source/dng_render.cpp)
- [Adobe Camera Raw tone controls](https://helpx.adobe.com/camera-raw/desktop/using/make-color-tonal-adjustments-camera.html)

Adobe describes EV exposure, midtone brightness/contrast, highlight recovery,
shadow fill that maintains blacks, and an input black point. These documents do
not publish numerical PV2010 transfer functions, adaptive kernels, slider
calibration tables or channel reconstruction code. The public DNG SDK supplies
a baseline shadow ramp, but does not identify it as the full PV2010 operator set.
The formulas below implement
those documented effects directly; they do not convert values into PV2012
sliders. **They are Tessera reference formulas, not reverse-engineered Adobe
operators or an Adobe pixel-parity claim.**

| Member | Status and specific reason |
| --- | --- |
| exposure | Approximate import mapping: implements the documented EV multiplier `2^E`, range -5..5, but no Adobe-rendered synthetic chart or public DNG+XMP validates the process/camera convention. |
| brightness | Approximate: Adobe does not publish the midtone transfer or slider-to-gain calibration. The rational curve below holds black and SDR white fixed. |
| contrast | Approximate: Adobe does not publish the PV2010 pivot, transfer or slider-to-slope calibration. Uses a 0.5 midtone pivot with a smooth monotone odds curve. |
| fill_light | Approximate: Adobe does not publish the spatial/adaptive shadow algorithm. This scalar reference lifts shadows while keeping black and SDR white fixed; it does not reproduce spatial adaptation. |
| recovery | Approximate: the tone stage has working RGB, not Adobe's original camera-channel clipping masks or reconstruction data. Adobe's RAW channel reconstruction is unpublished; a highlight shoulder reduces extreme highlights but cannot recover missing detail. |
| blacks | Approximate: Uses the public DNG baseline shadow ramp with ShadowScale and Stage3Gain assumed one. Camera-specific values are not carried into this tone block, and the SDK does not establish PV2010 parity. |

Every represented legacy key retains its exact original literal/fragment in
`lrcat_develop_source`. The shared `import_lrcat::diagnostics::push_approximate`
helper records a per-key info/approximate entry with lane `LR-2`, the matrix
recipe path, and the reason; readers use `diagnostics::entries()`. There are zero approximation
warnings in the import report. Stale legacy keys on modern-process images stay
untranslated. No real catalog or image is a calibration source.

## Reference equations and ordering

1. Multiply working RGB by `2^E`. Compute `Y = .2627 R + .6780 G + .0593 B`.
   Nonpositive luminance is left signed after exposure.
2. Recovery `a=Recovery/100`: above `Y=.75`, set
   `Y=.75+(Y-.75)/(1+a*(Y-.75))`.
3. Blacks `b=.001*Blacks`, slope `m=1/(1-b)`, radius
   `r=min(b/2,(1-b)/16)`: apply per RGB channel `x`, zero below `b-r`,
   linear `(x-b)*m` above `b+r`, quadratic `m*(x-b+r)^2/(4*r)` inside
   the toe, then recompute Y. Unlike the SDK's SDR output ramp, values
   above one retain headroom.
4. Fill Light `a=FillLight/100`: below white, `Y=Y+2*a*Y*(1-Y)^2`.
5. Brightness `g=2^(Brightness/100)`: below white,
   `Y=g*Y/(1+(g-1)*Y)`.
6. Contrast `s=2^(Contrast/100)`: for `0<Y<1`,
   `Y=1/(1+((1-Y)/Y)^s)`.
7. Recovery scales RGB by its luminance ratio; steps 4–6 scale the post-black RGB
   by their output/input luminance ratio. HDR values are not clipped to SDR white.

Recovery/FillLight/Blacks ranges are 0..100, Brightness -150..150, Contrast
-50..100. Import rejects malformed/out-of-range values and retains source.
Direct operator inputs require finite values; amounts are bounded to their
slider ranges. These are scalar, deterministic operations, independent of tile
boundaries. Tests pin 36 neutral-ramp samples (108 channels), four shadow-toe
samples and an independent-channel colored patch, and check 2,001-sample
monotonic ramps for each operator, and verify legacy exposure/brightness render
separately. Goldens were evaluated independently in double precision from this
specification, not generated by Adobe.

## Known operator limitations recorded in LR-2e (not corrected)

Both controls remain **approximate**. Contrast pivots at **linear 0.5**, not
mid-grey 0.18. **Contrast 25 alone** maps **0.18 → 0.141** and
**0.02 → 0.0097** (approximately **−1 EV in deep shadows**). Stacking legacy
defaults (Brightness 50, Contrast 25, Blacks 5) produces a strong S-curve.
Brightness's rational formula has its **largest relative lift in deep shadows**,
not near mid-grey, even though its absolute lift peaks farther up the ramp.
For Brightness 50, the gain tends to `sqrt(2)` as luminance approaches zero and
tends to one at SDR white. These are properties of the current reference
operators, not measured Adobe behavior. LR-2e deliberately leaves both formulas
and their goldens unchanged. A later calibration pass can pivot contrast at 0.18
or operate in a gamma domain; that change needs independent evidence and tests.

## Extended curve domain

`curves_extended: Option<ToneCurves>` is separate from ordinary `curves`.
Import divides both axes by 255 without clamping, and populates this block only
for nonidentity curves with HDREditMode=1 on modern Adobe versions. SDR-range
extended knots still belong here, not in `curves`. Identity extended curves and all inactive extended curves
are retained source only. This prevents a default extended identity from erasing
a user's custom normal curve. Omitted extended channels are identity; an active
extended block selects the HDR point/channel rendition at render time.
**Ordinary `curves.parametric` remains authoritative and composes before the
selected point curves** on native CPU/GPU and both Adobe CPU paths. The
`curves_extended.parametric` member is not consumed. This rule also applies when
an explicitly stored extended block is all identity. All-identity imported HDR
curves leave the block absent, keeping ordinary points and parametric controls.
Invalid or nonmonotone source stays retained with a warning.

Native CPU/GPU use the same monotone Hermite coefficients as ordinary curves,
with the signed log axis `sign(v)*ln(1+abs(v)/.18)/ln(1+1/.18)` and unit-slope
extrapolation outside knots. The Adobe CPU path uses its signed power axis
`sign(v)*abs(v)^(1/2.2)`. HDR input bypasses the fallback SDR profile clipping;
a supplied DCP retains its profile-tone semantics. Axis and range conventions
are unverified against Adobe-rendered charts, so active extended imports are
`approximate`, retain exact source, and emit info diagnostics without warnings.

## Monochrome and channel-curve order

The selected Tessera order is basic/legacy tone, B&W conversion, local tone and
point/channel curves, then other colour controls (including grading). It is
shared by standalone CPU, Adobe CPU, host GPU sessions and resident GPU paths.
B&W therefore cannot erase channel-curve toning. Enabled B&W/mixer changes enter
the tone-stage cache hash; absent/disabled B&W preserves the former tone hash.
The LR-1 ordinary/point-colour-only fast path must clear `monochrome` when testing
whether the remaining colour settings are neutral; this branch already clears
it in `color_detail.rs`.

[Adobe's tone/color guide](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/image-tone-color.html)
describes B&W mixing and profile-dependent response, but does not establish a
numeric pipeline order or transfer calibration. The order above is a deliberate
Tessera reference choice, tested for preserved toning, not verified Adobe parity.
All monochrome mappings remain `approximate` with retained source and info-only
reasons. Disabled B&W with an all-zero mixer is absent from settings; nonzero
inactive mixer values can round-trip without affecting pixels.
