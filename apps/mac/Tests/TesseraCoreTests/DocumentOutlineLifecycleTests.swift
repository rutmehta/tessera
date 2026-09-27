import Foundation
import XCTest
@testable import Tessera
@testable import TesseraCore

private final class HeldOutlineFetch: @unchecked Sendable {
    let entered: XCTestExpectation
    private let releaseFirst = DispatchSemaphore(value: 0)
    private let lock = NSLock()
    private var calls = 0
    private var released = false

    init(entered: XCTestExpectation) { self.entered = entered }

    var callCount: Int { lock.withLock { calls } }

    func open() {
        let signal = lock.withLock { () -> Bool in
            guard !released else { return false }
            released = true
            return true
        }
        if signal { releaseFirst.signal() }
    }

    func fetch(_ backend: any DocumentToolsBackend, _ level: UInt8) -> [SelectionOutline] {
        let call = lock.withLock { () -> Int in calls += 1; return calls }
        if call == 1 {
            entered.fulfill()
            _ = releaseFirst.wait(timeout: .now() + 10) // watchdog on a failed test
        }
        return [Self.outline(call)]
    }

    static func outline(_ tag: Int) -> SelectionOutline {
        SelectionOutline(points: [CanvasPoint(x: Float(tag), y: 0), CanvasPoint(x: Float(tag), y: 1)], closed: false)
    }
}

/// Exercises DocumentTools' real admission and publication path with tiny stub
/// documents. Only the outline fetch is gated; no engine contour, window or timer.
@MainActor
final class DocumentOutlineLifecycleTests: XCTestCase {

    private func install(_ workspace: DocumentWorkspace, size: UInt32 = 16) throws -> DocumentController {
        let backend = try StubDocumentEngine().newDocument(width: size, height: size, depth: .u8, profile: nil)
        try workspace.install(backend)
        return try XCTUnwrap(workspace.current)
    }

    private func select(_ doc: DocumentController, x: Int64) {
        doc.setMarquee(CanvasRect(x: x, y: 1, width: 4, height: 4))
        DocumentTools.shared.refreshOutline(doc)
    }

    private func cleanup(_ workspace: DocumentWorkspace, _ held: HeldOutlineFetch) {
        held.open()
        let tools = DocumentTools.shared
        tools.outlineFetchForTesting = nil
        tools.outlineCompletionForTesting = nil
        for doc in workspace.documents { workspace.discard(doc) }
        // Detach the singleton from this fixture before another test uses it.
        tools.attach(DocumentWorkspace())
    }

    func testSwitchDiscardsOldOutlineAndRunsOnlyNewestPendingDocument() async throws {
        let tools = DocumentTools.shared
        let workspace = DocumentWorkspace()
        tools.attach(workspace)
        let entered = expectation(description: "first outline fetch entered")
        let completed = expectation(description: "old and newest outline completions")
        completed.expectedFulfillmentCount = 2
        let held = HeldOutlineFetch(entered: entered)
        defer { cleanup(workspace, held) }
        tools.outlineFetchForTesting = { held.fetch($0, $1) }
        var publications: [(Bool, String)] = []
        tools.outlineCompletionForTesting = { _, accepted, id in
            publications.append((accepted, id))
            completed.fulfill()
        }

        let first = try install(workspace)
        select(first, x: 1)
        await fulfillment(of: [entered], timeout: 5)
        select(first, x: 2)
        select(first, x: 3)
        let second = try install(workspace)
        select(second, x: 4)
        select(second, x: 5)
        XCTAssertEqual(held.callCount, 1, "only the first fetch runs while held")

        held.open()
        await fulfillment(of: [completed], timeout: 5)
        XCTAssertEqual(held.callCount, 2, "superseded requests must not enter the worker")
        XCTAssertEqual(publications.map { $0.0 }, [false, true])
        XCTAssertEqual(publications.map { $0.1 }, [first.id, second.id])
        XCTAssertEqual(tools.outline, [HeldOutlineFetch.outline(2)])
    }

    func testClearDropsPendingAndCannotResurrectOldOutline() async throws {
        let tools = DocumentTools.shared
        let workspace = DocumentWorkspace()
        tools.attach(workspace)
        let entered = expectation(description: "first outline fetch entered")
        let completed = expectation(description: "invalidated outline completed")
        let held = HeldOutlineFetch(entered: entered)
        defer { cleanup(workspace, held) }
        tools.outlineFetchForTesting = { held.fetch($0, $1) }
        var accepted: [Bool] = []
        tools.outlineCompletionForTesting = { _, didAccept, _ in
            accepted.append(didAccept)
            completed.fulfill()
        }

        let doc = try install(workspace)
        select(doc, x: 1)
        await fulfillment(of: [entered], timeout: 5)
        select(doc, x: 2)
        doc.setMarquee(nil)
        tools.refreshOutline(doc)
        XCTAssertTrue(tools.outline.isEmpty)
        held.open()
        await fulfillment(of: [completed], timeout: 5)
        XCTAssertEqual(held.callCount, 1)
        XCTAssertEqual(accepted, [false])
        XCTAssertTrue(tools.outline.isEmpty)
    }

    func testCloseRejectsRunningResultAndDropsPendingWork() async throws {
        let tools = DocumentTools.shared
        let workspace = DocumentWorkspace()
        tools.attach(workspace)
        let entered = expectation(description: "first outline fetch entered")
        let completed = expectation(description: "closed outline completed")
        let held = HeldOutlineFetch(entered: entered)
        defer { cleanup(workspace, held) }
        tools.outlineFetchForTesting = { held.fetch($0, $1) }
        var accepted: [Bool] = []
        tools.outlineCompletionForTesting = { _, didAccept, _ in
            accepted.append(didAccept)
            completed.fulfill()
        }

        let doc = try install(workspace)
        select(doc, x: 1)
        await fulfillment(of: [entered], timeout: 5)
        select(doc, x: 2)
        workspace.discard(doc) // no window: closes the tiny stub immediately
        XCTAssertNil(workspace.current)
        held.open()
        await fulfillment(of: [completed], timeout: 5)
        XCTAssertEqual(held.callCount, 1)
        XCTAssertEqual(accepted, [false])
        XCTAssertTrue(tools.outline.isEmpty)
    }
}
