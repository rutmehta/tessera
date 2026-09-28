# EXP-45 candidate integration audit (source-only)

Scope: read-only comparison of current origin/main at 969935797b3635f1fe3df62d0c7e3c8c31f3fe06 with preserved gain-map branch c7164602ea96464f1d87bfb007e6dbc5b3f8f45b and its dirty worktree at /Users/rutmehta/.codex/worktrees/tessera-mailbox/tessera. Its HEAD is preserved, and its working tree has uncommitted candidate product/test files (listed below); I did not modify it, run tests, or build. The branch commit itself is evidence/docs; the implementation under review is in its working-tree diff. No runtime fix is established and no acceptance is claimed.

## Minimum candidate file set if disposition allows integration

Encoder/routing:
- crates/export/src/gain_map.rs (new)
- crates/export/src/lib.rs (gain_map setting, headroom and encode routing)
- crates/export/src/hdr.rs (shared headroom/render path)
- apps/tessera-cli/src/export.rs (gain-map CLI option and setting)
- crates/tessera-ffi/src/export.rs (FFI option, validation, conversion)
- crates/tessera-mcp/src/exports.rs (HDR JPEG maps to gain-map route; incompatible transfer rejected)

Tests:
- crates/export/tests/gain_map.rs
- crates/export/tests/support/gain_map_fixture.rs
- crates/export/tests/support/gain_map_imageio.rs
- crates/export/tests/support/gain_map_coreimage.m
- crates/export/tests/support/gain_map_coreimage.rs
- apps/tessera-cli/tests/gain_map_hosts.rs
- crates/tessera-ffi/tests/gain_map_hosts.rs
- crates/tessera-mcp/tests/gain_map_hosts.rs

The candidate hunk in crates/engine-api/src/tools.rs only updates the hdr_transfer documentation. The untracked tools/coordination and M2-45d plan/evidence are coordination material, not required product port. Do not cherry-pick/merge the whole historical branch or replace current export files.

## Compatibility and merge constraints

1. Current main includes commit 0473a87e: render_one_cancellable strips baked development XMP from every rendered output using packet.map(|p| p.without_development()). The gain-map candidate was based on older code that strips only DNG. When integrating its gain_map encoding branch, retain current main's unconditional sanitization. The candidate should feed that sanitized packet into the standard XMP/native metadata route so both embedded JPEG metadata and adjacent XMP sidecar preserve current behavior. Do not restore the old conditional or replace current metadata_packet/PreparedExport/commit code.

2. Adding public ExportSettings.gain_map is a Rust source-compatibility change for external struct literals. Workspace literals commonly use ..Default, but all workspace targets/tests still need compile verification; downstream literal users will need the new field. The FFI ExportOptions.gain_map record field changes the UniFFI-facing API and requires its generated/checksummed bindings to match if exposed to Swift. Candidate FFI/CLI/MCP mapping exists, but no Mac Swift reference was found in the product sources checked.

3. MCP deliberately changes HDR JPEG behavior: hdr=true with JPEG now routes to gain-map JPEG, while a non-null hdr_transfer is rejected. Keep that policy explicit and ensure tool schema/help/host tests describe it; don't silently revert it to SDR or PQ/HLG.

4. Candidate gain-map validation also rejects watermark, AI masks/depth, incompatible format/color-space/transfer, unsupported upscale, and absent/zero recipe headroom before output publication. Preserve these guards while reconciling with current main's newer XMP sanitization and generated-binding state.

## Rational wire-format review

gain_map.rs serializes log2-headroom and gain bounds as signed-numerator/unsigned-denominator rationals. It rounds stops × 1,000,000 and writes denominator 1,000,000; the existing accepted recipe bound is 0–16 stops, so numerator is far below signed 32-bit overflow, and denominators written here are nonzero. The test parser reads i32/u32, matching this representation. The primary explicit field-layout assertion is for a 2-stop case; resize/sharpen tests exercise 1/2/4-stop reconstructed peaks. The independent equivalent 4/1 versus 4,000,000/1,000,000 comparison found no change. This gives no basis for changing the serializer, but is isolated diagnostic evidence rather than validation of the production file path.

## Acceptance not established

The original frozen ImageIO gate remains 4 passed / 1 failed for the 4-stop target (rendered peak ~7.98376 vs requested 16) with its original 4% assertion. Supplemental Core Image and other control results do not supersede that. Candidate implementation has not been accepted or validated against the current main tree. Any later integration needs the current-tree focused export/native decode, CLI, FFI/generated-binding, and MCP gates, while preserving the original failure/disposition and its separate control evidence.
