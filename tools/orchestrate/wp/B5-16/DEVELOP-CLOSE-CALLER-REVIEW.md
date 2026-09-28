# Develop close recovery caller review — 651c9cf1

Reviewed origin/main435211b2 and native retry candidate85de2860, 2026-09-28 UTC.
Validated exact B target/expiry and accepted before review. Source-only: no product
edits, workloads, desktop interaction or heartbeat restart. Lines below refer to
main435211b2 unless explicitly marked otherwise. Contract is a proposal, not a
claim that recovery already exists. A owns native retry review/compilation/main.

## Findings and caller boundaries

**P1: failed native close still destroys the retry target.**
crates/tessera-ffi/src/develop.rs:2424-2427 flushes, then unconditionally calls
shared.close(), returning the flush result afterward. Candidate85de2860:2629-2632
still does this. Stage A retry tests therefore do not establish failed-CLOSE
recoverability. Swift result propagation alone is insufficient. Native close
must return failure while preserving worker/session/save state; successful retry
must drain and close once. Do not reactivate a closed controller as a workaround.

**P1: recovery must retain all pending edit queues, including masks.**
DevelopController.swift:379-393 clears the ordinary patch before setSettings and
returns true after failure; onPatchSent at385 is also premature as an acceptance
signal. apply at319-325 optimistically updates visible settings. Keep that visible
pending state, but do not describe it as persisted. Swift retention work is
already owned by A; this review does not duplicate it.
Develop/DevelopController+Masks.swift:199-211 additionally clears pendingComponent,
pendingMaskGroup and pendingMaskParams before acceptance. A failure midway loses
unattempted entries as well as the rejected entry. Drain acknowledged entries
individually; retain the rejected/unattempted suffix in stable order. Brush points
at195-197 are different: take() mutates a local stroke copy and commits it only
after addBrushPoints succeeds. Preserve that behavior; retry must not duplicate
already-appended strokes. A bool meaning 'something was sent' cannot be a close
success barrier. Mask failure must prevent downstream close even with no ordinary
settings patch pending. JSON encode failure at383 also cannot become no-op success.

**P1: controller/owner lifetime is lost before the close result.**
DevelopController.close at155-175 marks closed, detaches listeners and ignores
native errors. AppModel.closeDevelop at1752-1782 first clears active controller,
owner and failure callback, then records Task<Void,Never>. The task closure holds
the controller only until completion; its dictionary entry is removed regardless
of save outcome. Keep strong controller + exact EngineLibrary identity + stable
imageID in recovery independent of the displayed item index. ownerless branch
at1765-1768 also needs an application-owned recovery entry, not a fire-and-forget
Task. Do not cancel the shared save attempt when a caller abandons navigation.

**P1: asynchronous open cleanup is a separate close owner.**
AppModel.openDevelop at1683-1701 captures pending open/close tasks. A cancelled or
stale open can still produce a controller and calls await controller.close at1700.
Its defer at1687-1690 then removes the pending-open record. A future throwing
close must transfer failed cleanup into recovery BEFORE that removal, even if
Task.isCancelled, selection changed or library owner is offscreen. The catch at
1705 currently ignores stale/cancelled errors; it must not swallow save recovery.
The new selected-image open must not bypass an earlier failed owner/image save.

**P1: transition gates must precede side effects, not just install.**
AppModel.viewMode didSet203-218 and notifySelection1163-1167 run after mutation;
openDevelop1673 closes after a new focus request. returnToLibrary960-963 and
returnFromPhotoEdit1083 onward also begin leaving the workspace before a result.
openFolder380-409 leaves Photo Edit, sets load flags, remembers the requested
folder at396 and begins scanning before install444-468 replaces the library.
Place the save gate before leave callbacks, recent-folder updates, scan and
selection/mode changes. Cancel must retain the old workspace and not silently
change recent-folder preference. Same-folder rescan and item-removal remaps need
retained original owner/image recovery, even if its itemID no longer exists.

**P1: Layers/Review observe completion, not save success.**
pendingDevelopSaveBarrier1022-1029 returns Task<Void,Never> and snapshots opening
and closing tasks. Layers901-925 awaits it then calls documents.editInLayers.
Its existing owner/request/selection/source guards are valuable but do not prove
persistence. Review AgentReviewWorkspace.swift:138-149 awaits the barrier, then
invalidates/requests pixels unconditionally. Failed barriers must stop those
reads and present recovery rather than a spinner or an apparently current image.
Retain the existing observation-token guards after success. Do not let a close
created by an in-flight open fall between the captured two tasks and the consumer;
use one authoritative owner/image state entry, including open-cleanup recovery.
DocumentWorkspace.editInLayers remains a rendered copy and needs no semantic
change. Native Document close/discard at DocumentWorkspace334-343 is a different
lifecycle; don't substitute its void close for result-bearing Develop close.

