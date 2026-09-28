# Develop AppModel recovery API draft (source-only)

Reviewed at `codex/develop-app-recovery` (base `origin/main` 435307e3; product source still untouched). This is a proposed minimum coherent Swift contract for the accepted `APPMODEL-RECOVERY-BARRIER-PLAN.md` slices 2/3. It is not an implementation or validation claim. Luna owns the two initial behavioral RED tests; product edits start after that baseline is observed.

## Independent MainActor coordinator

New `App/DevelopRecoveryCoordinator.swift`; keep AppModel's private fields private, and pass captured values through explicit methods rather than widening access for an extension.

```swift
@MainActor @Observable
final class DevelopRecoveryCoordinator {
    struct Key: Hashable { let ownerID: ObjectIdentifier; let imageID: String }
    struct SessionID: Hashable { let rawValue: UUID }
    struct AttemptID: Hashable { let rawValue: UUID }
    enum Outcome { case saved; case failed(sessionID: SessionID, message: String) }
    enum Phase { case active; case saving(AttemptID); case failed(String); case closed }
    struct Presentation: Identifiable {
        let id: SessionID
        let imageID: String
        let displayName: String
        let phase: Phase
        let canKeepEditing: Bool
    }
    private(set) var presentations: [Presentation] = [] // tracked by Observation; records remain private
    // Private record retains strong EngineLibrary, DevelopController, immutable
    // source/display fallback, session ID, original workspace bookmark,
    // current attempt ID/task, and latest failure.

    func registerActive(owner: EngineLibrary, controller: DevelopController,
                        bookmark: WorkspaceBookmark) -> SessionID
    func retainUnowned(controller: DevelopController,
                       bookmark: WorkspaceBookmark) -> SessionID
    func requestSaveClose(_ sessionID: SessionID) -> Task<Outcome, Never>
    func retrySave(_ sessionID: SessionID) -> Task<Outcome, Never>
    func observe(owner: EngineLibrary, imageIDs: Set<String>) -> SaveBarrier
    func initiate(owner: EngineLibrary, imageIDs: Set<String>) -> SaveBarrier
    func cancelIntent(_ intentID: UUID) // never cancels Core close
}

struct SaveBarrier {
    let owner: EngineLibrary
    let imageIDs: Set<String>
    let operationID: UUID
    func result() async -> SaveBarrierOutcome
}
enum SaveBarrierOutcome {
    case saved
    case blocked([DevelopRecoveryCoordinator.SessionID])
}
```

`Outcome` is immutable per attempt; all joiners receive the same result. A retry starts a new attempt only from an explicit UI action outside `DevelopController`'s callback TaskLocal context. The coordinator maps `.failure(Error)` from Core to a retained record; it does not infer durability stage from text. A failed or still-active record makes an observer barrier blocked. The registry publishes a record before AppModel notifies live observers. An ownerless session is an invariant failure retained by SessionID (with optional owner key), blocks destructive transitions and all gates explicitly, and is never assigned a fake current owner or closed fire-and-forget.

An open ticket is a separate private type: `(owner: EngineLibrary, key: Key, token: UUID, task: Task<OpenSettlement,Never>)`. `openDevelop` captures nonoptional owner before suspension; cancelled/stale completion transfers any created controller into a recovery record before settling the ticket. Barriers drain every captured open/close, then recheck the same-key registry/open generation before returning `.saved`. If a new same-key open can race that recheck, admission reserves the key synchronously until its consumer commits or cancels. No barrier cancels shared durability work.

## AppModel adapters and navigation order

Replace `closeDevelop()`, `pendingDevelopSaveBarrier`, `prepareForAgent`, and `releaseDevelop`'s ignored flush with result-bearing adapters. Avoid a compatibility `Task<Void,Never>` shim. Suggested signatures:

```swift
@discardableResult func requestDevelopSaveClose() -> Task<SaveBarrierOutcome, Never>
func observeDevelopSave(imageID: String, library: EngineLibrary) -> SaveBarrier
func prepareForAgent(imageIDs: Set<String>, library: EngineLibrary) -> SaveBarrier
func retryDevelopRecovery(_ sessionID: SessionID)
func keepEditingDevelopRecovery(_ sessionID: SessionID)
```

Navigation uses synchronous intent capture followed by async save and a token-checked commit. A new intent supersedes the old caller's dispatch, not the save attempt. Prefer explicit adapters (e.g. `requestReturnToLibrary`, `requestEnterReview`, `requestPhotoSelection`, `requestOpenFolder`) that capture owner, stable image identity, bookmark and UUID **before** suspension; `commit…` performs the existing state changes only after `.saved` and checks intent, owner, target and action generation. `viewMode`/selection setters and command/router bindings must route through these adapters; `didSet` cannot tear down the old editor before the gate. If some direct binding must temporarily be intercepted, a guarded reentry path must prove no observer or teardown runs before save. `install(_:)` becomes commit-only. `createRequestedLayeredCopy` delays `returnToLibrary` and Document handoff until saved; B owns the DocumentWorkspace adapter after this API freezes.

## Consumer fail-closed migration boundary

- `AgentController` start/accept/revert/resume use initiating barriers, check `.saved` before engine mutation, and settle matching run/busy/resume state on blocked outcome. Review preview uses an observing barrier and checks result before invalidation/request.
- Output presentation and actual Export/Print dispatch check a result and keep their original captured owner/targets. This slice does **not** satisfy output read reservations or quit veto; those still require their planned tests and later implementation. Until then, uncovered output/termination paths must fail closed and must not ignore a result.
- `openFolder` owns a request UUID and callback settlement separately from `loadGeneration`; scan/install and completion callback occur only on a successful current commit. A newer request settles superseded callback false once. A blocked save retains the request for explicit Retry/Keep Editing settlement.
- No call site may discard `Result`/`SaveBarrierOutcome`, use `try?` to bypass the gate, or treat a cancelled/superseded consumer as proof of successful durability.

Initial acceptance remains narrow: Luna's admission REDs; then coordinator shared-attempt/recovery/open-ticket tests and caller-specific fail-closed tests before any merged consumer path. Output worker reservation, Document veto, termination restoration, and GUI recovery remain unaccepted until their dedicated checks.

## Direct-write audit to drive migration

Read-only inventory is [DEVELOP-APPMODEL-RECOVERY-CALLSITES.txt](DEVELOP-APPMODEL-RECOVERY-CALLSITES.txt). Direct `viewMode` assignments outside AppModel occur in `Grid/ThumbnailBrowser.swift`, `Tether/TetherController.swift`, `Document/DocumentWorkspace.swift` (B-owned adapter), `App/AppCommands.swift`, `Shell/ContentView.swift`, `App/TimingSelfTest.swift`, and `App/KeyRouter.swift`. A fail-closed migration must cover the A-owned command/router/grid/tether/shell paths rather than only `returnToLibrary` and `enterReview`. AppModel itself writes `viewMode` in install/remap/compare/photo-edit/review functions; those become private commits or are guarded by an already-saved transition token. `DocumentWorkspace.swift` remains B-owned and needs an explicit adapter contract, not an unreviewed A edit.
