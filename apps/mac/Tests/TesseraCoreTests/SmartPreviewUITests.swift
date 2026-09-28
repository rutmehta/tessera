import XCTest
@testable import TesseraCore

/// Source-only handoff: UNRUN on Machine B. Native/storage and GUI gates belong to A.
@MainActor
final class SmartPreviewUITests: XCTestCase {
    private func info(_ id: String = "raw", _ state: SmartPreviewSnapshot.State = .ready,
                      dirty: Bool = false, online: Bool = true) -> SmartPreviewSnapshot {
        .init(imageID: id, state: state, originalAvailable: online, dirty: dirty,
              width: 2560, height: 1707, message: "")
    }

    func testRouteUsesRequestedSessionAndNeverFallsBackAfterProxyFailure() async throws {
        var calls: [DevelopSourceRoute] = []
        let value: String = try await SmartPreviewRouting.open(info(), preferPreview: true) { route in
            calls.append(route); return "proxy session"
        }
        XCTAssertEqual(value, "proxy session")
        XCTAssertEqual(calls, [.smartPreview])
        calls.removeAll()
        do {
            let _: String = try await SmartPreviewRouting.open(info(), preferPreview: true) { route in
                calls.append(route); throw ProbeError.failed
            }
            XCTFail("Failed native preview open must remain an error")
        } catch {}
        XCTAssertEqual(calls, [.smartPreview])
        let _: String = try await SmartPreviewRouting.open(info(), preferPreview: false) { route in
            calls.append(route); return "original"
        }
        XCTAssertEqual(calls.last, .original)
    }

    func testOnlyCleanMissingPreviewFallsBackToAvailableOriginal() throws {
        XCTAssertEqual(try SmartPreviewRouting.route(info("raw", .missing), preferPreview: true), .original)
        for state in [SmartPreviewSnapshot.State.conflict, .stale, .failed] {
            XCTAssertThrowsError(try SmartPreviewRouting.route(info("raw", state), preferPreview: true))
            XCTAssertThrowsError(try SmartPreviewRouting.route(info("raw", state), preferPreview: false))
        }
        XCTAssertThrowsError(try SmartPreviewRouting.route(info(dirty: true), preferPreview: false))
        XCTAssertThrowsError(try SmartPreviewRouting.route(info("raw", .missing, online: false), preferPreview: true))
        XCTAssertEqual(try SmartPreviewRouting.route(info("raw", .originalOffline, dirty: true, online: false), preferPreview: true), .smartPreview)
    }

    func testBadgesDescribeOfflinePendingConflictAndActualRoute() {
        XCTAssertEqual(info("raw", .originalOffline, dirty: true, online: false).badge, "Smart Preview · Original offline · Pending edits")
        XCTAssertTrue(info("raw", .conflict).badge.contains("Conflict"))
        XCTAssertTrue(info("raw", .stale).badge.contains("Stale"))
        XCTAssertEqual(DevelopSourceRoute.original.label, "Original")
        XCTAssertEqual(DevelopSourceRoute.smartPreview.label, "Smart Preview")
    }

    func testSelectionGenerationSuppressesOldInfoCompletion() async {
        let probe = PreviewProbe()
        let controller = SmartPreviewController(api: probe.api)
        let first = controller.select(imageID: "old")!
        await probe.waitForRead()
        let second = controller.select(imageID: "new")!
        probe.finishRead(info("old", .conflict))
        await first.value
        await second.value
        XCTAssertEqual(controller.selectedImageID, "new")
        XCTAssertEqual(controller.selectedInfo?.imageID, "new")
        XCTAssertNil(controller.snapshots["old"])
    }

    func testSelectionReadStartedBeforeBatchCannotOverwriteNewOutcome() async {
        let probe = PreviewProbe()
        let controller = SmartPreviewController(api: probe.api)
        let oldRead = controller.select(imageID: "old")!
        await probe.waitForRead()
        let batch = Task { await controller.run(.build, targets: [.init(id: "old", name: "Old")]) }
        probe.finishRead(info("old", .missing))
        await oldRead.value
        await probe.waitForBuild()
        probe.finishBuild(info("old", .dirty, dirty: true))
        await batch.value
        XCTAssertFalse(probe.buildOverlappedRead)
        XCTAssertEqual(controller.selectedInfo?.state, .dirty)
        XCTAssertTrue(controller.selectedInfo?.hasPendingEdits == true)
    }

