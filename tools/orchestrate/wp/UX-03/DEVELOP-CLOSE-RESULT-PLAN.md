# Core Develop Close Result Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make one `DevelopController.close()` attempt report its real save/close outcome, share that outcome with concurrent callers, and leave the controller retryable after failure.

**Architecture:** Keep close orchestration in `DevelopController`; drain the host’s coalesced settings and masks before calling the native session close, and surface either host or native errors as a `Result`. Store one in-flight close task, clear it only after failure so a later explicit call can retry, and mark the controller closed/remove callbacks only after native success. This is a Core-only contract: existing AppModel callers can continue compiling while discarding the result, so they are not made safe by this slice.

**Tech Stack:** Swift 6, Swift concurrency, Tessera FFI `DevelopSession`, XCTest; Rust FFI Stage B is a prerequisite.

**Spec:** [DEVELOP-WRITER-LEASE-PROPOSAL.md](DEVELOP-WRITER-LEASE-PROPOSAL.md), specifically Stage B. This plan narrows that proposal to the Core result-bearing close contract; AppModel recovery and barrier migration remain required follow-up work.

## Global Constraints

- Do not enable or implement the Stage C Develop writer lease in this slice.
- Do not call native `DevelopSession.close()` when host settings or mask draining fails.
- Do not clear pending settings/masks, the controller’s failure callback, or session listeners on a failed close.
- Concurrent callers of one in-flight close receive that attempt’s same `Result`; their presence must not schedule an automatic retry.
- Publish the shared attempt and enter a synchronous `closing` state before invoking host flush callbacks. While closing, reject every settings/history/mask mutation without changing local or pending state; report the rejection through the existing failure callback. Clear `closing` on failure so Retry or Keep Editing can proceed; keep it set after success.
- Invalidate deferred settings/mask drains when close begins. A deferred task must check the close state before sending anything and must not retry a failed close drain.
- Surface/presentation mutations also reject during close: attachSurfaces before lastView or allocation, updateDisplay/syncPresentation before local presentation or backend headroom changes, and mask overlay attach/set. Host identity relink also rejects during close. Read-only getters and cleanup remain available; this slice grants no presentation-only admission exception. Guard before changing local state, not only at the backend call.
- Cancellation of one caller waiting on close must not cancel the stored shared save/close attempt or another caller’s wait.
- A later explicit `close()` call after a failure may retry; successful close is idempotent and drains/joins native save work.
- Preserve the existing public `flushPending() -> Bool` attempt semantics used by display-link callers; close needs an internal result-bearing drain path.
- The Swift implementation depends on merged and gated native Stage B retry/drain behavior and on the reviewed mask-pending-retention change. Retaining a failed mask operation is not sufficient by itself: the mask flush path must also return its actual error to close. A Boolean “work was attempted” result cannot distinguish a successful mask send from an encoded/send failure. The native prerequisite is now merged in main01ac555e and gated with archive8ab43f64; preserve that input. Earlier native close stopped shared rendering after a failed flush, so an old archive is not sufficient for safe retry.
- Do not claim user-visible recovery or safe barriers until the separate AppModel and barrier-consumer follow-up is complete.
- Use deterministic failure injection and coordination; do not use sleeps, network, GUI activation, or a large-photo fixture for these tests.

## Review Focus

- A settings `setSettings` rejection must be returned, retained, and retried before native close; test with `testSettingsFlushFailurePreventsNativeCloseAndRemainsRetryable`.
- A mask mutation rejection must not be swallowed or dropped, and must block native close; test with `testMaskFlushFailurePreventsNativeCloseAndRemainsRetryable`.
- Two callers arriving during a gated failing native close must see the same error and only one backend attempt; test with `testConcurrentCloseCallersShareFailureAndSessionCanRetry`.
- A native close failure after host drain must leave the controller and listeners save-capable until an explicit later retry; cover this in `testConcurrentCloseCallersShareFailureAndSessionCanRetry` and assert listener/callback ownership before retry.
- A successful close must be idempotent, finish native drain once, and not make another native call; test with `testSuccessfulCloseDrainsAndIsIdempotent`.
- A synchronous `onPatchSent`/failure callback that schedules another close must join the already-published attempt; test with `testClosePublishesSharedAttemptBeforeHostFlushCallbackReentry`.
- Closing rejects new edits but restores edit admission after failure; test with `testCloseBlocksMutationsUntilFailureThenAllowsKeepEditing`.
- Canceling one waiter must not cancel shared close for other waiters; test with `testCancelingOneCloseWaiterDoesNotCancelSharedAttempt`.

