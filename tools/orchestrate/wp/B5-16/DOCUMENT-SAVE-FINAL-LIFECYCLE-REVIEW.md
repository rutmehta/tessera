# Final Save As lifecycle source review

Request: `82acf317-b1a6-463a-8c9d-7b0a55c8f996`.
Reviewed candidate: `2a94a1878a760b41c6e6aa2c80be7a5feb60eeee` on
`origin/codex/document-save-validation`, against fetched main `a790d47f`.
All source references below refer to that candidate, not this note's branch.

## Finding: P2 — native dismissal barrier is missing for ordinary cancellation/supersession

`apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift:524-531` falls directly
through to `completeSaveAsPresentationDismissal` when no native dismissal record
exists. That method clears presentation ownership and publishes a queued Save As
request at lines 534-548. The native observer is installed only by the
existing-destination branch of `finishSaveAs` at lines 561-564.

Concrete sequence: a choosing Save As sheet is claimed and attached; a second
Save As supersedes it. `beginDocumentSave` cancels the old request (line 385),
`settleDocumentSave` removes its operation and clears the SwiftUI item (632-642),
and `presentDocumentSaveSheet` queues the successor (497-501). No native observer
was installed because the old request never submitted an existing destination.
When SwiftUI calls `onDismiss` while the old native sheet is still attached—the
ordering already observed by A—the fallback at line 531 publishes the successor
and clears the old identity immediately. The same gap occurs after explicit
Cancel followed by a queued new request. The application therefore requests a
second sheet without establishing that the first has detached. Actual native
presentation failure for this particular sequence remains untested; the missing
admission barrier is directly visible in the source.

`DocumentSheets.swift:225-229` resets the captured identity and invokes this
fallback from SwiftUI's callback. There is no additional native attachment guard
at the successor's claim boundary (`DocumentWorkspace.swift:512-520`). The
existing `DocumentSaveSettlementTests.swift:96-109` supersession test expressly
expects the successor immediately after that callback, without a separate native
detachment event. Replacement-path tests at 290-347 do not cover this path.

Recommended bounded correction: preserve the actual attached sheet's ownership
across cancellation/supersession as well as replacement. Install the matching
native observation before clearing an attached sheet's item; do not lose it when
the logical save operation settles. Require actual detachment before publishing
the successor. Preserve the unshown-request fast path and the claimed-but-still-
appearing case rather than requiring a native window that does not yet exist.
Add a deterministic claimed/attached choosing request -> supersede -> SwiftUI
dismiss -> native detach regression, asserting no successor before detach and
exactly-once cancellation; include stale/duplicate callbacks and window loss.

## Other reviewed boundaries and limits

The existing-destination path now joins SwiftUI dismissal with native detachment,
retains its observer through logical cancellation, removes observers on window
loss, and rejects an unrelated attached sheet before showing Replace. Its parent
attachment predicate correctly avoids requiring the old child's stale
`sheetParent` to clear. Request identity checks and the operation-removal latch
protect late replacement callbacks and duplicate writer completion. The probe
refreshes its captured request/workspace on representable reuse. No additional
concrete blocker was identified in the net Document/Shell diff inspected here.

A reports 32 passing targeted tests and same-target Replace/Cancel GUI success.
Those are A's results, not B reruns; the distinct-target matrix and full suite
remain pending in the assignment. Preserve all prior failed GUI evidence.
This review does not grant final acceptance or merge approval. B ran no builds,
tests, apps, benchmarks, or heartbeat. No product/test source was changed.
