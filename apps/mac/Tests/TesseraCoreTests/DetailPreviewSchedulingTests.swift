import XCTest
#if !DETAIL_SCHEDULER_STANDALONE
@testable import Tessera
#endif

final class DetailPreviewSchedulingTests: XCTestCase {
    func testEngineMutationRejectsDetailBeforeViewportCallback() throws {
        var schedule = DetailPreviewSchedule()
        schedule.observeSettings(revision: 1)
        let original = try XCTUnwrap(schedule.begin())
        XCTAssertFalse(schedule.complete(original, engineCurrent: false),
                       "a direct engine mask mutation must reject the old crop immediately")
        let refreshed = try XCTUnwrap(schedule.begin(), "stale engine output must request one refresh")
        XCTAssertNil(schedule.begin())
        XCTAssertTrue(schedule.complete(refreshed, engineCurrent: true))
        XCTAssertNil(schedule.begin())
    }

    func testViewportCompletionDoesNotSupersedeMatchingSettleWork() throws {
        var schedule = DetailPreviewSchedule()
        schedule.invalidate(interactive: true)
        schedule.observeSettings(revision: 1)
        schedule.invalidate(interactive: false)
        let settled = try XCTUnwrap(schedule.begin())
        schedule.observeSettings(revision: 1)
        XCTAssertNil(schedule.begin())
        XCTAssertTrue(schedule.complete(settled))
        schedule.observeSettings(revision: 1)
        XCTAssertNil(schedule.begin(), "a matching viewport completion is not a new edit")
        schedule.observeSettings(revision: 2)
        XCTAssertNotNil(schedule.begin(), "unwrapped edits must still refresh detail")
    }

    func testSettingsChangingBeforeCompletionRejectsOldDetail() throws {
        var schedule = DetailPreviewSchedule()
        schedule.observeSettings(revision: 1)
        let old = try XCTUnwrap(schedule.begin())
        schedule.observeSettings(revision: 2)
        XCTAssertFalse(schedule.complete(old))
        let current = try XCTUnwrap(schedule.begin())
        schedule.observeSettings(revision: 2)
        XCTAssertTrue(schedule.complete(current))
        XCTAssertNil(schedule.begin())
    }

#if !DETAIL_SCHEDULER_STANDALONE
    @MainActor func testModelNotifiesBeforeMutationAndSettlesAfterFinalSetter() async {
        let model = AppModel()
        let observer = InteractionObserver()
        model.addObserver(observer)
        model.withDevelopSettingsChange(final: false) {
            XCTAssertEqual(observer.events, [true])
        }
        XCTAssertEqual(observer.events, [true])
        model.withDevelopSettingsChange(final: true) {
            XCTAssertEqual(observer.events, [true, true])
        }
        XCTAssertEqual(observer.events, [true, true, false])
    }

    @MainActor private final class InteractionObserver: LibraryObserver {
        var events: [Bool] = []
        func libraryDidReload() {}
        func itemsDidChange(_ positions: IndexSet) {}
        func selectionDidChange(scrollToFocus: Bool) {}
        func developSettingsInteractionChanged(_ interactive: Bool) { events.append(interactive) }
    }
#endif

    func testCompletionDuringDragCannotPublishOrLaunchMoreWork() throws {
        var schedule = DetailPreviewSchedule()
        schedule.invalidate()
        let request = try XCTUnwrap(schedule.begin())
        schedule.invalidate(interactive: true)
        XCTAssertFalse(schedule.complete(request))
        XCTAssertNil(schedule.begin())
        schedule.invalidate(interactive: false)
        XCTAssertTrue(schedule.complete(try XCTUnwrap(schedule.begin())))
    }

    func testSessionChangeResetsGestureButRetainsSingleFlightOwnership() throws {
        var schedule = DetailPreviewSchedule()
        schedule.invalidate()
        let oldSession = try XCTUnwrap(schedule.begin())
        schedule.invalidate(interactive: true)
        // developDidChange uses this transition even when the old image was mid-gesture.
        schedule.invalidate(interactive: false)
        XCTAssertNil(schedule.begin())
        XCTAssertFalse(schedule.complete(oldSession))
        let newSession = try XCTUnwrap(schedule.begin())
        XCTAssertTrue(schedule.complete(newSession))
        XCTAssertNil(schedule.begin())
    }

    func testSurfaceOrVisibilityChangeRejectsInFlightResult() throws {
        var schedule = DetailPreviewSchedule()
        schedule.invalidate()
        let oldSurface = try XCTUnwrap(schedule.begin())
        schedule.invalidate()
        XCTAssertFalse(schedule.complete(oldSurface))
        XCTAssertTrue(schedule.complete(try XCTUnwrap(schedule.begin())))
    }

    func testInFlightWorkIsSupersededAndExactlyOneLatestRequestFollows() throws {
        var schedule = DetailPreviewSchedule()
        schedule.invalidate()
        let old = try XCTUnwrap(schedule.begin())
        for _ in 0..<100 {
            schedule.invalidate()
            XCTAssertNil(schedule.begin(), "at most one renderer owns the surface")
        }
        XCTAssertFalse(schedule.complete(old), "an old crop/settings/session cannot publish")
        let latest = try XCTUnwrap(schedule.begin())
        XCTAssertNotEqual(old, latest)
        XCTAssertFalse(schedule.complete(old), "duplicate completion must not clear the active job")
        schedule.invalidate()
        XCTAssertNil(schedule.begin())
        XCTAssertFalse(schedule.complete(latest))
        XCTAssertTrue(schedule.complete(try XCTUnwrap(schedule.begin())))
        XCTAssertNil(schedule.begin())
    }

    func testDragDefersEveryFrameAndSettleStartsWithoutAnotherFrame() throws {
        var schedule = DetailPreviewSchedule()
        for _ in 0..<100 {
            schedule.invalidate(interactive: true)
            XCTAssertNil(schedule.begin())
        }
        schedule.invalidate(interactive: false)
        let request = schedule.begin()
        XCTAssertNotNil(request, "mouse-up must not depend on a viewport callback")
        XCTAssertTrue(schedule.complete(try XCTUnwrap(request)))
        XCTAssertNil(schedule.begin())
    }
}