# M1-03 findings and handoff

## Status: partial implementation, not acceptance-complete

The required test/clippy/fmt command passes. That does not establish full develop export support. No engine-api files were changed.

Implemented: atomic recipe/XMP writes, deterministic recipe envelope, vector clocks and deterministic LWW, namespace-aware metadata import/update, raw foreign-property preservation, selection/mark mapping, all-table CRS extraction, scalar translations, normalized point curves, Look name/amount, and linear-gradient MaskGroupBasedCorrections translation with valid recipe history.

Remaining acceptance gaps:

* Native ProcessVersion has no Adobe CRS value (`ProcessVersion::crs_value()` returns None). Export currently returns Unsupported rather than falsely labeling native rendering as Adobe PV6. A reviewed native-to-Adobe export policy is needed. The all-mapped-keys test uses an Adobe PV6 recipe, not the native default.
* Newly authored nonempty PointColors, LensBlur, RetouchAreas/RetouchInfo, and masks other than additive linear gradients are not yet exported. Imported opaque blocks are retained byte-for-byte with warnings, but that is not a full translator. Unsupported exports return an error rather than inventing Adobe XML. This is implementation work still outstanding, not evidence that engine-api must change.
* The existing contract maps several independent Adobe metadata values to the same recipe field. Lens profile filename/digest/setup/name and CameraProfileDigest cannot all be recovered from a single profile ID. Enhance already-applied metadata cannot safely be interpreted as an instruction to run neural denoise. Existing source properties are preserved when the target did not change; new packets use neutral placeholders for unavailable ancillary fields. Contract owners should review the model before claiming full fidelity.
* The fixture is authored, not exported by a local Lightroom installation. Adobe's public namespace reference does not document modern mask internals; its generic RDF format is combined with publicly posted Adobe Camera Raw XML (sources below). No local Lightroom interoperability run occurred.

## Pick/reject: actual property, not a keyword

Use `xmpDM:pick`, in `http://ns.adobe.com/xmp/1.0/DynamicMedia/`, with values 1 / 0 / -1 for picked / unflagged / rejected. We write `xmpDM:good=True` for picks and False for rejects. Undecided removes rating, pick and good to follow spec 06's no-rating/no-flag row. Reject also writes `xmp:Rating=-1`; Keep and grades write 1/2/3/5. No `lr:` reject property was substantiated. `lr:hierarchicalSubject` is a keyword bag and must never carry a made-up flag literal.

Evidence:

* Adobe confirms flag states are saved to XMP beginning with Lightroom Classic 13.2: https://helpx.adobe.com/lightroom-classic/desktop/organize-photos-in-lightroom-classic/flag-label-rate-photos.html
* Adobe documents the Dynamic Media namespace and `good` as a keeper checkbox, but its published table omits `pick`: https://developer.adobe.com/xmp/docs/xmp-namespaces/xmp-dm/
* Public observed Lightroom output records the exact pick/good combinations: https://github.com/immich-app/immich/discussions/12198
* Independent parser evidence for lowercase `pick` and `good`: https://raw.githubusercontent.com/exiftool/exiftool/master/lib/Image/ExifTool/XMP2.pl

`MarkPreset::lightroom()` maps lowercase color names to Red/Yellow/Green/Blue/Purple, and preserves other label text. Our `ts:Mark` plus `ts:MarkLabel` companion properties retain the original name even when custom presets have duplicate or empty labels. Import only honors the private name if its companion still matches the visible XMP Label, avoiding stale names after external relabeling. External XMP with no companion imports the label by name.

## Disputed CRS names: evidence and confidence

Adobe's published Camera Raw namespace table is old and incomplete. It specifies the CRS namespace, WhiteBalance spellings, basic numeric fields, crop fields, and tone-curve point arrays, but does not list the disputed modern fields. Absence there is not proof a field is wrong:
https://developer.adobe.com/xmp/docs/xmp-namespaces/crs/

