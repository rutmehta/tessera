# Compact tier source audit

Source-only re-audit of the prepared Compact patch and staged codec/generator/tests. This is a second-pass author audit, not an independent reviewer: this same agent prepared the patch. No compilation, execution, measurements, or shared source changes.

No concrete compatibility, bounds, or precision defect identified in the scoped delta.

- `generate` preserves Detail2560 pixel computation; only explicit `generate_with_tier` selects Compact2048. The latter has seven parameters, so the earlier concern about an eight-argument Clippy violation does not apply to this signature.
- Header v1/generator1 accepts only absent tier and maps to Detail; v2 requires generator2 and a supported explicit tier. Null/missing/unknown tiers and incompatible version/generator pairs reject. New output is v2 even for legacy re-encoding, so old readers cannot read newly generated files; that intentional compatibility boundary is documented.
- Tier/scale/dimensions are checked against original crop metadata. Scale zero rejects before dimension division via short-circuit validation. Existing 2560 maximum dimensions, checked payload multiplication, metadata/compressed/payload bounds, hashes and bounded decompression are retained. Compact dimensions must satisfy its 2048-derived scale, rather than merely the global bound.
- Reduction stays after the camera-linear prefix and before calibration/WB. No new clipping or quantization is added. Existing per-sample F16 absolute/relative acceptance and whole-payload exact F32 fallback remain unchanged; signed/HDR tests exercise both tiers. Codec roundtrip tests cover custom WB after reopening.

Qualification limits: prepared tests are unrun; the legacy test constructs v1 bytes rather than loading an independently archived fixture. Edited-render tolerance is a practical codec roundtrip assertion, not a proof of original-resolution quality or absence of all visible differences. Compact still computes the full-resolution prefix. Smaller sample count predicts lower raw payload size, but compressed storage and runtime benefits require actual fixture measurements. FFI/UI do not yet select this tier.

Patch SHA-256: `54b229adffb3f983fe2ee3dc43dc8d2550f72011c19cc6bd3e8d483f1f942057`.
