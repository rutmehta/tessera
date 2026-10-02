# LR-9b — real-edit warning causes

Local Machine B work on `wp/LR-9-default-noise`, commits on top of `50f114da`.
Synthetic fixtures only. Real-catalog evidence is aggregate-only; no source
strings, identifiers, image names, resource names or media paths are recorded.
The supplied scratch catalog is opened read-only. No app is built or launched.

## Golden changes justified per row

- `point-color-compat.txt`, synthetic `legacy` row: the row is imported with modern
  PV despite its fixture label. `FillLight=0` now has one ignored LR-9b diagnostic
  because legacy tone controls are inactive in modern PV. Bytes change from 11,633
  to 11,869; hash changes from `fe85a1a43d4268ab1aa6f24ba8add80325b3cd274d5b54e181f66bfbe6e438ce`
  to `413c4cd0ebbbd90c84a1e980cfef82a573ba32555ec2b2f71b6af547ecf755a5`.
  `legacy_fixture_byte_change_is_only_the_inactive_fill_light_note` removes only
  that diagnostics object and proves the entire previous byte length and hash.
  Settings, history and retained source are unchanged. Every other row keeps its pin.
- `lr6b-untranslated-baseline.json`, synthetic empty `DepthBasedCorrections` row:
  only the warning becomes `depth-based local correction structure is not implemented`.
  Complete `recipe_bytes` remain unchanged and are still compared verbatim by the
  test. This replaces the generic unsupported-property text with a feature name.

## Evidence boundary

