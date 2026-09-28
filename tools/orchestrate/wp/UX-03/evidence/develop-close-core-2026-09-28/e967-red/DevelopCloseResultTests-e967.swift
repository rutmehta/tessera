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

        await controller.close()
        XCTAssertEqual(session.closeCount, 0, "a rejected host patch must stop native close")
        XCTAssertFalse(controller.closed, "the pending patch must remain retryable")
        XCTAssertEqual(session.listenerDetachCount, 0)
        XCTAssertNotNil(controller.onFailure)
        XCTAssertEqual(failures.count, 1)
        XCTAssertEqual(failures.first?.contains("injected settings rejection"), true)

        await controller.close() // Explicit retry, without another edit.
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
        await first.value
        await second.value
        XCTAssertEqual(session.closeCount, 1)
        XCTAssertFalse(controller.closed, "a rejected native close must leave Core retryable")
        XCTAssertEqual(session.listenerDetachCount, 0)
        XCTAssertNotNil(controller.onFailure)
        XCTAssertEqual(failures.count, 1)

        session.releaseClose()
        await controller.close() // Only this new, explicit call may retry.
        XCTAssertEqual(session.closeCount, 2)
        XCTAssertTrue(controller.closed)
        let reopened = try f.library.engine.openDevelopSession(imageId: f.imageID)
        let restored = try DevelopController(session: reopened, itemID: 0, imageID: f.imageID)
        XCTAssertEqual(restored.value(.exposure), 1.25)
        try reopened.close()
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
    private var closeFailures = 0
    private var listenerDetaches = 0

    var closeCount: Int { lock.withLock { closes } }
    var settingsAttempts: Int { lock.withLock { settingsCalls } }
    var listenerDetachCount: Int { lock.withLock { listenerDetaches } }

    init(_ wrapped: DevelopSession) {
        self.wrapped = wrapped
        super.init(noHandle: .init())
    }
    required init(unsafeFromHandle: UInt64) { fatalError("Use the wrapped real session initializer") }

    func rejectNextSettings() { lock.withLock { settingsFailures += 1 } }
    func rejectNextClose() { lock.withLock { closeFailures += 1 } }
    func releaseClose() { release.signal() }
    func waitForCloseEntry() async -> Bool {
        await Task.detached { [entered] in
            entered.wait(timeout: .now() + 5) == .success
        }.value
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
    case settings, close, gateTimeout
    var errorDescription: String? {
        switch self {
        case .settings: "injected settings rejection"
        case .close: "injected native close rejection"
        case .gateTimeout: "close gate timed out"
        }
    }
}
