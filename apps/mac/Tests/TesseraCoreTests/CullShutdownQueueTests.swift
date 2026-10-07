import AppKit
import Foundation
import XCTest
@testable import TesseraCore
@testable import Tessera

@MainActor
final class CullShutdownQueueTests: XCTestCase {
    func testBlockedNativeShutdownRunsOffMainAndDrainWaitsForRetirement() async throws {
        let queue = CullShutdownQueue()
        let release = DispatchSemaphore(value: 0)
        let entered = expectation(description: "native shutdown entered")
        let retired = expectation(description: "worker retired")
        queue.enqueue {
            XCTAssertFalse(Thread.isMainThread)
            entered.fulfill()
            XCTAssertEqual(release.wait(timeout: .now() + 10), .success)
            retired.fulfill()
        }
        defer { release.signal() }
        await fulfillment(of: [entered], timeout: 5)
        var drained = false
        let drainStarted = expectation(description: "drain started")
        let drain = Task {
            drainStarted.fulfill()
            try await queue.drain()
            drained = true
        }
        // The actor can service UI work while native shutdown is blocked.
        await fulfillment(of: [drainStarted], timeout: 1)
        XCTAssertFalse(drained)
        release.signal()
        try await drain.value
        XCTAssertTrue(drained)
        await fulfillment(of: [retired], timeout: 1)
    }

    func testDrainIncludesAnotherSessionRetiredWhileWaiting() async throws {
        let queue = CullShutdownQueue()
        let firstRelease = DispatchSemaphore(value: 0)
        let secondRelease = DispatchSemaphore(value: 0)
        let firstEntered = expectation(description: "first entered")
        let secondEntered = expectation(description: "second entered")
        queue.enqueue {
            firstEntered.fulfill()
            XCTAssertEqual(firstRelease.wait(timeout: .now() + 10), .success)
        }
        defer { firstRelease.signal(); secondRelease.signal() }
        await fulfillment(of: [firstEntered], timeout: 5)
        var drained = false
        let drainStarted = expectation(description: "drain started")
        let drain = Task {
            drainStarted.fulfill()
            try await queue.drain()
            drained = true
        }
        await fulfillment(of: [drainStarted], timeout: 1)
        queue.enqueue {
            secondEntered.fulfill()
            XCTAssertEqual(secondRelease.wait(timeout: .now() + 10), .success)
        }
        await fulfillment(of: [secondEntered], timeout: 5)
        firstRelease.signal()
        await Task.yield()
        XCTAssertFalse(drained)
        secondRelease.signal()
        try await drain.value
        XCTAssertTrue(drained)
    }

    func testNativeFailureCannotBeReportedAsJoinedShutdown() async {
        enum Failure: Error { case native }
        let queue = CullShutdownQueue()
        queue.enqueue { throw Failure.native }
        do {
            try await queue.drain()
            XCTFail("native failure must prevent successful shutdown")
        } catch { XCTAssertTrue(error is Failure) }
    }

    func testFailedShutdownDoesNotPoisonLaterDrains() async throws {
        enum Failure: Error { case native }
        let queue = CullShutdownQueue()
        queue.enqueue { throw Failure.native }
        do {
            try await queue.drain()
            XCTFail("the first drain reports the failure")
        } catch { XCTAssertTrue(error is Failure) }
        try await queue.drain()
        queue.enqueue {}
        try await queue.drain()
    }

    func testBoundedDrainReturnsWhileNativeShutdownIsStuck() async throws {
        let queue = CullShutdownQueue()
        let release = DispatchSemaphore(value: 0)
        let entered = expectation(description: "native shutdown entered")
        queue.enqueue {
            entered.fulfill()
            _ = release.wait(timeout: .now() + 10)
        }
        defer { release.signal() }
        await fulfillment(of: [entered], timeout: 5)
        let start = ContinuousClock.now
        let outcome = await queue.drain(timeout: .milliseconds(200))
        XCTAssertEqual(outcome, .timedOut)
        XCTAssertLessThan(ContinuousClock.now - start, .seconds(5))
        release.signal()
        let joined = await queue.drain(timeout: .seconds(5))
        XCTAssertEqual(joined, .joined)
    }

    func testAppQuitAfterFailedShutdownResetsStateAndCanQuitAgain() async throws {
        enum Failure: Error { case native }
        _ = NSApplication.shared
        let app = AppModel()
        app.cullShutdowns.enqueue { throw Failure.native }
        let failed = await app.shutdownCullSessions(timeout: .seconds(5))
        guard case .failed = failed else { return XCTFail("expected failure, got \(failed)") }
        XCTAssertFalse(app.isShuttingDownCull, "a failed shutdown must not disable libraries")
        let joined = await app.shutdownCullSessions(timeout: .seconds(5))
        XCTAssertEqual(joined, .joined)
    }
}
