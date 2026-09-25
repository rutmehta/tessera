# M2 additional tone operators

`src/tone_extra.rs` exports `tone_extra(&mut Tile, &ToneSettings) -> EngineResult<()>`.
It also exports `tone_extra_image(&crate::Image, &ToneSettings) -> EngineResult<crate::Image>`.
Both are exported in `lib.rs` and invoked **after** existing `tone`, before
colour and display. This implementation deliberately does not reapply exposure,
contrast, highlights, shadows, whites or blacks, or choose a display transform.
Input/output are planar f32, scene-linear Rec.2020. All allocated samples,
including halos, are processed. RGB planes and finite input/parameters are
validated before mutation. Neutral settings are bit-exact and retain COW sharing.
Slider values outside the nominal range are clamped; invalid curves/splits return
an error rather than silently sorting, deduplicating, or changing the recipe.

## Equations and order

Let `Y = .2627 R + .6780 G + .0593 B`. Define the native log axis
`E(v) = ln(1 + max(v,0)/.18) / ln(1 + 1/.18)` and inverse
`D(z) = .18 expm1(z ln(1 + 1/.18))`. White maps to 1; values above white
are **not clipped**. All pixel arithmetic, curves, statistics and filter buffers
use scalar f32. Large-value encoding uses `ln(v)-ln(.18)` to avoid division
overflow; large-value decoding moves the .18 factor inside the exponential.
Gains, secants and outputs saturate at representable f32 limits, not SDR white.
Flat spline segments return their exact knot value; interpolation is bounded
by segment endpoints, with monotonicity tested to f32 epsilon.

1. **Texture and clarity**, computed together from the same input luminance.
   A scalar guided filter has `a = cov(I,p)/(var(I)+epsilon)`,
   `b = mean(p)-a mean(I)`, `G(I,p)=mean(a) I+mean(b)`. Both means have
   radius `r`, hence total support radius `2r`. `epsilon=.001` on the log axis.
   Box means use separable local sums (O(N*r)) to avoid full-frame prefix-sum
   cancellation and preserve identical arithmetic for overlapping windows. Let `F=G_1(z,z)`, `M=G_3(z,z)`,
   `B=G_8(z,z)`. With normalized sliders `t,c` in [-1,1],
   `delta=t(F-M)+c*4u(1-u)(M-B)`, where `u=clamp(z,0,1)`.
   This excludes the finest `z-F` noise band from texture and excludes fine
   detail from clarity. The resulting `z+delta` is constrained to the original
   3x3 log-luminance min/max. It cannot create new extrema, and a hard two-level
   step acquires no overshoot/undershoot lobes. Recombine with RGB by `D(z')/Y`;
   nonpositive luminance is left unchanged. Flat regions remain unchanged.

2. **Dehaze** uses the dark-channel prior, not a global contrast proxy.
   Form a radius-3 minimum of the positive RGB channel minimum. Candidate airlight
   pixels are in its top 10%, excluding luminance above the 99th percentile.
   Airlight is the per-channel median of those candidates, not their maximum.
   After WB, constrain airlight channels to [.75,1.25] times their luminance to
   limit foreground-induced colour casts. Reject negligible airlight.
   Confidence is `smoothstep(((P90(Y)-P10(Y))/Y_air-.05)/.20)`;
   nearly uniform white/snow scenes therefore bypass the ambiguous prior.
   For normalized `d=min_radius3(min_c RGB_c/A_c)` and slider magnitude `m`,
   `t0=clamp(1-.85*m*confidence*clamp(d,0,1),.15,1)`.
   Refine `t0` with radius-4 guided filtering using log luminance, then constrain
   transmission again to [.15,1]. Positive dehaze uses
   `J_c=max(0,A_c+(I_c-A_c)/t)`; negative uses `J_c=t I_c+(1-t) A_c`.
   Existing negative components are passed through; HDR components are not
   clipped to white. Minimum transmission limits inverse gain.

3. **Native parametric curve** acts on log luminance before point curves.
   Splits `[0,shadow_split,midtone_split,highlight_split,100]/100` must be
   strictly increasing. In each interval `[a,b]`, set `u=(z-a)/(b-a)` and
   `z'=z+3*s*(b-a)*u^2*(1-u)^2`, with region slider `s` in [-1,1].
   The smooth endpoint envelope gives value and first-derivative continuity at
   every movable split, positive sliders lift their region, and derivative is
   bounded below by `1-1/sqrt(3)>0`, even at extreme settings. Values outside
   [0,1] pass through. Recombine by luminance ratio.
   This bounded native region warp intentionally replaces spec 01's suggested
   blended gamma approximation: it guarantees monotonicity for arbitrarily
   close valid split points. It is not Adobe parametric-curve matching.

