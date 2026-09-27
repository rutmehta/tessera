# M5-32 scope verification

Re-ran the requested exact chained gate with the inherited `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32`. Exit code: 101. Output: `round3-scope-verification-gate.log`.

The release test `restoration_missing_weights_is_atomic_on_pixels_and_smart_objects` fails at `crates/tessera-ffi/tests/document_restoration.rs:45` with `unsupported: neural/photo_restoration`. Because this is an `&&` chain, later Clippy, formatting, workspace check and Swift stages did not execute in this run. The focused restoration test independently reproduces the same failure.

The permitted dispatch is already present in `crates/tessera-ffi/src/document/filters.rs`: adapter ID at lines 196–197, shared evaluator call at line 223, operation mapping at lines 2174–2175. The actual rejection is in excluded `crates/filters/src/compositor_adapter.rs`: neural parameter decoding (lines 81–89), evaluation (lines 143–151), and explicit model-loading eligibility (lines 308–312) do not support restoration. Completing this path requires owner integration or extending the allowlist to that file. A special-case error or alias to JPEG removal would not implement restoration and was not added.

Separately, `crates/compositor/src/resident/program.rs:385–404` rejects enabled Color Lookup dither and Match Color neutralization because the shared interpreter shader lacks those operations. Completing GPU behavior requires integration in excluded `crates/compositor/src/resident/adjustments.wgsl`, not only the permitted program/specialize files. Prior remaining semantic/benchmark limitations are recorded in `round3-status.md`.

No implementation changes, commits, or out-of-scope edits were made in this verification attempt. Existing changes were preserved. No board task ID is available (`kanban_show` reports task_id required), so no lifecycle transition could be recorded.

RESULT: FAIL required restoration adapter and GPU shader changes are outside the allowed paths; exact release gate exits 101.
