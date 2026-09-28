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
        await second.value
        probe.finishRead(info("old", .conflict))
        await first.value
        XCTAssertEqual(controller.selectedImageID, "new")
        XCTAssertEqual(controller.selectedInfo?.imageID, "new")
        XCTAssertNil(controller.snapshots["old"])
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
    private var read: CheckedContinuation<SmartPreviewSnapshot, Never>?
    private var build: CheckedContinuation<SmartPreviewSnapshot, Never>?
    private var readEntered = false
    private var buildEntered = false
    private var readWaiter: CheckedContinuation<Void, Never>?
    private var buildWaiter: CheckedContinuation<Void, Never>?
    var api: SmartPreviewAPI {
        .init(info: { [self] id in
            if id != "old" { return snapshot(id) }
            return await withCheckedContinuation { read = $0; readEntered = true; readWaiter?.resume(); readWaiter = nil }
        }, build: { [self] id in
            builds.append(id)
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
