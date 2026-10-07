# DCP and embedded DNG rendering

The reader implements a bounded subset of the public DNG camera-profile model.
It bundles no Adobe Color, Adobe Standard, or other binary camera profiles.
`dcp_acr3.rs` contains the publicly published numeric ACR3 default tone samples,
with their source citation. This is a compatibility implementation, not a claim
that Tessera reproduces Lightroom's complete process-version renderer.

## Profile selection and scope

`RawImage::open` snapshots DNG profile metadata beside the decoded source.
Only Adobe process-family rendering of an external camera-linear LinearRaw
Smart Preview whose recipe names an Adobe profile can substitute that metadata
when no installed DCP was supplied. The scalar Adobe renderer applies the same
scope. An explicitly supplied installed DCP always wins. Ordinary CFA originals,
working-space RGB, Native recipes, and proxy recipes without an Adobe profile
name do not use this fallback. An Adobe profile name alone never changes the
process family; it is inert metadata in Native rendering.

Successful substitution reports `profile substituted (embedded DNG profile)`
through the existing Develop per-photo notice display. Missing, malformed or
unsupported embedded profiles use the previous no-profile rendering behaviour
and report an informational notice. Parser reasons and source identities are
not included in these messages. Profile names are identities, never paths.

Tessera-generated native Smart Preview admission is unchanged. Embedded profile
bytes enter an Adobe-only cache identity. Native keys and operators remain
unchanged. `DcpProfile::parse` retains installed profiles' historical rendering;
`parse_embedded` explicitly opts into the SDK defaults for proxy substitution.

## Reading and limits

`dcp::read_embedded_profile(&mut impl Read + Seek)` returns a small TIFF containing
only profile metadata. It reads IFD0 and the full camera IFD, inherits root tags
missing from a raw SubIFD, and never reads image strips or tiles. Both byte orders
and classic TIFF are supported. Traversal is bounded to 32 IFDs, 4,096 entries per
IFD, 24 MiB per field, and 48 MiB total reads. Cycles, duplicate consumed tags,
invalid offsets, and oversized payloads are rejected. ExtraCameraProfiles
selection and BigTIFF are not implemented.

`DcpProfile::parse_embedded` accepts those metadata bytes, standalone TIFF/DCP
data, or a DNG byte slice. It reads ColorMatrix1/2, ForwardMatrix1/2,
CalibrationIlluminant1/2, ProfileHueSatMapDims/Data1/Data2,
ProfileLookTableDims/Data, ProfileToneCurve, BaselineExposure,
BaselineExposureOffset, DefaultBlackRender, both HSV encoding tags, and
ColorimetricReference. Matrices, rational denominators, finite values, dimensions,
and consumed types/counts are validated. A field is limited to 3,000,000 numeric
values and a profile to 6,000,000; explicit curves have at most 65,536 points.

Absent CalibrationIlluminant tags default to 0. Unknown (0) and Other (255),
without spectral data, use the first matrix. Every EXIF illuminant is accepted;
fluorescent codes 2/14, 12, 13, 15 and 16 map to 4150, 6400, 5050, 3575 and
2925 K respectively, using the SDK's interval midpoints. Reserved values remain
errors. Unused second matrices/tables are still validated. Triple illuminants
remain unsupported. ForwardMatrix's D50 unit-neutral tolerance remains 0.002,
unchanged from main: a rejection is handled by the non-fatal fallback, not a
looser bound. Output-referred HDR (`ColorimetricReference=2`) is not silently
treated as SDR by the embedded reader.

## Render order

Input is normalized, un-white-balanced, reference-camera RGB. Output is linear
Rec.2020 D65, before display encoding. The renderer resolves white balance once
and, for embedded proxy profiles, applies these stages in order:

1. White balance and camera-to-XYZ D50 conversion. Camera neutral is normalized
   to a maximum of one, as in the public SDK. ForwardMatrix uses the diagonal
   inverse neutral; without it, ColorMatrix inversion and Bradford adaptation
   produce D50. Dual calibrations interpolate in reciprocal temperature.
2. Convert to ProPhoto D50 and apply HueSatMap. Tables use trilinear interpolation,
   hue wrap, and the same illuminant weight as the matrices.
3. Apply source BaselineExposure plus user exposure plus profile
   BaselineExposureOffset exactly once. The standalone renderer undoes the
   native preprocessing matrix before the profile. Native has no baseline gain.
