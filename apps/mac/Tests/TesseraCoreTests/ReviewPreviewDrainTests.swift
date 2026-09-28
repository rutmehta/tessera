import AppKit
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Review observation must retry after a previous view's non-interruptible preview read drains.
@MainActor
final class ReviewPreviewDrainTests: XCTestCase {
    private final class HeldPreviewRender: @unchecked Sendable {
        private let entered = DispatchSemaphore(value: 0)
        private let release = DispatchSemaphore(value: 0)
        private let lock = NSLock()
        private var renderCount = 0
        var renders: Int { lock.withLock { renderCount } }

        func holdFirstRender() {
            let shouldHold = lock.withLock { () -> Bool in
                renderCount += 1
                return renderCount == 1
            }
            guard shouldHold else { return }
            entered.signal()
            _ = release.wait(timeout: .now() + 10)
        }

        func waitUntilEntered() async -> Bool {
            await withCheckedContinuation { continuation in
                DispatchQueue.global(qos: .utility).async {
                    continuation.resume(returning: self.entered.wait(timeout: .now() + 5) == .success)
                }
            }
        }

        func letRenderFinish() { release.signal() }
    }

    func testReviewRetriesObservationAfterEarlierPreviewFlightDrains() async throws {
        try XCTSkipIf(ProcessInfo.processInfo.environment["CI"] != nil,
                      "Uses the local background window server")
        ShellHarness.prepare()
        let scratch = FileManager.default.temporaryDirectory
            .appendingPathComponent("review-preview-drain-\(UUID())")
        let folder = scratch.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        try ShellHarness.writeJPEG(folder.appendingPathComponent("held-preview.jpg"), shade: 70)

        let support = scratch.appendingPathComponent("support")
        let agent = AgentController(arguments: ["--fake-planner"], supportDirectory: support)
        let hold = HeldPreviewRender()
        defer { hold.letRenderFinish() }
        let loader = ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1, concurrency: 1,
                                     beforeRender: { hold.holdFirstRender() })
        let model = AppModel(agent: agent, thumbnailLoader: loader)
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        model.install(library)
        model.agent.preferences.sceneConsistency = false
        model.agent.preferences.personConsistency = false
        model.agent.start(itemIDs: model.library.items.map(\.id), provider: .scripted)
        let runDeadline = Date().addingTimeInterval(20)
        while model.agent.isRunning, Date() < runDeadline {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertFalse(model.agent.isRunning, "The one-photo scripted Review run must finish")
        await withCheckedContinuation { continuation in model.syncLibrary { continuation.resume() } }
        model.enterReview()
        let item = try XCTUnwrap(model.reviewTargetItem)

        let (firstWindow, _) = ShellHarness.window(model, size: CGSize(width: 960, height: 600), dark: false)
        defer { firstWindow.orderOut(nil); firstWindow.contentViewController = nil }
        let entered = await hold.waitUntilEntered()
        XCTAssertTrue(entered, "The first Review view must reach its held native preview read")
        guard entered else { return }
        guard await waitUntil({ model.developRecovery.activeGateCount == 1 }) else {
            XCTFail("The first preview read must own one recipe observation gate")
            return
        }
        XCTAssertNil(model.loader.cached(item, tier: .preview))
        XCTAssertEqual(hold.renders, 1)

        // Detach the first view while its native worker remains held. Its gate must remain
        // reserved until that worker drains, so a fresh same-identity Review view queues behind it.
        firstWindow.orderOut(nil)
        firstWindow.contentViewController = nil
        let (secondWindow, _) = ShellHarness.window(model, size: CGSize(width: 960, height: 600), dark: false)
        defer { secondWindow.orderOut(nil); secondWindow.contentViewController = nil }
        guard await waitUntil({ model.developRecovery.activeGateCount == 2 }) else {
            XCTFail("The second Review view must wait behind the first flight's observation gate")
            return
        }
        // Wait for evaluation to either suspend on the preceding drain or terminate its
        // reservation early. Both outcomes are observable before the held worker is released.
        guard await waitUntil({ model.developRecovery.waitingForPrecedingDrainCount == 1
            || model.developRecovery.activeGateCount == 1 }) else {
            XCTFail("The second Review observation did not reach a settled gate state")
            return
        }
        let secondObservationWaited = model.developRecovery.waitingForPrecedingDrainCount == 1
        XCTAssertNil(model.loader.cached(item, tier: .preview),
                     "No preview can be delivered while the first native read is held")

        // A canceled waiter must release only its own gate and must not cancel the first read.
        if secondObservationWaited {
            secondWindow.orderOut(nil)
            secondWindow.contentViewController = nil
            guard await waitUntil({ model.developRecovery.activeGateCount == 1 }) else {
                XCTFail("Disappearing Review must cancel its queued observation without releasing the active flight")
                return
            }
        }
        XCTAssertNil(model.loader.cached(item, tier: .preview))

        let (thirdWindow, _) = ShellHarness.window(model, size: CGSize(width: 960, height: 600), dark: false)
        defer { thirdWindow.orderOut(nil); thirdWindow.contentViewController = nil }
        guard await waitUntil({ model.developRecovery.activeGateCount == 2 }) else {
            XCTFail("A replacement Review view must queue behind the still-running native flight")
            return
        }
        guard await waitUntil({ model.developRecovery.waitingForPrecedingDrainCount == 1
            || model.developRecovery.activeGateCount == 1 }) else {
            XCTFail("The replacement Review observation did not reach a settled gate state")
            return
        }

        hold.letRenderFinish()
        guard await waitUntil({ hold.renders >= 2 && model.loader.cached(item, tier: .preview) != nil }) else {
            XCTFail("The new Review observation must resume and deliver after the prior flight drains")
            return
        }
        guard await waitUntil({ model.developRecovery.activeGateCount == 0 }) else {
            XCTFail("Both observation gates must finish after the replacement preview drains")
            return
        }
        XCTAssertGreaterThan(try XCTUnwrap(model.loader.cached(item, tier: .preview)).width, 0)
    }

