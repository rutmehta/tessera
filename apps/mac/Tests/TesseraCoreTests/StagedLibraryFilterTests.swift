import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Filter requests share one pending Develop save without publishing partial search state.
@MainActor
final class StagedLibraryFilterTests: XCTestCase {
    private struct Fixture {
        let root: URL
        let first: EngineLibrary
        let second: EngineLibrary
        let model: AppModel
        let saves: FilterSavePlan
        let personIDs: [String]
    }

    func testHeldCloseComposesTwoFilterFieldsWithoutEarlyPublication() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        let originalVisible = f.model.visible
        let entered = expectation(description: "filter close entered")
        let hold = f.saves.holdNext(entered: entered)
        defer { hold.release() }

        f.model.updateLibraryFilter { $0.grades.insert("2") }
        await fulfillment(of: [entered], timeout: 5)
        f.model.updateLibraryFilter { $0.cameras.insert("Camera A") }
        XCTAssertEqual(f.model.collections.filter, LibraryFilter())
        XCTAssertTrue(f.model.people.facet.isEmpty)
        XCTAssertEqual(f.model.visible, originalVisible)

        hold.release()
        guard await waitUntil({ f.model.develop == nil && !f.model.developRecovery.hasActiveReservations
            && f.model.collections.filter.grades == ["2"]
            && f.model.collections.filter.cameras == ["Camera A"] }) else { return }
        XCTAssertEqual(f.saves.closeCount, 1)
    }

    func testHeldCloseComposesDistinctPersonFacets() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        let entered = expectation(description: "person facet close entered")
        let hold = f.saves.holdNext(entered: entered)
        defer { hold.release() }

        f.model.togglePersonFacet(f.personIDs[0])
        await fulfillment(of: [entered], timeout: 5)
        f.model.togglePersonFacet(f.personIDs[1])
        XCTAssertTrue(f.model.people.facet.isEmpty)
        hold.release()
        guard await waitUntil({ f.model.develop == nil && !f.model.developRecovery.hasActiveReservations
            && f.model.people.facet == Set(f.personIDs) }) else { return }
        XCTAssertEqual(f.saves.closeCount, 1)
    }

    func testHeldCloseComposesRepeatedTextAndCancellingPersonToggle() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        let entered = expectation(description: "repeated filter close entered")
        let hold = f.saves.holdNext(entered: entered)
        defer { hold.release() }

        f.model.updateLibraryFilter { $0.text = "first" }
        await fulfillment(of: [entered], timeout: 5)
        f.model.updateLibraryFilter { $0.text = "second" }
        f.model.togglePersonFacet(f.personIDs[0])
        f.model.togglePersonFacet(f.personIDs[0])
        XCTAssertEqual(f.model.collections.filter.text, "")
        XCTAssertTrue(f.model.people.facet.isEmpty)

        hold.release()
        guard await waitUntil({ f.model.develop == nil && !f.model.developRecovery.hasActiveReservations
            && f.model.collections.filter.text == "second" }) else { return }
        XCTAssertTrue(f.model.people.facet.isEmpty)
        XCTAssertEqual(f.saves.closeCount, 1)
    }

    func testKeepEditingDiscardsFailedFilterDraft() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        f.saves.failNextClose()
        f.model.updateLibraryFilter { $0.grades.insert("2") }
        guard await waitUntil({ !f.model.developRecovery.hasActiveReservations
            && f.model.developRecoveries.contains { if case .failed = $0.phase { return true }; return false } }) else { return }
        XCTAssertEqual(f.saves.failureCount, 1)
        XCTAssertEqual(f.model.collections.filter, LibraryFilter())
        let sessionID = try XCTUnwrap(f.model.developRecoveries.first?.id)

        f.model.keepEditingDevelopRecovery()
        f.model.retryDevelopRecovery(sessionID)
        guard await waitUntil({ f.model.develop == nil && f.model.developRecoveries.isEmpty }) else { return }
        XCTAssertEqual(f.model.collections.filter, LibraryFilter(),
                       "Keep Editing must not replay the abandoned filter request")
        XCTAssertTrue(f.model.people.facet.isEmpty)
    }

    func testExplicitRetryCommitsAllEditsStagedAfterFailedClose() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        f.saves.failNextClose()
        f.model.updateLibraryFilter { $0.grades.insert("2") }
        guard await waitUntil({ !f.model.developRecovery.hasActiveReservations
            && f.model.developRecoveries.contains { if case .failed = $0.phase { return true }; return false } }) else { return }
        XCTAssertEqual(f.saves.failureCount, 1)
        f.model.updateLibraryFilter { $0.cameras.insert("Camera A") }
        f.model.togglePersonFacet(f.personIDs[0])
        XCTAssertEqual(f.model.collections.filter, LibraryFilter())
        XCTAssertTrue(f.model.people.facet.isEmpty)
        guard await waitUntil({ !f.model.developRecovery.hasActiveReservations }) else { return }

        let sessionID = try XCTUnwrap(f.model.developRecoveries.first?.id)
        f.model.retryDevelopRecovery(sessionID)
        guard await waitUntil({ f.model.develop == nil && f.model.developRecoveries.isEmpty
            && f.model.collections.filter.grades == ["2"]
            && f.model.collections.filter.cameras == ["Camera A"]
            && f.model.people.facet == [f.personIDs[0]] }) else { return }
        XCTAssertEqual(f.saves.closeCount, 2)
    }

    func testUnrelatedNavigationSupersedesHeldFilterDraft() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        let entered = expectation(description: "superseded filter close entered")
        let hold = f.saves.holdNext(entered: entered)
        defer { hold.release() }

        f.model.updateLibraryFilter { $0.grades.insert("2") }
        await fulfillment(of: [entered], timeout: 5)
        f.model.togglePersonFacet(f.personIDs[0])
        f.model.requestLibraryViewMode(.grid)
        hold.release()
        guard await waitUntil({ f.model.develop == nil && !f.model.developRecovery.hasActiveReservations
            && f.model.viewMode == .grid }) else { return }
        XCTAssertEqual(f.model.collections.filter, LibraryFilter())
        XCTAssertTrue(f.model.people.facet.isEmpty)
    }

    func testOwnerReplacementDropsHeldFilterDraft() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        let entered = expectation(description: "old owner filter close entered")
        let hold = f.saves.holdNext(entered: entered)
        defer { hold.release() }

        f.model.updateLibraryFilter { $0.grades.insert("2") }
        await fulfillment(of: [entered], timeout: 5)
        f.model.togglePersonFacet(f.personIDs[0])
        f.model.install(f.second)
        XCTAssertTrue(f.model.engineLibrary === f.second)
        hold.release()
        guard await waitUntil({ !f.model.developRecovery.hasActiveReservations }) else { return }
        XCTAssertEqual(f.model.collections.filter, LibraryFilter())
        XCTAssertTrue(f.model.people.facet.isEmpty)
    }

    func testFilterAndFacetRequestDuringLayersSaveCancelOnlyItsDestination() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        let entered = expectation(description: "Layers preparation entered Develop close")
        let hold = f.saves.holdNext(entered: entered)
        defer { hold.release() }
        var backendStarts = 0
        f.model.documents.documentLoadExecutor = { _, _, done in
            backendStarts += 1
            done(.failure(FilterSaveFailed()))
        }

        f.model.requestLayeredCopy()
        XCTAssertNotNil(f.model.layeredCopyRequest)
        f.model.createRequestedLayeredCopy()
        await fulfillment(of: [entered], timeout: 5)
        f.model.updateLibraryFilter { $0.grades.insert("2") }
        f.model.togglePersonFacet(f.personIDs[0])
        XCTAssertEqual(f.model.collections.filter, LibraryFilter())
        XCTAssertTrue(f.model.people.facet.isEmpty)

        hold.release()
        guard await waitUntil({ !f.model.developRecovery.hasActiveReservations
            && f.model.develop == nil }) else { return }
        XCTAssertEqual(backendStarts, 0, "the stale Layers destination must not dispatch")
        XCTAssertNotEqual(f.model.viewMode, .document)
        XCTAssertEqual(f.saves.closeCount, 1)
    }

    func testPersonFacetAloneDuringLayersSaveCancelsItsDestination() async throws {
        let f = try fixture()
        defer { try? FileManager.default.removeItem(at: f.root) }
        guard await openDevelop(f) else { return }
        let entered = expectation(description: "Layers preparation entered Develop close")
        let hold = f.saves.holdNext(entered: entered)
        defer { hold.release() }
        var backendStarts = 0
        f.model.documents.documentLoadExecutor = { _, _, done in
            backendStarts += 1
            done(.failure(FilterSaveFailed()))
        }

        f.model.requestLayeredCopy()
        XCTAssertNotNil(f.model.layeredCopyRequest)
        f.model.createRequestedLayeredCopy()
        await fulfillment(of: [entered], timeout: 5)
        f.model.togglePersonFacet(f.personIDs[0])
        XCTAssertTrue(f.model.people.facet.isEmpty)

        hold.release()
        guard await waitUntil({ !f.model.developRecovery.hasActiveReservations
            && f.model.develop == nil }) else { return }
        XCTAssertEqual(backendStarts, 0)
        XCTAssertNotEqual(f.model.viewMode, .document)
        XCTAssertEqual(f.saves.closeCount, 1)
    }

    private func openDevelop(_ f: Fixture) async -> Bool {
        f.model.requestViewMode(.loupe)
        guard let item = f.first.items.first else { XCTFail("Missing indexed photo"); return false }
        f.model.openDevelop(for: item)
        return await waitUntil { f.model.develop != nil && f.model.developStatus == .ready }
    }

    private func fixture() throws -> Fixture {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("staged-filter-\(UUID().uuidString)")
        let firstFolder = root.appendingPathComponent("photos-a")
        let secondFolder = root.appendingPathComponent("photos-b")
        let support = root.appendingPathComponent("support")
        try Self.writePhoto(to: firstFolder.appendingPathComponent("photo-a.jpg"))
        try Self.writePhoto(to: firstFolder.appendingPathComponent("photo-b.jpg"))
        try Self.writePhoto(to: secondFolder.appendingPathComponent("photo-b.jpg"))
        let first = try EngineLibrary.scan(folder: firstFolder, appSupport: support)
        let second = try EngineLibrary.scan(folder: secondFolder, appSupport: support)
        try Self.seedTwoPeople(in: first)
        let saves = FilterSavePlan()
        let model = AppModel(
            agent: AgentController(arguments: ["--fake-planner"], supportDirectory: support),
            developControllerOpener: { reference, itemID in
                let session = try reference.engine.openDevelopSession(imageId: reference.imageID)
                return try DevelopController(session: FilterSaveSession(session, plan: saves),
                                             itemID: itemID, imageID: reference.imageID)
            })
        model.install(first)
        model.people.reload(refresh: true)
        let personIDs = Array(Set(model.people.tiles.map(\.id))).sorted()
        XCTAssertGreaterThanOrEqual(personIDs.count, 2, "Two distinct real identities are needed for facet assertions")
        guard personIDs.count >= 2 else { throw FilterFixtureMissingPeople() }
        return Fixture(root: root, first: first, second: second, model: model,
                       saves: saves, personIDs: Array(personIDs.prefix(2)))
    }

    private func waitUntil(_ condition: @MainActor () -> Bool,
                           file: StaticString = #filePath, line: UInt = #line) async -> Bool {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(8))
        while clock.now < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(5))
        }
        XCTFail("Condition did not settle", file: file, line: line)
        return false
    }

    private static func writePhoto(to url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        let width = 32, height = 24
        let pixels = Data(repeating: 127, count: width * height * 4)
        let provider = try XCTUnwrap(CGDataProvider(data: pixels as CFData))
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

    private static func seedTwoPeople(in library: EngineLibrary) throws {
        var a = [Float](repeating: 0.01, count: 128)
        var b = [Float](repeating: 0.01, count: 128)
        a[3] = 1
        b[40] = 1
        for imageID in library.imageIDs {
            try library.engine.setFaces(imageId: imageID, faces: [
                FaceInput(x: 2, y: 2, width: 10, height: 15, focus: 0.9,
                          eyesOpen: 0.9, embedding: a),
                FaceInput(x: 17, y: 2, width: 10, height: 15, focus: 0.9,
                          eyesOpen: 0.9, embedding: b),
            ], width: 32, height: 24)
        }
    }
}

