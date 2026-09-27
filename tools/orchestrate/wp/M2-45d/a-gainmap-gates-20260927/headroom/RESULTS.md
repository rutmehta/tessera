# A four-stop native reconstruction diagnostic — unresolved

Exact original failing-test capture ran once, exit101 preserved, 5.512seconds; heavy slot released at2026-09-27T18:51:03.491493Z. Only an optional artifact-copy block was added before the native assertion. No production/assertion/tolerance edits, host tests or full rerun. B untouched. Original A frozen gate remains4pass/1fail.

Captured fresh1/2/4-stop80x16JPEGs, including resize+sharpen, before unchanged assertion. Independent ISO rational extraction gives alternate headroom/max log gain1/2/4; full base+gain libjpeg reconstruction peaks2/4/16 exactly. Thus retained metadata and JPEG samples describe the requested16 peak. This alone does not certify interoperability.

Twelve lightweight native runs used exactly these immutable files: default, explicit context EDR target0, image-specific luma scaling disabled, and float decoding allowed; each variable tested independently across all three files. Exact commands/environment and exit statuses are in probe-manifest.json. All produced identical raw HDR-provider SHA256 per source across variants.

| Requested stops | Independent peak | Decoded image headroom | Native drawn peak |
|---|---:|---:|---:|
|1|2|2|1.9959318638|
|2|4|4|4.0030903816|
|4|16|8|7.9837622643|

ImageIO returns10-bit,32bpp RGB101010 little-endian provider format (bitmapInfo204806). Packed10-bit field maxima669/746/823 are recorded as codes, not interpreted as linear samples. Native provider buffers and float-context outputs are preserved for each run. Bitmap context reports target0 already; setting0 succeeds without changing pixels. Disabling image-specific luma scaling and requesting allowed float output likewise do not change provider format/headroom/bytes or drawn pixels.

The loss is reflected in ImageIO's decoded CGImage/headroom before bitmap-context drawing. None of these options explains or repairs the16→8 result. No helper fix is justified. The cause could still lie in file semantics, decoder behavior, or requested representation; these probes alone cannot distinguish them.

Installed SDK primary references:
- CoreGraphics/CGContext.h667-672: context EDR headroom0 prevents tone mapping.
- ImageIO/CGImageSource.h248-254: luma scaling defaultsYES; DecodeRequestOptions is a dictionary but no public decoder target-headroom subkey is declared in the searched headers.
- CoreGraphics/CGImage.h253-262: copyWithContentHeadroom changes image metadata, not documented reconstruction; it would not be valid to relabel clipped pixels.
- CoreImage/CIImage.h206-218 and521-530: auxiliary HDR gain-map loading selects ISO if present; imageByApplyingGainMap:headroom: explicitly reconstructs at requested headroom bounded by full map headroom. This is a distinct supported path worth a separately approved tiny software-renderer probe.

Official Apple references: https://developer.apple.com/documentation/coregraphics/cgcontext/setedrtargetheadroom(_:), https://developer.apple.com/documentation/imageio/kcgimagesourcegenerateimagespecificlumascaling, https://developer.apple.com/documentation/imageio/kcgimagesourceshouldallowfloat. Apple WWDC24 session10177 discusses iPhone photo headroom up to8, but does not establish a universal ISO decoder cap; do not use it to excuse this failure.

Pinned Skia checked-in controls examined so far have display ratio4. Its source unit tests generate32-ratio gain maps at runtime, which is not yet an independently available >8 file. No verified external above8 control or documented ImageIO decode-target option has been found. Original4% checks remain unchanged.

Evidence: capturedJPEGs, independent extracted base/gain JPEGs+metadata, exact original test backup/capture patch/hashes, capture log/manifest, probe source/commands, provider and drawn float buffers, results.json/provider-summary.json/independent-reconstruction.json. Host-specific compiled probe remains a /tmp artifact; source supports reproduction.

## Supplemental Core Image software diagnostic

Approved separate probe used kCIContextUseSoftwareRenderer=YES, extended-linear-sRGB working/output color spaces, and RGBAf working/output pixels. On identical retained1/2/4-stop JPEGs, both CIImage default expandToHDR and imageByApplyingGainMap:headroom: at2/4/16 reconstructed finite opaque pixels with peaks2.0000004768/4.0000009537/16.0000038147 and reported content headroom2/4/16. Captured gain-map metadata confirms AlternateHeadroom4 and GainMapMax4 for the four-stop file. Raw float outputs/source/logs are in coreimage/ and coreimage-probe.m.

This demonstrates different native reader behavior for the exact same four-stop JPEG: ImageIO CGImage readback8, Core Image software readback16. It does not reclassify the original failed ImageIO gate as passing, prove universal interoperability, or require an encoder production change. Coordinator approved a separately named supplemental Core Image test with unchanged4% bounds, finite/opaque requirements and missing-association negative controls; existing ImageIO assertion remains. Full package/main acceptance remains held pending explicit disposition. Host correctness gates may be checked separately in the next assigned slot.

Documented Core Image API: https://developer.apple.com/documentation/coreimage/ciimage/applyinggainmap(_:headroom:). No metadata-only headroom override was used.
