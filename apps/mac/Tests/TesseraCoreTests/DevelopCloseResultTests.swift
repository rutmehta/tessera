import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Behavioral RED controls for the Core close contract. These deliberately use
/// the current Void close signature; Result assertions belong in the next step.
@MainActor
final class DevelopCloseResultTests: XCTestCase {
    private struct Fixture {
        let library: EngineLibrary
        let imageID: String
        let session: CloseFaultSession
        let controller: DevelopController
    }

    private func fixture() throws -> Fixture {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("develop-close-result-\(UUID().uuidString)")
        let photos = root.appendingPathComponent("photos")
        let support = root.appendingPathComponent("support")
        try FileManager.default.createDirectory(at: photos, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }

        let width = 32, height = 24
        let pixels = Data(repeating: 128, count: width * height * 4)
        let provider = try XCTUnwrap(CGDataProvider(data: pixels as CFData))
        let image = try XCTUnwrap(CGImage(width: width, height: height, bitsPerComponent: 8,
            bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            photos.appendingPathComponent("photo.jpg") as CFURL,
            UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))

        let library = try EngineLibrary.scan(folder: photos, appSupport: support)
        XCTAssertEqual(library.items.count, 1, "support files must not enter the photo library")
        let item = try XCTUnwrap(library.items.first)
        let imageID = try XCTUnwrap(item.engineImage?.imageID)
        let session = CloseFaultSession(try library.engine.openDevelopSession(imageId: imageID))
        let controller = try DevelopController(session: session, itemID: item.id, imageID: imageID)
        return Fixture(library: library, imageID: imageID,
                       session: session, controller: controller)
    }

    func testSettingsFlushFailurePreventsNativeCloseAndRemainsRetryable() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        // Every close gate is released before awaiting close, including on failure.
        session.releaseClose()
        session.releaseClose()
        controller.onNeedsFlush = {}
        var failures: [String] = []
        controller.onFailure = { failures.append($0) }
        controller.set(.exposure, 1.25, interactive: true)
        session.rejectNextSettings()

        let firstResult = await controller.close()
        if case .failure(let error) = firstResult {
            XCTAssertTrue(error.localizedDescription.contains("injected settings rejection"))
        } else { XCTFail("close must return the host error") }
        XCTAssertEqual(session.closeCount, 0, "a rejected host patch must stop native close")
        XCTAssertFalse(controller.closed, "the pending patch must remain retryable")
        XCTAssertEqual(session.listenerDetachCount, 0)
        XCTAssertNotNil(controller.onFailure)
        XCTAssertEqual(failures.count, 1)
        XCTAssertEqual(failures.first?.contains("injected settings rejection"), true)

