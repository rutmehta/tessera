import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

@MainActor
final class DevelopRecoveryStateTests: XCTestCase {
    private struct Rig {
        let root: URL
        let owner: EngineLibrary
        let otherOwner: EngineLibrary
        let imageID: String
        let controller: DevelopController
        let closePlan: ClosePlan
    }

    func testFailedCloseRetainsOwnerAndControllerUntilExplicitRetrySucceeds() async throws {
        var rig: Rig? = try makeRig(closeFailures: [true, false])
        let imageID = try XCTUnwrap(rig?.imageID)
        var ownerForRegistration: EngineLibrary? = rig?.owner
        var controllerForRegistration: DevelopController? = rig?.controller
        let weakReferences = WeakReferences(owner: try XCTUnwrap(ownerForRegistration),
                                            controller: try XCTUnwrap(controllerForRegistration))
        let coordinator = DevelopRecoveryCoordinator()
        let id = coordinator.register(owner: try XCTUnwrap(ownerForRegistration),
                                      controller: try XCTUnwrap(controllerForRegistration),
                                      displayName: "photo.jpg")
        let plan = try XCTUnwrap(rig?.closePlan)
        ownerForRegistration = nil
        controllerForRegistration = nil
        rig = nil

        XCTAssertNotNil(weakReferences.owner, "The recovery record retains its library owner")
        XCTAssertNotNil(weakReferences.controller, "The recovery record retains its controller")
        let owner = try XCTUnwrap(weakReferences.owner)
        guard case .failed(let failedID, let firstMessage) = await coordinator.requestClose(id).value else {
            return XCTFail("First close should fail and remain recoverable")
        }
        XCTAssertEqual(failedID, id)
        XCTAssertEqual(plan.callCount, 1)
        guard case .failed(let repeatedID, let repeatedMessage) = await coordinator.requestClose(id).value else {
            return XCTFail("An ordinary close request must return the cached failure, not retry")
        }
        XCTAssertEqual(repeatedID, failedID)
        XCTAssertEqual(repeatedMessage, firstMessage)
        XCTAssertEqual(plan.callCount, 1, "Only explicit retry may invoke native close again")
        XCTAssertEqual(coordinator.matchingSession(owner: owner, imageID: imageID), id)
        XCTAssertEqual(coordinator.presentations.count, 1)
        if case .failed = coordinator.presentations[0].phase {} else {
            XCTFail("Failed close should remain visible as a failed recovery record")
        }

        guard case .saved = await coordinator.retryClose(id).value else {
            return XCTFail("An explicit retry should close the retained controller")
        }
        XCTAssertEqual(plan.callCount, 2)
        XCTAssertNil(coordinator.matchingSession(owner: owner, imageID: imageID))
        XCTAssertTrue(coordinator.presentations.isEmpty)
        XCTAssertTrue(coordinator.canOpen(owner: owner, imageID: imageID))
        XCTAssertNil(weakReferences.controller, "A successful retry releases the retained controller")
    }

    func testJoinedCloseCallersShareOneFailureAndDoNotAutoRetry() async throws {
        let rig = try makeRig(closeFailures: [true])
        let coordinator = DevelopRecoveryCoordinator()
        let id = coordinator.register(owner: rig.owner, controller: rig.controller, displayName: "photo.jpg")
        let entered = expectation(description: "first close entered backend")
        rig.closePlan.gateNextClose(entered: entered)
        defer { rig.closePlan.releaseGatedClose() }

        let first = coordinator.requestClose(id)
        await fulfillment(of: [entered], timeout: 5)
        let second = coordinator.requestClose(id)
        XCTAssertEqual(rig.closePlan.callCount, 1)
        rig.closePlan.releaseGatedClose()

        let firstOutcome = await first.value
        let secondOutcome = await second.value
        guard case .failed(let firstID, let firstMessage) = firstOutcome,
              case .failed(let secondID, let secondMessage) = secondOutcome else {
            return XCTFail("Joined callers must receive the same failed attempt")
        }
        XCTAssertEqual(firstID, id)
        XCTAssertEqual(secondID, id)
        XCTAssertEqual(firstMessage, secondMessage)
        XCTAssertEqual(rig.closePlan.callCount, 1, "Waiters must not trigger an automatic retry")
    }

