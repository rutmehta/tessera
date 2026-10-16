import XCTest
@testable import Tessera

/// UX-15 (item 19): the Edits readout while a session opens.
final class DevelopLoadingReadoutTests: XCTestCase {
    func testRenderingAloneForTheFirstTwoSeconds() {
        let start = Date(timeIntervalSinceReferenceDate: 1_000)
        XCTAssertEqual(DevelopLoadingReadout.text(since: start, now: start), "Rendering…")
        XCTAssertEqual(DevelopLoadingReadout.text(since: start, now: start.addingTimeInterval(1.9)), "Rendering…")
    }

    func testReasonAndWholeSecondsAfterTwoSeconds() {
        let start = Date(timeIntervalSinceReferenceDate: 1_000)
        XCTAssertEqual(DevelopLoadingReadout.text(since: start, now: start.addingTimeInterval(2)), "Rendering… · Waiting for the engine · 2 s")
        XCTAssertEqual(DevelopLoadingReadout.text(since: start, now: start.addingTimeInterval(12.7)), "Rendering… · Waiting for the engine · 12 s")
        XCTAssertEqual(DevelopLoadingReadout.text(since: start, now: start.addingTimeInterval(61)), "Rendering… · Waiting for the engine · 61 s")
    }

    func testClockGoingBackwardsNeverGoesNegative() {
        let start = Date(timeIntervalSinceReferenceDate: 1_000)
        XCTAssertEqual(DevelopLoadingReadout.text(since: start, now: start.addingTimeInterval(-5)), "Rendering…")
    }

    /// The loupe shows the embedded preview (and the thumbnail before it) scaled to fit at once;
    /// engine frames keep the 1:1 cap (coarse levels are magnified through `displaySize`).
    func testPreviewFramesFillTheLoupeWhileEngineFramesStayAtOneToOne() {
        let drawable = CGSize(width: 2000, height: 1400)
        let thumbnail = (width: 384, height: 256)
        let capped = LoupePlacement.fit(display: thumbnail, drawable: drawable, scale: 2)
        XCTAssertEqual(capped.pixelsPerImageWidth, 384, accuracy: 0.5, "an engine-sized fit never upscales")
        let magnified = LoupePlacement.fit(display: thumbnail, drawable: drawable, scale: 2, magnify: true)
        let room = CGSize(width: drawable.width - 2 * Theme.Space.panelInset * 2, height: drawable.height - 2 * Theme.Space.panelInset * 2)
        let expected = min(room.width / 384, room.height / 256) * 384
        XCTAssertEqual(magnified.pixelsPerImageWidth, expected, accuracy: 0.5, "a preview fills the loupe inside the inset")
        // A preview larger than the loupe still fits down.
        let big = LoupePlacement.fit(display: (width: 6000, height: 4000), drawable: drawable, scale: 2, magnify: true)
        XCTAssertLessThanOrEqual(big.pixelsPerImageWidth, room.width + 0.5)
    }

    @MainActor
    func testModelRecordsWhenTheOpenStarted() {
        let model = AppModel()
        XCTAssertNil(model.developLoadingSince)
        model.loadStubItems(count: 3)
        model.select(position: 0)
        // Stub items cannot be developed: the status leaves loading at once and the start clears.
        model.enterPhotoEdit()
        XCTAssertNotEqual(model.developStatus, .loading)
        XCTAssertNil(model.developLoadingSince)
    }
}