---

## Files and Responsibilities

- Modify `apps/mac/Sources/TesseraCore/DevelopController.swift` to represent an in-flight close attempt, expose its result, perform a fallible host drain before native close, and preserve controller state after failure.
- Modify `apps/mac/Sources/TesseraCore/Develop/DevelopController+Masks.swift` only if the already-reviewed mask-retention implementation does not expose a failure-preserving drain usable by close. Keep that work limited to reporting failure and retaining the pending mask operation; do not refactor unrelated mask editing.
- Extend `apps/mac/Tests/TesseraCoreTests/AgentReviewOwnershipTests.swift` with deterministic close/retry and concurrency tests, reusing its injected session wrapper patterns.
- Extend `apps/mac/Tests/TesseraCoreTests/DevelopTests.swift` only if a native-session success/drain assertion belongs more naturally with its existing real-session persistence tests.
- Do not edit `apps/mac/Sources/Tessera/App/AppModel.swift`, `AgentController.swift`, `AgentReviewWorkspace.swift`, or Layers UI in this Core slice. Their failure barriers and recovery ownership are an explicit follow-up below.

## Proposed Core Contract

Use a discardable result so legacy `await controller.close()` call sites remain source-compatible while new callers can inspect failure:

```swift
@discardableResult
public func close() async -> Result<Void, Error>
```

Store the active operation as `Task<Result<Void, Error>, Never>?`. Publish it and set the synchronous `closing` admission state before invoking `onPatchSent`, `onFailure`, or other host callbacks during drain. A caller finding an active task awaits that task and returns its value. The attempt first obtains a result-bearing drain of host pending settings and masks. On host failure it reports through the existing failure callback once, returns `.failure(error)`, retains pending values and listeners, and does not invoke `session.close()`. If host drain succeeds, call native close off the MainActor. Native failure returns the same `.failure(error)` and leaves `closed == false`, callbacks/listeners installed, and the native session available for a later explicit call. Only native success marks `closed`, detaches listeners, clears callbacks, and returns `.success(())`. When a failed attempt has completed, clear the stored task so a subsequent explicit call can start a new attempt; do not retry merely because multiple waiters resumed. Cancel/invalidate deferred drains at close start and have them check `closing` before sending. Canceling a waiter cancels only that waiter’s wait, never the stored operation. While `closing`, reject mutations at every settings/history/mask entry point before changing local state; failure reopens admission for Keep Editing or Retry.

Keep `flushPending() -> Bool` behavior unchanged for display-link callers. Internally share the settings/mask send logic with a throwing or `Result`-returning close drain so close can distinguish “nothing pending” from “pending write rejected.” Do not infer drain success from its current Boolean “attempted work” value or from an `onFailure` side effect.

## Tasks

### Task 1: Add deterministic close failure tests

**Files:**

- Modify: `apps/mac/Tests/TesseraCoreTests/AgentReviewOwnershipTests.swift`
- Modify: `apps/mac/Tests/TesseraCoreTests/DevelopTests.swift` only if a real native-session drain case is needed.

**Test support:** Extend the test-only `BlockingCloseSession` wrapper with one-shot settings/mask/close failure injection and an explicit close-entry/release gate. Its failure injection must throw before delegating to the wrapped close, so a rejected close leaves the wrapped session open. Provide an internal read-only close-attempt token/waiter observation for `@testable` synchronization; tests must not depend on sleeps or GUI timing.

