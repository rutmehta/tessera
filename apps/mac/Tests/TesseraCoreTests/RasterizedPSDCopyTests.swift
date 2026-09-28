import Foundation
import Synchronization
import XCTest
@testable import TesseraCore
@testable import Tessera

// UNRUN on Machine B. A must compile against freshly generated UniFFI bindings.
private final class FakeCopy: RasterizedPSDCopyOperation {
    private struct State { var cancelled = false; var runs = 0; var commits = false }
    private let state = Mutex(State())
    func cancel() -> Bool {
        state.withLock { s in
            if s.commits { return false }
            s.cancelled = true
            return true
        }
    }
    func run(path: String) throws -> RasterizedPSDCopyResult {
        state.withLock { s in
            s.runs += 1
            if s.cancelled { return .cancelled }
            s.commits = true
            return .saved
        }
    }
    var cancelled: Bool { state.withLock { $0.cancelled } }
    var runs: Int { state.withLock { $0.runs } }
}

@MainActor
final class RasterizedPSDCopyTests: XCTestCase {
    func testHostCloseWhileWorkerDrainsSuppressesCompletionAndBalancesQueue() async throws {
        let backend = try StubDocumentEngine().newDocument(width: 2, height: 2, depth: .u8, profile: nil)
        let doc = try DocumentController(backend: backend)
        let me = DocumentTransforms.shared
        let entered = expectation(description: "worker entered")
        let op = GatedCopy(entered: entered)
        defer { op.release(); doc.close() }
        var reports: [String] = []
        doc.report = { reports.append($0) }
        me.enqueueCopy(op, for: doc, url: URL(fileURLWithPath: "/unused-tiny-copy.psd"))
        await fulfillment(of: [entered], timeout: 5)
        XCTAssertTrue(me.isCopying(doc))
        me.cancelCopy(doc)
        XCTAssertTrue(me.isCopying(doc), "cancel is draining, not finished")
        doc.close()
        XCTAssertFalse(me.isCopying(doc))
        let count = reports.count
        op.release()
        await me.idle()
        XCTAssertEqual(reports.count, count, "closed origin must not receive stale completion")
    }

    func testCancelBeforeWorkerRunsRetainsAdmissionUntilCompletion() throws {
        let slot = RasterizedPSDCopySlot()
        let op = FakeCopy()
        let id = try XCTUnwrap(slot.install(op))
        XCTAssertTrue(slot.cancel())
        XCTAssertTrue(op.cancelled)
        XCTAssertEqual(op.runs, 0)
        XCTAssertNil(slot.install(FakeCopy()), "cancel request must not vacate running ownership")
        XCTAssertEqual(try op.run(path: "unused"), .cancelled)
        XCTAssertTrue(slot.finish(id))
        XCTAssertNotNil(slot.install(FakeCopy()))
    }

    func testCloseCancelsOriginAndStaleCompletionCannotClearReplacement() throws {
        let slot = RasterizedPSDCopySlot()
        let old = FakeCopy()
        let oldID = try XCTUnwrap(slot.install(old))
        slot.close()
        XCTAssertTrue(old.cancelled)
        let fresh = FakeCopy()
        let id = try XCTUnwrap(slot.install(fresh))
        XCTAssertFalse(slot.finish(oldID))
        XCTAssertEqual(slot.id, id)
        XCTAssertFalse(fresh.cancelled)
        XCTAssertEqual(try old.run(path: "unused"), .cancelled)
        XCTAssertEqual(try fresh.run(path: "unused"), .saved)
        XCTAssertTrue(slot.finish(id))
    }

    func testSeparateDocumentSlotsAndTooLateCancellation() throws {
        let origin = RasterizedPSDCopySlot()
        let other = RasterizedPSDCopySlot()
        let op = FakeCopy()
        let id = try XCTUnwrap(origin.install(op))
        let otherID = try XCTUnwrap(other.install(FakeCopy()))
        XCTAssertEqual(try op.run(path: "unused"), .saved)
        XCTAssertFalse(origin.cancel())
        XCTAssertFalse(origin.cancelling)
        XCTAssertTrue(origin.finish(id))
        XCTAssertEqual(other.id, otherID, "origin completion must not alter a different document")
    }
}

// Every mutable field is protected by NSCondition; no document/UI object crosses
// the worker boundary. The wait has a watchdog so a broken host test cannot hang A.
private final class GatedCopy: RasterizedPSDCopyOperation, @unchecked Sendable {
    private let condition = NSCondition()
    private var released = false
    private var cancelled = false
    let entered: XCTestExpectation
    init(entered: XCTestExpectation) { self.entered = entered }
    func cancel() -> Bool {
        condition.lock(); defer { condition.unlock() }
        cancelled = true
        return true
    }
    func release() {
        condition.lock(); defer { condition.unlock() }
        released = true
        condition.broadcast()
    }
    func run(path: String) throws -> RasterizedPSDCopyResult {
        entered.fulfill()
        condition.lock(); defer { condition.unlock() }
        let deadline = Date().addingTimeInterval(5)
        while !released {
            guard condition.wait(until: deadline) else { throw DocumentError.invalid("copy test watchdog") }
        }
        return cancelled ? .cancelled : .saved
    }
}
