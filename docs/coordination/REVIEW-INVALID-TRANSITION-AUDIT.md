# Review refresh `InvalidTransition` source audit (2026-09-27)

Read-only audit. No test, app, compiler, or process was launched, and the frozen `codex/recipe-write-revision` branch was not changed for this audit.

## Observed boundary

The retained RED at `/Volumes/betterSSD/tessera-validation/psd-copy-operation/review-refresh-red-77c4733058af/red.log` reports `InvalidTransition { phase: idle, targetPhase: failed(deinit) }` from `AgentReviewLayoutTests.testRowsOnlyRecipeAndFileUpdateReloadsCurrentReviewPreview`. The diagnostic rerun at `review-refresh-red-81a6b761d4fa/red.log` prints `scanned`, `agent settled and catalog drained`, `initial preview ready`, and `recipe serialized`, but never `recipe written`. The next synchronous expression in the frozen test is `try library.engine.setRecipeJson(...)` (`AgentReviewLayoutTests.swift:52` in the diagnostic commit). Both runs use FFI archive SHA-256 `0a9b2de3dee742751da067147805715925ac005e7036d0268d36296dc25168ae`.

The generated Swift binding `TesseraFFI.swift:7758` passes the call to `uniffi_tessera_ffi_fn_method_engine_set_recipe_json` through `rustCallWithError(FfiConverterTypeBridgeError_lift)`. Its call-status dispatcher (`:320-365`) can throw a lifted `BridgeError` on `CALL_ERROR` or `UniffiInternalError.rustPanic(message)` on `CALL_UNEXPECTED_ERROR`. The retained XCTest text does not show the UniFFI status code or Swift dynamic error type, so it cannot distinguish those paths.

## Source and binary search

`rg` of Tessera Swift/Rust source, Cargo registry sources, and SwiftPM source checkouts found no definition of `InvalidTransition`, `targetPhase`, or `failed(deinit)` (unrelated Windows `DB_E_INVALIDTRANSITION` constants exist). The exact `InvalidTransition` and `targetPhase` bytes were also absent from the retained `libtessera_ffi.a`, linked `TesseraPackageTests` Mach-O, and dSYM by raw-byte/string search; `nm -m` found no corresponding named symbol. This does **not** exclude a dynamically linked Apple framework/runtime, a dynamically constructed error, or a panic payload formatted from nonliteral fragments. Linked frameworks include Foundation, AppKit, ImageIO, CoreML, SwiftUI, and ImageCaptureCore; the audit did not locate the exact string in their on-disk framework paths either. No stack trace, Rust panic hook output, or captured call status is present in the preserved logs.

`Engine::set_recipe_json` (`crates/tessera-ffi/src/lib.rs:412-446`) parses/validates the incoming recipe, checks append-only history, loads the current document, persists recipe/XMP/index, then notifies listeners. The test changes `settings.tone.exposure` directly in JSON without advancing `history.head`; `Recipe::validate` (`crates/engine-api/src/recipe/mod.rs:404-418`) would ordinarily reject settings that differ from history replay with `conflict: settings do not match history head`. This is a separate, source-proven test-input problem. That expected error is **not** the observed phase/deinit string, so it does not identify or dismiss the `InvalidTransition` origin.

## Conclusion and next discriminating evidence

Origin remains **unidentified**. The strongest bound is: the unexpected error surfaces during the synchronous UniFFI `setRecipeJson` call after valid Review preview setup; it has not been traced to Tessera source or the statically linked archive. A subsequent authorized diagnostic should record the caught Swift error's dynamic type and, for `BridgeError.Failure`, its message; also capture UniFFI call status and a Rust panic/backtrace if unexpected. That would separate a normal engine rejection from an FFI panic or framework/runtime transition error. Preserve the two existing RED logs and do not relabel the phase error as fixture misuse.
