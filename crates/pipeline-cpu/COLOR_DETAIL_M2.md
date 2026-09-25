# M2 scalar colour and detail reference

`src/color_detail.rs` implements `color(&mut Tile, &ColorSettings)` and
`detail(&mut Tile, &DetailSettings)`. Inputs are finite planar f32 linear
Rec.2020, D65, with exactly three channels. No gamma encoding, integer
intermediate, RGB clipping, or gamut mapping occurs here. Negative scene
values and HDR values are retained. These are deterministic scalar reference
formulas, not a claim of Adobe pixel equivalence, camera-noise calibration,
Richardson–Lucy deconvolution, neural NR, or cross-platform libm bit identity.

Scope follows docs/01 §2.3–2.7 and docs/07 §5. Tone curves belong to the Tone
operator, not this module. Point Color and LUT are explicitly rejected rather
than silently ignored. **B&W treatment and per-band B&W mix are absent from
`ColorSettings` and `DevelopSettings`**; no implicit recipe fields are invented.
Saturation −100 removes chroma, but is not a B&W treatment/mix implementation.

## Compatibility and validation

- Default Color settings bypass all arithmetic and mutation.
- **Unchanged Sharpening `(amount=40, radius=1, detail=25, masking=0)` bypasses
  sharpening**, even when NR is changed. A changed sharpening tuple with
  positive amount uses its absolute amount (not `amount−40`). Amount zero is off.
- **Unchanged chroma-NR tuple `(color=25, color_detail=50,
  color_smoothness=50)` bypasses chroma NR**, independently of luminance NR or
  sharpening. A changed tuple with positive color amount enables it using the
  absolute amount. Color zero is off.
- Luminance NR is off at amount zero; its Detail/Contrast alone cannot enable it.
- Consequently fully default detail is bit-neutral, including signed zeros and
  halo samples. This deliberately preserves M1 goldens. It introduces a
  compatibility discontinuity at the exact default tuples; a future process
  version can remove this rule without silently changing existing recipes.
- Nonfinite controls/samples and wrong channel/sample formats fail before any
  mutation, even on bypass paths. Detail parameters outside their declared
  ranges fail before mutation. Color slider values are clamped to their
  declared ranges; wheel angles wrap modulo 360 degrees.

## Halo contract

Color has support zero and applies identically to interior and supplied halo.
Detail reads a single immutable input snapshot and writes **only the interior**.
The output halo remains the **old input**, not valid post-detail neighbours.
Commit interiors to a separate destination image and regather before another
neighbourhood operator; do not read halos from partially updated images.

`detail_halo(settings)` gives required support (for validated settings):

- sharpening: `max(2, ceil(3*radius))`, at most 9 pixels;
- luminance NR: 2 pixels;
- chroma NR: `ceil(1+4*color_smoothness/100)`, at most 5 pixels;
- combined: maximum, not sum, because all branches use the same input;
- bypass: zero.

`DETAIL_HALO = 9` is a safe fixed allocation. Insufficient halos are errors,
not tile-edge clamping. Gather real neighbours across tile boundaries and
replicate the closest pixel only outside the **image**. Radius is measured in
pixels at the supplied tile's pyramid level. Tests compare a full region with
partitioned regions bit-for-bit.

## Rec.2020 ↔ Oklab ↔ OkLCh

All matrices and arithmetic below are f32. First `v = cbrt(M*RGB)` using signed
cube roots, then `Lab = A*v`:

```
M = [ .6167558   .3601984   .0230458
      .2651330   .6358394   .0990276
      .1001026   .2039065   .6959909 ]
A = [ .21045426   .7936178  -.004072047
     1.9779985  -2.4285922   .4505937
      .025904037 .78277177 -.80867577 ]
```

Inverse: cube each component of `B*Lab`, then multiply by `N`:

```
B = [ 1  .39633778   .21580376
      1 -.105561346 -.06385417
      1 -.08948418 -1.2914855 ]
N = [ 2.1399066 -1.2463895  .1064829
      -.8847359 2.163231  -.2784951
      -.0485738 -.4545031 1.5030769 ]
```

`C=hypot(a,b)`, `h=atan2(b,a)` wrapped to `[0,360)` degrees. Reverse with
`a=C*cos(h)`, `b=C*sin(h)`. Round-trip tests include negative and HDR RGB.

### Vibrance and saturation

Use normalized sliders `v,s ∈ [−1,1]`. Circular hue distance is
`d(h,k) = ((h-k+180) mod 360)-180`.

```
skin = exp(-.5*(d(h,50)/25)^2)
muted = 1/(1 + C/(.25*max(abs(L),.05)))
C' = C * (1 + v*muted*(1-.7*skin)) * (1+s)
```

This is hue-based skin protection, not semantic face detection. Muted colours
receive more vibrance; neutral colours do not acquire chroma.

### Eight-band HSL mixer

