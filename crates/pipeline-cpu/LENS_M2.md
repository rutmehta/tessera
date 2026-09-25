# M2-09 CPU optics integration

## Implemented and verified

The CPU renderer now resolves lens corrections, applies scene-linear lens vignette
and manual defringe before detail/tone. Lateral CA is corrected separately before
camera-profile mixing: supplied exact profiles use Bayer same-phase sublattice
bilinear resampling before the output demosaic; X-Trans, image-estimated and
OpcodeList3 corrections use original demosaiced camera RGB before matrices/tone.
Common distortion, Upright, manual transforms, straighten and crop retain **one
final inverse Lanczos3 resampling pass**. The integer sensor-margin extraction and final linear box
preview reduction remain separate; neither is an additional geometric warp.
No engine-api fields or contracts were changed. This handoff wires the synchronous
pipeline-cpu renderer; external image-core/GPU stage adapters are outside this
crate's ownership and were not changed.

### Profile inputs and priority

`render_linear_scaled_with_lens(settings, source, scale, &LensContext)` accepts
caller-owned `lens::Profile` / `lens::ProfileDatabase` values. Existing render
entry points use an empty context. `resolve_lens` exposes the selected
`CorrectionSource` and interpolated `CalibrationSample` (when applicable), so a
caller can inspect or save image estimates with the lens crate's profile API.
There is no implicit filesystem lookup, network access or bundled database.

* Auto: supported embedded data > supplied profile/database match > image
  calibration > manual-only fallback. This is **whole-source priority**, not
  per-coefficient merging. An identity supplied calibration is authoritative.
* Embedded explicitly requested but unavailable is an error.
* Database explicitly requested but not supplied/found is an error. A direct
  context profile is an explicitly resolved selection; otherwise the named
  profile matches `Profile.model`. Filename/digest are not resolved or verified.
* Auto database matching uses camera make/model as camera restrictions and the
  lens-model metadata for fuzzy lens selection. Lens maker is unavailable in
  RawMetadata and is not inferred from camera make (third-party lenses work).
  Equally ranked matches require explicit selection rather than an arbitrary
  calibration. No vendor correction coefficients are inferred from identity.
* AutoCalibrated bypasses embedded/database. None disables profile correction,
  but the independent `remove_chromatic_aberration` switch can still estimate CA.
* Manual distortion/vignette/defringe remain additive user controls for all sources.
* `LensContext.capture = Some([focal_mm, aperture, distance_m])` supplies capture
  coordinates. Otherwise focal/aperture come from metadata when positive and
  finite; missing coordinates, including unavailable focus distance, use the
  first supplied calibration sample's coordinates. This is an explicit fallback,
  not a measured or fabricated focus distance.

`CalibrationSample::distort` retains PTLens odd-radius and native profile metric
terms. CA uses each channel's radial coefficients, not averaged coefficients.
Profile strength scales displacement/gain departures from identity. The remove-CA
switch suppresses channel-specific correction independently of distortion.
Lensfun sensor crop/aspect normalization must already be encoded in supplied
`coordinate_scale`; this adapter does not invent crop-factor metadata.

### Image calibration and manual operators

Analysis is bounded to a 256-pixel long edge, with endpoint-aligned nearest
samples, never CFA decimation. It runs on visible-frame demosaiced camera-linear
RGB (or the supplied linear Rec.2020 RGB source), before white-balance/matrices.
The current resolver uses a preliminary demosaic even for exact Bayer profiles;
only the subsequently CA-corrected CFA demosaic enters the output pipeline.
Line traces fit k1 within [-0.3,0.3]; accepted k1/CA confidence is >=0.8,
vignette confidence >=0.9. Tiny numerical identity estimates are ignored.
CA estimates at most 2% fractional radial shift. Flat/degenerate data falls back
without correction. Confidence is heuristic and scene content can fool these
estimators; no real-photo optical accuracy claim is made.

Profile vignette interprets coefficients as illumination, takes the reciprocal,
blends strength and caps gain to [1/8,8]. DNG coefficients instead represent gain.
Manual vignette uses gain `2^((amount/50) * r2^(0.25+3.75*midpoint/100))`, where
r2 is axis-normalized squared radius divided by two. Positive amount brightens.
This is a documented native control, not an Adobe slider-equivalence claim.
There is no spatial noise-model feedback into denoise.

Defringe uses RGB hue in degrees with inclusive bands (including wrapped ranges),
relative chroma >15%, and a four-neighbor relative luminance edge threshold of
20%. Amount /20 desaturates selected pixels toward **their own luminance**.
It does not desaturate flat coloured areas. This is an axial-fringe heuristic,
not a classifier, perceptual hue model or neighboring-luminance reconstruction.

### Geometry

Destination crop/straighten -> inverse user transform -> inverse Upright ->
manual distortion -> selected common (green) distortion -> source sample.
CA never runs in this final map. Its earlier lookup preserves the full sensor
CFA phase and uses default-crop-relative optical coordinates. Bayer red/blue
interpolate only their own two-pixel sublattice; both green phases are unchanged.
Bilinear edge taps clamp within the same sublattice (RGB fallback clamps within
its channel). Disabled/zero-strength CA clones without interpolation.
For embedded warps the early channel residual is F_channel o inverse(F_green),
not a displacement subtraction; noninvertible maps return an error. RGB-only
sources have no recoverable camera primaries, so their supplied linear Rec.2020
planes are aligned before white balance/tone; this is not claimed to recover CFA.
Manual distortion maps amount/200 to Brown k1. User transform is forward
perspective, scale/aspect, pixel-metric rotation, then offset; the lookup reverses
that order. Perspective coefficients are horizontal/200 and vertical/200;
aspect is `2^(aspect/100)`; offsets are setting/50 in centered coordinates.
Out-of-source pixel centers become black; valid edge taps clamp to source edges.
The inactive geometry path clones pixels without sampling. Existing crop-only
arithmetic and disabled-lens immutable goldens are preserved.