        let retryResult = await controller.close() // Explicit retry, without another edit.
        if case .failure(let error) = retryResult { XCTFail("retry failed: \(error)") }
        XCTAssertEqual(session.settingsAttempts, 2, "the failed patch must be retained")
        XCTAssertEqual(session.closeCount, 1)
        XCTAssertTrue(controller.closed)
        let reopened = try f.library.engine.openDevelopSession(imageId: f.imageID)
        let restored = try DevelopController(session: reopened, itemID: 0, imageID: f.imageID)
        XCTAssertEqual(restored.value(.exposure), 1.25)
        try reopened.close()
    }

    func testConcurrentCloseCallersShareFailureAndSessionCanRetry() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        defer {
            controller.onCloseWaiterJoined = nil
            session.releaseClose() // Never strand a detached backend call on assertion failure.
        }
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        session.rejectNextClose()
        var failures: [String] = []
        controller.onFailure = { failures.append($0) }

        let first = Task { await controller.close() }
        let entered = await session.waitForCloseEntry()
        XCTAssertTrue(entered, "native close did not enter its gate")
        let secondJoined = expectation(description: "second caller joined the in-flight close")
        controller.onCloseWaiterJoined = { secondJoined.fulfill() }
        let second = Task { await controller.close() }
        await fulfillment(of: [secondJoined], timeout: 5)
        // Release only after the second caller crossed Core's join boundary.
        session.releaseClose()
        let firstResult = await first.value
        let secondResult = await second.value
        if case .failure(let error) = firstResult {
            XCTAssertTrue(error.localizedDescription.contains("injected native close rejection"))
        } else { XCTFail("first caller must receive native close failure") }
        if case .failure(let error) = secondResult {
            XCTAssertTrue(error.localizedDescription.contains("injected native close rejection"))
        } else { XCTFail("joined caller must receive the same failure") }
        XCTAssertEqual(session.closeCount, 1)
        XCTAssertFalse(controller.closed, "a rejected native close must leave Core retryable")
        XCTAssertEqual(session.listenerDetachCount, 0)
        XCTAssertNotNil(controller.onFailure)
        XCTAssertEqual(failures.count, 1)

        session.releaseClose()
        let retryResult = await controller.close() // Only this new, explicit call may retry.
        if case .failure(let error) = retryResult { XCTFail("retry failed: \(error)") }
        XCTAssertEqual(session.closeCount, 2)
        XCTAssertTrue(controller.closed)
        let reopened = try f.library.engine.openDevelopSession(imageId: f.imageID)
        let restored = try DevelopController(session: reopened, itemID: 0, imageID: f.imageID)
        XCTAssertEqual(restored.value(.exposure), 1.25)
        try reopened.close()
    }

    func testMaskFlushFailurePreventsNativeCloseAndRetainsPendingValue() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        session.releaseClose()
        controller.onNeedsFlush = {}
        let group = try XCTUnwrap(controller.addMask(
            LinearGradientShape(start: (0.2, 0.2), end: (0.8, 0.8)).json))
        controller.setMaskParam(group, "exposure", 0.7, interactive: true)
        session.rejectNextMaskParam()
        var failures: [String] = []
        controller.onFailure = { failures.append($0) }

        let firstResult = await controller.close()
        if case .failure(let error) = firstResult {
            XCTAssertTrue(error.localizedDescription.contains("injected mask rejection"))
        } else { XCTFail("close must return the mask error") }
        XCTAssertEqual(session.closeCount, 0)
        XCTAssertFalse(controller.closed)
        XCTAssertEqual(controller.pendingMaskParams[.init(group: group, name: "exposure")], Float(0.7))
        XCTAssertEqual(failures.first?.contains("injected mask rejection"), true)

        session.releaseClose()
        let retryResult = await controller.close()
        if case .failure(let error) = retryResult { XCTFail("retry failed: \(error)") }
        XCTAssertEqual(session.maskParamAttempts, 2)
        XCTAssertEqual(session.closeCount, 1)
        let reopened = try f.library.engine.openDevelopSession(imageId: f.imageID)
        let groups = try reopened.maskGroups()
        XCTAssertEqual(groups.first?.params.first { $0.name == "exposure" }?.value, Float(0.7))
        try reopened.close()
    }

    func testClosingRejectsMutationsBeforeChangingHostState() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        defer { session.releaseClose() }
        controller.onNeedsFlush = {}
        controller.set(.exposure, 0.5, interactive: true)
        let group = try XCTUnwrap(controller.addMask(
            LinearGradientShape(start: (0.2, 0.2), end: (0.8, 0.8)).json))
        session.rejectNextClose()
        var failures: [String] = []
        controller.onFailure = { failures.append($0) }
        let first = Task { await controller.close() }
        let entered = await session.waitForCloseEntry()
        XCTAssertTrue(entered)

        let settingsBefore = try XCTUnwrap(DevelopController.encode(controller.settingsObject))
        let itemBefore = controller.itemID
        let maskBefore = controller.pendingMaskParams
        controller.set(.exposure, 2, interactive: true)
        controller.relink(itemID: 99)
        controller.setMaskParam(group, "exposure", 1, interactive: true)
        controller.setMaskParam(group, "exposure", 2, interactive: false)
        XCTAssertFalse(controller.commit(label: "blocked while closing"))
        XCTAssertThrowsError(try controller.undo())
        XCTAssertThrowsError(try controller.attachSurfaces(viewWidth: 32, viewHeight: 24))
        controller.updateDisplay(nil)
        controller.setCropEditing(true)
        controller.setMaskingPreview(true)
        XCTAssertEqual(DevelopController.encode(controller.settingsObject), settingsBefore)
        XCTAssertEqual(controller.itemID, itemBefore)
        XCTAssertEqual(controller.pendingMaskParams, maskBefore)
        XCTAssertEqual(session.settingsAttempts, 1, "closing must not send a new patch")
        XCTAssertEqual(session.maskParamAttempts, 0)
        XCTAssertNil(controller.plan)
        XCTAssertGreaterThanOrEqual(failures.count, 1)

        session.releaseClose()
        let firstResult = await first.value
        if case .success = firstResult { XCTFail("injected native close failure was lost") }
        XCTAssertFalse(controller.closed)
        controller.set(.exposure, 1.25, interactive: true)
        XCTAssertEqual(controller.value(.exposure), 1.25, "failure reopens edit admission")
        session.releaseClose()
        let retryResult = await controller.close()
        if case .failure(let error) = retryResult { XCTFail("retry failed: \(error)") }
        XCTAssertEqual(session.closeCount, 2)
    }

    func testSuccessfulCloseDrainsOnceAndIsIdempotent() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        session.releaseClose()
        let firstResult = await controller.close()
        if case .failure(let error) = firstResult { XCTFail("close failed: \(error)") }
        XCTAssertTrue(controller.closed)
        XCTAssertEqual(session.settingsAttempts, 1)
        XCTAssertEqual(session.closeCount, 1)
        XCTAssertEqual(session.listenerDetachCount, 1)
        XCTAssertFalse(controller.flushPending())
        let secondResult = await controller.close()
        if case .failure(let error) = secondResult { XCTFail("idempotent close failed: \(error)") }
        XCTAssertEqual(session.closeCount, 1)
    }

    func testFailureCallbackCloseDoesNotStartAutomaticRetry() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        session.rejectNextSettings()
        var callbackClose: Task<Result<Void, Error>, Never>?
        var failureCount = 0
        controller.onFailure = { _ in
            failureCount += 1
            if callbackClose == nil { callbackClose = Task { await controller.close() } }
        }
        session.releaseClose()
        let firstResult = await controller.close()
        let callbackResult = await callbackClose?.value
        if case .success = firstResult { XCTFail("host rejection was lost") }
        if case .some(.success) = callbackResult { XCTFail("callback-triggered close retried automatically") }
        XCTAssertEqual(failureCount, 1)
        XCTAssertEqual(session.settingsAttempts, 1)
        XCTAssertEqual(session.closeCount, 0)
        XCTAssertFalse(controller.closed)
    }

    func testPatchCallbackCloseJoinsPublishedAttempt() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        defer {
            controller.onCloseWaiterJoined = nil
            session.releaseClose()
        }
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        let joined = expectation(description: "callback close joined published attempt")
        controller.onCloseWaiterJoined = { joined.fulfill() }
        var callbackClose: Task<Result<Void, Error>, Never>?
        controller.onPatchSent = { _ in callbackClose = Task { await controller.close() } }
        let first = Task { await controller.close() }
        let entered = await session.waitForCloseEntry()
        XCTAssertTrue(entered)
        await fulfillment(of: [joined], timeout: 5)
        session.releaseClose()
        let firstResult = await first.value
        let joinedResult = await callbackClose?.value
        if case .failure(let error) = firstResult { XCTFail("first close failed: \(error)") }
        if case .some(.failure(let error)) = joinedResult { XCTFail("joined close failed: \(error)") }
        XCTAssertEqual(session.settingsAttempts, 1)
        XCTAssertEqual(session.closeCount, 1)
    }

    func testCancelingOneWaiterDoesNotCancelSharedNativeClose() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        defer {
            controller.onCloseWaiterJoined = nil
            session.releaseClose()
        }
        let first = Task { await controller.close() }
        let entered = await session.waitForCloseEntry()
        XCTAssertTrue(entered)
        let joined = expectation(description: "second caller joined native close")
        controller.onCloseWaiterJoined = { joined.fulfill() }
        let second = Task { await controller.close() }
        await fulfillment(of: [joined], timeout: 5)
        first.cancel()
        session.releaseClose()
        let secondResult = await second.value
        _ = await first.value
        if case .failure(let error) = secondResult { XCTFail("joined waiter lost close: \(error)") }
        XCTAssertTrue(controller.closed)
        XCTAssertEqual(session.closeCount, 1)
    }

    func testAdmissionFailureCallbackCannotReenterIndefinitely() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        defer { session.releaseClose() }
        let first = Task { await controller.close() }
        let entered = await session.waitForCloseEntry()
        XCTAssertTrue(entered)
        var failures = 0
        controller.onFailure = { _ in
            failures += 1
            controller.set(.exposure, 5, interactive: true)
        }
        controller.set(.exposure, 2, interactive: true)
        XCTAssertEqual(failures, 1)
        XCTAssertNotEqual(controller.value(.exposure), 2)
        session.releaseClose()
        let result = await first.value
        if case .failure(let error) = result { XCTFail("close failed: \(error)") }
    }

    func testUnencodableSettingsBlockCloseAndRemainPending() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        controller.onNeedsFlush = {}
        var failures: [String] = []
        controller.onFailure = { failures.append($0) }
        controller.set(.exposure, .nan, interactive: true)
        let result = await controller.close()
        if case .failure(let error) = result {
            XCTAssertTrue(error.localizedDescription.contains("encoded as JSON"))
        } else { XCTFail("invalid JSON must stop close") }
        XCTAssertEqual(session.settingsAttempts, 0)
        XCTAssertEqual(session.closeCount, 0)
        XCTAssertFalse(controller.closed)
        XCTAssertTrue(controller.value(.exposure).isNaN)
        XCTAssertEqual(failures.count, 1)

        controller.set(.exposure, 1.25, interactive: true)
        session.releaseClose()
        let retry = await controller.close()
        if case .failure(let error) = retry { XCTFail("corrected retry failed: \(error)") }
        XCTAssertEqual(session.closeCount, 1)
    }

    func testCloseInvalidatesDeferredSettingsDrain() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        var reentered = false
        controller.onPatchSent = { _ in
            guard !reentered else { return }
            reentered = true
            controller.set(.exposure, 0.75, interactive: false)
        }
        controller.set(.exposure, 0.25, interactive: false)
        let obsolete = try XCTUnwrap(controller.deferredSettingsFlush)
        XCTAssertEqual(session.settingsAttempts, 1)
        session.releaseClose()
        let result = await controller.close()
        await obsolete.value
        if case .failure(let error) = result { XCTFail("close failed: \(error)") }
        XCTAssertEqual(session.settingsAttempts, 2, "close drains the pending edit exactly once")
        XCTAssertNil(controller.deferredSettingsFlush)
        XCTAssertEqual(session.closeCount, 1)
    }

    func testCloseInvalidatesDeferredMaskDrain() async throws {
        let f = try fixture()
        let session = f.session, controller = f.controller
        controller.onNeedsFlush = {}
        let group = try XCTUnwrap(controller.addMask(
            LinearGradientShape(start: (0.2, 0.2), end: (0.8, 0.8)).json))
        var reentered = false
        session.beforeMaskParam = {
            guard !reentered else { return }
            reentered = true
            controller.setMaskParam(group, "exposure", 0.75, interactive: true)
        }
        controller.setMaskParam(group, "exposure", 0.25, interactive: true)
        XCTAssertTrue(controller.flushMaskPending())
        let obsolete = try XCTUnwrap(controller.scheduledMaskFlushTask)
        XCTAssertEqual(session.maskParamAttempts, 1)
        session.beforeMaskParam = nil
        session.releaseClose()
        let result = await controller.close()
        await obsolete.value
        if case .failure(let error) = result { XCTFail("close failed: \(error)") }
        XCTAssertEqual(session.maskParamAttempts, 2, "close drains the pending mask once")
        XCTAssertNil(controller.scheduledMaskFlushTask)
        XCTAssertEqual(session.closeCount, 1)
    }
}

