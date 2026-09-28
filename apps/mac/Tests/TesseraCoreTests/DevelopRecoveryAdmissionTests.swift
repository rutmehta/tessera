import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Physical-source admission coverage for coordinator reservations.
/// These tests stay separate from the AppModel navigation tests owned by Resource.
@MainActor
final class DevelopRecoveryAdmissionTests: XCTestCase {
    private struct Fixture {
        let root: URL
        let first: EngineLibrary
        let samePath: EngineLibrary
        let hardLink: EngineLibrary
        let unrelated: EngineLibrary
        let firstImageID: String
        let hardLinkImageID: String
        let unrelatedImageID: String
    }

    func testReservationsCoverSamePathAndHardLinkAliasesButNotUnrelatedSources() async throws {
        let fixture = try fixture()
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let coordinator = DevelopRecoveryCoordinator()
        let gate = coordinator.reserveInitiate(owner: fixture.first, imageIDs: [fixture.firstImageID])
        defer { gate.finish() }

        XCTAssertFalse(coordinator.canOpen(owner: fixture.first, imageID: fixture.firstImageID))
        XCTAssertFalse(coordinator.canOpen(owner: fixture.samePath, imageID: fixture.firstImageID),
                       "A second library instance for the same path shares the source reservation")
        XCTAssertFalse(coordinator.canOpen(owner: fixture.hardLink, imageID: fixture.hardLinkImageID),
                       "A hard link names the same physical source")
        XCTAssertFalse(coordinator.isUnreservedForHostMutation(owner: fixture.hardLink,
                                                               imageID: fixture.hardLinkImageID))
        XCTAssertTrue(coordinator.canOpen(owner: fixture.unrelated,
                                          imageID: fixture.unrelatedImageID))

        guard case .saved = await gate.result() else {
            return XCTFail("An empty source reservation should settle without blocking")
        }
        XCTAssertTrue(coordinator.hasActiveReservations)
        XCTAssertFalse(coordinator.isUnreservedForHostMutation(owner: fixture.samePath,
                                                               imageID: fixture.firstImageID),
                       "Result settlement must not release the reservation before finish")
        gate.finish()

        XCTAssertFalse(coordinator.hasActiveReservations)
        XCTAssertTrue(coordinator.canOpen(owner: fixture.hardLink, imageID: fixture.hardLinkImageID))
        XCTAssertTrue(coordinator.isUnreservedForHostMutation(owner: fixture.samePath,
                                                               imageID: fixture.firstImageID))
    }

    func testLaterConflictingAliasGateWaitsForEarlierReservationBeforeClosing() async throws {
        let fixture = try fixture()
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let coordinator = DevelopRecoveryCoordinator()
        let closeCounter = CloseCounter()
        let ref = try XCTUnwrap(fixture.first.itemOfImage[fixture.firstImageID]
            .flatMap { fixture.first.items.indices.contains($0) ? fixture.first.items[$0].engineImage : nil })
        let session = CloseCountingSession(
            try ref.engine.openDevelopSession(imageId: ref.imageID), counter: closeCounter)
        let controller = try DevelopController(session: session, itemID: fixture.first.items[0].id,
                                               imageID: fixture.firstImageID)
        _ = coordinator.register(owner: fixture.first, controller: controller, displayName: "photo.jpg")

        let earlier = coordinator.reserveObserve(owner: fixture.first, imageIDs: [fixture.firstImageID])
        let later = coordinator.reserveInitiate(owner: fixture.hardLink, imageIDs: [fixture.hardLinkImageID])
        defer {
            earlier.finish()
            later.finish()
        }
        guard case .blocked(let blockers) = await later.result() else {
            return XCTFail("A later physical-alias gate must fail closed behind the earlier reservation")
        }
        XCTAssertTrue(blockers.isEmpty, "The earlier gate owns the work; no close attempt belongs to the later gate")
        XCTAssertEqual(closeCounter.count, 0, "The blocked gate must not begin closing the aliased session")

        guard case .saved = await earlier.result() else {
            return XCTFail("The earlier observation gate should settle")
        }
        XCTAssertEqual(closeCounter.count, 0, "An observe-only gate must not close Develop")
        earlier.finish()

        guard case .saved = await later.result() else {
            return XCTFail("The later gate should proceed after the earlier reservation is finished")
        }
        XCTAssertEqual(closeCounter.count, 1, "After admission opens, the later initiate gate saves the aliased session")
        later.finish()
        XCTAssertFalse(coordinator.hasUnresolvedSessions)
    }

    private func fixture() throws -> Fixture {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("develop-recovery-alias-\(UUID().uuidString)")
        let firstFolder = root.appendingPathComponent("first")
        let hardLinkFolder = root.appendingPathComponent("hard-link")
        let unrelatedFolder = root.appendingPathComponent("unrelated")
        try Self.writePhoto(to: firstFolder.appendingPathComponent("photo.jpg"), shade: 80)
        try FileManager.default.createDirectory(at: hardLinkFolder, withIntermediateDirectories: true)
        try FileManager.default.linkItem(at: firstFolder.appendingPathComponent("photo.jpg"),
                                         to: hardLinkFolder.appendingPathComponent("alias.jpg"))
        try Self.writePhoto(to: unrelatedFolder.appendingPathComponent("other.jpg"), shade: 140)

        let first = try EngineLibrary.scan(folder: firstFolder, appSupport: root.appendingPathComponent("support-a"))
        let samePath = try EngineLibrary.scan(folder: firstFolder, appSupport: root.appendingPathComponent("support-b"))
        let hardLink = try EngineLibrary.scan(folder: hardLinkFolder, appSupport: root.appendingPathComponent("support-c"))
        let unrelated = try EngineLibrary.scan(folder: unrelatedFolder,
                                               appSupport: root.appendingPathComponent("support-d"))
        let firstImageID = try XCTUnwrap(first.items.first?.engineImage?.imageID)
        let hardLinkImageID = try XCTUnwrap(hardLink.items.first?.engineImage?.imageID)
        let unrelatedImageID = try XCTUnwrap(unrelated.items.first?.engineImage?.imageID)
        return Fixture(root: root, first: first, samePath: samePath, hardLink: hardLink,
                       unrelated: unrelated, firstImageID: firstImageID,
                       hardLinkImageID: hardLinkImageID, unrelatedImageID: unrelatedImageID)
    }

    private static func writePhoto(to url: URL, shade: UInt8) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        let width = 64, height = 48
        let pixels = [UInt8](repeating: shade, count: width * height * 4)
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

private final class CloseCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var closes = 0
    var count: Int { lock.withLock { closes } }
    func recordClose() { lock.withLock { closes += 1 } }
}

private final class CloseCountingSession: DevelopSession, @unchecked Sendable {
    private let wrapped: DevelopSession
    private let counter: CloseCounter

    init(_ wrapped: DevelopSession, counter: CloseCounter) {
        self.wrapped = wrapped
        self.counter = counter
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
        counter.recordClose()
        try wrapped.close()
    }
}
