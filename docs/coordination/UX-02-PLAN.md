# UX-02 Navigable Review and Resume Index Implementation Plan

> For agentic workers: use superpowers:executing-plans to implement task-by-task; root assigns any parallel work and the heavy build slot. This is a read-only planning deliverable, not authorization to change B-owned code.

**Goal:** Make the current agent review queue a stable destination, with safe one-photo actions, reliable return navigation, and a resumable latest-queue index grounded in existing recipe persistence.

**Architecture:** Keep AgentReviewQueue's ordering/status rules and existing engine actions. Introduce a library-owned review session that stores stable image IDs, navigation state and an atomic local resume index; reconcile recipe-backed fields through agentProvenance. A Review destination displays that session without changing the Library's filter or selection. Retain Document mode precedence and the existing Develop controller for actual photo edits.

**Tech stack:** Swift/SwiftUI/AppKit, TesseraCore value models and Foundation file storage; existing TesseraFFI agent methods. No FFI generation or Rust product change required for this bounded slice.

**Spec:** current worktree `docs/design/workspace-redesign/first-slice-plan.md`; recovered `/tmp/tessera-redesign-ux-scope.md` follow-on contract 1; current source below. Provisional Precision Graphite continues through existing Theme/Components.

## Global constraints

- A owns Shell/shared/Library/Develop/Agent UI. Do not edit Document/**, TesseraCore/Document/**, document FFI or generated bindings.
- Existing M2-56 containment: 960×600, 1280×800, 1440×900, 1728×1117, light/dark. No foreground activation for offscreen harness tests.
- No new batch mutation, accept-all shortcut, automatic redo, fabricated before/after baseline, or claim that every review action is undoable.
- No persistent integer item positions. Every asynchronous action captures owner library, image ID, expected group, queue generation and engine reference before suspension; remap current item positions only when needed.
- Retain failed-first / confidence-ascending order on construction and merge; setStatus never reorders rows. A redo may reorder according to the existing merge contract.
- This is a latest-queue resume index, not a durable multi-run journal. Historical runs, cross-device sync and atomic revision-conflict resolution require a later engine contract.

## What already persists (and what does not)

| Fact | Source | Guarantee / limit |
|---|---|---|
| Per-photo latest agent report/group | `crates/agent/src/lib.rs:332–337`; `crates/tessera-ffi/src/agent_runs.rs:361–420` | `tessera_agent_v1` in recipe sidecar records latest group/report. Later runs replace this extension; history groups remain. |
| Per-photo user review status | `agent_runs.rs:791–841` | Accept writes review_status=accepted; Revert writes reverted. `update_agent_extension` at465–480 writes recipe and resyncs catalog. `crates/sidecar/src/lib.rs:160–164` uses atomic_write. |
| Reconstruct latest review row | `agent_runs.rs:768–789`; `AgentPanels.swift:193–196` | Existing agentProvenance(imageId) returns latest group, status, confidence and rationale/step information for a known image ID. It is not an enumeration of historical run results. |
| Revert reversibility | `agent_runs.rs:827–840`; `crates/tessera-ffi/tests/assist.rs:930–941` | Writes group amount0 as one history step, preserving later manual/redo changes. Undo belongs to photo recipe history. |
| Accept semantics | `agent_runs.rs:791–823` | Review metadata is saved; final settings can train the library profile. Feedback failure is separately reported. No inspected unlearn/revoke API; do not promise Cmd-Z reverses learned feedback. The `AgentReviewEntry.accepted` bool is the critic's acceptance, not user status. |
| Queue membership/provider/failures/position | `AgentController.swift:31–32,201–220`; `TesseraCore/Assist/AgentReview.swift:77–139` | App memory only. No queue load/save found. Same-provider results merge; different-provider fresh run replaces queue; redo replaces matching image entries. Failure strings are run report data, not reconstructible from the latest successful recipe provenance. |
| Current navigation | `AgentReviewSheet.swift:19–43`; `AppModel.swift:1690–1700` | Sheet Show dismisses and calls showInLoupe, which can clear person filtering/change source to All. This cannot serve as lossless Review→photo→Review navigation. |

The current AgentController also resolves accept/revert from `app.engineLibrary` at action time and calls current-app refresh after run completion. A retained queue can outlive a folder switch. UX-02 must attach the owning library/context to the queue and reject or explicitly reopen a foreign-library target; never train the current folder's profile from an old queue merely because that folder is active.

## Minimal user-visible slice

1. Add **Review (N)** as a named Library destination and keep the existing Develop→Agent Review menu as an alias. It remains discoverable with an empty/loading/error state and after all rows are reviewed. Auto Edit completion updates the destination/badge and offers Open Review; it does not steal an active text edit or replace the user's current workspace unexpectedly.
2. Review header names source library and scope: **Reviewing 1 photo · filename**, with queue summary and provider/run-origin text only where known. A virtualized list retains current failure/confidence order, selection by image ID, and stable scroll position. Status changes stay in place; after redo merge, retain the selected ID even if its row moves.
3. A fit preview uses the existing preview loader and is labeled **Current preview**. It is not an original/agent-before baseline. Show rationale, steps, critic result and user review status separately. Missing/unavailable rows remain visible with explicit reason; don't substitute another photo under the old name. Redo instruction draft is scoped to the selected image/queue generation.
4. Visible actions: **Accept**, **Accept and next**, **Redo…**, **Revert agent group**, **Edit photo**, **Back to Library**. Accept-and-next waits for success, then uses the existing `queue.next(after:)`; on failure retain selection/status. Revert does not silently accept or advance. Busy gating is enforced in controller as well as buttons.
5. Enter Review captures the UX-01 Library return bookmark once. Review selection never mutates Library filters or multi-selection. Edit photo from Review records a Review return frame (selected image ID, list anchor, filter if any, instruction draft), then opens that exact target through the existing Develop path; Back returns to Review. Leaving Review restores original Library place after final layout. A deliberate new folder clears visible navigation state and opens that folder's own resume index.
6. This slice need not introduce Review subfilters, pinned reference, before/after rendering or multi-run history. Those can follow once the navigation and persistence contract passes.

## Durability contract for the new resume index

Proposed new app-owned JSON under existing application support, namespaced by canonical library identity (not display title). Version the DTO; store no engine object, credentials or absolute raw pixel data. Store queue's stable image IDs/order, owner identity/source label, provider if known, latest group snapshots, reported status/confidence/rationale/failure rows, selected image ID and a write generation. Snapshot targets before starting a run and atomically persist its in-progress marker before dispatch; do not silently promise resumability if this write fails.

After engine result, atomically replace index with actual queue result. On successful Accept/Revert, update index, while treating recipe sidecar as authoritative for recipe-backed status. App quit/relaunch restores the saved list quickly, marks it loading, and reconciles known IDs with bounded background `agentProvenance` calls. Maintain captured order during reconciliation; do not sort on every row response. Failures from the saved report remain failures until an explicit redo result replaces them; a stale earlier successful provenance must not erase a later failed attempt. Write/index failure produces a clear nonblocking “Review list could not be saved” state; successful recipe edit is not misreported as failed or rolled back.

Interrupted run: keep its saved target IDs and state as interrupted/unknown, not completed; reconcile available latest provenance. Without a durable engine run ID and expected recipe revision API, do not assert that a recovered latest group definitely belongs to the interrupted invocation. Mark an unexpected group or changed provenance as **Changed since this review** and refresh/require re-review before action. Old group-specific Revert must not silently target a newer group.

External writers can still race a read-then-action guard because existing Accept has no expected-group/revision parameter. This slice can serialize this app's operations and reject known-stale targets, but must not claim atomic cross-process conflict handling. Full authoritative run history and compare-and-swap acceptance/revert remain a separate engine work package.

## Keyboard and undo contract

- Route Review after text/numeric/control/sheet and Document guards, before Library cull/Develop tool routing. Keep existing Document shortcuts untouched.
- Arrow keys move the selected review row when list/preview owns focus. Textfield arrows/letters/Return stay in the Redo instruction. Escape cancels the draft first, then returns one navigation frame. No bare X/P/Y/A/Delete culling or accept-all operation may reach Library while Review owns the target.
- Use explicit buttons/menu labels for Accept and Revert in this first slice; no new destructive letter shortcut or Return-to-accept default. Cmd-Return may be added only as a documented focused Redo submit, never a global acceptance shortcut.
- Cmd-Z in Review never falls through to cull history. Either route to a verified matching loaded photo session with title **Undo Photo Edit**, refreshing row provenance afterward, or disable/no-op with an **Edit photo to undo recipe changes** affordance. Existing photo history can reverse Revert/Redo; Accept's profile learning is not advertised as undoable.

## Implementation tasks and acceptance

### 1. Review ownership, target snapshots and resume store

Files: create `apps/mac/Sources/TesseraCore/Assist/AgentReviewResumeStore.swift`; extend `.../Assist/AgentReview.swift` with explicit DTO/context values without changing queue sort rules; integrate narrow state/action methods in `apps/mac/Sources/Tessera/Agent/AgentController.swift`. Tests: new `apps/mac/Tests/TesseraCoreTests/AgentReviewResumeTests.swift`, extend existing `AssistTests.swift`.

- [ ] Write failing tests for round-trip ordering/status/failures, canonical library isolation, partial/corrupt/version-mismatched index, atomic replacement failure, and stable image IDs after item insertion/removal.
- [ ] Add injected store and provenance loader seams; implement atomic versioned writes and bounded asynchronous reconciliation. Never call per-photo FFI inside SwiftUI body or on every render.
- [ ] Capture owner engine/folder, image ID, expected group and queue generation for Accept/Revert/Redo. Re-resolve position only within that owner. Guard busy/duplicate calls in controller; ignore stale completion for a newer queue while persisting results to their original owner.
- [ ] Test folder switch during accept/run, same image redo while old response arrives, missing group, unavailable file and persisted failure with older provenance. Test that accept feedback failure is distinguishable from failure to save review metadata.
- [ ] Test interrupted target snapshot restores as uncertain, not success. Read-only validation reports unexpected group without auto-applying old action.

### 2. Review destination and return navigation

Files: new `apps/mac/Sources/Tessera/Agent/AgentReviewWorkspace.swift`; extract reusable row/detail presentation from `AgentReviewSheet.swift`; focused changes to `App/AppModel.swift`, `Shell/ContentView.swift`, `Shell/WorkspaceHeader.swift`, `App/AppCommands.swift`. Extend `TesseraCore/Workspace/PhotoWorkspaceState.swift` with Review return values rather than a second mutable Document mode. Tests: `WorkspaceNavigationTests.swift` and new `AgentReviewNavigationTests.swift`.

- [ ] Write failing tests for filtered Library multi-selection→Review→Edit B→Review→Library restoring exact selection/source/view and stable anchors; delete/insert/resize while away; empty/all-reviewed queues remain reachable.
- [ ] Implement explicit non-document context including Review, preserving `viewMode == .document` precedence and existing renderer adapters. Retain queue and navigation state outside transient SwiftUI row/sheet instances.
- [ ] Implement list and Current preview, named one-photo scope, loading/missing/error states, existing ordering/status behavior and action progress. Keep Library selections unchanged while browsing review entries; avoid legacy showInLoupe's source-changing path.
- [ ] Test Accept-and-next succeeds once, does not move on error, skips reviewed/errors according to current queue.next contract, and keeps row positions stable. Redo retains selected stable ID after existing merge reorder.
- [ ] Update menu/toolbar aliases and completion notification without automatic focus theft.

### 3. Keyboard, reversibility and final validation

Files: focused `App/KeyRouter.swift`, `App/AppModel.swift` undo branch; `KeyFocusTests.swift`, `DocumentKeyRoutingTests.swift`, ShellLayoutHarness; `apps/mac/DESIGN.md`, `apps/mac/ACCEPTANCE.md`; `tools/orchestrate/wp/UX-02/` evidence.

- [ ] Add failing tests for text/numeric/control priority, Review cull-key suppression, draft-first Escape, document priority, undo never consuming cull history, stale target rejection and returned edit session identity.
- [ ] Implement context-safe commands and truthful status/error copy. Preserve engine action/history semantics; rerun existing queue ordering/merge tests.
- [ ] Add background-safe Review populated/empty/error/busy/long-name harness states at the four accepted sizes and both appearances. Verify list controls and back/target remain reachable.
- [ ] Run focused Swift release tests under root's granted slot; then one final full Swift release gate on frozen source if required. No Rust rebuild for app-only storage/navigation code. If engine contracts change later, obtain a new scoped engine gate.

Suggested focused test filter: `AgentReview|Workspace|KeyFocusTests|DocumentKeyRoutingTests|ThemeLintTests|ShellLayoutTests` using the current UI worktree's external scratch/build path and existing harness flags. Root owns paths and slot allocation; do not invent or reuse an occupied target.

## Definition of done and explicit follow-ons

UX-02 is complete only when a saved latest queue resumes after app restart with authoritative per-photo reconciliation, preserves failed rows/known uncertainty, stays navigable without modal context loss, and every action targets its captured owner/photo/group. If only destination/navigation is delivered, label it UX-02a and state that queue relaunch persistence is not shipped.

Deferred: durable multi-run engine journal, all-run discovery/enumeration, original/agent-before render snapshots, compare-and-swap external conflict resolution, acceptance reversal/profile unlearning, cross-device queue sync, accepted-only export and batch acceptance. Those are not implied by existing sidecar durability.
