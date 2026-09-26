import Foundation

/// The Layers panel's tree, built from the flat `layers()` list, and the minimal sequence of
/// outline edits that turns one tree into another (so `NSOutlineView` animates a reorder as one
/// move instead of reloading, and keeps selection and scroll position).
///
/// Children are top first (panel order). `DocumentOutline.root` (0, engine-api `LayerId::ROOT`)
/// is the document root.
public struct DocumentOutline: Equatable, Sendable {
    public static let root: DocLayerID = 0

    /// Children per parent, top first. Every group (and the root) has an entry.
    public private(set) var children: [DocLayerID: [DocLayerID]] = [root: []]
    public private(set) var parentOf: [DocLayerID: DocLayerID] = [:]
    public private(set) var nodes: [DocLayerID: LayerRecord] = [:]

    public init() {}

    /// From `layers()`: siblings are ordered by compositor index, highest (top) first, whatever the
    /// list order. Rows whose parent is unknown are attached to the root.
    public init(_ list: [LayerRecord]) {
        for n in list { nodes[n.id] = n }
        var byParent: [DocLayerID: [LayerRecord]] = [:]
        for n in list {
            let p = n.parent.flatMap { nodes[$0] != nil ? $0 : nil } ?? Self.root
            byParent[p, default: []].append(n)
            parentOf[n.id] = p
            if n.kind == .group, children[n.id] == nil { children[n.id] = [] }
        }
        for (p, kids) in byParent {
            children[p] = kids.enumerated().sorted { a, b in
                a.element.index != b.element.index ? a.element.index > b.element.index : a.offset < b.offset
            }.map(\.element.id)
        }
    }

    public func children(of parent: DocLayerID) -> [DocLayerID] { children[parent] ?? [] }
    public func node(_ id: DocLayerID) -> LayerRecord? { nodes[id] }
    public func contains(_ id: DocLayerID) -> Bool { nodes[id] != nil }
    public var count: Int { nodes.count }

    /// Pre-order, top first: the rows of a fully expanded panel.
    public var flattened: [DocLayerID] {
        var out: [DocLayerID] = []
        func walk(_ p: DocLayerID) { for c in children(of: p) { out.append(c); walk(c) } }
        walk(Self.root)
        return out
    }

    /// Parent and top-first index.
    public func position(of id: DocLayerID) -> (parent: DocLayerID, index: Int)? {
        guard let p = parentOf[id], let i = children(of: p).firstIndex(of: id) else { return nil }
        return (p, i)
    }

    /// Nesting depth (0 = root child).
    public func depth(of id: DocLayerID) -> Int {
        var d = 0, p = parentOf[id] ?? Self.root
        while p != Self.root { d += 1; p = parentOf[p] ?? Self.root }
        return d
    }

    /// True when `id` is `ancestor` or inside it.
    public func isDescendant(_ id: DocLayerID, of ancestor: DocLayerID) -> Bool {
        var p: DocLayerID? = id
        while let q = p, q != Self.root {
            if q == ancestor { return true }
            p = parentOf[q]
        }
        return ancestor == Self.root
    }

    /// Structure only (for comparing trees in tests and after applying changes).
    public var structure: [DocLayerID: [DocLayerID]] { children.filter { $0.key == Self.root || nodes[$0.key] != nil } }

    // MARK: Drag and drop

    /// The tree with `ids` moved (in their current top-first order) into `parent` before the row at
    /// top-first `index` of the *current* children (`index == count` = bottom). Nil when the drop is
    /// impossible (a group into itself or its descendants, unknown ids, a non-group target).
    public func moving(_ ids: [DocLayerID], into parent: DocLayerID, at index: Int) -> DocumentOutline? {
        guard parent == Self.root || children[parent] != nil, !ids.isEmpty else { return nil }
        let order = flattened
        // Drop nested selections whose ancestor also moves.
        let set = Set(ids)
        let moving = order.filter { id in
            set.contains(id) && !set.contains(where: { $0 != id && isDescendant(id, of: $0) })
        }
        guard !moving.isEmpty, moving.allSatisfy({ nodes[$0] != nil }) else { return nil }
        for id in moving where isDescendant(parent, of: id) { return nil }
        let before = children(of: parent)
        let clamped = min(max(index, 0), before.count)
        let shift = before.prefix(clamped).filter { moving.contains($0) }.count
        var next = self
        for id in moving {
            let p = next.parentOf[id]!
            next.children[p]?.removeAll { $0 == id }
        }
        var target = next.children[parent] ?? []
        target.insert(contentsOf: moving, at: min(clamped - shift, target.count))
        next.children[parent] = target
        for id in moving { next.parentOf[id] = parent }
        next.reindex()
        return next
    }