    func testClearedSelectionAndSeparateOwnerIgnoreOldCompletion() async {
        let probe = PreviewProbe()
        let oldOwner = SmartPreviewController(api: probe.api)
        let read = oldOwner.select(imageID: "old")!
        await probe.waitForRead()
        oldOwner.select(imageID: nil)
        let newOwner = SmartPreviewController(api: probe.api)
        await newOwner.select(imageID: "new")?.value
        probe.finishRead(info("old", .conflict))
        await read.value
        XCTAssertNil(oldOwner.selectedInfo)
        XCTAssertNil(oldOwner.snapshots["old"])
        XCTAssertEqual(newOwner.selectedInfo?.imageID, "new")
    }

    func testRepeatedSelectionAndOpeningShareOneStatusRead() async throws {
        let probe = PreviewProbe()
        let controller = SmartPreviewController(api: probe.api)
        let first = controller.select(imageID: "old")!
        await probe.waitForRead()
        let repeatSelection = controller.select(imageID: "old")
        let opening = Task { try await controller.statusForOpening(imageID: "old") }
        probe.finishRead(info("old"))
        await first.value
        await repeatSelection?.value
        let snapshot = try await opening.value
        XCTAssertEqual(snapshot.imageID, "old")
        await controller.select(imageID: "old")?.value
        XCTAssertEqual(probe.reads, ["old"])
    }

    func testRapidSelectionOnlyDecodesActiveAndLatestPhoto() async {
        let probe = PreviewProbe()
        let controller = SmartPreviewController(api: probe.api)
        let active = controller.select(imageID: "old")!
        await probe.waitForRead()
        let skipped = controller.select(imageID: "middle")!
        let latest = controller.select(imageID: "new")!
        probe.finishRead(info("old"))
        await active.value
        await skipped.value
        await latest.value
        XCTAssertEqual(probe.reads, ["old", "new"])
        XCTAssertEqual(controller.selectedInfo?.imageID, "new")
    }

    func testSavedStateInvalidationDoesNotAutomaticallyHashAssets() async throws {
        let probe = PreviewProbe()
        let controller = SmartPreviewController(api: probe.api)
        await controller.select(imageID: "new")?.value
        controller.invalidateStatus(imageID: "new")
        XCTAssertNil(controller.selectedInfo)
        XCTAssertEqual(probe.reads, ["new"])
        let snapshot = try await controller.statusForOpening(imageID: "new")
        XCTAssertEqual(snapshot.imageID, "new")
        XCTAssertEqual(probe.reads, ["new", "new"])
        await controller.select(imageID: "new", refresh: true)?.value
        XCTAssertEqual(probe.reads, ["new", "new", "new"])
    }

    func testStaleOpenerCannotRetargetCurrentSelectionOrStartAnotherRead() async {
        let probe = PreviewProbe()
        let controller = SmartPreviewController(api: probe.api)
        await controller.select(imageID: "new")?.value
        do {
            _ = try await controller.statusForOpening(imageID: "old")
            XCTFail("A stale opener must not change the current selection")
        } catch is CancellationError {} catch { XCTFail("Unexpected error: \(error)") }
        XCTAssertEqual(controller.selectedImageID, "new")
        XCTAssertEqual(probe.reads, ["new"])
    }

    func testOfflineAndPendingLibraryBadgesDisclosePreSyncThumbnail() {
        let offline = info("raw", .originalOffline, online: false)
        let dirty = info("raw", .dirty, dirty: true)
        XCTAssertTrue(offline.libraryBadge.contains("Thumbnail: last synchronized image"))
        XCTAssertTrue(dirty.libraryBadge.contains("Thumbnail: last synchronized image"))
        XCTAssertFalse(info().libraryBadge.contains("Thumbnail:"))
        XCTAssertTrue(SmartPreviewSnapshot.libraryThumbnailNotice.contains("only after Sync"))
    }

