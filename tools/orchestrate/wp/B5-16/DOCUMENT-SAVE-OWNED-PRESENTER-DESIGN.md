# Save As owned native presenter — design for independent review

Request ac23f8dc-8b72-4784-8122-efff55bd2042. DESIGN ONLY: no implementation, compiler, test or app run. B resource/heartbeat/writer hold remains. A reviews before authorizing source and owns validation/main.

Reviewed references: trace-free candidate ed41ff71 (including 3d08368e/fa3a69ba), current main bb4c3281, and the prior e8dd/cfdb/e51 evidence reported in Git. Main already has statusPublication and activateDocument through Layers/load/install/select. Port only the Save As region and adapter integration onto A's chosen current-main candidate; never replace DocumentWorkspace wholesale. No new framework/dependency, other-sheet redesign, theme change or Document load API change.

## 1. Problem and ownership contract

The existing SwiftUI `.sheet` adapter learns about the native sheet after creation, so an entire native lifetime can occur without positive attachment evidence. A parent-only didEnd cannot identify that lifetime. The intentionally failing test `testKnownGapEntireAttachmentAfterCancelWithoutAnyObservationNeedsProgress` remains an acceptance blocker; do not skip or waive it.

Replace only Save As presentation with an adapter that constructs its own NSWindow/NSHostingController and invokes `parent.beginSheet(capturedSheet, completionHandler:)`. The entry exists BEFORE the invocation; its completion closure captures an immutable native token for that exact invocation. A request UUID identifies a logical save; a separate presentation UUID distinguishes the form from its subsequent Replace alert and stale callbacks. Record parent and sheet ObjectIdentifiers plus the actual owned objects. Never reuse a sheet window across tokens.

Native completion identifies which invocation ended. Physical detachment is separately checked against that same parent: `parent.attachedSheet !== sheet && !parent.sheets.contains(where: { $0 === sheet })`. Use the latter to exclude a sheet still queued by AppKit. Do not require stale child `sheetParent` to be nil. Parent didEnd only triggers re-evaluation; it never selects which sheet completed. Both completion and detachment are required for a begun entry. No SwiftUI onDismiss, probe updates, view teardown, timer, task hop or polling is evidence of native completion.

## 2. Exact source surface

References below name symbols at ed41ff71; line numbers are navigation anchors, not transplant instructions.

| File / source location | Proposed change |
| --- | --- |
| `apps/mac/Sources/Tessera/Document/DocumentSavePresenter.swift` (new) | MainActor presenter protocol, AppKit implementation, native token/entry, owned form window, native completion/detachment reducer, shutdown and observer cleanup. No @Observable conformance. |
| `DocumentWorkspace.swift:374` `beginDocumentSave` | Preserve typed API, same-document writing guard and reentrant settlement. Submit a form request to injected presenter after operation registration. Queue only in the Save As owner, never implicitly behind an unrelated native sheet. |
| `DocumentWorkspace.swift:557` presentation methods | Replace inferred claim/probe/nativeSaveDismissals bookkeeping with explicit presenter tickets. Remove Save As-only saveAsPresentationWillPresent/DidDismiss, probe leases, synthetic window/clear-parent seams and attachment flags once replaced tests cover them. Keep the known-gap regression intent, migrate it to fake ownership events. |
| `DocumentWorkspace.swift` `finishSaveAs`, `beginReplacementPrompt:643` | Validate submitted value, store an identity-bound post-drain action: `.write(request)` or `.replace(request)`. End the owned form, then process the action only from its drained callback. Replacement uses a second native token. |
| `DocumentWorkspace.swift:672` `admitDocumentWrite`, `:706` `settleDocumentSave` | Preserve backend result and drain semantics. Logical settlement removes/latches operation completion once; presenter entry remains until native drain. Writing cancellation marks continuationCancelled; it does not pretend backend IO stopped. |
| `DocumentSheets.swift:170-264` | Remove Save As probe, presentation capture and SwiftUI `.sheet` modifier. Replace modifier with a non-presenting parent-window attachment bridge only if needed to obtain the exact host; it must not own or infer the sheet lifetime. |
| `DocumentSheets.swift:266` `SaveAsSheet` | Preserve Form/SheetScaffold/layout/identifiers. Replace strong workspace binding with immutable initial request and action closures `onCancel()` / `onSubmit(SaveAsRequest)`; keep local @State edits. Remove onDisappear cancellation. Actions capture workspace weakly and exact request/token. |
| `Shell/ContentView.swift:145` | Replace only DocumentSaveAsPresentation modifier with the parent-host bridge. All other sheets unchanged. |
| `Tests/TesseraCoreTests/DocumentSaveSettlementTests.swift`, new `DocumentSavePresenterTests.swift` | Preserve result/drain tests; replace inferred probe tests with deterministic fake native presenter reducer tests. Keep old failure evidence and meaningful assertions; do not delete the liveness requirement. |

