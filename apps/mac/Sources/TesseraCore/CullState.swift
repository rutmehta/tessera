import Foundation

/// The one culling decision (docs/06 §2).
public enum Decision: UInt8, Sendable, CaseIterable {
    case undecided = 0
    case reject = 1
    case keep = 2

    public var label: String {
        switch self {
        case .undecided: "Undecided"
        case .reject: "Reject"
        case .keep: "Keep"
        }
    }
}

/// Per-image selection state (docs/06 §2): decision, optional grade, one mark, basket membership.
/// 4 bytes, stored densely for 20k+ items.
public struct CullState: Sendable, Equatable, Hashable {
    public var decision: Decision = .undecided
    /// 0 = none, 1 keep, 2 good, 3 best. Only meaningful on a Keep.
    public var grade: UInt8 = 0
    /// 0 = none, otherwise the mark key 6...9.
    public var mark: UInt8 = 0
    public var inBasket: Bool = false

    public init(decision: Decision = .undecided, grade: UInt8 = 0, mark: UInt8 = 0, inBasket: Bool = false) {
        self.decision = decision
        self.grade = grade
        self.mark = mark
        self.inBasket = inBasket
    }

    public static let gradeNames = ["", "Keep", "Good", "Best"]
}

/// A user action from the culling key map (docs/06 §2, §3).
public enum CullAction: Sendable, Equatable {
    case reject          // X
    case undecided       // U
    case keep            // P
    case grade(UInt8)    // 1 / 2 / 3
    case mark(UInt8)     // 6 / 7 / 8 / 9
    case toggleBasket    // B

    /// Decisions and grades trigger auto-advance; marks and basket do not.
    public var advances: Bool {
        switch self {
        case .reject, .undecided, .keep, .grade: true
        case .mark, .toggleBasket: false
        }
    }
}

/// Dense in-memory store of cull state with a single global undo stack. Used only for the
/// synthetic stub library (performance testing); folders use the Rust `CullSession`.
public struct CullStore: Sendable {
    public private(set) var states: [CullState]
    public private(set) var counts = Counts()

    private var undoStack: [Change] = []
    private var redoStack: [Change] = []

    public struct Counts: Sendable, Equatable {
        public var undecided = 0
        public var reject = 0
        public var keep = 0
        public var basket = 0
        public init() {}
    }

    struct Change: Sendable {
        var ids: [Int]
        var before: [CullState]
        var after: [CullState]
    }

    public init(count: Int) {
        states = Array(repeating: CullState(), count: count)
        counts.undecided = count
    }

    public init(states: [CullState]) {
        self.states = states
        for state in states { adjust(state, by: 1) }
    }

    public subscript(id: Int) -> CullState { states[id] }

    public var canUndo: Bool { !undoStack.isEmpty }
    public var canRedo: Bool { !redoStack.isEmpty }

    /// Applies `action` to `ids`. Returns the ids whose state changed.
    @discardableResult
    public mutating func apply(_ action: CullAction, to ids: [Int]) -> [Int] {
        guard !ids.isEmpty else { return [] }
        let before = ids.map { states[$0] }
        var after = before

        switch action {
        case .reject:
            for i in after.indices { after[i].decision = .reject; after[i].grade = 0 }
        case .undecided:
            for i in after.indices { after[i].decision = .undecided; after[i].grade = 0 }
        case .keep:
            for i in after.indices { after[i].decision = .keep }
        case .grade(let g):
            // A grade implies Keep ("1/2/3 on a Keep").
            let g = min(max(g, 1), 3)
            for i in after.indices { after[i].decision = .keep; after[i].grade = g }
        case .mark(let m):
            // Toggle: if every target already has this mark, clear it; otherwise set it.
            let allHave = before.allSatisfy { $0.mark == m }
            for i in after.indices { after[i].mark = allHave ? 0 : m }
        case .toggleBasket:
            let allIn = before.allSatisfy(\.inBasket)
            for i in after.indices { after[i].inBasket = !allIn }
        }

        var changed: [Int] = []
        var changedBefore: [CullState] = []
        var changedAfter: [CullState] = []
        for (k, id) in ids.enumerated() where before[k] != after[k] {
            changed.append(id); changedBefore.append(before[k]); changedAfter.append(after[k])
        }
        guard !changed.isEmpty else { return [] }
        write(ids: changed, states: changedAfter)
        undoStack.append(Change(ids: changed, before: changedBefore, after: changedAfter))
        if undoStack.count > 10_000 { undoStack.removeFirst(undoStack.count - 10_000) }
        redoStack.removeAll()
        return changed
    }

    /// Sets a possibly different state per id as one undo step. Returns the ids that changed.
    @discardableResult
    public mutating func apply(states updates: [(Int, CullState)]) -> [Int] {
        let changed = updates.filter { states[$0.0] != $0.1 }
        guard !changed.isEmpty else { return [] }
        let ids = changed.map(\.0)
        let before = ids.map { states[$0] }
        write(ids: ids, states: changed.map(\.1))
        undoStack.append(Change(ids: ids, before: before, after: changed.map(\.1)))
        redoStack.removeAll()
        return ids
    }

    /// Returns the ids restored, or nil when there is nothing to undo.
    public mutating func undo() -> [Int]? {
        guard let change = undoStack.popLast() else { return nil }
        write(ids: change.ids, states: change.before)
        redoStack.append(change)
        return change.ids
    }

    public mutating func redo() -> [Int]? {
        guard let change = redoStack.popLast() else { return nil }
        write(ids: change.ids, states: change.after)
        undoStack.append(change)
        return change.ids
    }

    private mutating func write(ids: [Int], states newStates: [CullState]) {
        for (k, id) in ids.enumerated() {
            let old = states[id]
            let new = newStates[k]
            adjust(old, by: -1)
            adjust(new, by: 1)
            states[id] = new
        }
    }

    private mutating func adjust(_ s: CullState, by d: Int) {
        switch s.decision {
        case .undecided: counts.undecided += d
        case .reject: counts.reject += d
        case .keep: counts.keep += d
        }
        if s.inBasket { counts.basket += d }
    }
}
