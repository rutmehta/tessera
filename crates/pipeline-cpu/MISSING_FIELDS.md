# M2-04 schema gaps

## M2-09 additions (engine-api remains unchanged)

- Independent manual red/green and blue/green lateral CA coefficients are absent.
  `chromatic_aberration_scale` scales the selected profile/estimate, not an
  independent per-channel manual correction. A caller can currently supply a
  user `lens::Profile` through `LensContext`; persistent manual sliders need
  explicit red/blue scale or polynomial fields in LensSettings.
- GuideLine has no horizontal/vertical target axis. Guided mode currently
  infers it from dominant pixel direction; explicit guide-axis storage would
  allow strongly tilted guides without ambiguous classification.
- Capture focus distance, sensor crop factor/pixel aspect and DNG ActiveArea
  are needed in decoder metadata for exact profile coordinate normalization.
  These are metadata gaps, not recipe slider fields. LensContext accepts a
  focal/aperture/distance override, without pretending missing data was measured.
- A persisted calibration identity/content digest and provenance would let
  recipes resolve saved user profiles reproducibly. Current caller-owned
  profile injection does not validate LensProfileRef filename/digest.

## Previous M2-04 gaps

`engine-api/src/recipe/settings.rs` is unchanged.

- **B&W treatment and eight-band B&W mix:** DevelopSettings/ColorSettings has neither a treatment switch nor B&W mix coefficients. Skip this control. Saturation -100 remains available, but is not presented as the missing B&W mix.
- **User-configurable grain seed:** Grain exposes amount, size and roughness only. Use the fixed native seed `0x5445535345524132`; skip the seed control. Equal settings and image coordinates produce deterministic grain.
- **Detail activation resolved in native revision 2:** Sharpening 40 and colour NR 25 now render by default. Zero amounts disable them. No enable flags or hidden fields are needed, and the unchanged-substructure bypass is removed.

Other unsupported controls already have fields and are not schema gaps: Point Color, LUT, AI denoise, lens blur, orientation and constrain-crop, HDR/proofing and other display transforms remain explicit errors when changed. Crop aspect is UI metadata; the supplied rectangle is authoritative. Lens distortion, profile resolution, defringe, Upright and manual transforms are now wired in the synchronous CPU renderer; see LENS_M2.md for scope and remaining stage/metadata limitations.
