import XCTest
@testable import TesseraCore

final class LatestFrameMailboxTests: XCTestCase {
    func testLateOldGenerationCannotReplacePendingOrDeliveredFrame() {
        let box = LatestFrameMailbox<Int>()
        XCTAssertTrue(box.offer(5, sequence: 5))
        XCTAssertFalse(box.offer(4, sequence: 4))
        XCTAssertEqual(box.take(), 5)
        XCTAssertFalse(box.offer(3, sequence: 3))
        XCTAssertNil(box.take())
    }
    func testDelayedDrainOnlyTakesNewestAndSchedulesOnce() {
        let box = LatestFrameMailbox<Int>()
        XCTAssertTrue(box.offer(1))
        XCTAssertFalse(box.offer(2))
        XCTAssertFalse(box.offer(3))
        XCTAssertEqual(box.take(), 3)
        XCTAssertNil(box.take())
        XCTAssertTrue(box.offer(4))
        XCTAssertEqual(box.take(), 4)
    }
}
