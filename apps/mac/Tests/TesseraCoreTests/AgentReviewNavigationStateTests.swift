import XCTest
@testable import TesseraCore

final class AgentReviewNavigationStateTests: XCTestCase {
    private func queue() -> AgentReviewQueue {
        AgentReviewQueue(entries: [
            .init(imageID: "failed", name: "Failed", confidence: 0, error: "Decode failed"),
            .init(imageID: "a", name: "A", confidence: 0.2),
            .init(imageID: "b", name: "B", confidence: 0.7),
            .init(imageID: "done", name: "Done", confidence: 0.8, status: .accepted),
        ])
    }

    func testRedoReorderKeepsSelectedIdentityAndInvalidatesOldDraft() {
        let generation = UUID()
        var queue = queue(), state = ReviewNavigationState()
        state.reconcile(queue: queue, generation: generation)
        state.select("b", queue: queue)
        state.anchorID = "a"
        state.beginRedo()
        state.instruction = "Keep the sky"
        queue.merge([.init(imageID: "b", name: "B", confidence: 0.1)])
        state.reconcile(queue: queue, generation: UUID())
        XCTAssertEqual(state.selectedID, "b")
        XCTAssertEqual(state.anchorID, "a")
        XCTAssertFalse(state.isDrafting)
        XCTAssertTrue(state.instruction.isEmpty)
    }

    func testAcceptAndNextRequiresSuccessfulCurrentSelectionAndSkipsFailedReviewed() {
        let generation = UUID()
        var queue = queue(), state = ReviewNavigationState()
        state.reconcile(queue: queue, generation: generation)
        state.select("a", queue: queue)
        state.advanceAfterAccept(imageID: "a", generation: generation, succeeded: false, queue: queue)
        XCTAssertEqual(state.selectedID, "a")
        queue.setStatus(.accepted, for: "a")
        state.advanceAfterAccept(imageID: "a", generation: UUID(), succeeded: true, queue: queue)
        XCTAssertEqual(state.selectedID, "a")
        state.advanceAfterAccept(imageID: "a", generation: generation, succeeded: true, queue: queue)
        XCTAssertEqual(state.selectedID, "b")
        queue.setStatus(.accepted, for: "b")
        state.advanceAfterAccept(imageID: "b", generation: generation, succeeded: true, queue: queue)
        XCTAssertEqual(state.selectedID, "b", "All-reviewed destination stays on its last photo")
        XCTAssertEqual(queue.entries.map(\.imageID), ["failed", "a", "b", "done"])
    }

    func testUserSelectionChangeDoesNotAdvanceOnLateAcceptCompletion() {
        let generation = UUID()
        var queue = queue(), state = ReviewNavigationState()
        state.reconcile(queue: queue, generation: generation)
        state.select("a", queue: queue)
        state.select("failed", queue: queue)
        queue.setStatus(.accepted, for: "a")
        state.advanceAfterAccept(imageID: "a", generation: generation, succeeded: true, queue: queue)
        XCTAssertEqual(state.selectedID, "failed")
    }

    func testStatusUpdateKeepsDraftButSelectingAnotherPhotoClearsIt() {
        let generation = UUID()
        var queue = queue(), state = ReviewNavigationState()
        state.reconcile(queue: queue, generation: generation)
        state.select("a", queue: queue)
        state.beginRedo()
        state.instruction = "Warmer"
        queue.setStatus(.reverted, for: "b")
        state.reconcile(queue: queue, generation: generation)
        XCTAssertEqual(state.instruction, "Warmer")
        XCTAssertTrue(state.isDrafting)
        state.select("b", queue: queue)
        XCTAssertFalse(state.isDrafting)
        XCTAssertTrue(state.instruction.isEmpty)
    }

    func testRestoredCursorUsesStableIDsAndFallsBackWhenTheyAreMissing() {
        let queue = queue()
        var state = ReviewNavigationState()
        state.restoreCursor(selectedID: "b", anchorID: "a", queue: queue)
        XCTAssertEqual(state.selectedID, "b")
        XCTAssertEqual(state.anchorID, "a")
        XCTAssertFalse(state.isDrafting)

        state.restoreCursor(selectedID: "gone", anchorID: "gone", queue: queue)
        XCTAssertEqual(state.selectedID, queue.entries.first?.imageID)
        XCTAssertEqual(state.anchorID, state.selectedID)
    }
}
