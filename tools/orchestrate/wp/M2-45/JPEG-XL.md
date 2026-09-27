# M2-45 lossless JPEG XL checkpoint

## Delivered scope

- `Format::JpegXl { bits }`, extension `jxl`, with lossless modular encoding
  through zune-jpegxl 0.5.2. 8/16-bit quantization directly from developed
  float RGB, not via an 8-bit intermediate. The losslessness guarantee is
  relative to those quantized output samples, not the original RAW.
- Standard JPEG XL container: signature, file type, optional XML box for
  policy-filtered XMP, complete codestream. Existing XMP sidecar and atomic
  no-clobber publication are reused.
- CLI `--format jxl --bit-depth 8|16`; always lossless. `--quality` applies
  to JPEG/AVIF only. FFI export/preset JSON accepts `format: "jpeg_xl"` with
  the same depths. Defaults/old JSON remain unchanged. The JSON transport
  does not require a new UniFFI record or UI changes.
- MCP's existing `jpeg_xl` format accepts quality 100 (8-bit lossless) and
  rejects lower qualities rather than silently claiming lossy support.
- Encoder pool disabled to avoid nested threads inside batch exports.
  Cancellation checked before/after encoding and during row quantization;
  zune's single encode call itself cannot be interrupted.
- Input validation: finite samples, nonzero supported depths, at least 2x2,
  at most 100 megapixels, at most 1 MiB XMP. No invalid output is published.
- Independent jxl-oxide decoder tests check exact quantized pixels at both
  depths, odd dimensions and multi-group width, colour CICP, and XMP bytes.
  Bridge batch and MCP tests exercise real output. RAW orientation coverage
  runs the normal developed export path and decodes the resulting JXL.

## Explicit limitations

This is not full work-package completion. JPEG XL is sRGB-only. Upstream
zune-jpegxl's header hardcodes sRGB, with no public ICC setter. P3, Rec.2020
and ProPhoto are rejected, not mislabeled. The sRGB colour description lives
in the codestream, not a separately embedded ICC profile. Custom-profile
JPEG XL and literal ICC embedding remain unmet parts of the original brief.

The encoder rejects either dimension of one pixel; this is an upstream
constraint, surfaced as an error rather than padding/changing dimensions.
JPEG XL EXIF/DPI and alpha preservation are not added in this slice. The
existing developed-photo pipeline is RGB, as for PNG/TIFF/normal AVIF.

Lossy libjxl was not attempted, using round 2's permission to skip nontrivial
vendoring. No claim is made that it failed to build. CMake/Brotli/Highway
vendoring, provenance and build verification are deferred. GPL jpegxl-rs and
jpegxl-sys are not used. See docs/13-licensing.md.

## Remaining priorities

1. Prior AVIF commit remains intact, including its documented limits in ROUND2.md.
2. Lossless sRGB JPEG XL delivered here; broader ICC/profile support remains.
3. DNG export (linear, original+XMP, embed-original, 1.6 tags and LibRaw pixel
   round trip) remains unimplemented. The investigation in ROUND2.md still
   applies; do not label managed/gamma-encoded output as scene-linear DNG.
4. Expanded metadata policies/privacy/hierarchy and output-sharpening
   low/standard/high remain unimplemented. Existing output-type sharpening
   and basic all/copyright/none policies are unchanged.
5. PQ/HLG PNG/AVIF/JXL and gain-map JPEG remain unimplemented.
6. Structured post-actions, persisted previous export and multi-preset
   execution remain unimplemented.

Overall RESULT remains FAIL because priorities 3 and 4 are not complete.
Verification results are recorded separately after the gate finishes.
