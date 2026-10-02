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
