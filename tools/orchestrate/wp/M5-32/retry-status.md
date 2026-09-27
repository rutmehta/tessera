# M5-32 retry verification

The focused command `cargo test -p tessera-ffi --release --test document_restoration` was run in this attempt and failed with `unsupported: neural/photo_restoration` at `crates/tessera-ffi/tests/document_restoration.rs:45`.

The exact requested chained gate was also run in this attempt with the inherited `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32`. It exited 101 at the same regression. Full output is `retry-gate.log`. Because the commands are chained with `&&`, Clippy, formatting, workspace checking and the Mac build did not execute in this gate. Previous attempt results are not claimed as current verification.

## Confirmed scope blocker

`crates/tessera-ffi/src/document/filters.rs` already declares PhotoRestoration (line 75), recognizes its adapter ID (line 197), and maps the request to `neural/photo_restoration` (line 2175). Both pixel and native smart-object evaluation route through `filters::CompositorFilters`.

`crates/filters/src/compositor_adapter.rs` does not recognize restoration in its neural parameter decoder (lines 79–90) or evaluator (lines 140–151). Completing the shared adapter needs restoration parameter validation, a model slot and explicit loader, and dispatch to `ml_filters::PhotoRestoration`, including a missing-weights error. This path is excluded by the current allowlist. Adding an unconditional missing-weights error in the FFI solely to pass the regression would not implement restoration.

The other previously reported boundary remains: `crates/compositor/src/resident/program.rs:390–402` rejects enabled lookup dither and Match Color neutralization. Implementing these in the shared GPU interpreter requires coordinated access to the excluded `crates/compositor/src/resident/adjustments.wgsl`, rather than silently dropping options.

No source files were changed in this retry. Existing changes were preserved. Only this report and `retry-gate.log` were added under the allowed work-package directory. No commits or pushes. No Kanban task ID is present, so `kanban_show()` could not identify a card to block.

Required unblock: authorize minimal shared adapter/shader changes or have their owners provide those integrations. The existing round3-status.md details the remaining Photoshop neutralization and real-photo performance verification limitations.

RESULT: FAIL required shared restoration adapter is outside the allowlist; the exact gate fails on document_restoration.