    func testOwnerlessRecoveryRecordBlocksEveryLibraryUntilResolved() async throws {
        let rig = try makeRig(closeFailures: [true, false])
        let coordinator = DevelopRecoveryCoordinator()
        let id = coordinator.register(owner: nil, controller: rig.controller, displayName: "photo.jpg")

        XCTAssertFalse(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID))
        XCTAssertFalse(coordinator.canOpen(owner: rig.otherOwner, imageID: "unrelated-image"))
        guard case .failed = await coordinator.requestClose(id).value else {
            return XCTFail("First ownerless close should fail")
        }
        XCTAssertFalse(coordinator.canOpen(owner: rig.owner, imageID: "another-photo"))
        XCTAssertFalse(coordinator.canOpen(owner: rig.otherOwner, imageID: rig.imageID))

        guard case .failed = await coordinator.requestClose(id).value else {
            return XCTFail("An ordinary request after failure must not retry the ownerless close")
        }
        XCTAssertEqual(rig.closePlan.callCount, 1)
        guard case .saved = await coordinator.retryClose(id).value else {
            return XCTFail("Explicit retry should resolve the ownerless record")
        }
        XCTAssertTrue(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID))
        XCTAssertTrue(coordinator.canOpen(owner: rig.otherOwner, imageID: "unrelated-image"))
    }

    func testSuccessfulBarrierKeepsAdmissionReservedUntilGateFinish() async throws {
        let rig = try makeRig(closeFailures: [false])
        let coordinator = DevelopRecoveryCoordinator()
        _ = coordinator.register(owner: rig.owner, controller: rig.controller, displayName: "photo.jpg")
        let gate = coordinator.reserveInitiate(owner: rig.owner, imageIDs: [rig.imageID])

        guard case .saved = await gate.result() else {
            gate.finish()
            return XCTFail("Successful close barrier should report saved")
        }
        XCTAssertTrue(coordinator.presentations.isEmpty)
        XCTAssertFalse(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID),
                       "A completed result still holds the reservation until its owner commits finish")
        XCTAssertFalse(coordinator.isUnreservedForHostMutation(owner: rig.owner, imageID: rig.imageID))
        gate.finish()
        XCTAssertTrue(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID))
        XCTAssertTrue(coordinator.isUnreservedForHostMutation(owner: rig.owner, imageID: rig.imageID))
    }

    func testFailedStaleOpenCleanupRegistersBeforeInitiatingBarrierReturns() async throws {
        let rig = try makeRig(closeFailures: [true])
        let coordinator = DevelopRecoveryCoordinator()
        let openGate = OpenGate()
        let entered = expectation(description: "stale open entered")
        openGate.onEntered = { entered.fulfill() }
        let token = UUID()
        var didProduce = false
        let ticket = Task { @MainActor in
            await openGate.wait()
            didProduce = coordinator.producedOpen(rig.controller, owner: rig.owner, token: token)
            coordinator.finishOpen(token: token)
        }
        coordinator.beginOpen(owner: rig.owner, imageID: rig.imageID, token: token, task: ticket)
        defer { openGate.cancel() }
        await fulfillment(of: [entered], timeout: 5)
        guard openGate.didEnter else {
            openGate.cancel()
            _ = await ticket.value
            return XCTFail("The open ticket did not enter its gate")
        }

        let barrier = coordinator.reserveInitiate(owner: rig.owner, imageIDs: [rig.imageID])
        let resultTask = Task { @MainActor in await barrier.result() }
        let deadline = Date().addingTimeInterval(5)
        while !ticket.isCancelled && Date() < deadline { await Task.yield() }
        guard ticket.isCancelled else {
            openGate.cancel()
            barrier.finish()
            _ = await ticket.value
            _ = await resultTask.value
            return XCTFail("Initiating barrier did not cancel the stale open ticket")
        }
        openGate.resume()
        let result = await resultTask.value

        guard case .blocked(let ids) = result else {
            barrier.finish()
            return XCTFail("Failed stale cleanup must be registered before the barrier resolves")
        }
        XCTAssertTrue(didProduce)
        XCTAssertEqual(ids.count, 1)
        XCTAssertEqual(rig.closePlan.callCount, 1, "A failed cleanup must not retry automatically")
        XCTAssertFalse(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID))
        barrier.finish()
    }

    func testUnknownSessionCloseFailsClosed() async {
        let coordinator = DevelopRecoveryCoordinator()
        let unknownID = DevelopRecoveryCoordinator.SessionID(value: UUID())

        guard case .failed(let failedID, let message) = await coordinator.requestClose(unknownID).value else {
            return XCTFail("An unknown recovery record must never report a successful save")
        }
        XCTAssertEqual(failedID, unknownID)
        XCTAssertFalse(message.isEmpty)
        XCTAssertFalse(coordinator.hasUnresolvedSessions)
    }

    func testFinishOpenClosesProducedControllerThatWasNotTransferred() async throws {
        let rig = try makeRig(closeFailures: [false])
        let coordinator = DevelopRecoveryCoordinator()
        let token = UUID()
        let openTask = Task { @MainActor in }
        await openTask.value
        coordinator.beginOpen(owner: rig.owner, imageID: rig.imageID, token: token, task: openTask)

        XCTAssertTrue(coordinator.producedOpen(rig.controller, owner: rig.owner, token: token))
        XCTAssertTrue(coordinator.hasUnresolvedSessions, "The open ticket remains unresolved before settlement")
        XCTAssertFalse(coordinator.transferOpen(token: token, to: .init(value: UUID())),
                       "A ticket cannot be released without a registered exact controller recipient")
        coordinator.finishOpen(token: token)
        XCTAssertTrue(coordinator.hasUnresolvedSessions,
                      "Untransferred production must become a retained cleanup record")
        XCTAssertNotNil(coordinator.matchingSession(owner: rig.owner, imageID: rig.imageID))
        XCTAssertFalse(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID))

        let observer = coordinator.reserveObserve(owner: rig.owner, imageIDs: [rig.imageID])
        guard case .saved = await observer.result() else {
            observer.finish()
            return XCTFail("The produced but untransferred session should close successfully")
        }
        observer.finish()
        XCTAssertFalse(coordinator.hasUnresolvedSessions)
        XCTAssertTrue(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID))
    }

    func testTransferRequiresMatchingOwnerAndExactControllerRecipient() async throws {
        let rig = try makeRig(closeFailures: [])
        let coordinator = DevelopRecoveryCoordinator()
        let token = UUID()
        let openTask = Task { @MainActor in }
        await openTask.value
        coordinator.beginOpen(owner: rig.owner, imageID: rig.imageID, token: token, task: openTask)
        XCTAssertTrue(coordinator.producedOpen(rig.controller, owner: rig.owner, token: token))
        XCTAssertFalse(coordinator.transferOpen(token: token, to: .init(value: UUID())))

        // The identity deliberately matches, but the registered key belongs to another owner.
        let wrongOwner = coordinator.register(owner: rig.otherOwner, controller: rig.controller,
                                               displayName: "photo.jpg")
        XCTAssertFalse(coordinator.transferOpen(token: token, to: wrongOwner),
                       "A matching controller under a different owner cannot receive this ticket")
        let exactOwner = coordinator.register(owner: rig.owner, controller: rig.controller,
                                              displayName: "photo.jpg")
        XCTAssertTrue(coordinator.transferOpen(token: token, to: exactOwner))
        coordinator.finishOpen(token: token)

        guard case .saved = await coordinator.requestClose(exactOwner).value else {
            return XCTFail("Exact owner/controller recipient should own the produced session")
        }
        guard case .saved = await coordinator.requestClose(wrongOwner).value else {
            return XCTFail("The intentionally mismatched test record should clean up after assertions")
        }
        XCTAssertEqual(rig.closePlan.callCount, 1)
        XCTAssertFalse(coordinator.hasUnresolvedSessions)
    }

    func testLateProducedControllerIsRetainedAndCanBeRetried() async throws {
        let rig = try makeRig(closeFailures: [true, false])
        let coordinator = DevelopRecoveryCoordinator()
        let token = UUID()
        let openTask = Task { @MainActor in }
        await openTask.value
        coordinator.beginOpen(owner: rig.owner, imageID: rig.imageID, token: token, task: openTask)
        coordinator.finishOpen(token: token)

        XCTAssertFalse(coordinator.producedOpen(rig.controller, owner: rig.owner, token: token),
                       "Production after ticket settlement is late and must not transfer")
        let observer = coordinator.reserveObserve(owner: rig.owner, imageIDs: [rig.imageID])
        guard case .blocked(let ids) = await observer.result() else {
            observer.finish()
            return XCTFail("A failed late-result cleanup remains blocked")
        }
        observer.finish()
        XCTAssertEqual(ids.count, 1)
        XCTAssertTrue(coordinator.hasUnresolvedSessions)
        XCTAssertEqual(rig.closePlan.callCount, 1)

        let sessionID = try XCTUnwrap(coordinator.matchingSession(owner: rig.owner, imageID: rig.imageID))
        guard case .saved = await coordinator.retryClose(sessionID).value else {
            return XCTFail("An explicit retry should settle the late-produced controller")
        }
        XCTAssertFalse(coordinator.hasUnresolvedSessions)
        XCTAssertTrue(coordinator.canOpen(owner: rig.owner, imageID: rig.imageID))
    }

    private func makeRig(closeFailures: [Bool]) throws -> Rig {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("develop-recovery-state-\(UUID().uuidString)")
        let photos = root.appendingPathComponent("photos")
        let otherPhotos = root.appendingPathComponent("other-photos")
        let support = root.appendingPathComponent("support")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try Self.writePhoto(to: photos.appendingPathComponent("photo.jpg"))
        try Self.writePhoto(to: otherPhotos.appendingPathComponent("other.jpg"))
        let owner = try EngineLibrary.scan(folder: photos, appSupport: support)
        let otherOwner = try EngineLibrary.scan(folder: otherPhotos, appSupport: support)
        let item = try XCTUnwrap(owner.items.first)
        let reference = try XCTUnwrap(item.engineImage)
        let plan = ClosePlan(outcomes: closeFailures)
        let session = ClosePlanSession(try owner.engine.openDevelopSession(imageId: reference.imageID), plan: plan)
        let controller = try DevelopController(session: session, itemID: item.id, imageID: reference.imageID)
        return Rig(root: root, owner: owner, otherOwner: otherOwner,
                   imageID: reference.imageID, controller: controller, closePlan: plan)
    }

    private static func writePhoto(to url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let width = 64, height = 48
        var pixels = [UInt8](repeating: 255, count: width * height * 4)
        for y in 0..<height {
            for x in 0..<width {
                let offset = (y * width + x) * 4
                let value = UInt8(48 + ((x / 8 + y / 8) % 2) * 120)
                pixels[offset] = value
                pixels[offset + 1] = value
                pixels[offset + 2] = value
            }
        }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(width: width, height: height, bitsPerComponent: 8,
            bitsPerPixel: 32, bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }
}

