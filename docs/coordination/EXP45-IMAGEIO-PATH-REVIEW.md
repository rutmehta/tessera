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

Revision follow-up: reviewed ImageIO source SHA256 `3e623a55f30fbcd448c2401f8e4471fed30772056f8db06d8256d39fc6c882cc`; Core Image remains `d740b373fadae118aee00daa4c1c83ec99d8634066af7e888dbf0c18a5de80ca`. Source-property introspection now follows both decode/draw calls. New provider hashing operates on already-copied bytes and adds no decoder API. No source measurement-invariance blocker remains for compiling these fixed-fixture probes. Per-file process isolation and distinct output directories remain required because provider/ICC/property filenames are shared within each process directory. Runner artifact validation review remains pending; no runtime result claimed.

## Final runner preflight source review

Final Core Image source `56e45d95db9bf5672adc9f32ed6b67fbb507c2e3ed1d31ecbdc31237e2f7fc59` adds only reported width/height to the prior reviewed JSON. Final ImageIO source `ffefb2e681e51763f81f48b2b57b69e55dae0b1d4042476365fc227945968ab3` adds only `source_properties_plist` to the prior reviewed record. No decoder/render behavior changes in these reporting additions.

Runner `probe/run_geometry_probes.py` SHA256 `3640e3a3e6a2589949c5760718a4bd77cd0c5f8af1aa16d6066c4d4ddd4e8281` was reviewed without running it. A prior runner draft was rejected BEFORE execution; Luna preserves it as `run_geometry_probes.preflight-rejected.unrun.py`. Root independently found baseline-field, guessed-warning and baseline-order defects. This review additionally identified missing property-path JSON, mismatched CI raw filenames, absent ImageIO raw dump environment, and inherited PROBE option overrides. These are corrected in the final runner.

The corrected runner uses one process and distinct directory per input, unsets PROBE_OPTIONS/PROBE_LUMA_OFF/PROBE_TARGET_ZERO, and sets explicit profile/provider/drawn-pixel output paths. It verifies saved provider length and SHA against the probe record, saved ICC length and SHA against the probe record, source property plist parseability, and ImageIO/CI raw-float byte lengths against dimensions. It preserves stdout, stderr, exit, command and warning text per process. Warning text is observed, not asserted as a prediction. Baseline first-three inputs run in a separate invocation; only successful baseline manifest publication permits subsequent scaled phase with matching source/binary hashes. Its baseline comparisons account for the changed sample JSON schema.

No source blocker remains for first execution into fresh output directories. If execution fails, preserve attempt directories rather than overwrite them. Runner records drawn-float file hashes and relies on probe statistics for their content; independent post-run raw-pixel scanning remains useful. This preflight review is not native execution or an acceptance result. Final executable hashes must be frozen by the runtime owner because the source received reporting-field additions after the earlier build.

## Independent phase8 saved-artifact audit (no new native execution)

Read completed `size-640x128/native-geometry-manifest.json` and independently scanned its saved outputs with Python standard-library byte parsing. All20 floating-point image artifacts (ImageIO SDR/HDR and Core Image default/explicit for five inputs) have the expected byte counts, finite channels and exactly opaque alpha. Recomputed ImageIO RGB minima/maxima/means and every recorded sample agree with probe JSON; recomputed Core Image minima/maxima and recorded samples also agree. All10 provider dumps match recorded lengths/SHA256, and all10 returned ICC files match lengths/SHA256. Ordinary ICC headers/tag directories are in bounds. This is independent read-only artifact validation, not a repeated native decode.

The final saved provenance identifies ImageIO source `ffefb2e681e51763f81f48b2b57b69e55dae0b1d4042476365fc227945968ab3`, binary `30b852208589adba5bb37bdb37db90710860b89dfb33b31078c05b1f98801c25`; Core Image source `56e45d95db9bf5672adc9f32ed6b67fbb507c2e3ed1d31ecbdc31237e2f7fc59`, binary `301251827ae83ccc09e091a2e87bf2a88ea5595c23c7a62b3ed26989e5c69ff3`.

For all three80x16 inputs, the recorded SDR/HDR extrema, mean, headroom, dimensions, sample RGBA values, bit layout, color-space name/profile size, and context target before/after/set flag match phase7 EXACTLY. This is stronger than the runner's bounded numeric comparison for these values. It does not establish bitwise equality of historical provider bytes, which phase7 did not persist through this runner. Uniform returned HDR ICC hash matches phase7; split/A hashes change between runs despite identical numeric measurements, so whole-profile hashes must not be mistaken for stable semantic identities.

