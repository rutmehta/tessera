# DCP reader and rendering contract

`src/dcp.rs` is an independent, dependency-free Rust implementation of a **documented subset** of the public DNG camera-profile format. No Adobe SDK/source code or proprietary profile assets are included. It is not a claim of Adobe Camera Raw/Lightroom rendering parity.

## API and input contract

```rust
DcpProfile::parse(bytes: &[u8]) -> Result<DcpProfile, String>
profile.apply(rgb: [f32; 3], temperature: f32) -> [f32; 3]
```

Input RGB is **linear, normalized, un-white-balanced, reference-camera RGB**, after black subtraction, sensor normalization, and demosaicing. This is not sRGB, Rec.2020, or an already white-balanced image. The caller must account for per-camera calibration and analog balance upstream; this API has no image metadata with which to do so. The temperature parameter supplies white balance in Kelvin (no tint or custom xy parameter). Output is **linear Rec.2020, D65**, with no Rec.2020 transfer function applied.

Nonfinite input channels become zero. Nonfinite/nonpositive temperature uses CalibrationIlluminant1. Other temperatures are clamped to 1667–25000 K. Matrix-only conversion retains negative and above-one channels; final values are limited only to the finite f32 range. HSV processing clips negative ProPhoto channels to zero and modified S/V to [0,1]; the tone curve clamps its input/output to [0,1]. Final Rec.2020 conversion can again produce negative or above-one values for out-of-gamut colors. There is no hidden output gamut mapping.

## Parsing

- Supports little-endian `II` and big-endian `MM`, classic TIFF magic **42**, and the DNG camera-profile magic **0x4352**. Decimal 4352 is not a valid DCP magic and is rejected.
- Reads a single top-level IFD at its header-relative offset. A nonzero next-IFD pointer is rejected, so cycles and unbounded chains are not followed. This is a standalone profile reader, not a DNG image decoder or ExtraCameraProfiles selector.
- All IFD storage and field payloads, including unknown metadata, are checked against the input slice. Size/offset additions and payload products are checked. Unknown TIFF field types are rejected; known TIFF types on unknown tags are skipped after bounds checking.
- Consumed fields require their specified TIFF type and count. Signed rationals respect byte order and signed denominators. Zero rational denominators, nonfinite values, repeated consumed tags, singular/ill-conditioned matrices, inconsistent matrix/illuminant pairs, and invalid LUT/curve dimensions are rejected.
- Individual consumed fields are limited to 3,000,000 numeric values; total consumed fields to 6,000,000. A tone curve has at most 65,536 points, with successive x coordinates separated by at least 1e-8. These are intentional resource/numerical limits, not limits imposed by the file standard.

## Supported profile fields

| Field | Decimal tag | Handling |
|---|---:|---|
| ColorMatrix1 / 2 | 50721 / 50722 | 3×3 SRATIONAL, row-major XYZ-to-camera matrices |
| CalibrationIlluminant1 / 2 | 50778 / 50779 | SHORT; paired with their color matrix |
| ForwardMatrix1 / 2 | 50964 / 50965 | Optional 3×3 SRATIONAL camera-to-XYZ-D50 matrices |
| ProfileHueSatMapDims | 50937 | LONG[3]: hue, saturation, value divisions |
| ProfileHueSatMapData1 / 2 | 50938 / 50939 | FLOAT triples, V outer / H middle / S inner |
| ProfileLookTableDims / Data | 50981 / 50982 | Same dimensions/data layout as HueSatMap |
| ProfileHueSatMapEncoding / ProfileLookTableEncoding | 51107 / 51108 | LONG: 0 linear (default), 1 sRGB value encoding |
| ProfileToneCurve | 50940 | FLOAT input/output pairs in linear gamma |

Hue divisions must be >=1, saturation >=2, value >=1. Data count must exactly match the dimensions times three. Saturation/value scales cannot be negative; zero-saturation cells must have value scale 1 within 1e-6. A second HueSatMap requires the first and dual illuminants. One HueSatMap is used at every temperature. Missing tables/curve mean identity stages, not an inferred Adobe default curve.

## Rendering order

1. **Choose/interpolate calibration.** Interpolate ColorMatrix1/2 in reciprocal Kelvin, clamped to the nearest endpoint. Invert the interpolated matrix, not the endpoint inverses. Calibration order may be warm-first or cool-first. A singular interpolation between valid endpoints falls back to the nearer endpoint inverse.
2. **Camera to XYZ D50.** Without forward matrices, invert the color matrix and Bradford-adapt the selected white point to D50. With forward matrices, interpolate them with the same weight, derive camera neutral as `ColorMatrix × whiteXYZ`, divide camera channels by that neutral, then apply the forward matrix. Forward matrices must map unit RGB to D50 within 0.002 and must be invertible. A dual profile using forward matrices must supply both. If the selected neutral has a nonpositive/near-zero channel, use the inverse-color-matrix path instead of dividing by it.
3. **HueSatMap.** Convert XYZ D50 to linear ProPhoto RGB (RIMM), then HSV. Trilinearly sample the table, wrapping hue; interpolate dual table corrections with the same reciprocal-temperature weight. Add hue shift in degrees and multiply saturation/value by their scales.
4. **LookTable.** Apply the look table to the result of HueSatMap, in the same ProPhoto/HSV space. No exposure/fill-light operation is inserted by this API.
5. **ToneCurve.** Apply a natural cubic spline independently to linear ProPhoto channels. The first and last points must be (0,0) and (1,1); x must strictly increase and all coordinates must be in [0,1]. Spline overshoot is clipped. This per-channel choice is explicit and does not reproduce vendor-specific hue-preserving tone rendering.
6. **Working output.** ProPhoto → XYZ D50 → Bradford adaptation to D65 → linear Rec.2020.

