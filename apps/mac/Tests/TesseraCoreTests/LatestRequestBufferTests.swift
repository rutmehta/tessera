import XCTest
@testable import TesseraCore

/// Source-only candidate: UNRUN on B. Simulates a blocked worker without threads or sleeps.
final class LatestRequestBufferTests: XCTestCase {
    func testBurstRunsOnlyFirstAndLatest() {
        var buffer = LatestRequestBuffer<Int>()
        let first = buffer.submit(1)!
        for value in 2...100 { XCTAssertNil(buffer.submit(value)) }
        let completion = buffer.finish(first.generation)
        XCTAssertFalse(completion.accept)
        XCTAssertEqual(completion.next?.value, 100)
        let last = buffer.finish(completion.next!.generation)
        XCTAssertTrue(last.accept)
        XCTAssertNil(last.next)
        XCTAssertNotNil(buffer.submit(101))
    }

    func testClearDropsPendingButKeepsRunningSlotUntilCompletion() {
        var buffer = LatestRequestBuffer<Int>()
        let first = buffer.submit(1)!
        XCTAssertNil(buffer.submit(2))
        buffer.invalidate()
        XCTAssertNil(buffer.submit(3), "a clear must not allow a second simultaneous worker")
        let completion = buffer.finish(first.generation)
        XCTAssertFalse(completion.accept)
        XCTAssertEqual(completion.next?.value, 3)
    }

    func testCloseRejectsLateResultWithoutStartingOldPendingWork() {
        var buffer = LatestRequestBuffer<Int>()
        let first = buffer.submit(1)!
        _ = buffer.submit(2)
        buffer.invalidate()
        let completion = buffer.finish(first.generation)
        XCTAssertFalse(completion.accept)
        XCTAssertNil(completion.next)
        XCTAssertNotNil(buffer.submit(3))
    }

    func testDuplicateCompletionCannotReleaseAnotherRunningRequest() {
        var buffer = LatestRequestBuffer<Int>()
        let first = buffer.submit(1)!
        _ = buffer.submit(2)
        let second = buffer.finish(first.generation).next!
        XCTAssertFalse(buffer.finish(first.generation).accept)
        XCTAssertNil(buffer.submit(3))
        XCTAssertEqual(buffer.finish(second.generation).next?.value, 3)
    }
}
