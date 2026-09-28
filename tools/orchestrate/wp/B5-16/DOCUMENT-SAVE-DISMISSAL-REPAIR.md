# Save As native-dismissal repair candidate — UNRUN

Request031a10ea-4378-4533-b673-386587b16692. Prior result e9fe1c57 FAILED GUI acceptance: A exact5ad1cbcc passed 9save+4load+3adjacent tests, but two existing-file Save As attempts silently dismissed without Replace/Cancel. New-file write and Cancel had passed. This repair does not override that failure or claim GUI acceptance.

Source ordering verified: previous finishSaveAs cleared presentedSaveAs and immediately called beginSheetModal while SwiftUI was tearing down its sheet. This is consistent with the GUI failure, not a locally reproduced diagnosis; B ran no GUI/workloads.

Commits on codex/document-save-settlement:
- edb7a5ad: tests first, four new dismissal-gap cases plus existing tests adapted to explicit native dismissal; all UNRUN.
- ae0cd8e4: DocumentWorkspace/DocumentSheets handshake.
- c7f28b6c: separate tiny ContentView adapter replacement (one insertion, three deletions), the only shared Shell change. A owns resolving its local ContentView WIP; transplant only this Save As modifier line.

DocumentSaveOperation has waitingForDismissal phase and captured replacement request. Existing-file Save stores that request, clears the presented sheet and returns without presenting an alert or writing. DocumentSaveAsPresentation (DocumentSheets) owns the actual shown request UUID and calls saveAsPresentationDidDismiss(id) from SwiftUI onDismiss. Content onDisappear is not sufficient and cannot advance replacement. Workspace validates presentation identity, operation phase and exact controller before starting Replace; actual captured parent must exist and have no attached sheet, otherwise request fails explicitly. No timing delays or polling.

A successor Save As is queued while the old presentation identity remains owned. Native dismissal promotes only the current queued request. Cancellation/window loss/supersession removes old operation; its later native dismissal cannot start the stale Replace or a write, and a duplicate old dismissal cannot clear the successor. Native replacement alerts are retained on their operation and ended on cancellation; a new save while a native Replace alert is live reports busy. This is still a save settlement seam, not global close preparation or draft finalization. New-file writes keep existing behavior; Layers callback seam untouched.

Tests: Replace prompt count stays zero after finishSaveAs and content onDisappear; exactly one prompt after matching native onDismiss; Cancel during gap yields cancelled/no prompt; supersession queues successor then ignores old callbacks; lost window during gap yields failure/no prompt. Existing duplicate-approval/once-only writer/identity cases now explicitly advance native dismissal. Proposed tests use injected continuations, no native sheets/disk/GPU. Source diff check passed only; compile and all tests UNRUN on B.

Required A gates: compile/focused regressions; exact isolated GUI repeat new save, same-file existing overwrite Cancel, distinct existing-file Cancel, Replace acceptance once with verified file, Escape/dismissal, window close and supersession around teardown. Verify native onDismiss fires with captured presentation ID and detached parent; if attachedSheet still exists, this candidate fails visibly rather than guesses timing. Immediate cancellation before a SwiftUI presentation ever appears also requires GUI coverage. No general Sheet lifecycle/performance guarantee. Preserve original failed GUI evidence alongside new gates. B hold/paused heartbeat/writer unchanged; A sole main merges.