@MainActor
private final class OpenGate {
    private var continuation: CheckedContinuation<Void, Never>?
    var onEntered: (() -> Void)?
    private(set) var didEnter = false
    private var cancelled = false

    func wait() async {
        guard !cancelled else { return }
        didEnter = true
        onEntered?()
        onEntered = nil
        await withCheckedContinuation { continuation = $0 }
    }

    func resume() {
        continuation?.resume()
        continuation = nil
    }

    func cancel() {
        cancelled = true
        continuation?.resume()
        continuation = nil
    }
}

private final class ClosePlan: @unchecked Sendable {
    private struct Attempt {
        let fails: Bool
        let semaphore: DispatchSemaphore?
        let entered: XCTestExpectation?
    }
    private let lock = NSLock()
    private var outcomes: [Bool]
    private var gatedAttempt: Attempt?
    private var activeGatedSemaphore: DispatchSemaphore?
    private var attempts = 0
    var callCount: Int { lock.withLock { attempts } }

    init(outcomes: [Bool]) { self.outcomes = outcomes }

    func gateNextClose(entered: XCTestExpectation) {
        lock.withLock {
            let fails = outcomes.isEmpty ? false : outcomes.removeFirst()
            gatedAttempt = Attempt(fails: fails, semaphore: DispatchSemaphore(value: 0), entered: entered)
        }
    }