4. **Point curves** use Fritsch–Carlson monotone cubic Hermite interpolation.
   Input knots require finite coordinates in [0,1], strictly increasing x and
   nondecreasing y. Missing endpoints acquire (0,0)/(1,1); empty curves are
   identity. Tangents start from adjacent secant averages, become zero on flat
   segments, and are limited to the radius-3 circle in normalized tangent space.
   Evaluate directly rather than introducing 4096-entry LUT quantization.
   Apply master RGB to each log-encoded component, then its R/G/B curve, then
   luminance-only curve via luminance ratio. Absolute black can lift to neutral
   grey through the luminance curve. Negative component inputs bypass component
   curves; nonpositive nonblack luminance bypasses luminance curves.
   Above the final knot continue with unit slope and endpoint offset on the log
   axis, retaining HDR headroom. Identity curves are explicitly bypassed.
   Per-channel/master RGB curves may change chromaticity; only parametric and
   luminance curves promise RGB-ratio preservation. Log encoding follows spec
   07 §5/native M2 scope rather than spec 01's suggested display-gamma encoding.

## Halo contract

Supply real neighbouring pixels, with image-edge extension performed by the
caller (`Image::tile(..., halo, 1)` supplies nearest-edge RGB extension).
The kernels truncate/renormalize windows at the outer allocation boundary;
zero-halo standalone tiles work but are not seam-safe local image operators.

| Enabled local operators | Local dependency radius |
| --- | ---: |
| Curves only | 0 |
| Texture only | 6 |
| Clarity (with or without texture) | 16 |
| Dehaze transmission only, fixed statistics | 11 |
| Texture then dehaze, fixed statistics | 17 |
| Clarity then dehaze, fixed statistics | 27 |

Returned halo pixels are not all valid for a subsequent neighbourhood operation:
only the interior is guaranteed with the required input padding. Re-gather halos
between pipeline stages; do not chain local operators on an already exhausted
halo. For simultaneous texture/clarity the required radius is the maximum, not
the sum, because they share one decomposition.

**The Tile API uses tile-local airlight and confidence over the supplied
allocation. No finite halo alone makes dehaze tile-invariant.**
The radius-11/17/27 entries cover the transmission/decomposition path, not this
statistical dependency. Use `tone_extra_image` for seamless whole-image processing:
it shares the exact validated slice kernel with the Tile API and computes
statistics once over the full image after presence. `Tile::check_layout` rejects
extents above 256, so full-frame Tile construction is NOT supported; image-core
must call the image API instead of independently processing each tile.
Whole-image edge windows truncate/renormalize without replicated halos.
Texture/clarity overlap equivalence with halo 16 is tested independently.

## Scope and limitations

- Fixed radii are in current tile/pyramid pixels, not a percent of original image
  dimensions. No full-image/scale metadata is available in this API.
- Extrema limiting strongly suppresses halos but also caps sharpening at extrema;
  this is not a guarantee of perceptually artifact-free results on every image.
- Dark-channel assumptions fail for some skies, snow, bright objects and coloured
  illumination. Robust medians, gain limits and low-dynamic-range confidence are
  conservative heuristics, not learned scene classification. Confidence also
  deliberately reduces correction on uniformly foggy scenes.
- Native tone curves are not Adobe PV6 compatible. Display transforms, curve
  presets/UI state, Auto, sharpening/NR and colour controls belong elsewhere.
- Scalar CPU reference; guided means are O(N*r), small min/max windows are direct
  scans, and percentile statistics sort the supplied allocation. The image API
  uses whole-frame temporary buffers and sorting; no GPU or streaming path.

## Verification

Tests live in `tone_extra.rs`. Feature slices were exercised red then green:
master spline, parametric controls, presence, dehaze, and black-lift regression.
Additional tests cover identity/COW, RGB-vs-luminance semantics, invalid settings,
monotone steep/flat splines, negative controls, HDR finiteness, isolated airlight
outliers, snow protection, step ringing, and padded overlap agreement.

Additional regressions cover scalar f32 encoding, mixed-sign finite extremes,
subnormal curve knots, >256-pixel whole-image global statistics, Tile/image
kernel agreement, neutral image preservation, and grayscale rejection.
Run the registered module tests with:

```sh
CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-04 cargo test -p pipeline-cpu tone_extra --lib
```
