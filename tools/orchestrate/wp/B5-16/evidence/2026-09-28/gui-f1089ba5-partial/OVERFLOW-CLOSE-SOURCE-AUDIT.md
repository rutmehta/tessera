# Document overflow selection / Close ownership audit

Verdict: no causally established product defect from current source. Existing selection is synchronous, so “async selection settled” describes the observed UI/event-delivery interval, not an application-owned asynchronous selection operation. A menu-action-versus-shortcut delivery ordering question remains for bounded GUI verification.

Source-only audit; no builds, GUI, source edits or B branch changes. Main inspected at `ab7425d13dccd30d2fc0903f55393b03629528ac`. Also inspected exact GUI candidate `f1089ba52fb565a101e2aa0985f1d87a112f81f9` through git show: main's current DocumentTabs does not contain B's explicit overflow menu, so conclusions about that menu are grounded in f1089, not falsely attributed to main.

## Preserved observation

Root reports that choosing an overflow document and immediately sending Cmd-W closed the previously current CLEAN saved document. After selection was visibly settled, dirty Close and Cancel worked. This audit did not rerun that interaction. There is no evidence or claim that saved bytes were lost, that a dirty document was discarded without prompting in a real window, or that the application committed selection before the first Close.

## Causal source trace

- f1089 `DocumentView.swift:301–320`: DocumentTabs takes a local snapshot of `workspace.documents`, calculates visible/overflow indexes, and each hidden Button directly executes `workspace.select(docs[i])`. The action captures that snapshot; it does not later index a potentially shortened live array. No Task, dispatch, await, delayed publication or pending selection token appears here.
- f1089 `DocumentWorkspace.swift:72,180–186`: workspace is MainActor isolated. `select` optionally tells the text editor to apply the previous document's draft, immediately assigns `current = doc`, then sets document mode. It does not await text work. Selection ownership therefore changes within this synchronous action; subsequent viewport/render settlement is not the selection authority.
- `AppCommands.swift:252–253`: Cmd-W calls `docs.closeCommand()`; the action does not capture the old `doc` as its close target.
- f1089 `DocumentWorkspace.swift:633–670` (main equivalent 625–660): closeCommand resolves current at invocation. close resolves the document once, then clean documents are discarded; dirty documents with a window present a sheet. The sheet callback retains that resolved document for Save/Don't Save; Cancel does nothing. A later selection cannot redirect an already presented close sheet.
- Async document *loading* exists separately in workspace.load/Task.detached. It is not the overflow switch between already installed documents and should not be used to explain this observation.

There is no supported pending-overflow-selection ownership state in this model. Before the menu action executes, Close owns the old current document; after select returns, it owns the chosen document, regardless of whether the canvas has redrawn. For Close to remove the prior current document after completed select without another selection change would contradict this source. Proving that ordering would be actionable; the existing observation does not establish it. Text draft application can schedule separate work but does not suspend current assignment.

## Existing coverage and smallest meaningful follow-up

`DocumentInspectorTabsTests` at f1089 checks visible/overflow partitioning, caps and inclusion of current; `ShellLayoutTests` checks bounded strip layout. `DocumentLayersActivationTests` exercises direct workspace selection/mode activation. `DocumentKeyRoutingTests` covers document-mode keyboard routing and explicit document close without a window. None proves overflow menu action delivery order versus Cmd-W, and a no-window dirty close test cannot prove real Cancel behavior (the deliberate no-window branch discards).

Small deterministic model regression: install two distinct clean stub documents A/B, select A, call select(B) then closeCommand in the same MainActor turn without yielding; assert B is closed, A remains and current becomes A. Add the inverse ordering control closeCommand while A current then select(B), proving the first close targets A. This protects synchronous command ownership and requires neither GPU nor arbitrary sleeps. It cannot validate native Menu/AX dispatch.

Small actual-GUI verification on the immutable B candidate: use disposable distinct clean A and dirty B, force B into overflow, activate B's menu action, and wait on the observable selected-tab identity/title (not merely the automation click call returning or a fixed sleep). Send Cmd-W, assert the sheet names B, Cancel, assert both documents remain. Record originals' hashes before/after. Repeat a rapid action sequence separately with event/action ordering evidence: temporary diagnostic timestamps at menu action entry, select assignment and Close target would distinguish delivery-before-selection from routing-after-selection. Without that evidence, retain the rapid case as inconclusive rather than label it lost-edit or async-selection bug.

No fix is recommended before that distinction is established. In particular, adding asynchronous selection, delaying all Close commands, or capturing a stale command-menu document would introduce new behavior without an identified source defect. B can own any later explicit pending-interaction UX design if actual menu delivery proves a user-visible problem.