    /// Recomputes `index` / `parent` / `depth` of the stored nodes from the structure.
    private mutating func reindex() {
        func walk(_ p: DocLayerID, _ depth: UInt32) {
            let kids = children(of: p)
            for (i, c) in kids.enumerated() {
                nodes[c]?.parent = p == Self.root ? nil : p
                nodes[c]?.index = UInt32(kids.count - 1 - i)
                nodes[c]?.depth = depth
                walk(c, depth + 1)
            }
        }
        walk(Self.root, 0)
    }

    // MARK: Diff

    /// Applies `changes` to a copy of this tree's structure (the node records are carried along; new
    /// ids get placeholder records). Used by tests and to check a diff before animating it.
    public func applying(_ changes: [OutlineChange]) -> [DocLayerID: [DocLayerID]] {
        var sim = Sim(self)
        for c in changes { sim.apply(c) }
        return sim.children.filter { $0.key == Self.root || sim.alive.contains($0.key) }
    }

    /// The edits that turn `old` into `new`: inserts, removals and moves, each expressed in the
    /// indices of the tree as it is at that step (the order `NSOutlineView` batch updates use).
    /// Siblings that keep their relative order (a longest increasing subsequence) never move, so a
    /// single drag produces a single move. Nil when a move would nest a node inside itself (swapped
    /// nesting); callers then reload.
    public static func diff(from old: DocumentOutline, to new: DocumentOutline) -> [OutlineChange]? {
        var sim = Sim(old)
        var out: [OutlineChange] = []
        // Anchored: survivors that keep their parent and relative order among such siblings.
        var anchored = Set<DocLayerID>()
        for (p, target) in new.children {
            let oldKids = old.children(of: p)
            let oldIndex = Dictionary(uniqueKeysWithValues: oldKids.enumerated().map { ($1, $0) })
            let staying = target.filter { oldIndex[$0] != nil && old.parentOf[$0] == p }
            for i in longestIncreasingSubsequence(staying.map { oldIndex[$0]! }) { anchored.insert(staying[i]) }
        }
        // Parents in new pre-order, so a parent exists (or has been inserted) before its children.
        var parents: [DocLayerID] = [root]
        func collect(_ p: DocLayerID) {
            for c in new.children(of: p) where new.children[c] != nil { parents.append(c); collect(c) }
        }
        collect(root)
        for p in parents {
            let target = new.children(of: p)
            for (i, c) in target.enumerated() where !anchored.contains(c) {
                let predecessor = i == 0 ? nil : target[i - 1]
                if let from = sim.position(of: c) {
                    if sim.isAncestor(c, of: p) { return nil }
                    sim.detach(c)
                    let to = predecessor.map { sim.children[p]!.firstIndex(of: $0)! + 1 } ?? 0
                    sim.insert(c, into: p, at: to)
                    out.append(.move(id: c, fromParent: from.parent, fromIndex: from.index, toParent: p, toIndex: to))
                } else {
                    let to = predecessor.map { sim.children[p]!.firstIndex(of: $0)! + 1 } ?? 0
                    sim.insert(c, into: p, at: to)
                    if new.children[c] != nil { sim.children[c] = [] }
                    out.append(.insert(id: c, parent: p, index: to))
                }
            }
        }
        // Leftovers: nodes that are not in the new tree (their surviving descendants moved out above).
        func sweep(_ p: DocLayerID) {
            let kids = sim.children[p] ?? []
            for (i, c) in kids.enumerated().reversed() where !new.contains(c) {
                out.append(.remove(id: c, parent: p, index: i))
                sim.remove(c)
            }
            for c in sim.children[p] ?? [] { sweep(c) }
        }
        sweep(root)
        return out
    }

