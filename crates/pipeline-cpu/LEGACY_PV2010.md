# PV2003/PV2010 reference branch (LR-2b)

`ToneSettings::legacy_pv2010` selects a dedicated legacy operator before any
explicit modern tone values. Import populates it only for Adobe process revisions
1–2; modern properties suppress their corresponding legacy control. Native and
Adobe CPU renderers both execute the branch. Native GPU Tone rejects the block
with `legacy process requires CPU rendering`, including fused/resident parameter
construction. Adobe rendering uses its existing CPU compatibility barrier.

The optional block and each of its six members default to absent. No inferred
camera defaults, version bump, or serialized defaults are introduced. All-zero
controls are neutral; the usual Adobe Brightness=50 and Contrast=25 are actual
positive adjustments, not subtracted offsets. Only controls present in source
are imported. HighlightRecovery precedes Recovery; Shadows precedes Blacks.
Exposure2012 suppresses both legacy Exposure and Brightness, as in LR-2;
Contrast2012, Shadows2012, Highlights2012 and Blacks2012 suppress their respective
legacy Contrast, FillLight, Recovery and Blacks controls.

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
| exposure | Documented EV multiplier `2^E`, range -5..5; no approximation to EV scaling. This does not claim the whole Adobe camera/profile pipeline is reproduced. |
| brightness | Approximate: Adobe does not publish the midtone transfer or slider-to-gain calibration. The rational curve below holds black and SDR white fixed. |
| contrast | Approximate: Adobe does not publish the PV2010 pivot, transfer or slider-to-slope calibration. Uses a 0.5 midtone pivot with a smooth monotone odds curve. |
| fill_light | Approximate: Adobe does not publish the spatial/adaptive shadow algorithm. This scalar reference lifts shadows while keeping black and SDR white fixed; it does not reproduce spatial adaptation. |
| recovery | Approximate: the tone stage has working RGB, not Adobe's original camera-channel clipping masks or reconstruction data. Adobe's RAW channel reconstruction is unpublished; a highlight shoulder reduces extreme highlights but cannot recover missing detail. |
| blacks | Approximate: Uses the public DNG baseline shadow ramp with ShadowScale and Stage3Gain assumed one. Camera-specific values are not carried into this tone block, and the SDK does not establish PV2010 parity. |

Exposure's represented source key leaves pending source. Approximate controls
retain their exact original literals/fragments, alongside the raw numeric fields.
The import warning names the approximate operator families and their reasons.
Stale legacy keys on modern process images remain untranslated and byte-identical.

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

## Extended curve domain

`curves_extended: Option<ToneCurves>` replaces the ordinary curve block when
present. Import divides both source axes by 255 without clamping and copies
ordinary channels into the block before applying per-channel extended-key
precedence. Any signed/HDR knot routes the resulting block into the new field.
Malformed/nonmonotone source remains retained; no synthetic SDR endpoint is
inserted into extended curves. Empty curves remain identity.

Native CPU and GPU use the same monotone Hermite coefficients as ordinary curves,
with a signed extension of the existing logarithmic axis:
`sign(v)*ln(1+abs(v)/.18)/ln(1+1/.18)`. They extrapolate with unit slope outside
the knot domain. The Adobe CPU compatibility path uses its existing signed power
axis (`sign(v)*abs(v)^(1/2.2)`), with the same whole-block precedence, and does not
clip HDR input through the default SDR profile approximation. A supplied DCP
still has its own profile-tone semantics. These preserve HDR knots; they do not
establish Adobe-exact HDR tone-curve calibration.
