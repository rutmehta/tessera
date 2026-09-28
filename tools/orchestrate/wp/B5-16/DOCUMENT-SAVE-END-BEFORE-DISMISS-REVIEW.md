# End-before-dismiss source review — UNRUN, remaining liveness gap

Request36b2b1ad-887a-42f7-a780-435ecf9bfd1e accepted after exact B target/expiry validation.
Trace-free branch codex/document-save-dismiss-attachment retains e8ddabfa base and 3d08368e attachment-at-dismiss correction. Apply narrow commits only to A current main; preserve Layers/status/activation APIs.

Tests6877a4af then1b33e2e3; productfa3a69ba. ALL UNRUN. The final test set contains one deliberately unsatisfied contract regression; this is NOT an all-green/merge-ready package. No skip/XCTExpectFailure suppresses it.

## API evidence and conclusion

Apple didEndSheetNotification identifies only the parent and has no userInfo: https://developer.apple.com/documentation/appkit/nswindow/didendsheetnotification
SwiftUI sheet(item:onDismiss:content:) documents a dismissal callback, not relative ordering with AppKit notifications: https://developer.apple.com/documentation/swiftui/view/sheet(item:ondismiss:content:)
Neither contract establishes that SwiftUI dismissal always precedes native end. The reversed sequence is therefore a required defensive permutation, not a sequence proved impossible by API contract. It has not been observed in A's supplied e51 trace, which had dismissal first. Do not claim a new native reproduction from these synthetic tests.

## Identity-safe bounded improvement

Cancellation/supersession settlement is an earlier point than clearing the presented item. It can observe the captured parent's actual attachment before native end. fa3a69ba factors the existing exact-request/captured-parent/captured-sheet equality check into observeCapturedSaveSheetAttachment and calls it during settleDocumentSave before clearing presentation, as well as the existing dismissal boundary. This records positive attachment evidence, never detachment. The existing native-end and SwiftUI join still controls release. A wrong request, unrelated attached sheet, or missing attachment cannot set the flag. No new observable reads, notifications, timers, polling or forced sheet ending.

A synthetic regression records initial unattached capture, then actual attachment at Cancel, native end before SwiftUI dismissal, and a retained live probe. This should now progress with positive captured identity. Negative control sends an unrelated sheet's end and must remain held. Positive control confirms already-observed attachment can end before SwiftUI dismissal, and still waits for that dismissal. All are UNRUN; these are source predictions.

## Remaining contract gap — explicit acceptance blocker

`testKnownGapEntireAttachmentAfterCancelWithoutAnyObservationNeedsProgress` is expected by source inspection to FAIL both 3d08368e and fa3a69ba. Run it as an explicit RED characterization, not as a passing acceptance gate. Its sequence: initial unattached capture; Cancel while unattached; attachment and detachment entirely between all callbacks; parent-only end; SwiftUI dismissal; live probe never dismantles. No positive lifetime evidence exists, so the guard correctly refuses to release, leaving successor blocked.

That state is indistinguishable from an unrelated sheet ending while the captured sheet never attached. Releasing it solely from a clear parent or a parent-only end would make the negative control unsafe. A stale sheetParent value is not a per-generation completion token. This patch intentionally does not relax those guards.

To guarantee progress for that fully unseen lifetime, the presentation adapter needs an additional identity-bearing terminal contract: for example owning a specific AppKit beginSheet(sheet, completionHandler:) invocation with a per-generation token, then joining that completion with actual parent detachment. Apple binds that handler to the supplied sheet's modal session: https://developer.apple.com/documentation/appkit/nswindow/beginsheet(_:completionhandler:)
This is a design direction, not an implemented adapter rewrite or a claim the callback alone proves physical detachment. SwiftUI currently owns presentation; attaching a handler to its already-started native session is not supplied by this code. No guessed KVO compliance, willBegin-as-didBegin replacement, callback-order assumption, or teardown timing workaround is proposed as proof.

A should retain this remaining gap as an acceptance blocker or explicitly narrow the accepted contract; do not infer full lifecycle liveness from the bounded positive fix. Product changes are limited to the shared identity helper and earlier positive observation. B ran only source inspection and git diff --check, no tests/build/apps/benchmarks. A owns all gates/main; B hold, writer and heartbeat unchanged.
