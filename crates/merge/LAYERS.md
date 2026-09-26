# Layer alignment / blending API (M5-24)

Public module `merge::layers`. Inputs `LinearImage` are straight RGB; color metadata is preserved, not interpreted. All functions return `merge::Result`. Existing option fields and defaults are unchanged.

- `AlignMode::{Auto, Perspective, Cylindrical, Spherical, Collage, Reposition}`.
- `AlignOptions { mode, reference, seed, vignette_removal, geometric_distortion, lens_corrections }`, Default.
- `align_layers(images: &[LinearImage], options: &AlignOptions) -> Result<AlignedLayers>`.
- `AlignedLayers { width, height, origin: [f64;2], transforms: Vec<transform::TransformOp>, images: Vec<LinearImage>, coverage: Vec<Vec<bool>>, source_gains: Vec<Vec<f32>> }`.
- `BlendMode::{Panorama, StackImages}`.
- `BlendOptions { mode, seamless_tones, fill_transparent, pyramid_levels, seed }`, Default.
- `blend_layers(images: &[LinearImage], coverage: &[Vec<bool>], options: &BlendOptions) -> Result<BlendedLayers>`.
- `BlendedLayers { masks: Vec<Vec<f32>>, corrections: Vec<Vec<[f32;3]>>, image: LinearImage, coverage: Vec<bool>, fill_mask: Vec<bool> }`.

Masks and corrections use aligned canvas coordinates. Corrections are additive RGB deltas: `aligned_pixel + correction`. Binary masks partition covered pixels; corrections make mask compositing reconstruct multiband RGB exactly. `image` is convenience flattened output. `coverage` always records real source support. `fill_mask` records uncovered pixels requested for content-aware fill; it is NOT synthesized coverage. Parent must invoke `filters::caf::fill` on this mask, because filters depends on compositor and compositor depends on merge (direct merge->filters would introduce a cycle).

## Coordinates and nonlinear projections

TransformOps map original image pixel-edge coordinates into union canvas (first center 0.5,0.5). Registration uses integer pixel centers internally; conversion conjugates by +/-0.5. Origin is integer floor of union bounds. Rendering uses the same TransformOp with bilinear reconstruction. All bounds must be finite, projective source rectangles must not cross a horizon, and union allocation is capped at 64 MP.

Cylindrical and spherical modes register into the reference plane, then reproject that plane with an **assumed**, uncalibrated focal length `f = max(reference.width, reference.height)` and principal point at its center. For normalized reference-plane coordinates `(x,y)`, cylindrical output is `f*(atan(x), y/sqrt(1+x²))`; spherical output is `f*(atan(x), atan(y/sqrt(1+x²)))`, translated back by the principal point. This is not camera-pose bundle adjustment or a calibrated 360-degree stitch.

The nonlinear map is represented as an editable `Operation::Warp(WarpMesh)`: sampled quadrilateral patches are degree-elevated to bicubic Bezier control nets. A regular grid starts at 4x4 patches and doubles up to 128x128. Each patch is checked against the analytic map on 5x5 probes; acceptance requires maximum sampled Euclidean error <=0.05 destination pixel. Failure to meet this bound returns an error, never a homography substituted for the projection. This is a **sampled tolerance**, not a rigorous continuous error bound. The Bezier control hull supplies conservative union bounds (unlike corners alone).

**WarpMesh.width/height always remain original source dimensions, even when the compositor applies the op to a larger padded SmartObject child canvas.** Only destination control points are shifted by union origin. The padded-canvas regression test compares actual TransformOp renders.

### Calibrated lens correction

Enable either lens-removal flag and supply `lens_corrections: Vec<LensCorrection>`
with one entry per input in the same order. `LensCorrection { distortion: [k1,k2,k3],
vignette: [v1,v2,v3] }` defaults to identity. Coefficients use centered axis-normalized
coordinates `q = (2*x/width-1, 2*y/height-1)` in pixel-edge space, so source pixel
centers use x+0.5,y+0.5. Convert camera/profile coefficients into this convention
before calling; automatic profile lookup and scene-based inference are not included.

Distortion is the ideal-to-observed radial model `q*(1+k1*r²+k2*r⁴+k3*r⁶)`.
Temporary rectified images feed registration; final rendering does not use those
resampled images. The existing `lens::BrownConrady::undistort` maps original source
points to ideal coordinates, followed by estimated registration and optional
projection in a single WarpMesh with the same 0.05 px sampled tolerance. Thus the
document retains one editable source-to-union TransformOp, not a destructive
prewarp. Calibration must be finite (coefficient magnitude <=1), cover observed
radius sqrt(2) by ideal radius 2, and have radial derivative >=0.05 over [0,2].
Validation checks all extrema of the derivative polynomial, rejecting folds.
Only centered radial calibration is supported, not tangential/decentered/fisheye
models. Temporary registration views edge-extend missing samples; actual output
coverage always comes from original source support.

Vignette illumination is `1+v1*r²+v2*r⁴+v3*r⁶` at observed source centers. Its
reciprocal multiplies linear RGB before registration/geometry. Illumination must
be finite and in [0.05,20] at every source pixel; corrected RGB must remain finite.
`source_gains` supplies these source-size planes for non-destructive hosts (empty
when disabled). The compositor stores them as editable clipped F32 Multiply layers
before the geometry stage. The original RGB/masks remain untouched. Missing
calibration or invalid values return errors; flags never silently become no-ops.