/// A real session wrapper that fails before delegation, leaving native retry possible.
/// The no-handle base never receives a getter: all controller getters are forwarded.
private final class CloseFaultSession: DevelopSession, @unchecked Sendable {
    private let wrapped: DevelopSession
    private let entered = DispatchSemaphore(value: 0)
    private let release = DispatchSemaphore(value: 0)
    private let lock = NSLock()
    private var closes = 0
    private var settingsCalls = 0
    private var settingsFailures = 0
    private var maskParamCalls = 0
    private var maskParamFailures = 0
    private var closeFailures = 0
    private var listenerDetaches = 0
    var beforeMaskParam: (@MainActor () -> Void)?

    var closeCount: Int { lock.withLock { closes } }
    var settingsAttempts: Int { lock.withLock { settingsCalls } }
    var maskParamAttempts: Int { lock.withLock { maskParamCalls } }
    var listenerDetachCount: Int { lock.withLock { listenerDetaches } }

    init(_ wrapped: DevelopSession) {
        self.wrapped = wrapped
        super.init(noHandle: .init())
    }
    required init(unsafeFromHandle: UInt64) { fatalError("Use the wrapped real session initializer") }

    func rejectNextSettings() { lock.withLock { settingsFailures += 1 } }
    func rejectNextMaskParam() { lock.withLock { maskParamFailures += 1 } }
    func rejectNextClose() { lock.withLock { closeFailures += 1 } }
    func releaseClose() { release.signal() }
    func waitForCloseEntry() async -> Bool {
        await Task.detached { [entered] in Self.waitForEntry(entered) }.value
    }
    private static func waitForEntry(_ semaphore: DispatchSemaphore) -> Bool {
        semaphore.wait(timeout: .now() + 5) == .success
    }