OkLCh hue centres, in Red/Orange/Yellow/Green/Aqua/Blue/Purple/Magenta order:
`[25,55,95,145,195,255,295,335]` degrees. Between adjacent centres `hi,hj`,
including the wrap interval, set `t=(h-hi)/(hj-hi)`, `w=(1-cos(pi*t))/2`.
Only the two adjacent bands contribute with weights `1-w,w`. These weights
partition unity, meet with zero slope at each centre, and wrap continuously.
All three controls use the **same original hue** membership:

```
h' = h + 30 * sum(weight * hue_slider/100)
C' = C * (1 + sum(weight * saturation_slider/100))
L' = L * (1 + .5*sum(weight * luminance_slider/100))
```

Membership is skipped when post-global-saturation chroma is ≤1e−6 to avoid
assigning arbitrary hue adjustments to neutrals. HSL lightness does not scale
chroma. The 30-degree maximum band rotation is this reference's definition.

### Grading

After HSL, use `t=clamp(L+.25*balance/100,0,1)` and
`width=.15+.5*blending/100`. For shadow/mid/high centres `[0,.5,1]`, compute
`r_i=exp(-.5*((t-centre_i)/width)^2)` and normalize `w_i=r_i/sum(r)`.
Positive balance favours highlights; larger blending increases overlap.
The global wheel has weight 1. Each wheel adds:

```
k = .2 * saturation/100 * weight * min(abs(L),1)
a += k*cos(hue)
b += k*sin(hue)
L += .25 * luminance/100 * weight
```

All wheel masks and chroma protection use the pre-wheel L, so wheel traversal
cannot feed back into later masks. Chroma adjustments retain Oklab lightness
unless a wheel explicitly changes it; linear Y is not claimed invariant.

## Detail formulas

`Y=.2627R+.6780G+.0593B`. Spatial kernels are square truncated Gaussians,
`G(dx,dy)=exp(-.5*(dx²+dy²)/sigma²)`. Weighted differences from the centre are
accumulated rather than weighted absolute values, preserving constant fields.
All filters include the centre, so normalization denominators are positive.

### Sharpening

Gaussian sigma is Radius, support `ceil(3*Radius)`. Let
`r=Y−Gaussian(Y)` and `d=Detail/100`:

```
limit = .05*(abs(Y)+.1)
boost = (1-d)*clamp(r,-limit,limit) + 1.5*d*r
Ytarget = Y + Amount/100 * mask * boost
```

Detail interpolates between halo-limited USM and stronger high-frequency
boost; this is not a true inverse-PSF solver. Masking zero gives mask 1.
Otherwise compute Sobel magnitude `/8`, box-average it over 3×3, then:

```
t = clamp((mean_sobel/(abs(Y)+.1) - .15*Masking/100)/.05,0,1)
mask = t*t*(3-2*t)
```

This requires two pixels of support. Large edges are not guaranteed halo-free
at maximum Detail; the low-detail branch explicitly limits overshoot.

### Luminance NR

A luminance-guided bilateral, support 2, spatial sigma 1.2:

```
rho = (.02+.18*(1-LuminanceDetail/100))*(abs(Y)+.1)
w_q = G_q * exp(-.5*((Yq-Y)/rho)^2)
D = sum(w_q*(Yq-Y))/sum(w_q)
V = sum(G_q*(Yq-Y)^2)/sum(G_q)
protection = 1/(1+8*(LuminanceContrast/100)*V/rho²)
Ytarget += Luminance/100 * protection * D
```

Higher Detail narrows the range kernel; higher Contrast retains high-variance
regions. This M2 baseline does not consume camera/ISO noise profiles and is
not the later multi-scale/learned denoiser described in the long-term spec.

### Chroma NR and recombination

Convert the immutable source to Oklab. Support is
`ceil(1+4*ColorSmoothness/100)`, spatial sigma `.7+2*ColorSmoothness/100`.
Let `rho=.02+.18*(1-ColorDetail/100)` and:

```
w_q = G_q * exp(-.5*((delta_a²+delta_b²)/rho² + (delta_L/.08)²))
a' = a + Color/100 * sum(w_q*delta_a)/sum(w_q)
b' = b + Color/100 * sum(w_q*delta_b)/sum(w_q)
```

Keep L, invert to RGB. If chroma differences are zero, retain original RGB
without a round trip. Finally add `(Ytarget−Y(RGB))` equally to RGB channels.
This restores linear luminance after chroma filtering and applies the combined
sharpening/luminance-NR delta without division near black. It preserves linear
RGB channel differences for luminance-only changes, not exact OkLCh chroma.
No output clamp is applied.

## Verification

Unit tests cover desaturation, vibrance protection, all eight hue centres,
HSL partition/wrap/lightness, grading balance/blending/global luminance,
default and signed-zero neutrality, signed/HDR colour round trips, sharpening
controls, luminance and chroma noise attenuation, NR subsetting independence,
invalid inputs/unsupported controls, halo bounds, and partition invariance.
Tests for missing operations were run red against bypass stubs before their
implementations. Both modules are registered in `lib.rs` and exercised by Cargo,
the public reference renderer and image-core's StageOp integration tests.
