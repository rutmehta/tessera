import Foundation
import XCTest
@testable import Tessera

/// The Try cards render lazily (ST-2): after the photo's own first frame, once the loupe has been
/// idle for a second, never during a slider drag or an export, one card at a time.
final class TryCardPolicyTests: XCTestCase {
    private let t0 = Date(timeIntervalSince1970: 1_000)

    func testNothingRendersBeforeTheFirstFrameOrWithinTheIdleSecond() {
        var p = TryCardPolicy()
        XCTAssertFalse(p.mayRender(at: t0), "no frame yet")
        XCTAssertNil(p.delay(at: t0), "nothing to wait for until a frame arrives")
        p.lastFrameAt = t0
        XCTAssertFalse(p.mayRender(at: t0 + 5), "a progressive level is not the first final frame")
        p.firstFrameSeen = true
        XCTAssertFalse(p.mayRender(at: t0 + 0.5))
        XCTAssertEqual(p.delay(at: t0 + 0.25) ?? -1, 0.75, accuracy: 1e-9)
        XCTAssertTrue(p.mayRender(at: t0 + 1))
        XCTAssertEqual(p.delay(at: t0 + 3), 0)
    }

    func testEveryLoupeFrameRearmsTheIdleWait() {
        var p = TryCardPolicy(firstFrameSeen: true, lastFrameAt: t0)
        XCTAssertTrue(p.mayRender(at: t0 + 1))
        p.lastFrameAt = t0 + 0.9   // a drag produced a frame
        XCTAssertFalse(p.mayRender(at: t0 + 1))
        XCTAssertTrue(p.mayRender(at: t0 + 1.9))
    }

    func testDragsExportsAndARunningCardBlockTheNextCard() {
        var p = TryCardPolicy(firstFrameSeen: true, lastFrameAt: t0)
        p.interacting = true
        XCTAssertFalse(p.mayRender(at: t0 + 2))
        p.interacting = false
        p.exporting = true
        XCTAssertFalse(p.mayRender(at: t0 + 2))
        p.exporting = false
        p.busy = true
        XCTAssertFalse(p.mayRender(at: t0 + 2), "one card at a time")
        p.busy = false
        XCTAssertTrue(p.mayRender(at: t0 + 2))
    }

    func testCardsAskForACoarseLevel() {
        XCTAssertGreaterThanOrEqual(TryCardPolicy.minLevel, 5)
        XCTAssertEqual(TryCardPolicy.idle, 1)
    }
}