    override func info() -> DevelopInfo { wrapped.info() }
    override func historyState() throws -> HistoryState { try wrapped.historyState() }
    override func getSettingsJson() throws -> String { try wrapped.getSettingsJson() }
    override func getHistogram() throws -> Histogram { try wrapped.getHistogram() }
    override func ignoredSettings() throws -> [String] { try wrapped.ignoredSettings() }
    override func setListener(listener: DevelopListener?) {
        if listener == nil { lock.withLock { listenerDetaches += 1 } }
        wrapped.setListener(listener: listener)
    }
    override func setMaskListener(listener: MaskListener?) { wrapped.setMaskListener(listener: listener) }
    override func planSurface(width: UInt32, height: UInt32) -> SurfacePlan {
        wrapped.planSurface(width: width, height: height)
    }
    override func attachSurface(iosurfaceId: UInt32, width: UInt32, height: UInt32) throws {
        try wrapped.attachSurface(iosurfaceId: iosurfaceId, width: width, height: height)
    }
    override func setDisplayHeadroom(headroom: Float) throws {
        try wrapped.setDisplayHeadroom(headroom: headroom)
    }
    override func setCropEditing(editing: Bool) throws { try wrapped.setCropEditing(editing: editing) }
    override func setMaskingPreview(enabled: Bool) throws { try wrapped.setMaskingPreview(enabled: enabled) }
    override func commit(label: String) throws -> Bool { try wrapped.commit(label: label) }
    override func undo() throws -> Bool { try wrapped.undo() }
    override func setSettings(jsonPatch: String, interactive: Bool) throws {
        let fail = lock.withLock {
            settingsCalls += 1
            guard settingsFailures > 0 else { return false }
            settingsFailures -= 1
            return true
        }
        if fail { throw CloseFaultError.settings }
        try wrapped.setSettings(jsonPatch: jsonPatch, interactive: interactive)
    }
    override func addMask(definitionJson: String, interactive: Bool) throws -> UInt32 {
        try wrapped.addMask(definitionJson: definitionJson, interactive: interactive)
    }
    override func maskGroups() throws -> [MaskGroupInfo] { try wrapped.maskGroups() }
    override func setMaskParam(groupId: UInt32, name: String, value: Float, interactive: Bool) throws {
        MainActor.assumeIsolated { beforeMaskParam?() }
        let fail = lock.withLock {
            maskParamCalls += 1
            guard maskParamFailures > 0 else { return false }
            maskParamFailures -= 1
            return true
        }
        if fail { throw CloseFaultError.mask }
        try wrapped.setMaskParam(groupId: groupId, name: name, value: value, interactive: interactive)
    }
    override func close() throws {
        let fail = lock.withLock {
            closes += 1
            guard closeFailures > 0 else { return false }
            closeFailures -= 1
            return true
        }
        entered.signal()
        guard release.wait(timeout: .now() + 5) == .success else {
            throw CloseFaultError.gateTimeout
        }
        if fail { throw CloseFaultError.close }
        try wrapped.close()
    }
}

private enum CloseFaultError: LocalizedError {
    case settings, mask, close, gateTimeout
    var errorDescription: String? {
        switch self {
        case .settings: "injected settings rejection"
        case .mask: "injected mask rejection"
        case .close: "injected native close rejection"
        case .gateTimeout: "close gate timed out"
        }
    }
}