    func testBatchCancelDrainsCurrentPhotoThenStopsAndRetainsOutcome() async {
        let probe = PreviewProbe()
        let controller = SmartPreviewController(api: probe.api)
        let task = Task { await controller.run(.build, targets: [.init(id: "a", name: "A"), .init(id: "b", name: "B")]) }
        await probe.waitForBuild()
        controller.cancel()
        XCTAssertTrue(controller.isRunning)
        XCTAssertTrue(controller.progressLabel.contains("current photo"))
        probe.finishBuild(info("a"))
        await task.value
        XCTAssertEqual(probe.builds, ["a"])
        XCTAssertEqual(controller.results.map(\.succeeded), [true, false])
        XCTAssertEqual(controller.results.last?.message, "Cancelled before starting")
        XCTAssertFalse(controller.isRunning)
    }

    func testConflictFailedAndDirtySyncAreNotSuccessAndDirtyDiscardNeverCallsNative() async {
        let conflict = info("a", .conflict)
        let dirty = info("a", .dirty, dirty: true)
        var discardCalls = 0
        let controller = SmartPreviewController(api: .init(
            info: { _ in dirty }, build: { _ in conflict },
            discard: { _ in discardCalls += 1 }, synchronize: { _ in dirty }))
        let targets = [SmartPreviewTarget(id: "a", name: "A")]
        await controller.run(.build, targets: targets)
        XCTAssertFalse(controller.results[0].succeeded)
        await controller.run(.synchronize, targets: targets)
        XCTAssertFalse(controller.results[0].succeeded)
        await controller.run(.discard, targets: targets)
        XCTAssertEqual(discardCalls, 0)
        XCTAssertFalse(controller.results[0].succeeded)
        let failed = SmartPreviewController(api: .init(info: { _ in throw ProbeError.failed },
            build: { _ in throw ProbeError.failed }, discard: { _ in }, synchronize: { _ in throw ProbeError.failed }))
        await failed.run(.build, targets: targets)
        XCTAssertFalse(failed.results[0].succeeded)
    }
}

private enum ProbeError: Error { case failed }

@MainActor
private final class PreviewProbe {
    var builds: [String] = []
    var reads: [String] = []
    private var readActive = false
    private(set) var buildOverlappedRead = false
    private var read: CheckedContinuation<SmartPreviewSnapshot, Never>?
    private var build: CheckedContinuation<SmartPreviewSnapshot, Never>?
    private var readEntered = false
    private var buildEntered = false
    private var readWaiter: CheckedContinuation<Void, Never>?
    private var buildWaiter: CheckedContinuation<Void, Never>?
    var api: SmartPreviewAPI {
        .init(info: { [self] id in
            reads.append(id)
            readActive = true
            defer { readActive = false }
            if id != "old" { return snapshot(id) }
            return await withCheckedContinuation { read = $0; readEntered = true; readWaiter?.resume(); readWaiter = nil }
        }, build: { [self] id in
            builds.append(id)
            buildOverlappedRead = buildOverlappedRead || readActive
            return await withCheckedContinuation { build = $0; buildEntered = true; buildWaiter?.resume(); buildWaiter = nil }
        }, discard: { _ in }, synchronize: { [self] id in snapshot(id) })
    }
    func snapshot(_ id: String) -> SmartPreviewSnapshot {
        .init(imageID: id, state: .ready, originalAvailable: true, dirty: false, width: 2560, height: 1707, message: "")
    }
    func waitForRead() async { if !readEntered { await withCheckedContinuation { readWaiter = $0 } } }
    func waitForBuild() async { if !buildEntered { await withCheckedContinuation { buildWaiter = $0 } } }
    func finishRead(_ info: SmartPreviewSnapshot) { read?.resume(returning: info); read = nil }
    func finishBuild(_ info: SmartPreviewSnapshot) { build?.resume(returning: info); build = nil }
}