The parent bridge reports the main content view's actual window, not NSApp.keyWindow (which can be a panel). Workspace stores that reference in @ObservationIgnored weak storage. make/update/viewDidMove report identity; dismantle invalidates exactly that host binding and invokes explicit presenter shutdown for it. Rebinding on view reuse releases old binding before attaching the new identity. Presenter owns drain beyond bridge teardown. This bridge introduces no Save As claim or lifecycle dependence on SwiftUI updates.

## 3. Proposed seams and events

Sketch, not compiled API:

    @MainActor protocol DocumentSavePresenting: AnyObject {
        func present(_ ticket: SavePresentationTicket,
                     events: @escaping (SavePresentationEvent) -> Void)
        func end(_ token: SavePresentationToken, reason: SavePresentationEndReason)
        func shutdown(host: SaveHostIdentity)
    }

`SavePresentationToken = (requestID, presentationID)`. Ticket contains exact parent binding, content kind (`form(request)` or `replace(request)`), and weak-owner actions. Events are `.began(token)`, `.drained(token, result)`, or `.failedBeforeBegin(token, reason)`. Drained is the ONLY post-begin release event to workspace. Raw native-completion and attachment events stay inside presenter; fake driver controls them independently. API need not expose NSWindow to ordinary workspace tests.

Split presenter into a deterministic entry reducer and small native driver. Driver operations: create/release owned window; begin exact invocation with completion(token,response); requestEnd(parent,sheet,token); read captured parent's attachment/membership; install/remove observers. Fake driver supplies immutable host/sheet IDs and snapshots, held/reentrant completions, unrelated notifications and begin/end counters. No test timer or real sheet required.

Entry flags/state: `prepared`, `beginInFlight`, `begun`, `endRequested`, `endIssued`, `completionReceived(response)`, `parentClosing`, `retired`. Store `nativeDetached` only from actual captured-parent membership checks. Separate workspace logical operation states remain choosing/waitingForPresentationDrain/replacing/writing/settled. Do not overload a saved result to mean navigation permission or native sheet lifetime.

## 4. Transition and admission rules

1. **Before begin:** create token/entry and register parent didEnd/willClose observers before constructing callbacks that may reenter. Reject missing/closing parent or existing unrelated parent sheets. Set active entry before begin. If cancellation/supersession wins before begin is invoked, retire as never-begun immediately: no native completion expected, begin/end counts zero. Discard stale queued ticket without allocating a window when possible.
2. **During begin call:** mark beginInFlight and beginIssued before entering AppKit. Cancellation reentry sets endRequested; do not call endSheet before begin has returned/established ownership. After return, if entry has not already completed synchronously, mark begun, refresh membership and service endRequested. Completion arriving synchronously latches against the already-registered token. Every transition rechecks current entry identity after external calls.
3. **Attached or native-queued:** cancel the logical save exactly once; retain entry and request end of ONLY the captured sheet. Verify captured parent's actual membership (attachedSheet or sheets) and token. Mark endIssued BEFORE calling endSheet to tolerate reentrant callbacks; never end parent.attachedSheet by lookup. If the captured sheet is not a member, do not end an unrelated sheet: re-evaluate completion/detachment and wait for the owned invocation's completion if still outstanding. Unexpected AppKit queueing remains owned and is ended by captured identity; no successor starts meanwhile.
4. **Completion and native end in either order:** owned completion sets completionReceived, then samples detachment. Parent didEnd samples all live entries for THAT parent, but cannot set completionReceived. If completion first and still attached, wait for didEnd; if didEnd first, wait for completion and re-sample. The fully unseen attachment case now has an explicit begin token and matching completion, so needs no probe/dismantle signal. Missing completion or observed membership still blocks release; no fabricated deadline success.
5. **Form Save:** invalid input keeps form. Valid input stores post-drain action and requests end; no backend write/Replace alert until form drains. If logical cancel/supersession/parent loss occurs during drain, clear action and never write. The current successful new-file write starts a little later (after form drain), but typed completion retains its meaning and exactly-once behavior.
6. **Replace:** after form drain, revalidate active request, document open and exact parent not closing/no attached or queued sheet. Construct alert with existing wording/buttons and its own presentation UUID; retain alert/window. Use alert.beginSheetModal's owned completion for that exact alert as the native driver's replacement invocation; the callback is identity-bearing just like beginSheet's form completion. Save/Cancel response is latched, then join actual detachment. Only affirmative response plus still-live operation admits writer. Cancel result settles once; native entry still drains. No same-token reuse between form and alert.
7. **Queued successor:** retain latest choosing request only; settle replaced queued requests once. Drain old native entry first, then prefer its post-drain action only if still active, otherwise offer queued successor. Old operation completions cannot clear newer IDs. Before every new native begin, reject unrelated sheet membership safely. Do not commandeer other modal windows or silently enqueue behind them.
8. **Writing:** after actual writer admission, cancel/parent close marks continuationCancelled while physical operation and its resource ownership drain. Result is real saved/failed, never fabricated cancelled after persistence. Presentation entry can be retired independently; backend ownership remains in the existing save operation. Reentrant completion sees already-latched state.