For 3D LUT encoding 1, **only HSV V** is sRGB-encoded before lookup and scaling, then decoded after the correction. Hue/saturation are not gamma-transformed. The encoding tag has no effect when the value dimension is one. Lookup coordinates are bounded to the table domain; hue wraps continuously at 360 degrees.

## White-point approximations and unsupported features

The standalone renderer uses `apply_without_tone` for the matrix/table part and
`apply_tone` on working Rec.2020 after Detail and the basic tone sliders. This
keeps the DCP tone curve from preceding exposure or being applied twice.
`render_*_with_profile` takes an explicitly resolved profile. It undoes the native
preprocessing WB/profile matrix before DCP conversion, then approximates tint by
the residual native CAT16 matrix at fixed temperature. This assumes native point
optics do not clip and spatial resampling commutes with those matrices. Native
AsShot estimation determines the interpolation temperature, not a DCP iterative
camera-neutral solver. The profile itself never reads a filesystem path.

Calibration illuminant codes supported: daylight/fine weather/flash (1/9/4, 5500 K), tungsten/standard A (3/17, 2856 K), cloudy/D65 (10/21, 6504 K), shade/D75 (11/22, 7504 K), standard B (18, 4874 K), standard C (19, 6774 K), D55 (20, 5503 K), D50 (23, 5003 K), ISO studio tungsten (24, 3200 K). Unknown, fluorescent, and custom/spectral illuminants are rejected rather than assigned an invented temperature.

The temperature-to-xy model uses an analytic daylight-locus approximation at >=4000 K and a Planckian-locus approximation below that, with exact conventional D50/D65 white coordinates at their listed temperatures. This is an approximation, especially for standard B/C and real-world mixed/fluorescent lighting. There is no tint control, camera-neutral iterative solver, measured illuminant spectrum, or Robertson CCT solver.

Explicitly rejected: BigTIFF, IFD chains, unsupported field types/encodings, non-three-channel matrices, and third-illuminant calibration fields. Custom illuminants are rejected by their illuminant code.

Other tags are not interpreted: camera-model restrictions, per-image CameraCalibration/AnalogBalance, reduction matrices, exposure offsets, black-render hints, profile gain maps, RGB/semantic tables, opcodes, copyright/embed policy enforcement, and vendor private metadata. Their presence does not imply that their effects are applied. Profiles requiring these extensions are outside this subset. No camera-model match is checked because the API does not receive a model identifier. Callers remain responsible for profile provenance and licensing.

## Public sources

- [DNG Specification 1.6.0.0 (Adobe, public format specification)](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/dng_spec_1_6_0_0.pdf): p.46 standalone camera-profile header; pp.47–49 HueSatMap/linear-gamma tone-curve tags; p.55 LookTable ordering; pp.61–62 sRGB value encoding; pp.85–88 reciprocal-temperature calibration, forward matrices, Bradford adaptation, ProPhoto HSV layout/interpolation and clipping. This document, not SDK code, is the implementation reference.
- [ExifTool EXIF/DNG tag reference](https://exiftool.org/TagNames/EXIF.html): independent cross-check of numeric tag IDs/types and encoding values.
- [Bruce Lindbloom RGB/XYZ matrices](http://www.brucelindbloom.com/index.html?Eqn_RGB_XYZ_Matrix.html): conventional ProPhoto primary/white-point matrices.
- [ITU-R BT.2020](https://www.itu.int/rec/R-REC-BT.2020): Rec.2020 primaries and D65 reference white; output here is linear, not transfer-encoded.

## Tests

Unit tests build synthetic little/big-endian TIFF payloads; no licensed camera profiles are bundled. They check header variants, known XYZ/Rec.2020 anchors, matrix inversion and inverse-Kelvin interpolation, forward-matrix neutral handling, noncommutative HueSatMap → LookTable → ToneCurve ordering, HSV trilinear interpolation and hue wrap, sRGB value indexing, dual HueSatMap selection, cubic interpolation, malformed metadata, every truncated prefix of a valid profile, and deterministic mutated-profile inputs. These are numerical/structural regression tests, not a vendor-rendering comparison.