## Registration

Auto/Perspective/nonlinear modes use seeded FAST/BRIEF homography RANSAC plus direct refinement, with rigid fallback. Reposition is translation only. Collage fits a true four-parameter similarity (uniform scale, rotation, x/y translation): feature homography initialization followed by robust direct least-squares constrained to that model. All non-Reposition modes connect through previously registered neighboring sources, processing outward from the selected reference, so the first and last images need not overlap. Both reference 0 and reference 2 are tested on three ordered crops; Auto also tests the disconnected endpoints.

Remaining registration limitations: no SIFT/scale-space descriptor, global bundle adjustment, or loop closure; large rotation/scale changes, low texture, parallax, repeated patterns, disconnected images, or an unordered overlap graph can fail or misregister. Sequential composition can accumulate drift. The rigid fallback is bounded to +/-3 degrees and 25% translation and requires same-sized textured images. Seed controls feature RANSAC deterministically (zero maps to nonzero PRNG state).

## Blending and focus

Panorama ownership uses deterministic sequential binary s/t graph cuts (Dinic max-flow), on a grid capped at 160 samples along the longest edge. Exclusive coverage supplies hard terminal constraints; RGB disagreement supplies pairwise seam cost. Upsampling labels checks full-resolution coverage and repairs uncovered coarse samples. This is not globally optimal multilabel alpha expansion and can miss sub-grid seam features. Full-resolution Laplacian/Gaussian multiband blending runs after ownership; additive corrections retain its result in editable layer form. Seamless tones uses per-channel overlap-mean gains, bounded 1/8..8, anchored to earlier covered layers; it does not model local illumination gradients. Panorama mode skips unnecessary focus scoring.

Focus uses a Gaussian pyramid (separable binomial kernel represented as a 5x5 convolution), absolute Laplacian residuals `G_l - expand(G_(l+1))` at every requested level, 5x5 local integration per band, and recursive bilinear expansion/summation of all band energies. The low-frequency DC residual is not a sharpness signal and is excluded. `pyramid_levels` controls the number of bands, capped naturally by image dimensions. Coverage is filtered alongside pixels; bands whose derivative support crosses missing data are suppressed. First-layer ties are deterministic. This is a multiscale focus measure, not depth estimation; strong sensor noise, textureless areas and focus-boundary halos remain limitations.

`BlendOptions.seed` is reserved for the parent CAF call; graph-cut/focus algorithms have no random stages. CAF remains delegated via fill_mask, not replaced by flood fill.

## Verification and performance

Parent verification after correcting the multiband source-support bug: crop RMS
0 / 0.001427 / 0.003601 px, overlap MAE 0.001221, ownership-boundary gradient MAE
0.005049 (threshold unchanged at 0.025). Multiband now extends RGB only outside
actual source coverage, not outside ownership, preserving real detail on both
sides of each seam. The complete required merge/compositor release-test, clippy
and fmt gate passed: 194 passed, 10 ignored, no failed functional tests.
Parent-run benchmark: align/render 49.787681667 s, panorama blend 8.519031916 s,
total 58.306713583 s; the <6 s assertion failed. Raw output is in
`../../tools/orchestrate/wp/M5-24/benchmark.log`, gate output in `gate.log` there.

`tests/layer_quality.rs` covers three shifted/rotated/scaled crops (first/last disconnected), <0.5 px RMS, overlap MAE <0.025, ownership-boundary gradient error <0.025, nonzero reference, nonlinear analytic accuracy/union, padded source extents, and >=95% three-source focus selection. Measured release results: crop RMS 0 / 0.001427 / 0.003601 px; overlap MAE 0.004771; analytic Gaussian-blurred focus ownership 99.7806%; low-frequency focus versus fine-noise distractors 99.1444%. Existing tests cover masks/correction reconstruction, coverage, tones, determinism, invalid inputs and translation-only behavior.

Ignored **real** benchmark:

```sh
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-24 cargo test -p merge --release --test layers benchmark_three_24mp_layers -- --ignored --nocapture
```

It creates three shifted 6000x4000 RGB inputs, measures Reposition registration + full union rendering, drops originals, then measures default Panorama graph-cut/multiband blending. Data generation is outside timers. The measured union is 6081x4000. It does not benchmark nonlinear warps or claim performance for all alignment modes.

**Current verification:** the supplied <6 s CPU benchmark passes. Layer registration
calls global-only rigid alignment rather than computing and discarding HDR tile
residuals; HDR retains its existing refinement. The latest run measured 1.012007750 s
alignment/render + 3.168050667 s blend = 4.180058417 s (`final-benchmark.log`).
This supersedes the historical timing failures above, but does not establish
performance for feature-based/projective/nonlinear registration, optional lens
correction or compositor document operations. Normal tests exclude this
allocation-heavy benchmark. The required release tests/clippy/fmt gate passes:
201 passed, 10 ignored, zero failures (`final-gate.log`).

`final-quality.log` reproduces the panorama RMS/MAE and focus metrics above.
`lens-registration.log` additionally measures three differently calibrated,
vignetted, shifted sources: RMS 0.003693 / 0.034394 / 0.018650 px, deterministic
geometry and RGB. `lens_layers`, `lens_merge`, and `lens_merge_roundtrip` test
analytic barrel/pincushion correction with both projections, color restoration,
editable gains, compositor agreement, native/PSD roundtrips and atomic errors.
