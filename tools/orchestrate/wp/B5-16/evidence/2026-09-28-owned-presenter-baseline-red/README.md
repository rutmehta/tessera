# Owned presenter historical regression — actual RED

Exact preserved source010617b835be756495a7ca917a5b673c467a3401, unchanged FFI8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03. One selected test compiled and executed: DocumentSaveDismissAttachmentTests/testKnownGapEntireAttachmentAfterCancelWithoutAnyObservationNeedsProgress. It failed the intended line707 assertion: successor saveAsRequest was nil instead of the new request UUID. Direct child exit1, no watchdog timeout. This is an actual failure, not XCTExpectFailure or a waived acceptance gate.

Root verified the raw log SHA6741eaa1c36452c0046015b8bbcccaa2089381154a9fb77b5b6f0c6d974f4c0c and identical 305-entry source/generated/archive freeze records before/after. HEAD stayed010617b8 and tracked checkout stayed clean. In-memory AppKit/stub fixture needed no external image. Command, process identity, direct exit, raw compiler/test log, and source hashes are retained here; large Swift scratch stays external at /Volumes/betterSSD/tessera-validation/document-save-settlement/baseline-red-010617b8-20260928-attempt1/scratch.

This proves the prior inferred-lifecycle liveness failure; it is not current presenter acceptance. Replacement93244f8f with current main/current4a45 archive still needs focused/full and real GUI gates. Standalone native probe main379ae615 is complementary evidence, not a substitute.
