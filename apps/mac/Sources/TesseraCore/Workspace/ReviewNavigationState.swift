import Foundation

/// Session-local navigation only. The queue owns order/status; this owns the user's place.
public struct ReviewNavigationState: Equatable, Sendable {
    public private(set) var selectedID: String?
    public var anchorID: String?
    public private(set) var generation: UUID?
    public private(set) var isDrafting = false
    public var instruction = ""

    public init() {}

    /// Restores only the user's stable photo identities. Draft text is intentionally session-only.
    public mutating func restoreCursor(selectedID: String?, anchorID: String?, queue: AgentReviewQueue) {
        self.selectedID = selectedID.flatMap { queue.entry($0) == nil ? nil : $0 } ?? queue.entries.first?.imageID
        self.anchorID = anchorID.flatMap { queue.entry($0) == nil ? nil : $0 } ?? self.selectedID
        cancelRedo()
    }

    public mutating func reconcile(queue: AgentReviewQueue, generation: UUID) {
        if self.generation != generation { cancelRedo() }
        self.generation = generation
        if selectedID.flatMap({ queue.entry($0) }) == nil {
            selectedID = queue.entries.first?.imageID
            cancelRedo()
        }
        if anchorID.flatMap({ queue.entry($0) }) == nil { anchorID = selectedID }
    }

    public mutating func select(_ imageID: String, queue: AgentReviewQueue) {
        guard queue.entry(imageID) != nil, selectedID != imageID else { return }
        selectedID = imageID
        cancelRedo()
    }

    public mutating func move(_ delta: Int, queue: AgentReviewQueue) {
        guard !queue.isEmpty else { return }
        let index = selectedID.flatMap { id in queue.entries.firstIndex { $0.imageID == id } } ?? 0
        let entry = queue.entries[min(max(index + delta, 0), queue.count - 1)]
        select(entry.imageID, queue: queue)
        anchorID = entry.imageID
    }

    public mutating func beginRedo() {
        guard selectedID != nil else { return }
        isDrafting = true
    }

    public mutating func cancelRedo() { isDrafting = false; instruction = "" }

    public mutating func advanceAfterAccept(imageID: String, generation: UUID, succeeded: Bool,
                                           queue: AgentReviewQueue) {
        guard succeeded, self.generation == generation, selectedID == imageID,
              queue.entry(imageID)?.status == .accepted, let next = queue.next(after: imageID) else { return }
        select(next.imageID, queue: queue)
        anchorID = next.imageID
    }
}
