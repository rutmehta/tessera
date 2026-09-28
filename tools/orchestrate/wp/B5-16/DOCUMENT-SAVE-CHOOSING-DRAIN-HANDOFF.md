# Choosing-sheet native drain correction — source only

Request 9ddfd017-dfe5-40ef-b357-7a76f258dea8, accepted after exact B target/expiry validation.

Tests: 13b0bd97 and 41d88bc0 (five new regressions, UNRUN).
Product: 2cc48419. Apply these three commits onto A's validation candidate; do not merge B branch ancestry or restore its old diagnostic instrumentation. The product patch replaces a previously instrumented method region, so A's diagnostic revert may require a context-only cherry-pick resolution.

Native ownership now starts at saveAsPresentationWillPresent, independent of the logical save operation. Parent begin/end/close observers are installed before cancellation can clear the requested item. The existing probe may capture the actual sheet even after the operation settled. Capture or didBeginSheet records actual parent attachment; didEndSheet can release only that observed sheet identity. No child sheetParent clearing requirement, delay, poll, or scheduling assumption is added. The replacement path reuses this ownership.

Logical cancellation still settles once immediately. Presentation ownership requires both SwiftUI dismissal and native detachment before a successor is published. Parent loss removes observers and fails a queued request belonging to the closing parent. An unrelated attached sheet rejects queued admission without ending that sheet. Unclaimed requests still cancel/supersede immediately. Claimed-but-appearing requests retain ownership when a native window has not yet been captured: missing capture is not interpreted as detached.

The existing injected saveSheetDetachmentObserver seam now registers at presentation claim, rather than only on existing-destination submission. No public save/load API changes. New regressions cover delayed detach after supersession, exactly-once cancellation/stale and duplicate callbacks, cancellation then parent loss, missing native capture during appearing cancellation, queued parent loss, and native-first versus SwiftUI-first ordering. Existing unshown and late-claim regressions remain.

Limits: all tests UNRUN on B; no compile or GUI claim. A must validate actual didBegin/probe ordering, cancellation/supersession during appearance, and both replacement GUI paths. If a claimed presentation never produces a native attachment or parent-loss event, it deliberately stays blocked; source does not invent a detach signal. A should specifically check this disappearing-before-attachment case for liveness. Existing A candidate 2a94a187's reported full 569 XCTest/1 skip plus 5 Swift Testing and same/distinct Cancel/Replace GUI passes remain evidence for that candidate only. Preserve all earlier failed GUI evidence.

Only source inspection and git diff --check were performed here. B resource hold, paused heartbeat and writer remain unchanged. A alone compiles, validates and merges main.
