import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// AppModel-level coverage for failed/retried Develop saves and destinations that arrive
/// while a save reservation is held. These tests use tiny generated JPEGs only.
@MainActor
final class DevelopRecoveryAdmissionBehaviorTests: XCTestCase {
    private struct Fixture {
        let root: URL
        let library: EngineLibrary
        let model: AppModel
        let closePlan: ClosePlan
        let openerCount: () -> Int
    }

    func testFailedSaveKeepsWorkspaceAndExplicitRetryCommitsThenReopensFreshController() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let item = try XCTUnwrap(fixture.library.items.first)
        model.enterPhotoEdit()
        model.openDevelop(for: item)
        await waitUntil { model.develop != nil }
        let originalController = try XCTUnwrap(model.develop)
        let originalSelection = model.selection

        fixture.closePlan.failNextClose()
        model.requestLibraryViewMode(.grid)
        await waitUntil {
            model.developRecoveries.contains { presentation in
                if case .failed = presentation.phase { return true }
                return false
            }
        }

        XCTAssertTrue(model.photoEditing, "A failed save must leave the edit workspace attached")
        XCTAssertEqual(model.viewMode, .loupe)
        XCTAssertTrue(model.develop === originalController)
        XCTAssertTrue(model.engineLibrary === fixture.library)
        XCTAssertEqual(model.selection, originalSelection)
        let sessionID = try XCTUnwrap(model.developRecoveries.first?.id)

        model.retryDevelopRecovery(sessionID)
        await waitUntil { model.viewMode == .grid && model.develop == nil }
        XCTAssertFalse(model.photoEditing)
        XCTAssertTrue(model.developRecoveries.isEmpty)

        model.enterPhotoEdit()
        model.openDevelop(for: item)
        await waitUntil { model.develop != nil }
        XCTAssertEqual(fixture.openerCount(), 2)
        XCTAssertFalse(model.develop === originalController,
                       "Reopening after successful retry must create a new native session")
        if let close = model.closeDevelop() { _ = await close.value }
    }

    func testLatestViewIntentCommitsAfterOneHeldDevelopClose() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        await waitUntil { model.develop != nil }

        let entered = expectation(description: "native close is held")
        let hold = fixture.closePlan.holdNext(entered: entered)
        model.requestViewMode(.document)
        await fulfillment(of: [entered], timeout: 5)
        XCTAssertTrue(model.photoEditing)
        XCTAssertNotEqual(model.viewMode, .document,
                          "The requested destination is not published before save settlement")

        model.requestLibraryViewMode(.grid)
        hold.release()
        await waitUntil { model.viewMode == .grid && model.develop == nil }
        XCTAssertFalse(model.photoEditing)
        XCTAssertEqual(fixture.closePlan.closeCount, 1,
                       "Replacing a pending destination must share, not restart, the native close")
        XCTAssertEqual(fixture.openerCount(), 1)
    }

    func testSupersededFolderCallbackSettlesOnceAndMayReenterOpenFolder() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let defaults = UserDefaults.standard
        let priorLastFolder = defaults.object(forKey: "LastFolderPath")
        let priorRecentFolders = defaults.object(forKey: "RecentFolderPaths")
        defer {
            if let priorLastFolder { defaults.set(priorLastFolder, forKey: "LastFolderPath") }
            else { defaults.removeObject(forKey: "LastFolderPath") }
            if let priorRecentFolders { defaults.set(priorRecentFolders, forKey: "RecentFolderPaths") }
            else { defaults.removeObject(forKey: "RecentFolderPaths") }
        }
        let firstDestination = fixture.root.appendingPathComponent("destination-a")
        let reentrantDestination = fixture.root.appendingPathComponent("destination-c")
        try FileManager.default.createDirectory(at: firstDestination, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: reentrantDestination, withIntermediateDirectories: true)
        try Self.writePhoto(to: firstDestination.appendingPathComponent("a.jpg"), shade: 91)
        try Self.writePhoto(to: reentrantDestination.appendingPathComponent("c.jpg"), shade: 151)

        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        await waitUntil { model.develop != nil }
        let entered = expectation(description: "folder navigation close is held")
        let hold = fixture.closePlan.holdNext(entered: entered)
        var originalCallbacks: [(Bool, ViewMode)] = []
        var reentrantCallbacks: [Bool] = []

        model.openFolder(firstDestination) { model, loaded in
            originalCallbacks.append((loaded, model.viewMode))
            if !loaded {
                model.openFolder(reentrantDestination) { _, nestedLoaded in
                    reentrantCallbacks.append(nestedLoaded)
                }
            }
        }
        await fulfillment(of: [entered], timeout: 5)
        model.requestLibraryViewMode(.grid)

        XCTAssertEqual(originalCallbacks.count, 1)
        XCTAssertFalse(try XCTUnwrap(originalCallbacks.first).0)
        XCTAssertEqual(try XCTUnwrap(originalCallbacks.first).1, .loupe,
                       "The superseded folder callback settles before the pending mode commits")
        hold.release()
        await waitUntil { !model.isLoading && model.library.items.first?.name == "c.jpg" }
        XCTAssertEqual(originalCallbacks.count, 1)
        XCTAssertEqual(reentrantCallbacks, [true],
                       "The callback's reentrant folder request should become the latest destination")
        XCTAssertFalse(model.photoEditing)
    }

    func testLayeredCompletionKeepsSaveGateAndStaleResultCannotStealNavigation() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let item = try XCTUnwrap(fixture.library.items.first)
        model.enterPhotoEdit()
        model.openDevelop(for: item)
        await waitUntil { model.develop != nil }

        let backendReady = expectation(description: "layered backend opened but installation is held")
        let heldLoad = HeldLayerLoad()
        model.documents.documentLoadExecutor = { engine, body, receive in
            do {
                let backend = try body(engine)
                MainActor.assumeIsolated {
                    heldLoad.backend = backend
                    heldLoad.finish = { receive(.success(backend)) }
                }
                backendReady.fulfill()
            } catch {
                MainActor.assumeIsolated { heldLoad.failure = error }
                backendReady.fulfill()
            }
        }
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        await fulfillment(of: [backendReady], timeout: 8)

        XCTAssertEqual(model.viewMode, .loupe,
                       "The document destination stays unpublished until the backend settles")
        model.openDevelop(for: item)
        XCTAssertEqual(fixture.openerCount(), 1,
                       "The source photo remains reserved while layered installation is outstanding")
        model.select(position: 1)
        let changedSelection = model.selection
        if let failure = heldLoad.failure { XCTFail("Layered backend fixture failed: \(failure)") }
        try XCTUnwrap(heldLoad.finish)()
        await waitUntil { heldLoad.backend != nil && model.layeredCopyRequest == nil }
        await Task.yield()

        XCTAssertEqual(model.selection, changedSelection)
        XCTAssertNotEqual(model.viewMode, .document,
                          "A late installation must not replace a newer selection destination")
        XCTAssertNotNil(heldLoad.backend)
        if let document = model.documents.current { model.documents.close(document) }
    }

    func testPresentingExportSheetDoesNotCloseOrReserveDevelopPixels() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.setSelectionFromUI([0, 1], clicked: 0)
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        await waitUntil { model.develop != nil }
        let controller = try XCTUnwrap(model.develop)
        controller.set(.exposure, 0.25, interactive: true)

        model.presentExport()

        XCTAssertTrue(model.showExport)
        XCTAssertTrue(model.photoEditing)
        XCTAssertTrue(model.develop === controller)
        XCTAssertFalse(model.developRecovery.hasActiveReservations,
                       "Preparing target IDs for the sheet does not read image pixels")
        XCTAssertEqual(fixture.closePlan.closeCount, 0)
        model.showExport = false
        if let close = model.closeDevelop() { _ = await close.value }
    }

    private func makeFixture(photoCount: Int) throws -> Fixture {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("develop-recovery-appmodel-\(UUID().uuidString)")
        let folder = root.appendingPathComponent("photos")
        let support = root.appendingPathComponent("support")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for index in 0..<photoCount {
            try Self.writePhoto(to: folder.appendingPathComponent("photo-\(index).jpg"),
                                shade: UInt8(70 + index * 50))
        }
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let closePlan = ClosePlan()
        var opens = 0
        let model = AppModel(
            agent: AgentController(arguments: ["--fake-planner"], supportDirectory: support),
            developControllerOpener: { reference, itemID in
                opens += 1
                let session = try reference.engine.openDevelopSession(imageId: reference.imageID)
                return try DevelopController(
                    session: ClosePlanSession(session, plan: closePlan),
                    itemID: itemID, imageID: reference.imageID)
            })
        model.install(library)
        return Fixture(root: root, library: library, model: model, closePlan: closePlan,
                       openerCount: { opens })
    }

    private func waitUntil(_ condition: @MainActor () -> Bool,
                           file: StaticString = #filePath, line: UInt = #line) async {
        for _ in 0..<2_000 {
            if condition() { return }
            await Task.yield()
        }
        XCTFail("Condition did not settle", file: file, line: line)
    }

    private static func writePhoto(to url: URL, shade: UInt8) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        let width = 64, height = 48
        var pixels = [UInt8](repeating: 255, count: width * height * 4)
        for offset in stride(from: 0, to: pixels.count, by: 4) {
            pixels[offset] = shade
            pixels[offset + 1] = shade
            pixels[offset + 2] = shade
        }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }
}