    func releaseGatedClose() {
        let semaphore = lock.withLock { activeGatedSemaphore ?? gatedAttempt?.semaphore }
        semaphore?.signal()
    }

    func performClose(_ wrapped: DevelopSession) throws {
        let attempt = lock.withLock { () -> Attempt in
            attempts += 1
            if let gatedAttempt {
                self.gatedAttempt = nil
                activeGatedSemaphore = gatedAttempt.semaphore
                return gatedAttempt
            }
            return Attempt(fails: outcomes.isEmpty ? false : outcomes.removeFirst(), semaphore: nil, entered: nil)
        }
        attempt.entered?.fulfill()
        attempt.semaphore?.wait()
        if let semaphore = attempt.semaphore {
            lock.withLock {
                if activeGatedSemaphore === semaphore {
                    activeGatedSemaphore = nil
                }
            }
        }
        if attempt.fails { throw InjectedRecoveryCloseFailure() }
        try wrapped.close()
    }
}

private final class ClosePlanSession: DevelopSession, @unchecked Sendable {
    private let wrapped: DevelopSession
    private let plan: ClosePlan

    init(_ wrapped: DevelopSession, plan: ClosePlan) {
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
    override func close() throws { try plan.performClose(wrapped) }
}

private struct InjectedRecoveryCloseFailure: LocalizedError {
    var errorDescription: String? { "injected recovery close failure" }
}

private final class WeakReferences {
    weak var owner: EngineLibrary?
    weak var controller: DevelopController?

    init(owner: EngineLibrary, controller: DevelopController) {
        self.owner = owner
        self.controller = controller
    }
}
