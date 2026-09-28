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
        await settleDevelop(model, imageID: ref.imageID, owner: library)
        XCTAssertEqual(openCount, 1)
        XCTAssertNotNil(model.develop)

        closeGate.failNextClose()
        model.closeDevelop()
        await settleDevelop(model, imageID: ref.imageID, owner: library)
        XCTAssertEqual(closeGate.failureCount, 1, "The test must reach the injected close failure")

        model.openDevelop(for: item)
        await settleDevelop(model, imageID: ref.imageID, owner: library)
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
        await settleDevelop(model, imageID: ref.imageID, owner: f.library)
        XCTAssertEqual(closeGate.failureCount, 1, "The stale controller cleanup must reach the injected failure")

        model.install(f.library)
        model.openDevelop(for: item)
        await settleDevelop(model, imageID: ref.imageID, owner: f.library)
        XCTAssertEqual(openCount, 1, "A failed stale-open cleanup must block same-owner/photo reopen")
    }

    /// Losing the current opener's error or retaining its empty ticket would hide the
    /// conflict, block navigation, or prevent the next real editor from opening.
    func testThrownAdmissionErrorIsVisibleAndFreshOpenSucceedsAfterReentry() async throws {
        let f = try fixture()
        let opener = GatedOpener()
        defer { opener.cancelPendingOpen() }
        let entered = expectation(description: "rejected opener entered")
        opener.onEntered = { entered.fulfill() }
        var openCount = 0
        let model = AppModel(
            agent: AgentController(arguments: ["--fake-planner"], supportDirectory: f.support),
            developControllerOpener: { ref, itemID in
                openCount += 1
                if openCount == 1 { return try await opener.open() }
                return try await DevelopController.open(ref, itemID: itemID)
            }
        )
        model.install(f.library)
        let item = try XCTUnwrap(f.library.items.first)
        let ref = try XCTUnwrap(item.engineImage)
        model.enterPhotoEdit()
        let selection = model.selection
        model.openDevelop(for: item)
        await fulfillment(of: [entered], timeout: 5)
        guard opener.didEnter else { return }
        XCTAssertEqual(model.developStatus, .loading)

        opener.resume(throwing: InjectedAdmissionFailure())
        await settleDevelop(model, imageID: ref.imageID, owner: f.library)

        XCTAssertEqual(model.developStatus, .unavailable("injected editor admission conflict"))
        XCTAssertEqual(model.photoEditAvailabilityHint, "injected editor admission conflict")
        XCTAssertNil(model.develop)
        XCTAssertTrue(model.developRecoveries.isEmpty)
        XCTAssertFalse(model.developRecovery.hasUnresolvedSessions)
        XCTAssertFalse(model.developRecovery.hasActiveReservations)
        XCTAssertTrue(model.engineLibrary === f.library)
        XCTAssertEqual(model.selection, selection)
        XCTAssertEqual(model.focusedItem?.engineImage?.imageID, ref.imageID)
        XCTAssertTrue(model.isPhotoEditing)

        model.returnToLibrary(grid: true)
        XCTAssertEqual(model.viewMode, .grid, "A rejected open owns no unsaved editor to block leaving")
        XCTAssertFalse(model.isPhotoEditing)
        model.enterPhotoEdit()
        // No LoupeController is mounted by this AppModel test; invoke its ordinary
        // selection-triggered call after exercising the real navigation methods.
        model.openDevelop(for: item)
        await settleDevelop(model, imageID: ref.imageID, owner: f.library)
        let controller = try XCTUnwrap(model.develop)
        XCTAssertEqual(openCount, 2, "The failed ticket must not block a fresh opener")
        XCTAssertEqual(model.developStatus, .ready)
        XCTAssertEqual(controller.imageID, ref.imageID)
        XCTAssertTrue(model.engineLibrary === f.library)
        let close = try XCTUnwrap(model.closeDevelop())
        guard case .saved = await close.value else { return XCTFail("Fresh editor should close normally") }
        XCTAssertNil(model.develop)
        XCTAssertFalse(model.developRecovery.hasUnresolvedSessions)
    }

    /// Removing old-token/cancellation ownership checks would let an old conflict
    /// replace the new photo's loading/ready state or clear its pending open.
    func testSupersededAdmissionErrorPreservesReplacementLoadingAndReadyState() async throws {
        for replacementReady in [false, true] {
            let f = try fixture()
            let oldOpener = GatedOpener()
            let newOpener = GatedOpener()
            defer {
                oldOpener.cancelPendingOpen()
                newOpener.cancelPendingOpen()
            }
            let oldEntered = expectation(description: "old opener entered (ready=\(replacementReady))")
            let newEntered = expectation(description: "replacement opener entered (ready=\(replacementReady))")
            oldOpener.onEntered = { oldEntered.fulfill() }
            newOpener.onEntered = { newEntered.fulfill() }
            var openCount = 0
            let model = AppModel(
                agent: AgentController(arguments: ["--fake-planner"], supportDirectory: f.support),
                developControllerOpener: { _, _ in
                    openCount += 1
                    if openCount == 1 { return try await oldOpener.open() }
                    guard openCount == 2 else { throw UnexpectedRecoveryReopen() }
                    return try await newOpener.open()
                }
            )
            model.install(f.library)
            let oldItem = try XCTUnwrap(f.library.items.first)
            let oldRef = try XCTUnwrap(oldItem.engineImage)
            model.openDevelop(for: oldItem)
            await fulfillment(of: [oldEntered], timeout: 5)
            guard oldOpener.didEnter else { return }

            model.install(f.otherLibrary)
            let newItem = try XCTUnwrap(f.otherLibrary.items.first)
            let newRef = try XCTUnwrap(newItem.engineImage)
            model.openDevelop(for: newItem)
            await fulfillment(of: [newEntered], timeout: 5)
            guard newOpener.didEnter else { return }
            let controller = try await DevelopController.open(newRef, itemID: newItem.id)
            if replacementReady {
                newOpener.resume(returning: controller)
                await settleDevelop(model, imageID: newRef.imageID, owner: f.otherLibrary)
                XCTAssertTrue(model.develop === controller)
                XCTAssertEqual(model.developStatus, .ready)
            } else {
                XCTAssertNil(model.develop)
                XCTAssertEqual(model.developStatus, .loading)
            }

            // The first Task was canceled by install(), but this noninterruptible
            // opener deliberately throws later, as an in-flight native open can.
            oldOpener.resume(throwing: InjectedAdmissionFailure())
            await settleDevelop(model, imageID: oldRef.imageID, owner: f.library)
            XCTAssertTrue(model.engineLibrary === f.otherLibrary)
            XCTAssertEqual(model.focusedItem?.engineImage?.imageID, newRef.imageID)
            XCTAssertEqual(model.developStatus, replacementReady ? .ready : .loading)
            if !replacementReady {
                XCTAssertNil(model.develop)
                newOpener.resume(returning: controller)
                await settleDevelop(model, imageID: newRef.imageID, owner: f.otherLibrary)
            }
            XCTAssertEqual(openCount, 2)
            XCTAssertEqual(model.developStatus, .ready)
            XCTAssertTrue(model.develop === controller)
            XCTAssertEqual(model.developRecoveries.count, 1, "Only the replacement editor owns recovery")
            let close = try XCTUnwrap(model.closeDevelop())
            guard case .saved = await close.value else { return XCTFail("Replacement editor should close normally") }
            XCTAssertNil(model.develop)
            XCTAssertTrue(model.developRecoveries.isEmpty)
            XCTAssertFalse(model.developRecovery.hasUnresolvedSessions)
            XCTAssertFalse(model.developRecovery.hasActiveReservations)
        }
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

    private func settleDevelop(_ model: AppModel, imageID: String, owner: EngineLibrary) async {
        let gate = model.pendingDevelopSaveBarrier(imageID: imageID, library: owner)
        _ = await gate.result()
        gate.finish()
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

    func resume(throwing error: Error) {
        continuation?.resume(throwing: error)
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

private struct InjectedAdmissionFailure: LocalizedError {
    var errorDescription: String? { "injected editor admission conflict" }
}
