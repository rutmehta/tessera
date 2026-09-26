import Foundation
import TesseraFFI

/// One photo in the agent's review queue (docs/10 §2 "Batch mode": sorted by the critic's
/// confidence so the photographer looks first at what the agent was least sure about).
public struct AgentReviewEntry: Sendable, Equatable, Identifiable {
    public enum Status: String, Sendable, CaseIterable {
        case needsReview = "needs review"
        case accepted
        case reverted
        public var title: String {
            switch self {
            case .needsReview: "To review"
            case .accepted: "Accepted"
            case .reverted: "Reverted"
            }
        }
    }

    public struct Step: Sendable, Equatable, Identifiable {
        public var entryID: UInt64
        public var title: String
        public var rationale: String
        public var enabled: Bool
        public var id: UInt64 { entryID }
        public init(entryID: UInt64, title: String, rationale: String, enabled: Bool = true) {
            self.entryID = entryID; self.title = title; self.rationale = rationale; self.enabled = enabled
        }
    }

    public var imageID: String
    /// The library item, when the photo is in the open folder.
    public var itemID: Int?
    public var name: String
    public var groupID: UInt32?
    public var accepted: Bool
    public var confidence: Double
    public var stopReason: String
    public var criticReasons: [String]
    public var steps: [Step]
    public var status: Status
    public var error: String?
    public var id: String { imageID }

    public init(imageID: String, itemID: Int? = nil, name: String, groupID: UInt32? = nil, accepted: Bool = false,
                confidence: Double, stopReason: String = "", criticReasons: [String] = [], steps: [Step] = [],
                status: Status = .needsReview, error: String? = nil) {
        self.imageID = imageID; self.itemID = itemID; self.name = name; self.groupID = groupID
        self.accepted = accepted; self.confidence = confidence; self.stopReason = stopReason
        self.criticReasons = criticReasons; self.steps = steps; self.status = status; self.error = error
    }

    public init(_ item: AgentReviewItem, itemID: Int?) {
        self.init(imageID: item.imageId, itemID: itemID, name: item.name, groupID: item.groupId,
                  accepted: item.accepted, confidence: item.confidence, stopReason: item.stopReason,
                  criticReasons: item.criticReasons,
                  steps: item.steps.map { Step(entryID: $0.entryId, title: $0.title, rationale: $0.rationale, enabled: $0.enabled) },
                  status: Status(rawValue: item.reviewStatus) ?? .needsReview, error: item.error)
    }

    /// "Low · 32 %", "Medium · 55 %", "High · 84 %".
    public var confidenceText: String {
        if error != nil { return "Failed" }
        let pct = Int((confidence * 100).rounded())
        return "\(AgentReviewQueue.band(confidence)) · \(pct) %"
    }

    /// The first rationale, as the row's one-line explanation.
    public var summary: String {
        if let error { return error }
        return steps.first?.rationale ?? (stopReason.isEmpty ? "No edit was needed" : stopReason)
    }
}

/// The review queue: failures first, then least confident first; statuses change in place so
/// rows do not jump while the photographer works through them.
public struct AgentReviewQueue: Sendable, Equatable {
    public private(set) var entries: [AgentReviewEntry]
    /// Provider that produced the queue ("scripted planner", "Anthropic claude-…").
    public var provider: String

    public init(entries: [AgentReviewEntry] = [], provider: String = "") {
        self.entries = Self.ordered(entries)
        self.provider = provider
    }

    public static func ordered(_ entries: [AgentReviewEntry]) -> [AgentReviewEntry] {
        entries.sorted { a, b in
            if (a.error != nil) != (b.error != nil) { return a.error != nil }
            if a.confidence != b.confidence { return a.confidence < b.confidence }
            return a.name.localizedStandardCompare(b.name) == .orderedAscending
        }
    }

    public static func band(_ confidence: Double) -> String {
        confidence < 0.4 ? "Low" : confidence < 0.7 ? "Medium" : "High"
    }

    public var isEmpty: Bool { entries.isEmpty }
    public var count: Int { entries.count }
    public func entry(_ imageID: String) -> AgentReviewEntry? { entries.first { $0.imageID == imageID } }

    /// Photos still waiting for accept / redo / revert (failures excluded).
    public var pendingCount: Int { entries.filter { $0.status == .needsReview && $0.error == nil }.count }
    public func count(_ status: AgentReviewEntry.Status) -> Int { entries.filter { $0.status == status && $0.error == nil }.count }
    public var failedCount: Int { entries.filter { $0.error != nil }.count }

    /// "3 to review · 1 accepted · 1 reverted · 1 failed" (zero counts omitted, except to review).
    public var summary: String {
        var parts = ["\(pendingCount) to review"]
        if count(.accepted) > 0 { parts.append("\(count(.accepted)) accepted") }
        if count(.reverted) > 0 { parts.append("\(count(.reverted)) reverted") }
        if failedCount > 0 { parts.append("\(failedCount) failed") }
        return parts.joined(separator: " · ")
    }

    public mutating func setStatus(_ status: AgentReviewEntry.Status, for imageID: String) {
        guard let i = entries.firstIndex(where: { $0.imageID == imageID }) else { return }
        entries[i].status = status
    }

    /// A redo (or rerun) replaces those photos' entries; the rest keep their state.
    public mutating func merge(_ fresh: [AgentReviewEntry]) {
        let ids = Set(fresh.map(\.imageID))
        entries = Self.ordered(entries.filter { !ids.contains($0.imageID) } + fresh)
    }

    /// The next photo that still needs review after `imageID` (wrapping), for "accept and next".
    public func next(after imageID: String?) -> AgentReviewEntry? {
        let pending = { (e: AgentReviewEntry) in e.status == .needsReview && e.error == nil }
        guard let imageID, let i = entries.firstIndex(where: { $0.imageID == imageID }) else {
            return entries.first(where: pending)
        }
        return (entries[(i + 1)...] + entries[..<i]).first(where: pending)
    }
}