| Contract key | Finding |
| --- | --- |
| EnhanceDenoiseAlreadyApplied | ExifTool places this in `aux:`, not `crs:`. It describes pixels already enhanced. |
| EnhanceDenoiseVersion | Also `aux:` in ExifTool, string; even that implementation marks its numeric typing uncertain. |
| EnhanceDenoiseLumAmount | Suspect namespace AND spelling: observed `aux:EnhanceDenoiseLumaAmount`, with **Luma**. |
| EnhanceDetailsAlreadyApplied | Observed in `aux:`, Boolean. |
| EnhanceSuperResolutionAlreadyApplied | Observed in `aux:`, Boolean. |
| HDREditMode | Name corroborated in an ACR 17.5 preset, value 0. Full domain not established from Adobe's schema. |
| HDRMaxValue | Recognized by ExifTool as real. This does not verify the contract's 0–16 range or mapping to stops. |
| LensProfileSetup | Name corroborated with Auto and Custom. Adobe public schema does not establish the closed enum or LensDefaults value. |
| PostCropVignetteStyle | ExifTool maps 1=Highlight Priority, 2=Color Priority, 3=Paint Overlay, agreeing with the contract range. Not an Adobe normative guarantee. |

Evidence beyond the incomplete Adobe namespace reference:

* https://exiftool.org/TagNames/XMP.html
* https://raw.githubusercontent.com/exiftool/exiftool/master/lib/Image/ExifTool/XMP.pm (namespace ownership, enum values, type caveats)
* https://community.adobe.com/feature-requests-676/p-ai-denoise-level-data-666006 (Luma spelling in enhanced DNG metadata)
* https://community.adobe.com/t5/camera-raw-bugs/p-denoise-not-applied-when-included-in-an-import-preset-camera-specific-default/idc-p/15462946 (ACR 17.5 preset with HDREditMode=0, LensProfileSetup=Auto, newer Enhance in FilterList/Filters with CompressedSettings)
* https://onebitious.net/lightroom_xmp/ (published Lightroom 14.2 XML with LensProfileSetup=Custom and xmpDM:pick=0)

Community examples are empirical evidence, not Adobe schema guarantees. The implementation continues to recognize every exact contract table key and does not silently rename the contract. `aux:` properties remain preserved as foreign XML.

## Fixture and metadata format

`crates/sidecar/tests/fixtures/lightroom-pv6.xmp` explicitly labels itself authored. It contains ProcessVersion=15.4 (the existing contract's PV6 spelling), Exposure2012=1.25, a three-point ToneCurvePV2012 sequence, and a correction resource with nested CorrectionMasks. The mask's FullX/FullY become the linear start, ZeroX/ZeroY the end. CorrectionAmount converts from a multiplier to percent. Tests check imported exposure, white balance, vignette style, normalized curve coordinates, mask geometry/parameters and recipe history validation.

* Adobe Dublin Core definitions for title, description, creator, rights, subject: https://developer.adobe.com/xmp/docs/xmp-namespaces/dc/
* Adobe XMP specifications (RDF/XML packet, arrays, language alternatives): https://developer.adobe.com/xmp/docs/XMPSpecifications/
* Modern correction resource/sequence format from posted Camera Raw XML: https://community.adobe.com/questions-712/ai-camera-raw-masks-not-re-computed-when-used-in-an-action-1167094

No raw decoding is needed for these sidecar tests, so they do not depend on the optional raw fixture directory or change originals.

## Storage and merge decisions

JSON envelope: `{recipe: Recipe, vector_clock: {machine_id: counter}, last_writer: {timestamp_ms, machine_id, counter}}`. Existing envelopes without last_writer deserialize with a default stamp. Call `record_write` exactly once per logical edit, not on each serialization. It advances both the writer's vector counter and a monotonic logical timestamp. Merge selects by stamp with deterministic serialized-recipe tie-breaking, and independently takes componentwise clock maxima. This makes merge order-independent and avoids the old incorrect vector-sum-as-write-time heuristic. LWW deliberately selects an entire recipe, not a union of history branches.

Atomic write uses a same-directory create_new temporary file, write_all, sync_all and rename, cleaning up on failure. Schema and recipe invariants are checked before touching the destination. This is atomic per file, not an atomic transaction across the JSON/XMP pair. No directory fsync guarantee is claimed.