Upright supports Off, Auto, Level, Vertical, Full and Guided. Detected line points
and guides are numerically inverse-mapped through the complete green lens lookup
before estimation, avoiding an intermediate corrected image. Guided mode requires
2–4 valid normalized guides; their axis is inferred from dominant pixel direction
because engine-api has no guide-axis field. Degenerate guides are errors; no
usable automatic evidence produces identity. Upright is projective in the lens
crate's independently axis-normalized convention, not camera-calibrated metric
rectification. Estimation occurs at the geometry stage; tone/effects can therefore
influence automatic line detection. Maps are not cached across renders.

### Embedded DNG subset and boundaries

Selected-IFD raw metadata is used, never unrelated preview opcode lists.
The supported subset is OpcodeList3 WarpRectilinear (one or three planes) and
FixVignetteRadial preceding any warp. Radius is physical pixel distance divided
by the farthest sensor-plane corner from the declared center. Coordinates retain
full sensor dimensions and the default-crop offset; no independent DNG ActiveArea
or pixel-aspect metadata is available here. Thus this is not complete DNG SDK
stage/coordinate-conformance coverage.

* Malformed list framing fails. Required unknown opcodes (including GainMap)
  fail rather than silently disappearing. FixBadPixelsConstant/List (IDs 4/5)
  are intentionally ignored, including required instances, per requested policy;
  this is not bad-pixel execution. Unknown optional opcodes may be skipped after
  framing validation.
* Supported opcodes requiring a version newer than 1.3 or unknown flags fail
  unless optional. Preview-skip is not used: even scaled output runs full render.
* Supported lens opcodes in OpcodeList1/2 fail explicitly: they are **not** moved
  to an incorrect post-demosaic stage. Two-plane warps fail for this RGB pipeline.
* Warp lookups compose in reverse execution order. Three-plane coefficients stay
  independent, with green carrying common geometry. Vignette after a warp fails
  because that stage-coordinate gain composition has not been implemented.
* Single-plane scalar vignette is pre-tone. OpcodeList3 CA uses original camera
  RGB immediately after demosaic, before camera conversion or nonlinear tone.
  Only common geometry is delayed to the final shared map. This is not full DNG
  stage-coordinate conformance: gain/warp composition remains the bounded subset
  described above.

Not implemented: proprietary ARW/RAF/CR3/NEF calibration tables (not exposed by
LibRaw), GainMap execution, pre-demosaic X-Trans CA, PSF/softness correction,
volume deformation, content-aware fill, geometric map caching, EXIF orientation
application or constrain-crop. Explicit non-default softness, geometry orientation
or constrain-crop returns Unsupported. File orientation remains the existing
renderer behavior. Crop aspect is a UI constraint; the stored rectangle determines
output dimensions, as before. No unsupported feature is silently represented as
an applied correction.

## Validation

Tests were introduced and observed failing for missing manual controls, context
resolution, image calibration, embedded priority/stage validation, Guided Upright,
and required-unknown opcode rejection, then implemented. Synthetic coverage also
checks auto-CA edge-error reduction, profile vignette/channel maps, database vs
image/manual priority, three-plane embedded CA, hue edge selectivity, zero profile
strength identity and a counted one-lookup-per-channel composed geometry map with
an independent source-coordinate reference. Stage regressions use actual CFA
inputs across all Bayer phases, odd crop origins, MHC and bilinear demosaic, and
compare against independently pre-corrected CFA through mixed camera matrices and
nonlinear contrast. Embedded fallback has an independent camera-RGB interpolation,
matrix and tone reference for Bayer and X-Trans. The old stage path failed the
CFA regression (max error 0.24613641); disabling the new RGB fallback failed its
regression (max error 0.61032796). Required bad-pixel skipping was observed failing
before implementation. Working-space green is correctly allowed to change when
camera red/blue corrections propagate through the color matrix.

Executed in the M2-09 target directory:

```
cargo fmt -p pipeline-cpu --check
cargo clippy -q -p pipeline-cpu --all-targets -- -D warnings
cargo test --release -q -p pipeline-cpu -- --include-ignored --nocapture
```

All **78 tests passed, 0 failed, 0 ignored** in the stage-correct CA acceptance run. The
real-RAW acceptance test is normally ignored because it needs external fixtures;
`--include-ignored` explicitly ran it. It fails if required fixtures are absent.

Five actual full-resolution CFA renders, downsampled only after processing by 8,
with Auto lens and Auto Upright enabled, produced finite f32 output:

| Fixture | Output |
|---|---|
| canon-cr3.CR3 | 500 x 500 |
| sony-arw.ARW | 615 x 410 |
| nikon-nef.NEF | 923 x 616 |
| fuji-raf.RAF | 612 x 408 |
| sample.dng | 652 x 434 |

All five report **no opcode-list bytes**. They verify actual decode/render
robustness, not embedded/vendor correction accuracy. Embedded tests are synthetic.
All five immutable pre-optics goldens have **0/255 maximum error** with profile
None and remove-CA false; those tests now explicitly select this off path. Auto
was previously inert and can intentionally change images now; no golden was
regenerated or weakened with a tolerance.