## 5. Parent loss, teardown and retention

willClose means about-to-close, not detached. Mark the captured host closing, reject new tickets, settle queued/not-writing operations as window-lost, mark writing continuation cancelled, and request end of the owned native sheet only. Do not advance successor/replacement on that host. Observe completion/detachment normally; never release just because willClose fired. A stale close from another host/token is inert.

`shutdown(host:)` is explicit on bridge teardown/rebinding and calls the same close/drain path. Do not depend on SwiftUI onDisappear or a MainActor deinit doing UI work. Observer callbacks capture presenter/entry weakly; active entry owns its observer-removal handles, sheet and hosting controller. Hosted actions retain neither workspace nor presenter strongly. Parent/native completion captures a token plus a small drain context, not the workspace; an in-flight context survives owner invalidation until its native completion/detachment cleanup. No workspace -> presenter -> hosted content -> workspace cycle.

Retire once: remove observer tokens, disable action closures, drop hosting content and alert references, order out/close ONLY the now-detached owned window, clear active entry, nil terminal callback, then invoke captured callback. Duplicate callbacks see retired token and cannot recreate ownership. A never-begun cancellation does the same cleanup without native drain. Closing/detaching the SwiftUI bridge cannot strand a retained logical save completion: logical settlement happens at shutdown; only bounded native drain context remains.

Liveness obligation is explicit: every successful begin is paired with owned completion/end, and membership transitions wake the join through installed didEnd or the completion itself. Fake tests must prove references/observer counts return to zero in each supported order, including parent close and owner teardown. If A discovers AppKit completes/ends without either callback ever exposing detachment, that is a new adapter contract failure, not permission to use a timer or retain a hidden window forever. Diagnose before acceptance. No unconditional liveness claim outside the documented native callback contract.

## 6. Existing content, keyboard and accessibility

Host the existing SaveAsSheet in NSHostingController, 520 x 330 content size; preserve SheetScaffold, typography, format picker, folder label and all `document.saveAs.*` identifiers. A fixed-size titled sheet window with hidden title/titlebar styling must visually match current content; no new theme. No resizable/minimize controls. Set accessible window title "Save As" and preserve labelled controls. Do not set representedURL or change document title/path before writer success.

The owned sheet handles cancelOperation/Escape by forwarding the exact token's cancel action (not orderOut/close directly). Keep Cancel `.cancelAction`, Save `.defaultAction`, disabled validation and Name onSubmit; duplicate Return routes are harmless via operation phase/token guards. Do not capture global keyboard events. Verify Escape first cancels a nested folder chooser rather than the parent Save As. Native close requests forward cancel and suppress unowned direct destruction until presenter drain.

Keep name FocusState behavior scoped to UI focus: establish initial name focus when hosted content appears/becomes key. Existing deferred focus assignment may remain solely for focus if needed, but cannot release ownership or prove native attachment. A must verify selection, typing, Tab/Shift-Tab order, Return, Escape, VoiceOver/AX labels, and same focus on queued successor. No lifecycle logic belongs in body or onAppear.