All new geometry/retouch translations are approximate. Adobe-rendered pixel parity
is not asserted. Documented structure names are cross-checked against
[ExifTool's original XMP tag reference](https://exiftool.org/TagNames/XMP.html).
The absolute-Y interpretation of `OffsetY` is an inference from the paired source
coordinate fields in [the published Camera Raw example](https://community.adobe.com/questions-563/photoshop-camera-raw-14-0-throws-away-masks-180486),
with a synthetic displacement regression. Retained Adobe source remains available.

## Final aggregate results

Measured against production commit `0df613d3`; documentation commit follows it.
Counts are image/key occurrences, not distinct images, unless explicitly stated.
No real source strings, values, identifiers or media paths appear in the artifacts.

| Measure | Before | After |
|---|---:|---:|
| Develop warnings | 7,658 | 715 |
| Generic retained/unsupported noise | present | 0 |
| Not fully supported report groups | 78 | 70 |
| Mask images warning | 697 | 71 |
| RetouchAreas images warning | 330 | 38 |
| RemoveAreas images warning | 42 | 36 |

Develop warnings fall by 6,943 (90.66%). Approximate means Tessera can represent
and render the edit with its own implementation; it does not establish Adobe
pixel parity. AI masks require regeneration; individual AI object instances are
explicitly rejected rather than silently broadened to a whole object/subject.

## Legacy process classification

All 21,615 audited Develop rows use PV2012+; no genuine PV2003/PV2010 rows occur.
For the affected modern rows, Brightness, Contrast and Shadows each contributed
1,896 warnings (5,688 total). Exposure, FillLight and HighlightRecovery each
contributed another 27. All six now emit ignored diagnostics, zero warnings and
no report entry. They are inactive leftovers in modern PV.

| Process family | Affected Brightness/Contrast/Shadows images | Exposure images | Before warnings for these four keys | After |
|---|---:|---:|---:|---:|
| PV2012+ | 1,896 | 27 | 5,715 | 0 |
| PV2003/PV2010 | 0 | 0 | 0 | 0 |

Synthetic tests exercise both genuine legacy families and verify the existing
`legacy_pv2010` approximate translation remains populated and warning-free.

## Mask causes and disposition

Of 697 mask images: **0 exact, 626 approximate with zero mask warnings, 71 still
warn**. Promotion accepts documented metadata and provably inactive parameters;
it still refuses active unsupported adjustments. Counts below overlap.

| Previously blocking sub-key names / classes | Images containing keys | Disposition |
|---|---:|---|
| CorrectionReferenceX, CorrectionReferenceY | 697 each | Accept reference metadata |
| LocalBrightness, LocalContrast, LocalExposure, LocalClarity | 697 each | Accept neutral legacy local controls |
| LocalCurveRefineSaturation | 697 | Accept neutral setting |
| LocalGrain | 656 | Accept zero only |
| LocalCorrectedDepth | 87 | Accept zero only |
| LocalColorVariance | 18 | Accept zero scalar/vector only |
| LocalPointColors | 39 | Accept empty only; active selection still warns |
| LocalToningHue / LocalToningSaturation | 697 each | Hue is inactive when saturation is zero; active overlay still warns |
| CorrectionMasks.Version | 46 | Accept documented radial version metadata |
| CorrectionMasks.FullMaskSize | 29 | Accept AI raster metadata for regeneration |
| CorrectionMasks.LocalInputDigest / LocalInputDigestVersion | 2 each | Accept AI provenance metadata |

| Observed mask kind | Images containing kind |
|---|---:|
| Brush (nested) | 32 |
| Gradient | 172 |
| Radial | 46 |
| Luminance range | 3 |
| Colour range | 5 |
| AI subject | 143 |
| AI sky | 462 |
| AI object | 63 |
| AI person part | 48 |
| Nested group | 32 |
| Depth range / AI background / whole AI people | 0 observed |

Kind counts overlap and do not mean the kind caused rejection. The earlier
catch-all diagnostic wrongly blamed supported masks when local adjustments
blocked promotion. `MaskValue` and nested composition retain their existing
semantics and are not residual blockers here.

The following mutually exclusive combinations partition the 71 residual images:

| Remaining mask reason class | Images |
|---|---:|
| Individual AI instance selection (all observed parents are objects) | 17 |
| Local color overlay | 9 |
| Local defringe | 1 |
| Local defringe + color overlay | 1 |
| Local point color | 17 |
| Local tone curve | 2 |
| Local tone curve + color overlay | 1 |
| Local tone curve + point color | 22 |
| Conflicting radial inversion flags | 1 |

Observed curve key counts are `MainCurve` 23, `ExtendedMainCurve` 23,
`BlueCurve` 3 and `ExtendedBlueCurve` 2 (overlapping). Other rejected active
adjustments use `LocalPointColors`, `LocalDefringe`, and `LocalToningSaturation`.
AI instances use `InstanceIDs` and `InstanceBounds`, 17 images each. The full
field-name/count inventory is in `aggregate-after.json`.

## Retouch causes and disposition

| Source | Before warning images | Approximate, zero warnings | Cloud-only, zero warnings | Still warn |
|---|---:|---:|---:|---:|
| RetouchAreas | 330 | 289 | 3 | 38 |
| RemoveAreas | 42 | 0 | 6 | 36 |

Modern RetouchAreas contain heal on 267 images, clone on 23 and Adobe patch
removal on 41; these overlap. Gaussian methods occur on 289 images. RemoveAreas
are patch/cloud removal on all 42 images. Brush selections occur on 326
RetouchAreas images; ellipse selections on 26. Patch payload fields remain
unsupported because they require Adobe patch pixels, not ordinary heal/clone.

| Unsupported payload field name | RetouchAreas images | RemoveAreas images |
|---|---:|---:|
| pm_patch | 41 | 42 |
| pm_source_type | 41 | 42 |
| pm_clio_model_version | 3 | 8 |
| pm_patch_variations | 3 | 8 |

Payload field counts overlap. Cloud-marked edits receive the cloud note; a mixed
image still warns for its separate non-cloud patch edits.

The 289 promoted images contain `OffsetY`, `SourceX`, `Feather` and `HealVersion`.
Fixes accept documented metadata and equal upper/lower-case aliases, interpret
OffsetY as absolute source Y before deriving displacement, decode stateful
radius/flow/hardness brush stamps, and preserve bounded off-image stamps.
Redundant ellipse representations are accepted only for an exactly matching flat
circle. Conflicting aliases, malformed modern rows and unsupported selections
fail closed; legacy RetouchInfo cannot silently replace an authoritative modern
edit. RemoveAreas IDs append after existing retouch IDs.

There are **10 distinct images** with one ignored-style note:
“requires Adobe cloud; not translatable.” Cloud and non-cloud patch edits can
coexist, so this total is not the sum of the two cloud-only columns. An active
EnableDistractionRemoval switch contributes to this unified note. The remaining
38 RetouchAreas and 36 RemoveAreas warnings explicitly identify content-aware
Adobe patch pixels and unimplemented patch decoding. RetouchInfo's three old
warnings disappear; all 26 present legacy representations are ignored in favor
of modern source authority.

## Other dispositions

- SDR controls remain named unsupported SDR rendition features: 126 occurrences.
- Relative white balance remains explicitly unsupported without rendered-image
  calibration: IncrementalTemperature 22 and IncrementalTint 20.
- OverrideLookVignette remains a named embedded-profile override gap: 10.
- ConvertToGrayscale is already approximated (two active source occurrences); zero warnings.
- Upright centre controls already translate (one source occurrence); zero warnings.
- AutoWhiteVersion (52), Preset (146), and CropConstrainAspectRatio (47) are saved
  metadata/editor constraints, now ignored with zero warnings. Actual white
  balance, explicit Develop settings and crop geometry remain separate.
- Every remaining key and count is listed below. Saved custom presets and cached
  transforms are explicitly identified as such, not described as lost active edits.

## Every remaining warning key

The key counts sum to 715. Each row counts affected images for that key; images
can contribute to multiple keys. The 71 mask warnings are partitioned above.

| Key | Specific reason | Images |
|---|---|---:|
| `AutoTone` | Adobe automatic tone computation is not available; unresolved tone controls cannot be reproduced | 27 |
| `Blacks2012` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |
| `Contrast2012` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |
| `CustomIncrementalTemperature` | saved custom relative white balance temperature is not implemented | 2 |
| `CustomIncrementalTint` | saved custom relative white balance tint is not implemented | 2 |
| `CustomLensProfileDigest` | custom lens-profile resource resolution is not implemented | 2 |
| `CustomLensProfileDistortionScale` | custom lens-profile distortion scaling is not implemented | 2 |
| `CustomLensProfileFilename` | custom lens-profile resource resolution is not implemented | 2 |
| `CustomLensProfileIsEmbedded` | custom embedded lens-profile selection is not implemented | 2 |
| `CustomLensProfileName` | custom lens-profile resource resolution is not implemented | 2 |
| `CustomLensProfileVignettingScale` | custom lens-profile vignetting scaling is not implemented | 2 |
| `CustomTemperature` | saved custom white-balance temperature preset is not imported; active white balance is imported separately | 42 |
| `CustomTint` | saved custom white-balance tint preset is not imported; active white balance is imported separately | 42 |
| `DepthBasedCorrections` | depth-based local correction structure is not implemented | 16 |
| `Exposure2012` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |
| `GrainSeed` | Adobe grain random-seed parity is not implemented | 10 |
| `Highlights2012` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |
| `IncrementalTemperature` | relative white balance temperature requires a rendered-image calibration that is not implemented | 22 |
| `IncrementalTint` | relative white balance tint requires a rendered-image calibration that is not implemented | 20 |
| `MaskGroupBasedCorrections` | Local adjustments, individual AI instances or radial inversion conflict; see partition above | 71 |
| `OverrideLookVignette` | embedded profile vignette override is not implemented | 10 |
| `PointColors` | point-color selection encoding cannot be decoded | 1 |
| `RangeMaskMapInfo` | Adobe cached range-mask raster decoding is not implemented | 8 |
| `RemoveAreas` | Content-aware removal requires Adobe patch pixels; patch decoding is not implemented | 36 |
| `RetouchAreas` | Content-aware removal requires Adobe patch pixels; patch decoding is not implemented | 38 |
| `SDRBlend` | separate SDR rendition blending while HDR editing is active is not implemented | 19 |
| `SDRBrightness` | separate SDR rendition brightness while HDR editing is active is not implemented | 20 |
| `SDRClarity` | separate SDR rendition clarity while HDR editing is active is not implemented | 18 |
| `SDRContrast` | separate SDR rendition contrast while HDR editing is active is not implemented | 18 |
| `SDRHighlights` | separate SDR rendition highlights while HDR editing is active is not implemented | 20 |
| `SDRShadows` | separate SDR rendition shadows while HDR editing is active is not implemented | 19 |
| `SDRWhites` | separate SDR rendition whites while HDR editing is active is not implemented | 12 |
| `Saturation` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |
| `Shadows2012` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |
| `ToggleStyleAmount` | Adobe style-amount interpolation is not implemented | 4 |
| `ToggleStyleDigest` | Adobe style resource resolution is not implemented | 4 |
| `UprightDependentDigest` | Adobe cached Upright dependency metadata is not interpreted; selected transform is imported separately | 1 |
| `UprightTransform_0` | Unselected cached Upright transform is not applied; selected transform imports separately | 1 |
| `UprightTransform_2` | Unselected cached Upright transform is not applied; selected transform imports separately | 1 |
| `UprightTransform_3` | Unselected cached Upright transform is not applied; selected transform imports separately | 1 |
| `UprightTransform_4` | Unselected cached Upright transform is not applied; selected transform imports separately | 1 |
| `UprightTransform_5` | Unselected cached Upright transform is not applied; selected transform imports separately | 1 |
| `Vibrance` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |
| `Whites2012` | Control outside supported numeric range; unresolved Adobe auto-tone result cannot be reproduced | 27 |

## Verification boundary and gates

- 27 synthetic LR-9b regressions cover the reason classes, with test commits
  preceding production fixes. Matrix/guard updated. Golden changes and their
  per-row byte-level justification are recorded above.
- The final catalog audit is read-only. The isolated FFI profile used a fresh
  authorized scratch app directory, then deleted it (absence verified).
- Profile: 21,656 catalog images, 21,615 parsed edited rows; 715 Develop warning
  occurrences; 70 unsupported groups; 20 approximate groups in the translation
  spool. Other report categories remain Catalog 13, Faces 9,293, History 30,460,
  Smart collections 2 and Stacks 220 occurrences.
- The profile deliberately resolves originals as missing: imported/indexed 0,
  skipped 21,656, fidelity samples 0. It verifies translation/reporting, not
  rendered real-image fidelity or a completed user-library import. No app was
  built or launched.
- Final release clean removed 1,617 files (3.2 GiB) from the touched crates.
- Full six-crate release gate passed: 1,355 passed, 0 failed, 36 ignored by existing test attributes; no command-level exclusions. Workspace release Clippy, all targets, `-D warnings` passed; `cargo fmt --all -- --check` passed. The ignored aggregate audit and isolated FFI profile were run explicitly and passed separately.
- Source remains retained, dependency manifests/lockfile and board unchanged.
  Local commits only, on top of `50f114da`; no rebase and no writes to Pictures.