@MainActor
private final class HeldLayerLoad {
    var backend: (any DocumentBackend)?
    var finish: (@MainActor () -> Void)?
    var failure: Error?
}

private final class ClosePlan: @unchecked Sendable {
    struct Hold {
        let semaphore: DispatchSemaphore
        let entered: XCTestExpectation
        func release() { semaphore.signal() }
    }

    private let lock = NSLock()
    private var queuedFailures = 0
    private var failureCountStorage = 0
    private var nextHold: Hold?
    private var closeCountStorage = 0
    var failureCount: Int { lock.withLock { failureCountStorage } }
    var closeCount: Int { lock.withLock { closeCountStorage } }

    func failNextClose() { lock.withLock { queuedFailures += 1 } }

    func holdNext(entered: XCTestExpectation) -> Hold {
        let hold = Hold(semaphore: DispatchSemaphore(value: 0), entered: entered)
        lock.withLock { nextHold = hold }
        return hold
    }

    func closeWillFailOrWait() throws {
        let (hold, shouldFail) = lock.withLock { () -> (Hold?, Bool) in
            closeCountStorage += 1
            let hold = nextHold
            nextHold = nil
            let shouldFail = queuedFailures > 0
            if shouldFail { queuedFailures -= 1; failureCountStorage += 1 }
            return (hold, shouldFail)
        }
        if let hold {
            hold.entered.fulfill()
            guard hold.semaphore.wait(timeout: .now() + 10) == .success else {
                throw CloseFixtureTimedOut()
            }
        }
        if shouldFail { throw InjectedDevelopCloseFailure() }
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

    required init(unsafeFromHandle: UInt64) { fatalError("Use wrapped session initializer") }
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
        try plan.closeWillFailOrWait()
        try wrapped.close()
    }
}

private struct InjectedDevelopCloseFailure: LocalizedError {
    var errorDescription: String? { "injected Develop close failure" }
}

private struct CloseFixtureTimedOut: LocalizedError {
    var errorDescription: String? { "test close gate did not receive release" }
}
