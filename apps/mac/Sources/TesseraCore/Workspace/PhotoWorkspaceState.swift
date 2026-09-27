import Foundation

/// One entry-time snapshot, resolved by identity after catalog insertions/removals.
/// No per-frame or per-gesture copy of the library is needed.
public struct WorkspaceSelectionBookmark: Equatable, Sendable {
    public let order: [String]
    public let selected: Set<String>
    public let focus: String?
    public let anchor: String?

    public init(order: [String], selected: Set<String>, focus: String?, anchor: String?) {
        self.order = order; self.selected = selected; self.focus = focus; self.anchor = anchor
    }

    public struct Resolution: Equatable, Sendable {
        public let selected: IndexSet
        public let focus: Int?
        public let anchor: Int?
        public let usedFallback: Bool
    }

    public func resolve(in keys: [String]) -> Resolution {
        let positions = Dictionary(uniqueKeysWithValues: keys.enumerated().map { ($0.element, $0.offset) })
        var chosen = IndexSet(selected.compactMap { positions[$0] })
        let exact = focus.flatMap { positions[$0] }
        var resolved = exact
        if resolved == nil {
            let origin = focus.flatMap { order.firstIndex(of: $0) } ?? 0
            // Nearest surviving original neighbor, following item first on ties.
            // Scan only on deletion, and do not sort a 20k-item library to return.
            for distance in 0..<order.count {
                let following = origin + distance
                if following < order.count, let position = positions[order[following]] {
                    resolved = position
                    break
                }
                let preceding = origin - distance
                if preceding >= 0, preceding < order.count, let position = positions[order[preceding]] {
                    resolved = position
                    break
                }
            }
            if resolved == nil { resolved = keys.indices.first }
        }
        if chosen.isEmpty, let resolved { chosen.insert(resolved) }
        return Resolution(selected: chosen, focus: resolved, anchor: anchor.flatMap { positions[$0] } ?? resolved,
                          usedFallback: focus != nil && exact == nil)
    }
}

public enum PhotoInspectorTab: String, CaseIterable, Identifiable, Sendable {
    case develop = "Develop", masks = "Masks"
    public var id: String { rawValue }
}
