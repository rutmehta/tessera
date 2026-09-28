# Compact tier source-only handoff

Patch: `/tmp/tessera-smart-preview-compact.patch`
Copies: `/tmp/tessera-smart-preview-compact/{base,new}/crates/pipeline-cpu/...`
Base: clean relevant files at `6c05764180192b05a9fbaa553642c277ee1d6cc8`.

No shared source changes, compilation, benchmarks, dependency changes or new quantization. Temporary-copy rustfmt with edition 2024 and skip_children=true succeeded; `git apply --check` succeeded against the feature checkout. All added tests UNRUN.

## Behavior

- Public `SmartPreviewTier::{Compact2048, Detail2560}`, `max_edge()`, `CameraLinearProxy::tier()`.
- Existing `generate(...)` still selects Detail2560 and the same pixel computation. `generate_with_tier(..., tier)` explicitly selects the alternate spatial bound. Immutable metadata, original identity, camera-linear prefix and pre-WB/calibration reduction order are retained.
- Generator revision and container header advance to 2; new metadata requires an explicit supported tier. Decoder accepts legacy header 1 / generator 1 without a tier as Detail2560; explicit tier metadata in v1, absent/null/unknown v2 tier, generator/version mismatch and tier/scale/dimension mismatch are rejected. Re-encoding a legacy decoded proxy writes v2. Legacy pixels/calibration are not re-generated.
- Maximum decoder/allocation bound remains 2560. Tier-specific expected scale is strictly recomputed from original crop dimensions. Tier and generator are bound by the existing container digest.
- Existing F16 error criterion and full-payload F32 fallback are unchanged. Signed/HDR tests now cover both tiers; new tests cover roundtrip dimensions/scale, default-generation equivalence, custom-WB edited render roundtrip, mismatched-tier refusal and legacy v1 decode/re-encode.

## Measurement option

The existing ignored `real_raw_fixture_measurement` test accepts `TESSERA_CODEC_TIER=compact2048` or `detail2560` (absent means Detail). Existing required `TESSERA_CODEC_RAW_FIXTURE` and `TESSERA_CODEC_OUTPUT_DIR` remain. Compact results go to `sony-compact2048-camera-linear.clp` and `sony-compact2048-measurement.json`, leaving historical Detail filenames unchanged. Reports now include tier/max-edge. Run later on the sole compiler lane; nothing was measured here.

For the known 4920x3276 source crop, predicted integer scales are Detail=2 and Compact=3, giving 2460x1638 and 1640x1092 respectively. That is sample-count arithmetic, not a compressed-size/performance claim. Compact still performs the full-resolution prefix/demosaic before reduction.

## Integration boundary

No FFI/UI/default-product selection changes. Root chooses whether/when to apply and expose Compact after measurements. Existing service calls to `generate` preserve Detail. New v2 output is intentionally unreadable by old v1-only readers; updated readers retain v1 compatibility. Validate codec tests (including unit signed/HDR cases), existing generator/image-core roundtrips, strict checks and opt-in fixture measurements after application. The compatibility test reconstructs a v1-formatted container from unchanged payload bytes; it is not an archived independently produced binary fixture.
