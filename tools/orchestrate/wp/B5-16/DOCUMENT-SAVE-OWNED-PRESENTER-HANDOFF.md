# Owned Save As presentation source handoff — 2026-09-28

Request: be627c0e-b85b-4f4f-8b07-88ee651038c1. Baseline: 010617b8 from
origin/codex/document-save-dismiss-integration. Branch: codex/document-save-owned-presenter.
**SOURCE ONLY / UNRUN on B. Not product acceptance. A owns compilation, native gates and main.**

## Checkpoints and scope

Tests-first commits: 3a7c0d1c, b3277f15, 98141b95, c3de5e08, 412654d0, 84dc314c.
Production adapter: 8fd107f5. Apply the contiguous range after 010617b8; tests use the new
protocol and do not compile on their own before the production API exists. No claimed RED/green.
A can establish the historical behavioral RED using the explicit unseen-lifetime test on
010617b8, then establish behavior against the migrated owned-driver tests and product.

Product source is restricted to the new DocumentSavePresenter.swift, DocumentSheets.swift,
DocumentWorkspace.swift Save As state/operations, and the one ContentView Save As modifier.
Source comparison confirmed Opening/load/status/activation region and legacy write/export/close
region byte-identical to 010617b8. AppModel, other sheets, theme, dependencies and engine untouched.
`git diff --check 010617b8..8fd107f5` passed. This is a whitespace/source check, not compilation.

## Ownership implementation

- DocumentSavePresenter.swift: token/driver contract near lines 4–52, binding generation near
  81–112, reentrant invocation/drain reducer near 113–194, AppKit driver near 198–292.
- Every form and Replace gets a fresh presentation UUID in addition to logical request UUID.
  Active entry is installed before factory/begin; begin-in-flight latches cancel until return.
  Completion response is latched once. Native parent events only recheck membership.
- Physical detach requires neither attachedSheet nor ANY entry in captured parent.sheets to be
  the captured owned sheet. Completion and membership clear are joined in either order. The
  old sheet.sheetParent property is intentionally not used as physical evidence.
- endSheet receives ONLY the exact created sheet and captured parent after membership check.
  End issuance is latched before external calls; unrelated admission fails closed. The driver
  never ends a looked-up attached sheet and never synthesizes a completion.
- The parent bridge (DocumentSheets.swift near 181) creates a UUID per NSView incarnation.
  Updates do not register an old UUID again. remove/clear/update require current UUID; close
  callbacks additionally carry captured window identity. Old same-window bridge teardown and
  old-window close cannot clear a newer binding/ticket. Temporary window loss clears the current
  host; dismantle invalidates that incarnation. Reattachment can update only a still-current UUID.
- Workspace event handler near 440 advances form -> Replace or write only after owned drain.
  Replacement is another owned invocation and must drain before write. Logical cancellation
  settles once while native entry can keep draining. A newer request waits for that entry.
- Admitted writer preserves its actual result, marking continuationCancelled on cancellation or
  exact host loss. Host-loss callback remains active after form retirement; it targets operations
  captured on that binding/window pair. Existing GUI with unavailable bridge fails presentation
  instead of accidentally entering legacy headless automatic Save As.
- SaveAsSheet near 228 uses local request state plus weak workspace actions. Original filename,
  chosen folder, format rename behavior, 520x330 content, SheetScaffold, existing accessibility IDs,
  Return/default and Escape/cancel shortcuts are retained. Existing deferred initial field focus
  remains focus-only; there are no lifecycle timers or added Observable lifecycle reads.

## Nested chooser and lifetime assumptions

Captured NSOpenPanel remains alive only through its runModal call. Cancellation sends cancel to
that panel alone. Parent end waits for actual child return; response/membership alone cannot
retire while childActive. The return clears childActive, rechecks the parent reducer, and applies
folder only if the same uncancelled invocation is still active. No global abortModal.

Native callbacks intentionally keep the presenter/entry/session drain context alive after weak
UI action owners disappear. Normal retirement clears callbacks, native notification observers,
NSHostingController, alert and parent/sheet references BEFORE outward drained callback. Parent
bridge explicit dismantle/close initiates cancellation; deinit is not used for AppKit mutation.
Idle close observer uses weak view action and a token wrapper with thread-safe deinit removal.
One active entry plus one workspace successor is retained; requests do not accumulate native windows.

Supported lifetime assumption requiring A verification: after beginSheet succeeds, endSheet or
native parent close must eventually deliver that invocation's completion and remove captured
membership (after nested chooser returns). If AppKit does not fulfill this for queued-only or
closing-parent sheets, this implementation intentionally stays held and may retain that single
drain context. **That is an unresolved native contract/acceptance gap, not a leak-free guarantee.**
Do not waive the gap by clearing ownership on window close, absent membership alone, a parent-only
notification, view teardown, timer, or another MainActor turn.

## Explicit native queued-sheet contract verification limit

Read installed NSWindow.h (MacOSX SDK), lines 578–602. beginSheet documents queueing behind an
existing presentation; sheets explicitly includes presented AND queued windows. endSheet has no
queued-only cancellation/completion guarantee in the header. Apple documentation describes ending
the specified sheet but does not explicitly settle that queued-only edge:

- https://developer.apple.com/documentation/appkit/nswindow/sheets
- https://developer.apple.com/documentation/appkit/nswindow/beginsheet(_:completionhandler:)
- https://developer.apple.com/documentation/appkit/nswindow/endsheet(_:returncode:)
- https://developer.apple.com/documentation/appkit/nswindow/didendsheetnotification

B has NOT executed AppKit to prove it. The fake's queued-membership test verifies the reducer,
not native queued cancellation. Production normally refuses unrelated initial membership; native
reentrancy/queueing still needs a real gate. A must create an owned queued sheet behind a separately
captured unrelated sheet, end ONLY the owned queued sheet, and observe its completion plus removal
from parent.sheets while the unrelated sheet stays unchanged. If this does not work, report the
contract gap and revise the design before merge; do not manufacture synthetic event proof.

## Test migration and required A gates

DocumentSaveSettlementTests retains the explicit name
`testKnownGapEntireAttachmentAfterCancelWithoutAnyObservationNeedsProgress`: cancellation and
successor happen without any SwiftUI appearance/update/disappearance/dismantle, and the actual
successor must start after owned completion+detach. It is not skipped or expected-failed.
Historical e51 actual GUI failure and baseline inferred-probe tests remain in Git; they are not
reclassified as passing. Old probe bookkeeping tests have been replaced by invocation outcomes.

New deterministic requirements cover both join orders; completion while physically/queued held;
parent-only event negative control; duplicate/stale completion; cancel-before-begin from factory;
whole native lifetime inside begin; reentrant cancel/end completion; fresh same-window binding
versus old update/teardown; old parent close after rebind; child-held/reentrant cancel; child still
held after early parent completion; parent loss with pending successor and admitted writer;
Replace's distinct token/drain; stale approval, wrong controller, duplicate write; reentrant logical
cancellation; source write failure and folder preservation. All tests **UNRUN** on B.

A must compile strictly and establish meaningful behavioral RED/green. Then test real tiny-document:
Cancel -> repeat; queued successor and rapid supersession; same target Replace/Cancel/Replace;
different target; Escape/Return/focus; format/source filename/folder; nested Choose cancellation and
parent close while nested; parent close during native drain; same-window bridge rebind; queued-only
owned cancellation described above. Observe no unrelated sheet mutation, exactly-once outcomes,
no stuck successors, and retirement of observers/windows after drain. Full suite and these native
checks are required before acceptance; no performance, large-document, or leak-free claim here.