    func testReviewObserveDoesNotRetryARegisteredFailedDevelopClose() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("review-preview-failed-close-\(UUID())")
        let folder = root.appendingPathComponent("photos")
        let support = root.appendingPathComponent("support")
        let photo = folder.appendingPathComponent("failed-close.jpg")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try ShellHarness.writeJPEG(photo, shade: 95)
        let owner = try EngineLibrary.scan(folder: folder, appSupport: support)
        let item = try XCTUnwrap(owner.items.first)
        let reference = try XCTUnwrap(item.engineImage)
        let plan = FailedClosePlan()
        let session = FailingCloseSession(
            try owner.engine.openDevelopSession(imageId: reference.imageID), plan: plan)
        let controller = try DevelopController(session: session, itemID: item.id, imageID: reference.imageID)
        let coordinator = DevelopRecoveryCoordinator()
        let sessionID = coordinator.register(owner: owner, controller: controller, displayName: item.name)

        let firstOutcome = await coordinator.requestClose(sessionID).value
        if case .failed(let failedID, _) = firstOutcome {
            XCTAssertEqual(failedID, sessionID)
        } else {
            XCTFail("The controlled Develop close should fail and retain its record")
        }
        XCTAssertEqual(plan.closeAttempts, 1)

        let observation = coordinator.reserveObserve(owner: owner, imageIDs: [reference.imageID])
        let observationOutcome = await observation.resultWaitingForPrecedingDrain()
        observation.finish()
        guard case .blocked(let blockedIDs) = observationOutcome else {
            return XCTFail("A failed session must block Review without retrying its close")
        }
        XCTAssertEqual(blockedIDs, [sessionID])
        XCTAssertEqual(plan.closeAttempts, 1, "Review observation must not implicitly retry failed saves")
        XCTAssertTrue(coordinator.hasUnresolvedSessions)
        if case .saved = await coordinator.retryClose(sessionID).value {
            XCTAssertEqual(plan.closeAttempts, 2)
            XCTAssertFalse(coordinator.hasUnresolvedSessions)
        } else {
            XCTFail("Explicit retry must close the wrapped session for fixture cleanup")
        }
    }

    private func waitUntil(_ condition: @MainActor () -> Bool,
                           timeout: TimeInterval = 8) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition(), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(20))
        }
        return condition()
    }
}

private final class FailedClosePlan: @unchecked Sendable {
    private let lock = NSLock()
    private var attempts = 0
    private var outcomes = [true, false]
    var closeAttempts: Int { lock.withLock { attempts } }

    func close(_ wrapped: DevelopSession) throws {
        let fails = lock.withLock { () -> Bool in
            attempts += 1
            return outcomes.isEmpty ? false : outcomes.removeFirst()
        }
        if fails { throw InjectedReviewCloseFailure() }
        try wrapped.close()
    }
}

private final class FailingCloseSession: DevelopSession, @unchecked Sendable {
    private let wrapped: DevelopSession
    private let plan: FailedClosePlan

    init(_ wrapped: DevelopSession, plan: FailedClosePlan) {
        self.wrapped = wrapped
        self.plan = plan
        super.init(noHandle: .init())
    }

    required init(unsafeFromHandle: UInt64) { fatalError("Use the wrapped real session initializer") }
    override func info() -> DevelopInfo { wrapped.info() }
    override func historyState() throws -> HistoryState { try wrapped.historyState() }
    override func getSettingsJson() throws -> String { try wrapped.getSettingsJson() }
    override func getHistogram() throws -> Histogram { try wrapped.getHistogram() }
    override func ignoredSettings() throws -> [String] { try wrapped.ignoredSettings() }
    override func setListener(listener: DevelopListener?) { wrapped.setListener(listener: listener) }
    override func setMaskListener(listener: MaskListener?) { wrapped.setMaskListener(listener: listener) }
    override func setSettings(jsonPatch: String, interactive: Bool) throws {
        try wrapped.setSettings(jsonPatch: jsonPatch, interactive: interactive)
    }
    override func close() throws { try plan.close(wrapped) }
}

private struct InjectedReviewCloseFailure: LocalizedError {
    var errorDescription: String? { "injected Review close failure" }
}