4. Apply ProfileLookTable after exposure and the basic tone adjustments.
5. Apply the explicit profile curve, or the public SDK ACR3 default when absent.
   Explicit curves use natural cubic interpolation. Tone maps the channel extrema
   and interpolates the middle channel, following `RefBaselineRGBTone` to preserve
   hue. Display output does not add Tessera's native sigmoid.

For encoding flag 1, only HSV V is sRGB-encoded before table lookup and scaling,
then decoded afterward. The flag has no effect on a table with one value division.
Table lookup coordinates and saturation remain bounded. HueSatMap value outputs
retain float headroom above one, including encoded tables, until exposure has
run. The post-exposure SDR LookTable and profile tone inputs retain their clamps.

`ColorimetricReference=1` is output-referred: its absent profile curve means
identity and automatic shadow subtraction is disabled, following
`dng_render::Render`. An explicit profile curve still takes precedence. This
prevents an additional default tone curve on already rendered DNG sources.

The low-level `apply_without_tone` method retains its unit-Y temperature-white
convention for calibration/operator checks. Embedded rendering uses
`resolve_white_balance` and `apply_camera` with a normalized selected neutral.
`apply_exposure`, `apply_look`, and `apply_tone` expose the later stages separately.

## Native and installed-profile compatibility

Native rendering applies no BaselineExposure. The field remains source data,
including in generated preview serialization. Adobe rendering adds source
BaselineExposure and user exposure once at Tone, after the white-balanced prefix.
The prefix itself skips Adobe Tone so exposure and black subtraction cannot run
twice. Without a resolved DCP the existing Adobe default curve remains in place.

Installed DCPs retain main's unit-Y temperature calibration, native tint residual,
pre-tone LookTable placement, per-channel explicit curve, and exact identity
when no curve exists (including headroom). They do not inherit embedded ACR3,
automatic black subtraction, or DNG-only profile defaults. Source baseline plus
user exposure still belongs to the Adobe Tone stage. The restored fixtures keep
their original matrix scaling and absent-curve cases; the CFA expectation is
`0.2`, not the SDK-normalized `0.21781155` used by the rejected broad fallback.

## Exact behavior and approximations

The embedded tag interpretation, reciprocal-temperature table interpolation,
HSV indexing/order/encoding, exposure addition, published ACR3 sample values and
linear table interpolation, output-referred default policy, and black-render
None policy are explicit, tested behavior. Synthetic tests cover hue wrap,
hand-computed encoded lookups, neutral preservation with and without a forward
matrix, stage order, both TIFF byte orders, bounded reads, and source dispatch.

DefaultBlackRender Auto uses the SDK's piecewise exposure-ramp shape with fixed
shadows=5 and unit ShadowScale/Stage3Gain (black=0.005). The ramp uses the combined
exposure gain; it does not reproduce the SDK's separate negative-exposure
highlight shoulder or Lightroom's image-dependent shadows heuristic. The DNG
specification leaves the Auto subtraction amount/method reader-dependent.
DefaultBlackRender None performs no subtraction.

As-shot camera neutral is used directly. Its interpolation CCT is found by bounded
iteration and nearest-locus search in CIE 1960 uv, using analytic daylight and
Planckian loci; this approximates the SDK's Robertson temperature solver. Custom
tint uses Tessera's existing Duv convention, not Adobe's proprietary slider
mapping. White balance is applied before profile tables, not as a tint correction
afterward. Rounded color matrices cause small floating-point conversion error.

Per-camera AnalogBalance/CameraCalibration signature matching, custom spectral
illuminants, HDR profile extensions, and ExtraCameraProfiles selection are not
added here. The existing reference-camera input contract still applies. Native
preprocessing demosaic/denoise/optics, detail and basic tone controls, spatial
resampling, and other imported operators retain their documented approximations.
Undoing the native preprocessing matrix assumes those optics do not clip and
commute with the matrix. No image-specific fitting is performed.

## Public references

- [DNG Specification 1.7.1](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/DNG_Spec_1_7_1_0.pdf):
  ColorimetricReference pp. 46–47; profile tags pp. 49–57; DefaultBlackRender and
  exposure/encoding pp. 62–65; camera transforms and HSV tables pp. 100–104.
- [SDK dng_render.cpp](https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_render.cpp):
  ACR3 table, exposure ramp, shadows defaults, and output-referred policy.
- [SDK dng_reference.cpp](https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_reference.cpp):
  `RefBaselineRGBTone` channel-extrema interpolation.
- [SDK dng_color_spec.cpp](https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_color_spec.cpp):
  camera-neutral normalization, forward matrix, and unknown-illuminant fallback.
