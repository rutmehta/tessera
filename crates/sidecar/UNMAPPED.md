# Develop XMP mapping boundary (M2-02)

## Native round trip is not Adobe interoperability

All `CrsTarget::Field` pointers are exercised by `tests/mapped_properties.rs` against freshly exported XMP and then re-exported XMP. Structured native values round-trip using granular `ts:` RDF extensions where no verified Adobe representation is known. This is not evidence that Lightroom can render those extensions. Native recipes export as best-effort Adobe PV6, not pixel-identical Adobe rendering.

Remaining Adobe translation gaps (acceptance is incomplete):

| Property | Native preservation implemented | Adobe interoperability gap |
| --- | --- | --- |
| PointColors | Typed resource sequence, OkLCh sample and all shifts | Adobe writes a string sequence with an undocumented grammar/colour space. Native resource items are not compatible Adobe PointColors strings. Foreign strings are warned and retained. |
| LensBlur | Active/BlurAmount plus ts focus_range, bokeh, depth_model | Four-value Adobe FocalRange and numeric BokehShape are not translated; foreign versions are warned and retained. |
| RetouchAreas / RetouchInfo | All native heal/clone/remove/skin variants, targets, IDs, enable flags, opacity/feather | Native targets and operations are extensions, not Adobe spot/dab payloads. Foreign payloads are warned and retained. |
| MaskGroupBasedCorrections | Linear/radial CRS geometry, local sliders, plus native brush strokes, pressure/erase, ranges and AI placeholders/model refs | Brush Dabs and Adobe colour sample encodings/range type codes are not decoded. AI placeholders carry requests/models, not Adobe rasters. Native field-level extensions cannot be rendered by Adobe. Foreign unsupported groups are retained atomically. |
| Look | Name and Amount, including present-but-empty name | Recipe LookSettings has only a style ID and amount, no Look Parameters/tables/profile data. Imported opaque tables survive unchanged-target export via the source packet but cannot be authored from the model. Engine-api change or an explicit external Look resource contract is needed for authoring tables. |

The property test proves mapped-subset preservation within this codec. It does not close the above gaps. No Lightroom interoperability execution occurred.

## DevelopSettings fields without a contract CRS path

Pointers below are relative to `/settings`. They stay in the native recipe JSON, not newly authored CRS properties. Imported unknown source XMP remains available as `Recipe.unknown["sidecar_xmp"]`.

| Fields | Reason |
| --- | --- |
| `/decode/frame_index`, `/decode/pixel_shift_merge` | Native raw decode/multi-frame policy; no contract CRS key. |
| `/linearize/highlight_reconstruction` | Native reconstruction algorithm, no Adobe equivalent in the contract. |
| `/denoise/method`, `/denoise/amount`, `/denoise/chroma_only` | Native denoise instructions/models. aux Enhance fields describe already-baked pixels, not equivalent instructions. |
| `/demosaic/method`, `/demosaic/model` | Native demosaic algorithm/model; no CRS mapping. |
| `/lens/softness_correction` | Not represented by the contract table. |
| `/camera_profile/amount`, `/camera_profile/working_space` | Native profile strength and working-space policy, no contract CRS pointer. |
| `/tone/curves/luminance`, `/tone/display_transform` | Native luminance-only curve/display transform, not Adobe RGB channel curves. |
| `/color/lut` | Native LUT resource/strength, distinct from the mapped camera Look ID. |
| `/geometry/orientation` | EXIF orientation is outside the develop CRS table. |
| `/geometry/upright/guides` | No Upright guide keys in contract 1.1; only PerspectiveUpright mode is mapped. Engine-api table needs a reviewed mapping for guides. |
| `/geometry/crop/aspect` | Editor aspect-ratio lock is not the visible crop rectangle. No contract key. |
| `/output/gamut_mapping`, `/output/proof_profile` | Native output/proof rendering policy, no contract CRS mapping. |

Non-settings recipe members (image identity, schema, history, counters, provenance, unknown data) are not develop CRS fields. Selection has its existing separate metadata mapping. Legacy ChromaticAberrationR/B have no native target and are warned/retained. Informational aux Enhance properties feed provenance only and are never authored from develop settings.

## Precision and private companions

- Adobe curves remain integer 0–255 `rdf:li` values. Per-point `ts:x`/`ts:y` retain exact native f32 coordinates only while their rounded visible point still agrees. External curve changes win.
- Radial center/radii preserve sub-bound precision privately only while CRS bounds agree.
- `ts:LensProfileSource` distinguishes native Embedded/AutoCalibrated and empty-identity Database from Adobe Auto. It is trusted only with a matching export hash.
- `ts:ExportHash` is BLAKE3 of a namespace-resolved representation of every top-level CRS property including unknown properties and complete nested structures. Attribute order and prefix spelling do not affect it; nested sequence order does. It detects edits, not malicious forgery or authorship.
- `ts:NativeRevision` is only passed to `ProcessVersion::from_xmp` when ExportHash matches. Re-export always rewrites/removes ProcessVersion and NativeRevision together, preventing reactivation of an externally stale companion.
- Recipe envelopes deserialize through `Recipe::from_json` for schema upgrades, including direct `RecipeDocument` serde use.

## References

Contract: `crates/engine-api/CONTRACTS.md` v1.1 and `recipe/crs.rs`. Prior findings: `tools/orchestrate/wp/M1-03/FINDINGS.md`.

Tag-name/structure evidence: https://raw.githubusercontent.com/exiftool/exiftool/master/lib/Image/ExifTool/XMP.pm (sCorrectionMask, sCorrRangeMask, sRetouchArea, sLensBlur, PointColors). This is an empirical tag catalog, not a complete normative Adobe payload specification. Native extension formats are explicitly ours, not claims from that reference.