**P1: Agent failure paths must unwind bookkeeping and durable intent.**
AgentController start writes its running resume record at413, establishes progress
and running source ownership at414-426, then awaits the void barrier427-433.
Accept and revert allocate busy/mutation owners at569-574 and606-612 before await.
On barrier failure invoke shared cleanup for progress/cancelFlag/runID/running
owners and accept/revert busy ownership, call accept completion(false) exactly
once, and update any already-written running resume record to accurately indicate
no mutation launched. Do not record a successful or actively running engine job.
Retry after recovery may resume only the current captured intent, not a stale
Review target. releaseDevelop2050-2057 must be result-bearing or removed if unused;
its try? flush currently bypasses failure propagation.

**P1: last-window close is also a quit path.**
TesseraApp.swift:296 has only applicationWillTerminate cleanup, and300 returns
true for applicationShouldTerminateAfterLastWindowClosed. DocumentWorkspace347-348
routes non-document Cmd-W directly to performClose. Gate termination with
applicationShouldTerminate and an async terminateLater/reply flow; on failure
cancel termination and make retained recovery visible even if the last window
was closed (restore/show a recovery window). Test Cmd-Q, menu quit and last-window
close; don't depend solely on toolbar navigation or willTerminate. Preserve
existing layered-document unsaved-close prompts as a separate participant.

Adjacent consumer for A triage: ExportController.swift:273-289 dispatches
exportBatch without a result-bearing Develop save gate at that method. This
review does not establish every upstream/export-engine guarantee; explicitly
classify whether export reads saved recipe or a flushed snapshot, then cover
pending/rejected Develop edits before claiming recovery protects every reader.
No export implementation expansion is authorized by this note.

## Smallest implementation split

1. A native + TesseraCore: failed-close retryable lifecycle, typed close/save
   result and publication-state report; coalesced settings AND mask retention;
   shared in-flight close outcome; keep editable after failure. No discard API.
   Freeze editing while close is truly in progress or reject writes explicitly;
   Keep Editing during a flight waits/invalidates intent rather than racing close.
2. A AppModel coordinator: single observable owner/image entry covering opening,
   ready, closing, failed(recovery) and closed, strong owner/controller retained.
   Separate operation token from navigation intent token. A failed task result
   remains until explicit retry; new callers join that outcome rather than
   accidentally treating a removed dictionary entry as success. Successful retry
   clears only matching identity, then executes at most the current intent.
3. A shared UI/consumers: navigation/folder/Review/Agent/quit gates and persistent
   photo-specific Retry Save / Keep Editing (Cancel Quit) UI. Initial release
   offers no Discard: recipe may already be published while XMP/index repair is
   pending. B can review the Document handoff adapter and source tests when
   explicitly assigned; B does not independently alter A's shared AppModel.

Result contract: success means pending host edits accepted AND native durability
repair completed before close; failure retains controller and publication state.
No rollback, crash recovery, transactional recipe/XMP/index or leases implied.

## Minimum deterministic acceptance additions (proposed / UNRUN)

- Native close fails after recipe publication: same session still accepts retry,
  repairs XMP/index without new edits, closes once; concurrent closes share result.
- Mask component/group/param rejection retains unaccepted suffix; accepted prefix
  and brush points never replay. Encoding failure blocks close. Retry saves exact
  final visible settings, not stale pre-rejection values.
- Cancel an open after backend creation, inject cleanup-close failure and switch
  library: recovery retains original owner/image after open defer; later opening
  the same image cannot bypass it. No stale callback into replacement workspace.
- Two navigation intents during one failing close: neither applies prematurely;
  Keep Editing cancels continuation, retry resumes at most latest valid intent.
  Folder failure performs no scan/install/recent-folder mutation.
- Layer handoff and Review preview counters stay zero on failed/open-cleanup
  barrier; after recovery exactly one valid read/open occurs. Existing copy
  disclosure/pixels remain snapshot behavior.
- Agent start/accept/revert on barrier failure launch zero engine mutations, clear
  ownership/busy state, correctly finalize durable intent and callback once.
- Cmd-Q and closing last window fail safely: process/owner retained, recovery
  visible; cancel keeps editing; retry permits quit only after all entries pass.
- Add export pending-edit test only after A resolves its actual source contract.

Review published only. No tests/builds/apps/benchmarks were run, no auth or desktop
writer changes, B hold/paused heartbeat unchanged. Stage A evidence is A-reported
and source-reviewed here, not independently executed on B.