- [ ] **Step 1: Write `testSettingsFlushFailurePreventsNativeCloseAndRemainsRetryable`.** Queue exposure `1.25`, reject the next `setSettings`, and call `await controller.close()`. Before the result API exists, assert baseline-visible behavior: `session.closeCount == 0`, `controller.closed == false`, the pending exposure remains queued, and `onFailure` reports the injected error once. Those assertions must fail against the current implementation without a compile failure. After Task 2 introduces `Result`, extend the same test to assert the returned `.failure` carries that error. Remove the injected rejection and explicitly call close again; assert success, one native close, and reopened exposure `1.25`.
- [ ] **Step 2: Write `testMaskFlushFailurePreventsNativeCloseAndRemainsRetryable`.** Create a mask group, queue one mask parameter, reject its send once, and assert the existing failure callback receives the error, native close was not invoked, and the pending mask mutation remains available. This must compile and fail behaviorally before the Result API exists. After Task 2, assert the close `.failure` value and retry after clearing the fault; verify the saved mask parameter/group value after reopening.
- [ ] **Step 3: Adapt the existing concurrent-close RED scenario into `testConcurrentCloseCallersShareFailureAndSessionCanRetry`.** Queue exposure `1.25`; gate the first native close and inject one close failure. Start two callers and deterministically observe that both joined the same attempt before releasing it. Before Task 2, assert the failure callback was invoked once, `closeCount == 1`, and `controller.closed == false`; after Task 2, additionally assert both calls return the same `.failure`, callbacks/listeners remain installed, and no second backend call occurs automatically. Start a later explicit close; assert success, `closeCount == 2`, and reopened exposure `1.25`.
- [ ] **Step 4: Write `testSuccessfulCloseDrainsAndIsIdempotent`.** Queue settings, gate native close until the pending write has been sent, then release. Assert success, one native close, pending work empty, and a second close returns success without another backend invocation.
- [ ] **Step 5: Write `testClosePublishesSharedAttemptBeforeHostFlushCallbackReentry`.** In `onPatchSent`, schedule a second `close()` while the first close is draining settings. Release the host send and assert both callers receive the same outcome and only one native close occurs.
- [ ] **Step 6: Write `testCloseBlocksMutationsUntilFailureThenAllowsKeepEditing`.** Hold native close in flight; attempt a settings change, history action, and queued mask change. Assert each is rejected before local/pending state changes. Fail the close, then submit an edit again and assert it is accepted and can be saved by a later explicit close.
- [ ] **Step 7: Write `testCancelingOneCloseWaiterDoesNotCancelSharedAttempt`.** Start two waiters on one gated native close, cancel the first waiter, then release the backend. Assert the second receives success, the session closes once, and controller state reaches closed.
- [ ] **Step 8: Run only the new tests against the accepted native Stage B archive.** Reuse the pre-existing close-state RED checkpoint in `evidence/develop-close-controller-red-2026-09-27/` for the error/result behavior. A test that directly requires the new `Result` signature may first fail to compile against the old `Void` API; record that as a signature RED, not behavioral proof. Add baseline-compatible assertions first to preserve a behavioral RED for close state/native attempt count. Preserve raw log and direct exit before implementation.

### Task 2: Implement result-bearing, retryable Core close

**Files:**

- Modify: `apps/mac/Sources/TesseraCore/DevelopController.swift`
- Modify: `apps/mac/Sources/TesseraCore/Develop/DevelopController+Masks.swift` only if Task 1 established that mask draining cannot yet return its failure without losing queued data.
- Modify: every mutating entry point in the two controller files so an active close synchronously rejects settings, history, component, brush, stroke, and mask-group changes before touching live or pending state. Read-only queries and surface cleanup remain available while closing.

- [ ] **Step 1: Set closing admission and publish one in-flight result task before invoking synchronous host drain callbacks.** Concurrent `close()` callers must await the task already stored; do not create a second native close task for them. Keep all test-only observation internal, not in the public API.
- [ ] **Step 2: Refactor close’s pending drain to return the actual settings or mask error.** Preserve `flushPending() -> Bool` as the ordinary render-loop interface, but let close distinguish success from failure and stop before native close when host drain fails. In particular, update/consume the mask path’s result-bearing internal drain; its retention behavior alone does not report an error to close.
- [ ] **Step 3: Return `.failure(error)` without marking closed or detaching callback/listener ownership.** Retain the failed settings/mask operation, restore the non-closing state, report the failure once, and clear the completed attempt task only after its waiters can receive that attempt’s result.
- [ ] **Step 4: Call native close only after successful host drain and off the MainActor.** If native close returns an error, preserve controller and session ownership and allow a later explicit call to invoke close again. Do not schedule retries from waiters, callbacks, or a deferred settings task.
- [ ] **Step 5: On failure, clear closing admission only after the shared attempt result is fixed; on success only, mark closed and release listeners/callbacks.** Preserve idempotent success behavior; later calls return success without another native invocation.

