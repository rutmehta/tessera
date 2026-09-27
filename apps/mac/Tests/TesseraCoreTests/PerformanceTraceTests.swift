import Foundation
import XCTest
@testable import TesseraCore

final class PerformanceTraceTests: XCTestCase {
    func testDisabledTraceDoesNotRecord() {
        let trace = PerformanceTrace(enabled: false, capacity: 2)
        trace.record("input", session: "a", input: 1)
        XCTAssertTrue(trace.snapshot().events.isEmpty)
    }

    func testBoundedTraceReportsDropsAndPreservesIdentity() {
        let trace = PerformanceTrace(enabled: true, capacity: 2)
        trace.record("input", session: "a", input: 1)
        trace.record("callback_enqueue", session: "a", generation: 42, width: 800, height: 600, level: 2)
        trace.record("input", session: "b", input: 2)
        let snapshot = trace.snapshot()
        XCTAssertEqual(snapshot.events.count, 2)
        XCTAssertEqual(snapshot.dropped, 1)
        XCTAssertEqual(snapshot.events[1].generation, 42)
        XCTAssertNil(snapshot.events[1].input)
        XCTAssertEqual(snapshot.events[1].residency, "unavailable")
        XCTAssertEqual(snapshot.events[1].width, 800)
    }

    func testSpanDurationAndConcurrentCallbacks() {
        let trace = PerformanceTrace(enabled: true, capacity: 1000)
        let span = trace.begin("flush", session: "a", input: 3)
        trace.end(span)
        DispatchQueue.concurrentPerform(iterations: 100) { i in
            trace.record("callback_enqueue", session: "a", generation: UInt64(i))
        }
        let events = trace.snapshot().events
        XCTAssertEqual(events.count, 102)
        XCTAssertEqual(events[0].span, events[1].span)
        XCTAssertGreaterThanOrEqual(events[1].durationMs ?? -1, 0)
        XCTAssertEqual(Set(events.dropFirst(2).compactMap(\.generation)).count, 100)
    }
}