The Save As-owned Choose Folder panel needs explicit containment: retain only its NSOpenPanel while runModal is active; on parent/save cancellation call cancel on THAT panel, never NSApp.abortModal/stopModal for an arbitrary modal session. Its normal return releases a child-interaction lease and notifies presenter to continue a deferred end. No parent Save As teardown while this owned modal interaction is active. Guard returned folder assignment by still-current token. This is localized to SaveAsSheet.chooseFolder; no changes to other app sheets. Cover Cancel/Escape/parent close while chooser is open in acceptance. If root prefers converting this one chooser to a captured nested beginSheet completion, review that choice explicitly; it is not a required broader sheet rewrite.

## 7. Observation boundary

Presenter and token/entry storage are ordinary MainActor types, referenced via @ObservationIgnored workspace fields. SwiftUI observes only request-local editable form values and existing document title as required for display. Native entry creation/end/join occur from explicit actions/presenter callbacks, never during body evaluation. Parent bridge updates are identity-idempotent. No unconditional reads of presentedSaveAs/saveAsRequest are added for logging. Remove obsolete observed presentedSaveAs state if no longer used; retain a test-facing read-only semantic snapshot only from ignored presenter storage if needed. Diagnostics remain separate, disabled and non-observing.

## 8. Deterministic acceptance matrix (all proposed, UNRUN)

- Prepared cancel, supersession and host detach before begin: no begin/end call, one terminal result, no retained host/content.
- Cancel reentered during begin; begin completes synchronously; complete before return; end reenters completion: same token, <=1 end and <=1 drained event.
- Captured sheet attaches/ends with zero probe callbacks; completion before didEnd and reverse order; active SwiftUI content never dismantles: successor progresses only after completion + membership clear. This must turn the deliberately failing unseen-lifetime requirement into a real passing behavior test, not remove it.
- Wrong token/parent/sheet callback, unrelated didEnd, parent still owns captured sheet, captured sheet native-queued, new unrelated sheet after old detaches: no unrelated end, no premature write/next begin.
- Repeated Cancel, Escape, programmatic supersession and duplicate/stale completion: exactly-once logical outcome and native retirement; latest successor preserved.
- Save new path, existing-path Replace/Cancel, cancel between form drain and alert, stale affirmative response, parent closing while form/alert drains: no unintended writes; expected post-drain ordering.
- Admitted writer held, then cancellation/close: native window may retire; actual writer callback alone releases backend ownership and reports true outcome with continuationCancelled.
- Folder chooser held, parent cancellation and return: only captured child cancelled, parent waits for child return; stale chosen URL ignored.
- Workspace/view host invalidation and parent close in every phase: no successor on closing host, observers/content/windows/callback counts zero after native drain; no strong owner cycles. Late callbacks after replacement owner is installed are inert.
- Observation test/source audit: repeated view rendering does not call begin/end or add observed workspace dependencies; presenter transitions do not require a redraw.

## 9. A's real GUI gate and integration order

After root design review: fake contract tests first (retain honest RED on old adapter), product adapter and tiny Shell/content bridge in separate commits, then focused/full validation on exact current-main candidate preserving Layers APIs. No diagnostic source in final candidate.

Use isolated app/test-owned tiny 32x32 documents. Repeated Cancel -> immediate enabled Save As, Escape -> repeat, queued supersession via deterministic host seam plus actual visible presentation, valid new save/reopen, same-target and distinct-target Replace/Cancel with file hashes/reopen content, parent close/Don't Save, and Choose Folder nested cancel. Verify existing Document remains open on Save As Cancel, no stale queued sheet appears after document close, no unrelated sheet is ended, no duplicate native window, key focus and AX are correct. Compare old failure against exact new candidate and retain all previous logs. Full suite plus GUI are required; fake contracts alone are not native acceptance. Main integration remains A-only.

## Primary API references

- https://developer.apple.com/documentation/appkit/nswindow/beginsheet(_:completionhandler:) — completion belongs to the supplied sheet's modal session; existing sheets can cause queueing.
- https://developer.apple.com/documentation/appkit/nswindow/didendsheetnotification — parent notification, no sheet userInfo.
- https://developer.apple.com/documentation/appkit/nswindow/sheets — current and queued sheet membership, not nested sheets.
- https://developer.apple.com/documentation/appkit/nswindow/willclosenotification — about-to-close event, not proof of detachment.

This proposal is ready for independent review, not implementation authorization or evidence of fixed behavior.