Uniform80 and uniform640 both reach15.951576232910156/headroom16. Split80 and split640 both reach7.983762741088867/headroom8. Original A reaches7.983762264251709/headroom8. The sample warning occurs only for successful16 uniform files (two lines at80, five at640); split80, split640 and A emit none. Therefore this warning is not a necessary condition for the8 result, and the tested8x integer enlargement does not resolve the result. Replication retains a sparse value distribution, so this is not a universal exclusion of every statistical/content-dependent mechanism.

### Generated profile observations, without undocumented tag interpretation

Uniform80/640 returned HDR ICC bytes are identical (13,300B, SHA256 `63cd9f4a8443c71df3633aa0a2ec4594aa8fbca23b1a3231b517bc726a567b16`). Its ordinary `desc` multilocalized-Unicode record says `Rec. ITU-R BT.2100 PQ`. Tags are desc/cprt/wtpt/A2B0/B2A0/chad/cicp/lumi.

Both split profiles have26,620B and `desc` records beginning `BT.2020 Primaries; PQ (Adaptive Gain Curve ...)`. Their whole ICC/description/hdgm bytes differ, but their A2B0 payloads match exactly (19,478B, SHA256 `8a9032f4c494820ee6ff6fd9259509d638827e7fa565c96b34aaca84568d8506`), as do their B2A0 payloads. A's description instead begins `BT.709 Primaries; PQ (Adaptive Gain Curve ...)`. All split/A profiles carry tag signature `hdgm` (194B) where uniform has `lumi`; this review does not assign private or undocumented semantics to its payload. The profile itself labels the split/A path Adaptive Gain Curve. That label and the associated byte differences localize a useful observable; they do not prove which transform caused the reduced pixels.

## Public decoder contract assessment

The primary sources reviewed do not establish an exact numeric ISO reconstruction/pixel-preservation promise for `CGImageSourceDecodeToHDR`. The installed ImageIO header declares the HDR representation request without specifying decoder target headroom or arithmetic equivalence to full-capacity ISO gain application. The float option is conditional on file-format support and is not a force-linear-output promise. WWDC24 describes reading the HDR representation for maximum fidelity, and explains gain-derived custom tone mapping; it supplies no explicit16x versus8x bound for these files. The generic headroom getter is metadata access, not a required numerical peak reconstruction oracle. Sources: https://developer.apple.com/videos/play/wwdc2024/10177/ ; https://developer.apple.com/documentation/imageio/kcgimagesourceshouldallowfloat ; local CGImageSource.h:239–257 and CGImage.h:268–277.

Conversely, absent an exact numerical promise does not prove that halving is intended, compliant, or acceptable. A crucial documented boundary is installed CGContext.h:667–670: context target headroom0 means unknown and prevents tone mapping. The measured destination is0 before/after in all controls. Thus it would be unsupported to explain this result as ordinary display-target tone mapping. The custom generated profile may participate in the decoder/color conversion path, but causality has not been isolated by these observations.

There are three distinct contracts: (1) file structure and encoded ISO gain mathematics, tested by parsers/reference reconstruction; (2) Apple's selected HDR decode/color-conversion representation, observed through its public APIs; (3) Tessera's product requirement that this ImageIO path preserve the intended highlight within4%. The failing third contract is real regardless of whether a sufficiently precise first-party API promise exists. Independent libultrahdr reproduction weakens an A-specific encoder explanation; it neither validates all Tessera encoding nor justifies changing the product gate. This review therefore establishes neither a mismatched test contract nor a proven Apple defect. Original A4pass/1fail remains open, without waiver or product acceptance.

## Separate documented-stats option proposal — source-only, UNRUN

One final bounded public-option comparison is worth doing if root authorizes it because it changes a documented computation policy without modifying fixtures, profiles, or render target. The installed CGImageSource.h:256–257 declares `kCGComputeHDRStats`; Apple explicitly shows it nested beneath `kCGImageSourceDecodeRequestOptions` in https://developer.apple.com/documentation/coregraphics/adopting-advancements-in-hdr-image-rendering . The declared purpose is computing HDR metadata. It is NOT a documented request to decode at a target headroom and must not be described as such.

Use the three exact80x16 fixtures, existing read/draw options and per-file processes, with one isolated probe copy adding only `options[(id)kCGImageSourceDecodeRequestOptions] = @{(id)kCGComputeHDRStats: @YES};`. Preserve default outputs as contemporaneous controls, exact source/binary/input/options hashes, provider dumps, rendered pixels, and generated ICC data. No profile transplant, color-space reassignment, guessed key, or Beta HAGC API is justified. Outcomes: metadata-only change would identify reporting sensitivity; pixel/profile/provider change would identify an additional decoder-policy sensitivity; no change would eliminate this documented opt-in as a resolution for these three inputs. All outcomes retain the original failed gate. This experiment is UNRUN by this reviewer and is not authorized here; root owns any later lane grant.
