import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// AppModel recovery admission tests. These exercise deterministic open/close gates;
/// they intentionally do not assert registry internals or strong-retention details.
@MainActor
final class DevelopRecoveryCoordinatorTests: XCTestCase {
    private struct Fixture {
        let library: EngineLibrary
        let otherLibrary: EngineLibrary
        let support: URL
    }

    func testFailedActiveCloseDoesNotAdmitAnotherSamePhotoOpen() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("develop-recovery-active-\(UUID().uuidString)")
        let support = root.appendingPathComponent("support")
        let folder = root.appendingPathComponent("photos")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try Self.writePhoto(to: folder)
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let closeGate = FailedCloseGate()
        var openCount = 0
        let model = AppModel(
            agent: AgentController(arguments: ["--fake-planner"], supportDirectory: support),
            developControllerOpener: { ref, itemID in
                openCount += 1
                guard openCount == 1 else { throw UnexpectedRecoveryReopen() }
                let session = CloseFailureSession(try ref.engine.openDevelopSession(imageId: ref.imageID), gate: closeGate)
                return try DevelopController(session: session, itemID: itemID, imageID: ref.imageID)
            }
        )
        model.install(library)
        let item = try XCTUnwrap(library.items.first)
        let ref = try XCTUnwrap(item.engineImage)

        model.openDevelop(for: item)
        await model.pendingDevelopSaveBarrier(imageID: ref.imageID, library: library).value
        XCTAssertEqual(openCount, 1)
        XCTAssertNotNil(model.develop)

        closeGate.failNextClose()
        model.closeDevelop()
        await model.pendingDevelopSaveBarrier(imageID: ref.imageID, library: library).value
        XCTAssertEqual(closeGate.failureCount, 1, "The test must reach the injected close failure")

        model.openDevelop(for: item)
        await model.pendingDevelopSaveBarrier(imageID: ref.imageID, library: library).value
        XCTAssertEqual(openCount, 1, "A failed close must keep this owner/photo out of normal reopen admission")
    }

    func testFailedCleanupOfStaleOpenDoesNotAdmitSameOwnerPhotoOpen() async throws {
        let f = try fixture()
        let opener = GatedOpener()
        defer { opener.cancelPendingOpen() }
        let openStarted = expectation(description: "stale opener entered")
        opener.onEntered = { openStarted.fulfill() }
        let closeGate = FailedCloseGate()
        var openCount = 0
        let model = AppModel(
            agent: AgentController(arguments: ["--fake-planner"], supportDirectory: f.support),
            developControllerOpener: { _, _ in
                openCount += 1
                guard openCount == 1 else { throw UnexpectedRecoveryReopen() }
                return try await opener.open()
            }
        )
        model.install(f.library)
        let item = try XCTUnwrap(f.library.items.first)
        let ref = try XCTUnwrap(item.engineImage)

        model.openDevelop(for: item)
        await fulfillment(of: [openStarted], timeout: 5)
        guard opener.didEnter else {
            opener.cancelPendingOpen()
            return
        }
        model.install(f.otherLibrary) // Invalidates the captured owner/generation.

        let session = CloseFailureSession(
            try ref.engine.openDevelopSession(imageId: ref.imageID), gate: closeGate)
        let staleController = try DevelopController(session: session, itemID: item.id, imageID: ref.imageID)
        closeGate.failNextClose()
        opener.resume(returning: staleController)
        await model.pendingDevelopSaveBarrier(imageID: ref.imageID, library: f.library).value
        XCTAssertEqual(closeGate.failureCount, 1, "The stale controller cleanup must reach the injected failure")

        model.install(f.library)
        model.openDevelop(for: item)
        await model.pendingDevelopSaveBarrier(imageID: ref.imageID, library: f.library).value
        XCTAssertEqual(openCount, 1, "A failed stale-open cleanup must block same-owner/photo reopen")
    }

    private func fixture() throws -> Fixture {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("develop-recovery-stale-\(UUID().uuidString)")
        let support = root.appendingPathComponent("support")
        let folder = root.appendingPathComponent("photos")
        let otherFolder = root.appendingPathComponent("other")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try Self.writePhoto(to: folder)
        try Self.writePhoto(to: otherFolder)
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let other = try EngineLibrary.scan(folder: otherFolder, appSupport: support)
        return Fixture(library: library, otherLibrary: other, support: support)
    }

    private static func writePhoto(to folder: URL) throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
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
        let image = try XCTUnwrap(CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            folder.appendingPathComponent("photo.jpg") as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }
}

@MainActor
private final class GatedOpener {
    private var continuation: CheckedContinuation<DevelopController, Error>?
    var onEntered: (() -> Void)?
    private(set) var didEnter = false
    private var cancelled = false

    func open() async throws -> DevelopController {
        guard !cancelled else { throw UnexpectedOpenCancellation() }
        didEnter = true
        onEntered?()
        onEntered = nil
        return try await withCheckedThrowingContinuation { continuation = $0 }
    }

    func resume(returning controller: DevelopController) {
        continuation?.resume(returning: controller)
        continuation = nil
    }

    func cancelPendingOpen() {
        cancelled = true
        continuation?.resume(throwing: UnexpectedOpenCancellation())
        continuation = nil
    }
}

private final class FailedCloseGate: @unchecked Sendable {
    private let lock = NSLock()
    private var failuresRemaining = 0
    private var observedFailures = 0
    var failureCount: Int { lock.withLock { observedFailures } }

    func failNextClose() { lock.withLock { failuresRemaining += 1 } }
    func shouldFailClose() -> Bool {
        lock.withLock {
            guard failuresRemaining > 0 else { return false }
            failuresRemaining -= 1
            observedFailures += 1
            return true
        }
    }
}

/// Delegates every operation touched by controller callbacks to a real engine session.
private final class CloseFailureSession: DevelopSession, @unchecked Sendable {
    private let wrapped: DevelopSession
    private let gate: FailedCloseGate

    init(_ wrapped: DevelopSession, gate: FailedCloseGate) {
        self.wrapped = wrapped
        self.gate = gate
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
    override func close() throws {
        if gate.shouldFailClose() { throw InjectedCloseFailure() }
        try wrapped.close()
    }
}

private struct InjectedCloseFailure: LocalizedError {
    var errorDescription: String? { "injected develop close failure" }
}

private struct UnexpectedRecoveryReopen: LocalizedError {
    var errorDescription: String? { "unexpected reopen after failed close" }
}

private struct UnexpectedOpenCancellation: LocalizedError {
    var errorDescription: String? { "test cleanup cancelled the gated opener" }
}
