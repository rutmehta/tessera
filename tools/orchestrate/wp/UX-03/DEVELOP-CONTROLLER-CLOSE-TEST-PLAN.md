# Stage B controller test-first checkpoint (UNRUN)

Branch `codex/develop-close-contract`, based on current `origin/main` `8526cdb7`. Test-only checkpoint, intentionally unbuilt and unrun for root review. No Develop product code was changed.

## Existing injection seam

`apps/mac/Tests/TesseraCoreTests/AgentReviewOwnershipTests.swift` already has a `BlockingCloseSession: DevelopSession` wrapper around a real engine session. It forwards `info`, `historyState`, `getSettingsJson`, listeners, `setSettings`, and `close`, while using an `NSLock` and semaphore to deterministically gate close. This is sufficient for controller-level faults; no production protocol, fake engine, or broad backend abstraction is needed.

The checkpoint extends that wrapper with one-shot `setSettings` rejection and one-shot close failure, using a tiny localized test error. The retry test starts from a real `EngineLibrary` fixture and reopens the recipe through the engine to confirm persistence. The close-failure wrapper waits deterministically, then throws before calling `wrapped.close()`; it verifies Swift controller retention, shared concurrent failure, and later retry at the controller seam only. It does not simulate a native/Rust save-worker close failure or prove that a Rust session remains recoverable after such a failure. The semaphore overlaps concurrent close callers without timing sleeps.

## Two regression cases

1. `testRejectedDevelopPatchRemainsPendingForRetry`: hold an interactive exposure patch; inject the first `setSettings` rejection; assert the failure callback and first patch attempt; clear the fault; flush again; assert the exact same patch is attempted again; close and reopen, verifying the exposure was saved. On current source the pending map is cleared before `setSettings`, so retry does not resend and the reopened value remains at baseline.
2. `testConcurrentCloseCallersShareFailureAndSessionCanRetry`: make the first wrapped backend close wait, then fail; start a second controller close while the first is pending; release the deterministic gate; assert both callers receive the same failure, controller remains open, and only one backend attempt occurred. A later close retries, succeeds, and the reopened recipe has the manual exposure. Current source swallows the failure and marks the controller closed, so the outcome/recovery assertions fail.

## Smallest implementation shape for review

- Keep `pending` until `session.setSettings` succeeds. `flushPending` may preserve its current Bool surface initially; the regression observes callback, repeated identical patch, and persisted engine state rather than adding a new generic transport layer.
- Change `DevelopController.close()` to return a failure-bearing result (suggested simple contract: `async throws`). Store an in-flight result-bearing close task so concurrent waiters receive the same error. Clear it after completion; after failure leave `closed == false`, listeners/controller state intact, and a later close can retry. Only detach listeners and mark closed after successful backend close.
- Keep error conversion/localized message narrow to Develop close. Do not add AppModel navigation recovery, global error routing, Rust adapter protocols, or writer leases in this checkpoint. Stage A must independently ensure native close stays save-capable after a real Rust save failure; this wrapper test only validates Swift controller retention and error sharing.

The modified test source is committed separately as the UNRUN checkpoint. The retry assertion safely compares the first retry attempt when present; an intentional RED count of one must report an XCTest failure rather than trap by indexing past the array. No test or Swift build was run; compiler slot is not claimed.