private final class FilterSavePlan: @unchecked Sendable {
    struct Hold {
        let semaphore: DispatchSemaphore
        let entered: XCTestExpectation
        func release() { semaphore.signal() }
    }

    private let lock = NSLock()
    private var failuresRemaining = 0
    private var observedFailures = 0
    private var observedCloses = 0
    private var nextHold: Hold?
    var closeCount: Int { lock.withLock { observedCloses } }
    var failureCount: Int { lock.withLock { observedFailures } }

    func failNextClose() { lock.withLock { failuresRemaining += 1 } }
    func holdNext(entered: XCTestExpectation) -> Hold {
        let hold = Hold(semaphore: DispatchSemaphore(value: 0), entered: entered)
        lock.withLock { nextHold = hold }
        return hold
    }
    func close() throws {
        let (hold, shouldFail) = lock.withLock { () -> (Hold?, Bool) in
            observedCloses += 1
            let hold = nextHold
            nextHold = nil
            let shouldFail = failuresRemaining > 0
            if shouldFail { failuresRemaining -= 1; observedFailures += 1 }
            return (hold, shouldFail)
        }
        if let hold {
            hold.entered.fulfill()
            guard hold.semaphore.wait(timeout: .now() + 8) == .success else { throw FilterHoldTimedOut() }
        }
        if shouldFail { throw FilterSaveFailed() }
    }
}

private final class FilterSaveSession: DevelopSession, @unchecked Sendable {
    private let wrapped: DevelopSession
    private let plan: FilterSavePlan
    init(_ wrapped: DevelopSession, plan: FilterSavePlan) {
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
    override func setSettingsIdentified(jsonPatch: String, interactive: Bool, inputId: UInt64?) throws {
        try wrapped.setSettingsIdentified(jsonPatch: jsonPatch, interactive: interactive, inputId: inputId)
    }
    override func close() throws {
        try plan.close()
        try wrapped.close()
    }
}

private struct FilterSaveFailed: LocalizedError {
    var errorDescription: String? { "injected Develop close failure" }
}
private struct FilterHoldTimedOut: LocalizedError {
    var errorDescription: String? { "staged filter close hold timed out" }
}
private struct FilterFixtureMissingPeople: LocalizedError {
    var errorDescription: String? { "tiny fixture did not load both synthetic people" }
}
