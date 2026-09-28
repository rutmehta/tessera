# EXP-45 ImageIO path review — source and primary documentation only

2026-09-28. Reviewed main coordination EXP45 documents, portable phase7 raw results and probe sources, the installed Xcode SDK headers, and Apple primary documentation. No build, test, native execution, product edit, or runtime acceptance is claimed. This report itself is the only authored file. Original A core4pass/1fail remains unchanged.

## Evidence and bounded hypothesis

The strongest hypothesis is content-dependent HDR representation selection in ImageIO, potentially affected by insufficient samples for gain-derived profile analysis. The warning is a diagnostic clue, not proof of the mechanism.

Phase7 results (`tools/orchestrate/wp/EXP-45/evidence/2026-09-28/reference-control-phase7/native/imageio-parsed.json`): uniform gain produces named `kCGColorSpaceITUR_2100_PQ`, 13,300 ICC bytes, headroom16 and linear peak15.9515762329. Split gain produces unnamed RGB, 26,620 ICC bytes, headroom8 and peak7.9837627411. Original A produces unnamed RGB, 26,620 ICC bytes, headroom8 and peak7.9837622643. All are packed10bpc/32bpp, bitmap_info204806. Split and A output ICC hashes differ; equal profile length does not mean equivalent profiles.

The two `too few samples` messages in phase7 `native/imageio-prefix.txt` cannot be attributed individually because all three fixtures ran in one process. A per-file process replay is useful new attribution, not a new acceptance gate. The existing probe hashes output profiles but does not save their bytes. Input ICC swaps have already been negative; generated output ICC differences are a separate observable.

## Public API findings

1. Float placement is correct. Installed `/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/System/Library/Frameworks/ImageIO.framework/Versions/A/Headers/CGImageSource.h:40–62` explicitly lists top-level `CGImageSourceCreateImageAtIndex` options and describes `kCGImageSourceShouldAllowFloat` as conditional on format support. The retained probe sets it to YES by default. Packed10-bit output proves floating output was not produced, not that option placement was wrong. Apple: https://developer.apple.com/documentation/imageio/kcgimagesourceshouldallowfloat

2. No public decoder target-headroom, requested output color-space, or pixel-format selector was found in this installed header or the primary docs searched. Bitmap context color space is a subsequent render/conversion choice. `CGContextSetEDRTargetHeadroom` does not request an ImageIO decoder headroom. No guessed key should be introduced.

3. A distinct documented but UNRUN metadata option exists: `options[(id)kCGImageSourceDecodeRequestOptions] = @{(id)kCGComputeHDRStats: @YES};`. Apple explicitly nests it under DecodeRequestOptions. The contract is calculating HDR metadata, not selecting decode capacity. This is a later separate experiment, without runtime authorization in this review. Apple: https://developer.apple.com/documentation/coregraphics/adopting-advancements-in-hdr-image-rendering

4. Apple's current HAGC documentation says ImageIO analyzes ISO gain maps when deriving adaptive tone-mapping metadata. Its new HAGC APIs are marked Beta and were absent from the installed ColorSync SDK headers searched. Do not compile against, dynamically guess, or introduce them. Existing `CGColorSpaceCopyICCData` suffices to preserve profiles. The HAGC article's example uses DecodeToHDR as a dictionary key inconsistently with the known working public DecodeRequest:DecodeToHDR mapping; retain the latter. Sources: https://developer.apple.com/documentation/colorsync/headroom-adaptive-gain-curve and https://developer.apple.com/documentation/colorsync/authoring-headroom-adaptive-gain-curve-metadata

5. WWDC24 describes gain-derived custom tone mapping and the supported ImageIO-to-extended-bitmap path, but does not establish this host's failing mechanism or an eight-times-reference-white limit. https://developer.apple.com/videos/play/wwdc2024/10177/

## Authorized-next geometry experiment and source review boundaries

Luna owns the sole native lane for root-authorized isolated per-file baseline and bounded geometric scaling. Preserve exact80x16 inputs and producer hashes. Save returned ICC bytes and provider hashes and record effective option dictionary. No decoder/draw option change should be mixed with geometry. Requalify unchanged80x16 baselines before new fixtures.

For geometric variants, preserve normalized base/gain distributions through deterministic integer replication, including maximum gain over the white patch. A uniform companion controls producer/container policy. Record all unavoidable codestream/MPF/dimension differences; do not claim byte-for-byte image identity after geometric re-encoding. Confirm actual decoded gain samples and independent reference half-float reconstruction before native comparison. Core Image's existing explicit80x16 restriction and hard-coded sample points must be adapted with explicit bounded validation rather than silently bypassed. All measured paths retain their original linear extended-sRGB output contracts.

Generated ICC inspection can compare ordinary ICC tag directories: signature, offset, byte length, and hash. Do not assign undocumented tag semantics or transplant profiles. A profile transplant changes pixel interpretation and cannot establish correct decode.

## Falsification criteria

- Larger split reaches~16 while same-size uniform also does: supports a size-sensitive path, without proving sample-count versus geometry versus scan structure.
- Larger split remains~8 when warning disappears: the warning is not a necessary correlate of the failure.
- Uniform and split both warn but only split halves: warning alone is insufficient.
- Larger split remains~8: does not eliminate insufficient distinct-value sampling; replication preserves the number of distinct base/gain combinations.
- A later isolated Stats=YES changes reported headroom but not provider/profile/pixels: metadata reporting effect only.
- Stats=YES changes provider/profile/pixels together: decoder option sensitivity; not an encoder correction.

No outcome substitutes for or relaxes original A ImageIO acceptance. No Apple-bug conclusion follows from these controls alone.

## Independent geometry-probe source review

Reviewed the following exact scratch sources by diff against the original pinned probes, without compiling or executing:

- `size-640x128/probe/imageio-probe-geometry.m`, SHA256 `a0462ebfdee9d3b264e61e95d68643f57e4d91a84d2c2b5cbdbc0315d346fa5f`.
- `size-640x128/probe/coreimage-probe-geometry.m`, SHA256 `d740b373fadae118aee00daa4c1c83ec99d8634066af7e888dbf0c18a5de80ca`.

Root directory is `/Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404`.

The diffs preserve decoder options, ImageIO extended-linear-sRGB RGBAf draw, Core Image software renderer, extended-linear-sRGB working/output spaces, explicit gain-map application, and global pixel scans. Normalized points reproduce x=8,24,40,56,72/y=8 on80x16 and scale to x=64,192,320,448,576/y=64 on640x128. The sample JSON schema changes from arrays to explicit coordinate/rgba objects; comparisons must unwrap rgba rather than assume old array schema. ICC persistence uses the already-copied ICC bytes, without modifying the image/profile.

One requested ordering correction: the initial ImageIO revision fetches source properties before the pre-existing auxiliary/decode sequence. Move this newly introduced lookup/serialization after both decode/render calls, or to a separate inspection process, to avoid priming the source before measurement. This is a conservative measurement-invariance correction, not a proven cache bug. Luna/root were notified; revised source review is pending.

The probe's file writes do not test returned success. The runner must independently require each ICC file's length/hash to equal the JSON and raw pixel lengths to match dimensions; otherwise a missing/stale artifact could evade provenance. These are fixed qualified tiny fixtures, not a general arbitrary-image decoder test. Runtime baseline requalification remains required and no runtime pass is claimed here.
