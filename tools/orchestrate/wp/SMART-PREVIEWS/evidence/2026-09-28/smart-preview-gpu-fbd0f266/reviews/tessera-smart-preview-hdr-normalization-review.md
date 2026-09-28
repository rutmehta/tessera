# Bounded HDR presentation normalization source review

Source-only review of the export-integration candidate. No edits, builds, GPU workloads or runtime acceptance by reviewer. No actionable source finding in this follow-on.

Generation copies settings and clears only output.hdr/output.hdr_headroom_stops for strict prefix validation. Original settings are borrowed and remain unchanged; encoded proxy prefix does not capture presentation. Unsupported proofing remains rejected. Generic pipeline-cpu render.rs validator is unchanged, preserving SDR/managed public API rejection contracts.

Thumbnail worker validates original journal and prefix first, copies settings, clears the same two fields only, then uses the temporary settings consistently for full admission, extent and explicitly SDR rendering. Cache key and freshness identity still use the full captured recipe; normalization does not weaken stale-publication checks or mutate history. Generation and thumbnail do not call broad renderable sanitization.

select_proxy calibrates with a temporary clone clearing only HDR policy, appropriate to its pre-display SDR ring. The actual recipe is retained for live Develop presentation. Forced-GPU EDR tests do not substitute for Auto calibration measurements.

Tests reviewed: exact encoded-prefix equality between SDR/HDR recipes; original settings unchanged; generic validator still rejects HDR; unsupported proof profile rejected; actual proxy Develop HDR edit/flush/close followed by SDR thumbnail byte equality; exact serialized journal settings/history unchanged; reopen retains HDR+2stops; proofing thumbnail rejection leaves journal unchanged and original folder absent.

Actual Engine harness checks full recipe preservation across build and Engine reopen, retains true HDR for EDR runs, and records/asserts actual resident receipt RenderOutput::DisplayLinear(4). After flush/close, reopened session retains HDR policy. Explicit Auto routes exist; runtime results are pending FFI. Scalar CPU Auto frames do not emit resident receipts and must remain reported as such.

Reviewed file SHA256 snapshots (not a full build-input freeze):

- eed404174143b28d470ca888e3291d46a580c36f3f73f49b749992cbf6233ecb  crates/pipeline-cpu/src/smart_preview.rs
- 01a4786f5135ff5eadbd77b796f2fa9535614723fd9503457d76769b972edbeb  crates/pipeline-cpu/tests/smart_preview.rs
- 9fc0353120c72a3f13a32231bf431de513e145b79f544a1409171bebbdb34801  crates/tessera-ffi/src/smart_preview_thumbnail.rs
- 885a8aaae31762ee54349057d7c29bfca5449ef9c380548b77919f66ebd67235  crates/tessera-ffi/src/smart_preview_thumbnail/tests.rs
- 8d0d3599e22c9bc61b459fdf0ebbce8d08990f3f2b95109755695956d3d2bbe9  crates/tessera-ffi/src/backend.rs
- 0391ff43ff69b125213d13e60da90709d19e6ea749bbeb97c95ba1e134a40a7b  crates/tessera-ffi/src/develop/preview_qualification.rs

Final test-only enhancement inspected: actual EDR pixel readback is finite within0..4 and includes a >1 HDR highlight per route, outside timed interval. This strengthens presentation evidence without changing product or tolerance.
