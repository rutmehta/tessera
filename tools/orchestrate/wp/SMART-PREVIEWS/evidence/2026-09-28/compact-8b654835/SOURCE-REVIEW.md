# Compact tier source audit

Source-only re-audit of the prepared Compact patch and staged codec/generator/tests. This is a second-pass author audit, not an independent reviewer: this same agent prepared the patch. No compilation, execution, measurements, or shared source changes.

No concrete compatibility, bounds, or precision defect identified in the scoped delta.

- `generate` preserves Detail2560 pixel computation; only explicit `generate_with_tier` selects Compact2048. The latter has seven parameters, so the earlier concern about an eight-argument Clippy violation does not apply to this signature.
- Header v1/generator1 accepts only absent tier and maps to Detail; v2 requires generator2 and a supported explicit tier. Null/missing/unknown tiers and incompatible version/generator pairs reject. New output is v2 even for legacy re-encoding, so old readers cannot read newly generated files; that intentional compatibility boundary is documented.
- Tier/scale/dimensions are checked against original crop metadata. Scale zero rejects before dimension division via short-circuit validation. Existing 2560 maximum dimensions, checked payload multiplication, metadata/compressed/payload bounds, hashes and bounded decompression are retained. Compact dimensions must satisfy its 2048-derived scale, rather than merely the global bound.
- Reduction stays after the camera-linear prefix and before calibration/WB. No new clipping or quantization is added. Existing per-sample F16 absolute/relative acceptance and whole-payload exact F32 fallback remain unchanged; signed/HDR tests exercise both tiers. Codec roundtrip tests cover custom WB after reopening.

Qualification limits: prepared tests are unrun; the legacy test constructs v1 bytes rather than loading an independently archived fixture. Edited-render tolerance is a practical codec roundtrip assertion, not a proof of original-resolution quality or absence of all visible differences. Compact still computes the full-resolution prefix. Smaller sample count predicts lower raw payload size, but compressed storage and runtime benefits require actual fixture measurements. FFI/UI do not yet select this tier.

Patch SHA-256: `54b229adffb3f983fe2ee3dc43dc8d2550f72011c19cc6bd3e8d483f1f942057`.

## Candidate 8b654835 scoped review

Compared candidate `8b654835` against `94557873` and the preserved staged Compact patch. All three production files (`pipeline-cpu/src/lib.rs`, `smart_preview.rs`, `smart_preview_codec.rs`) are byte-for-byte identical to the previously reviewed staged sources. No unreviewed production delta.

Independently reviewed the subsequently added tests (not authored by this reviewer). No actionable test defect found. The ignored actual-v1 test reads an external preserved asset, checks v1 framing and original length/digest against the historical report, checks exact edited-render hash against that report, upgrades to v2, requires exact decoded samples and edited-render equality, and verifies the original asset remains unchanged. Its historical evidence requires passing the actual archived asset/report paths; merely running the normal test suite does not execute this ignored gate. Exact historical hash equality is intentionally stronger than an approximate visual comparison and may require the same deterministic rendering environment.

The added image-core scale-three test exercises actual Compact generation -> encode -> decode -> distinct render identity with original recipe owner, nonzero crop origin, portrait orientation metadata, original crop preservation and 1640x6 proxy extent. Custom WB/exposure output is checked against scalar reference at L0–L2 with the established tile comparator. This qualifies scale-three CPU route/level behavior; it does not establish GPU parity, full-dimensional upright geometry, or all output color modes. Those remain separately scoped gates rather than claims of this test.

Source-only acceptance of this test delta; no builds/tests run by this reviewer. Production authorship disclosure above still applies.