### Task 3: Verify focused Core behavior and integration prerequisites

**Files:** no additional product files.

- [ ] **Step 1: Run the close tests from Task 1 in Release with the accepted current native Stage B archive and record exact source/archive hashes and direct exit.** Expected: all pass; the same-attempt test observes one close call for both concurrent callers and a later explicit call performs the only retry.
- [ ] **Step 2: Run the adjacent `AgentReviewOwnershipTests` and `DevelopTests` selections.** Confirm existing success-path close, coalesced mask persistence, and already-closing Review/Agent ordering remain unchanged.
- [ ] **Step 3: Confirm the exact native Stage B retry/drain archive and test evidence are the accepted versions before the Swift gate.** Do not rerun unchanged Rust suites; rerun only if the Rust source, build inputs, or native test behavior changes.
- [ ] **Step 4: Recheck the final source and test hashes, `git diff --check`, and commit only the Core implementation/tests.** Do not merge or activate Stage C as part of this plan.

## Required Follow-up Before Stage B Is Complete

A discardable result only preserves compilation; ignored results are still ignored. The existing AppModel paths clear `develop`, `developLibrary`, `onFailure`, and `onFrame` before the background close resolves, and existing barriers return `Task<Void, Never>`. Therefore this Core slice alone does not keep a user-visible recovery owner and must not be described as safe application recovery.

Before enabling a writer lease or declaring Stage B complete, separately migrate:

- `apps/mac/Sources/Tessera/App/AppModel.swift`: retain a strong recovery record keyed by the captured `EngineLibrary` object and image ID, keep callbacks attached until success, expose the failure, and provide the product-approved Retry / Keep Editing / Cancel Quit behavior. A folder switch must not attach the failed session to the new library. Explicit discard is blocked until rollback/discard semantics are separately designed and tested.
- `pendingDevelopSaveBarrier`, `prepareForAgent`, and `releaseDevelop`: propagate close success/failure rather than completing as `Void` after a failed close.
- `apps/mac/Sources/Tessera/Agent/AgentController.swift`, `AgentReviewWorkspace.swift`, Layers request handling in `AppModel.swift`, and Review preview loading: withhold recipe reads, Accept/Revert, Agent runs, and saved-pixel preview loads when the captured close fails; retry should resume only the still-valid request.
- Add AppModel-level tests for failure visible after navigation, retry writes the old owner’s dirty recipe then removes recovery state, Keep Editing reopens admission without losing the current controller, Cancel Quit leaves the window/application open, Agent/Layers/Review barriers stop on failure, caller cancellation does not cancel the shared close, and switching to a different folder does not orphan or rebind the failed controller.

The pre-existing successful barrier tests (`testAlreadyClosingDevelopSavePrecedesCapturedRun`, `testAcceptWaitsForAlreadyClosingDevelopSave`, `testRevertWaitsForAlreadyClosingDevelopSave`, and the Layer/Review preview tests) remain important controls. Their current `Task<Void, Never>` shape does not verify failure behavior.

Additional reviewed acceptance inventory: [DEVELOP-CLOSE-ADMISSION-TEST-MATRIX.md](DEVELOP-CLOSE-ADMISSION-TEST-MATRIX.md). Admission failure reporting must be reentrancy-safe when onFailure itself attempts another edit; reject the edit without unbounded callback recursion.

## Callback operation scope ruling — 2026-09-28

A close invoked by a task descended from a synchronous close callback joins that
callback's originating attempt for the descendant task's lifetime, including when
it starts after the attempt finishes. This prevents failure callbacks from creating
automatic retry loops. Repeated closes in that callback task keep the same result.
An explicit Retry action starts outside this callback attempt context, as a normal
independent UI action, and may start a new attempt. Preserve this distinction in
the API comments and test a delayed descendant across a newer explicit attempt.
The context must retain only its own outcome and weak controller identity, not an
unbounded global history or one overwriteable last-error slot.
