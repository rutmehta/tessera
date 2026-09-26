# M2-29 current retry

Result: FAIL. No production code changed in this retry. The explicit allowlist still prevents fixing the contradictory test and completing MCP integration.

Executed the exact requested validation chain with CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-29. Exit 101; evidence: current-verification.log.

Failures:
- crates/tessera-ffi/tests/develop.rs:501, jpeg_images_are_refused: unwraps a nonexistent error because JPEG opening now succeeds. This test is outside the allowlist. It must be replaced with successful rendering coverage, not bypassed or accommodated in production.
- crates/tessera-ffi/tests/develop.rs:1179, export_batch_does_not_starve_slider_drag: failed its L2 assertion during the full suite. An isolated rerun passed (current-drag-retry.log). This is not evidence that the full suite passes.

The JPEG non-black develop regression and resident RGB test passed in the full run. Clippy with the requested flags and cargo fmt --check were run independently and passed (current-clippy.log and current-fmt.log). git diff --check passed. The && chain did not reach Swift, and Swift was not independently rerun in this retry.

Read-only inspection confirms crates/tessera-mcp/src/pixels.rs still omits HEIC from its extension gate and performs an independent sRGB-only conversion instead of using the new ICC-aware RgbSource. That integration remains outside the allowlist. See REPORT.md for the existing implementation and needed engine-api fields.

Required scope decision: allow crates/tessera-ffi/tests/develop.rs and the relevant crates/tessera-mcp decoder/preview, manifest, and tests before another implementation retry. No tests were disabled, no out-of-scope files edited, and no commits created.

Board orientation returned task_id is required (or set HERMES_KANBAN_TASK in the env), so no task lifecycle transition is available in this session.