    /// The backend calls (`move_layer(id, parent, index)`, compositor indices, 0 = bottom) that turn
    /// this tree into `target` when both hold the same layers. Nil when they differ in membership.
    public func backendMoves(to target: DocumentOutline) -> [(id: DocLayerID, parent: DocLayerID?, index: UInt32)]? {
        guard Set(nodes.keys) == Set(target.nodes.keys), let changes = Self.diff(from: self, to: target) else { return nil }
        var sim = Sim(self)
        var out: [(DocLayerID, DocLayerID?, UInt32)] = []
        for change in changes {
            guard case .move(let id, _, _, let p, let to) = change else { return nil }
            sim.detach(id)
            let n = sim.children[p]?.count ?? 0
            sim.insert(id, into: p, at: to)
            out.append((id, p == Self.root ? nil : p, UInt32(n - to)))
        }
        return out
    }

    /// Indices (into `values`) of one longest strictly increasing subsequence.
    static func longestIncreasingSubsequence(_ values: [Int]) -> [Int] {
        guard !values.isEmpty else { return [] }
        var tails: [Int] = []          // indices into values
        var prev = Array(repeating: -1, count: values.count)
        for (i, v) in values.enumerated() {
            var lo = 0, hi = tails.count
            while lo < hi {
                let mid = (lo + hi) / 2
                if values[tails[mid]] < v { lo = mid + 1 } else { hi = mid }
            }
            if lo > 0 { prev[i] = tails[lo - 1] }
            if lo == tails.count { tails.append(i) } else { tails[lo] = i }
        }
        var out: [Int] = []
        var k = tails.last!
        while k >= 0 { out.append(k); k = prev[k] }
        return out.reversed()
    }

    /// Structural simulation used by the diff.
    struct Sim {
        var children: [DocLayerID: [DocLayerID]]
        var parentOf: [DocLayerID: DocLayerID]
        var alive: Set<DocLayerID>

        init(_ o: DocumentOutline) {
            children = o.children
            parentOf = o.parentOf
            alive = Set(o.nodes.keys)
        }

        func position(of id: DocLayerID) -> (parent: DocLayerID, index: Int)? {
            guard alive.contains(id), let p = parentOf[id], let i = children[p]?.firstIndex(of: id) else { return nil }
            return (p, i)
        }

        func isAncestor(_ a: DocLayerID, of id: DocLayerID) -> Bool {
            var p: DocLayerID? = id
            while let q = p, q != DocumentOutline.root {
                if q == a { return true }
                p = parentOf[q]
            }
            return false
        }

        mutating func remove(_ id: DocLayerID) {
            guard let p = parentOf[id] else { return }
            children[p]?.removeAll { $0 == id }
            parentOf[id] = nil
            alive.remove(id)
            // Removing a group removes whatever is still inside it.
            for c in children[id] ?? [] where parentOf[c] == id { remove(c) }
        }

        /// Takes `id` (with its subtree) out of its parent's list.
        mutating func detach(_ id: DocLayerID) {
            if let p = parentOf[id] { children[p]?.removeAll { $0 == id } }
        }

        mutating func insert(_ id: DocLayerID, into p: DocLayerID, at index: Int) {
            var kids = children[p] ?? []
            kids.insert(id, at: min(max(index, 0), kids.count))
            children[p] = kids
            parentOf[id] = p
            alive.insert(id)
        }

        mutating func apply(_ c: OutlineChange) {
            switch c {
            case .insert(let id, let p, let i): insert(id, into: p, at: i)
            case .remove(let id, _, _): remove(id)
            case .move(let id, _, _, let p, let i):
                detach(id)
                insert(id, into: p, at: i)
            }
        }
    }
}

/// One outline edit. Indices are top-first and relative to the tree at that step; a move's
/// `toIndex` is the position after the node was taken out of its old place.
public enum OutlineChange: Equatable, Sendable {
    case insert(id: DocLayerID, parent: DocLayerID, index: Int)
    case remove(id: DocLayerID, parent: DocLayerID, index: Int)
    case move(id: DocLayerID, fromParent: DocLayerID, fromIndex: Int, toParent: DocLayerID, toIndex: Int)
}
