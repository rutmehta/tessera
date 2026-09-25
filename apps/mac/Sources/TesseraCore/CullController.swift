import Foundation
import TesseraFFI

/// Derived, read-only status (docs/06 §2). Computed by the engine from recipe history, the
/// export log and library.json membership.
public enum DerivedPhase: String, Sendable, CaseIterable {
    case unedited, edited, exported, published
}

public struct ItemStatus: Sendable, Equatable {
    public var phase: DerivedPhase
    /// Albums this image belongs to (including the basket target).
    public var albums: [String]
    public init(phase: DerivedPhase = .unedited, albums: [String] = []) {
        self.phase = phase
        self.albums = albums
    }
}

/// Group navigation (docs/06 §3). Semantics follow `crates/cull`: group jumps land on the
/// group's first frame; all moves stop at boundaries.
public enum GroupMove: Sendable {
    case nextGroup, previousGroup, nextInGroup, previousInGroup
}

/// Outcome of a mutation, undo or redo, in item ids.
public struct CullChange: Sendable, Equatable {
    public var ids: [Int]
    /// Cursor the engine restored (undo/redo), if any.
    public var current: Int?
    public var albumsChanged: Bool
}

public struct AlbumSummary: Sendable, Equatable, Identifiable {
    public var name: String
    /// Item ids in manual album order.
    public var members: [Int]
    public var id: String { name }
}

/// One defect threshold. Signal names and scales are producer-defined (docs/06 §3).
public struct DefectRule: Sendable, Equatable, Identifiable {
    public var title: String
    public var signal: String
    public var threshold: Double
    /// True: defect when the signal is below the threshold (focus). False: above (closed eyes).
    public var below: Bool
    public var enabled = true
    public var id: String { signal }

    public static let focus = DefectRule(title: "Missed focus", signal: "focus", threshold: 0.40, below: true)
    public static let closedEyes = DefectRule(title: "Closed eyes", signal: "closed_eyes", threshold: 0.80, below: false)
    public static let highlights = DefectRule(title: "Blown highlights", signal: "highlight_clipping", threshold: 0.05, below: false)
    public static let defaults = [focus, closedEyes, highlights]
}

public struct DefectFinding: Sendable, Equatable, Identifiable {
    public var item: Int
    public var reasons: [String]
    public var id: Int { item }
}

public enum CullError: LocalizedError {
    case unavailable(String)
    public var errorDescription: String? {
        switch self { case .unavailable(let what): what }
    }
}

/// The app's one culling model. Engine libraries delegate every decision, group move, undo
/// and redo to the Rust `CullSession` and mirror the results densely for 60 fps cell updates;
/// the synthetic stub library uses the in-memory `CullStore` with the same semantics.
/// Not thread-safe: use it from the main actor. Engine calls write sidecars synchronously.
public final class CullController {
    public let groups: [Range<Int>]
    /// Suggested best item id per group (engine scorer; stub: first frame).
    public let bestOfGroup: [Int]
    public private(set) var states: [CullState]
    public private(set) var statuses: [ItemStatus]
    public private(set) var counts = CullStore.Counts()
    public private(set) var basketTarget: String
    public private(set) var albums: [AlbumSummary] = []

    private enum Backend {
        case engine(EngineLibrary)
        case memory(CullStore)
    }
    private var backend: Backend

    public init(engine library: EngineLibrary) {
        groups = library.groups
        bestOfGroup = library.bestOfGroup
        states = library.initialStates
        statuses = library.initialStatuses
        basketTarget = (try? library.session.basketTarget()) ?? EngineLibrary.defaultBasketTarget
        backend = .engine(library)
        recount()
        refreshAlbums()
    }

    public init(memory library: StubLibrary) {
        groups = library.groups
        bestOfGroup = library.groups.map(\.lowerBound)
        states = Array(repeating: CullState(), count: library.items.count)
        statuses = Array(repeating: ItemStatus(), count: library.items.count)
        basketTarget = "Basket"
        backend = .memory(CullStore(count: library.items.count))
        recount()
        refreshAlbums()
    }

    public var isEngineBacked: Bool { if case .engine = backend { true } else { false } }
    public subscript(id: Int) -> CullState { states[id] }

    public var canUndo: Bool {
        switch backend {
        case .engine(let lib): (try? lib.session.canUndo()) ?? false
        case .memory(let store): store.canUndo
        }
    }
    public var canRedo: Bool {
        switch backend {
        case .engine(let lib): (try? lib.session.canRedo()) ?? false
        case .memory(let store): store.canRedo
        }
    }

    public func group(of id: Int) -> Int? {
        groups.firstIndex { $0.contains(id) }
    }
    public func isSuggestedBest(_ id: Int) -> Bool {
        guard let g = group(of: id), groups[g].count > 1 else { return false }
        return bestOfGroup[g] == id
    }

    // MARK: Decisions

    /// Applies `action` to `ids` (one undo step). Single images use the session cursor, so the
    /// engine records the right before/after position for undo.
    @discardableResult
    public func apply(_ action: CullAction, to ids: [Int]) throws -> CullChange {
        guard !ids.isEmpty else { return CullChange(ids: [], current: nil, albumsChanged: false) }
        switch backend {
        case .memory(var store):
            let changed = store.apply(action, to: ids)
            backend = .memory(store)
            absorb(store)
            return CullChange(ids: changed, current: nil, albumsChanged: action == .toggleBasket)
        case .engine(let lib):
            let s = lib.session
            let keys = ids.map { lib.imageIDs[$0] }
            let update: CullUpdate
            if ids.count == 1 {
                try s.setCurrent(imageId: keys[0])
                update = switch action {
                case .reject: try s.decide(decision: .reject)
                case .undecided: try s.decide(decision: .undecided)
                case .keep: try s.decide(decision: .keep)
                case .grade(let g): try s.grade(grade: g)
                case .mark(let m): try s.mark(name: states[ids[0]].mark == m ? "" : Self.markName(m))
                case .toggleBasket: try s.toggleBasket()
                }
            } else {
                update = switch action {
                case .reject: try s.decideImages(imageIds: keys, decision: .reject)
                case .undecided: try s.decideImages(imageIds: keys, decision: .undecided)
                case .keep: try s.decideImages(imageIds: keys, decision: .keep)
                case .grade(let g): try s.gradeImages(imageIds: keys, grade: g)
                case .mark(let m):
                    try s.markImages(imageIds: keys, name: ids.allSatisfy { states[$0].mark == m } ? "" : Self.markName(m))
                case .toggleBasket: try s.setBasket(imageIds: keys, add: !ids.allSatisfy { states[$0].inBasket })
                }
            }
            return absorb(update, library: lib)
        }
    }

    /// A different decision per image as one undo step ("choose this" in compare).
    @discardableResult
    public func decide(_ decisions: [(Int, Decision)]) throws -> CullChange {
        switch backend {
        case .memory(var store):
            let updates = decisions.map { id, d in
                var s = states[id]
                s.decision = d
                if d != .keep { s.grade = 0 }
                return (id, s)
            }
            let changed = store.apply(states: updates)
            backend = .memory(store)
            absorb(store)
            return CullChange(ids: changed, current: nil, albumsChanged: false)
        case .engine(let lib):
            let update = try lib.session.decideEach(decisions: decisions.map { id, d in
                ImageDecision(imageId: lib.imageIDs[id], decision: Self.ffi(d))
            })
            return absorb(update, library: lib)
        }
    }

    /// Keeps the group's suggested best and rejects the rest (docs/06 §3), one undo step.
    public func keepBestRejectRest(group g: Int) throws -> (best: Int, change: CullChange) {
        guard groups.indices.contains(g) else { throw CullError.unavailable("No such group") }
        switch backend {
        case .memory:
            let best = bestOfGroup[g]
            return (best, try decide(groups[g].map { ($0, $0 == best ? .keep : .reject) }))
        case .engine(let lib):
            let result = try lib.session.keepBestRejectRest(group: UInt32(g))
            let best = lib.itemOfImage[result.best] ?? bestOfGroup[g]
            return (best, absorb(result.update, library: lib))
        }
    }

    public func undo() throws -> CullChange? {
        switch backend {
        case .memory(var store):
            guard let ids = store.undo() else { return nil }
            backend = .memory(store)
            absorb(store)
            return CullChange(ids: ids, current: ids.count == 1 ? ids[0] : nil, albumsChanged: true)
        case .engine(let lib):
            return try lib.session.undo().map { absorb($0, library: lib) }
        }
    }

    public func redo() throws -> CullChange? {
        switch backend {
        case .memory(var store):
            guard let ids = store.redo() else { return nil }
            backend = .memory(store)
            absorb(store)
            return CullChange(ids: ids, current: nil, albumsChanged: true)
        case .engine(let lib):
            return try lib.session.redo().map { absorb($0, library: lib) }
        }
    }

    // MARK: Navigation

    /// Mirrors the host cursor into the session (undo restores it).
    public func setCurrent(_ id: Int) {
        if case .engine(let lib) = backend { try? lib.session.setCurrent(imageId: lib.imageIDs[id]) }
    }

    /// Returns the target item, or nil at a boundary.
    public func navigate(_ move: GroupMove, from id: Int) throws -> Int? {
        switch backend {
        case .engine(let lib):
            let s = lib.session
            try s.setCurrent(imageId: lib.imageIDs[id])
            let key: String? = switch move {
            case .nextGroup: try s.nextGroup()
            case .previousGroup: try s.prevGroup()
            case .nextInGroup: try s.nextInGroup()
            case .previousInGroup: try s.prevInGroup()
            }
            guard let target = key.flatMap({ lib.itemOfImage[$0] }), target != id else { return nil }
            return target
        case .memory:
            guard let g = group(of: id) else { return nil }
            switch move {
            case .nextGroup: return g + 1 < groups.count ? groups[g + 1].lowerBound : nil
            case .previousGroup: return g > 0 ? groups[g - 1].lowerBound : nil
            case .nextInGroup: return groups[g].contains(id + 1) ? id + 1 : nil
            case .previousInGroup: return groups[g].contains(id - 1) ? id - 1 : nil
            }
        }
    }

    // MARK: Basket and albums (docs/06 §4.2)

    public func setBasketTarget(_ name: String) throws {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { throw CullError.unavailable("Album name is empty") }
        switch backend {
        case .memory:
            basketTarget = name
            refreshAlbums()
        case .engine(let lib):
            try lib.session.setBasketTarget(name: name)
            basketTarget = name
            refreshAlbums()
            let members = Set(albums.first { $0.name == name }?.members ?? [])
            for id in states.indices where states[id].inBasket != members.contains(id) {
                states[id].inBasket.toggle()
            }
            recount()
        }
    }

    /// Safe delete inside an album: removes membership only; files and decisions are untouched.
    @discardableResult
    public func removeFromAlbum(_ name: String, ids: [Int]) throws -> CullChange {
        switch backend {
        case .memory:
            guard name == basketTarget else { throw CullError.unavailable("No album named \(name)") }
            let members = ids.filter { states[$0].inBasket }
            return members.isEmpty ? CullChange(ids: [], current: nil, albumsChanged: false)
                : try apply(.toggleBasket, to: members)
        case .engine(let lib):
            let update = try lib.session.removeFromAlbum(album: name, imageIds: ids.map { lib.imageIDs[$0] })
            return absorb(update, library: lib)
        }
    }

    public func members(ofAlbum name: String) -> [Int] {
        albums.first { $0.name == name }?.members ?? []
    }

    /// "Delete from disk" (docs/06 §4.2): a separate, confirmed action. Removes the images from
    /// every album, then moves each file and its sidecars to the Trash (recoverable in Finder,
    /// not via ⌘Z). A shared `<stem>` sidecar is kept while a same-stem sibling remains.
    /// Returns the trashed originals. The caller reopens the folder afterwards.
    /// `trash` is injectable so tests never touch the user's Trash.
    public func moveToTrash(_ ids: [Int], trash: (URL) throws -> Void = {
        try FileManager.default.trashItem(at: $0, resultingItemURL: nil)
    }) throws -> [URL] {
        guard case .engine(let lib) = backend else { throw CullError.unavailable("Stub items have no files") }
        let doomed = Set(ids)
        for album in albums where album.members.contains(where: doomed.contains) {
            try removeFromAlbum(album.name, ids: album.members.filter(doomed.contains))
        }
        let fm = FileManager.default
        var trashed: [URL] = []
        for id in ids.sorted() {
            guard let url = lib.items[id].url else { continue }
            try trash(url)
            trashed.append(url)
            let folder = url.deletingLastPathComponent()
            let stem = url.deletingPathExtension().lastPathComponent
            let siblings = (try? fm.contentsOfDirectory(atPath: folder.path)) ?? []
            let stemShared = siblings.contains { name in
                let u = folder.appendingPathComponent(name)
                return u.deletingPathExtension().lastPathComponent == stem
                    && StubLibrary.kind(forExtension: u.pathExtension) != nil
            }
            var sidecars = [URL(fileURLWithPath: url.path + ".xmp")]
            if !stemShared {
                sidecars.append(folder.appendingPathComponent(stem + ".xmp"))
                sidecars.append(folder.appendingPathComponent(".edits").appendingPathComponent(stem + ".json"))
            }
            for sidecar in sidecars where fm.fileExists(atPath: sidecar.path) {
                try trash(sidecar)
            }
        }
        return trashed
    }

    // MARK: Defect sweep (review-only until applied)

    public func defectSweep(_ rules: [DefectRule]) throws -> [DefectFinding] {
        guard case .engine(let lib) = backend else { return [] }
        let thresholds = rules.filter(\.enabled).map {
            DefectThreshold(signal: $0.signal, value: $0.threshold, direction: $0.below ? .below : .above)
        }
        guard !thresholds.isEmpty else { return [] }
        let titles = Dictionary(uniqueKeysWithValues: rules.map { ($0.signal, $0.title) })
        return try lib.session.defectSweep(thresholds: thresholds).compactMap { candidate in
            guard let item = lib.itemOfImage[candidate.imageId] else { return nil }
            return DefectFinding(item: item, reasons: candidate.reasons.map { r in
                String(format: "%@ %.2f %@ %.2f", titles[r.signal] ?? r.signal, r.value,
                       r.direction == .below ? "<" : ">", r.threshold)
            })
        }.sorted { $0.item < $1.item }
    }

    // MARK: Helpers

    /// Keep names stable, rather than persisting UI key numbers as mark identifiers.
    public static let markNames: [UInt8: String] = [6: "Needs Retouch", 7: "Client Favourite", 8: "Print", 9: "Review"]
    static func markName(_ m: UInt8) -> String { markNames[m] ?? "Mark \(m)" }

    static func state(from s: TesseraFFI.Selection, inBasket: Bool) -> CullState {
        let decision: Decision = switch s.decision { case .keep: .keep; case .reject: .reject; case .undecided: .undecided }
        let mark = markNames.first { $0.value == s.mark }?.key ?? 0
        return CullState(decision: decision, grade: s.grade ?? 0, mark: mark, inBasket: inBasket)
    }

    static func ffi(_ d: Decision) -> TesseraFFI.Decision {
        switch d { case .keep: .keep; case .reject: .reject; case .undecided: .undecided }
    }

    private func absorb(_ store: CullStore) {
        states = store.states
        counts = store.counts
        refreshAlbums()
    }

    private func absorb(_ update: CullUpdate, library lib: EngineLibrary) -> CullChange {
        var ids: [Int] = []
        for change in update.changed {
            guard let id = lib.itemOfImage[change.imageId] else { continue }
            let new = Self.state(from: change.selection, inBasket: change.inBasket)
            adjust(states[id], by: -1)
            adjust(new, by: 1)
            states[id] = new
            ids.append(id)
        }
        if update.albumsChanged {
            refreshAlbums()
            refreshStatuses(ids)
        }
        return CullChange(ids: ids, current: update.current.flatMap { lib.itemOfImage[$0] },
                          albumsChanged: update.albumsChanged)
    }

    /// Re-reads derived status for `ids` (after album or recipe changes).
    public func refreshStatuses(_ ids: [Int]) {
        guard case .engine(let lib) = backend, !ids.isEmpty,
              let fresh = try? lib.session.derivedStatuses(imageIds: ids.map { lib.imageIDs[$0] })
        else { return }
        for (id, status) in zip(ids, fresh) { statuses[id] = ItemStatus(status) }
    }

    private func refreshAlbums() {
        var result: [AlbumSummary]
        switch backend {
        case .memory:
            result = [AlbumSummary(name: basketTarget, members: states.indices.filter { states[$0].inBasket })]
        case .engine(let lib):
            result = ((try? lib.session.albums()) ?? []).map { album in
                AlbumSummary(name: album.name, members: album.images.compactMap { lib.itemOfImage[$0] })
            }
        }
        if !result.contains(where: { $0.name == basketTarget }) {
            result.append(AlbumSummary(name: basketTarget, members: []))
        }
        albums = result.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    private func recount() {
        counts = CullStore.Counts()
        for s in states { adjust(s, by: 1) }
    }

    private func adjust(_ s: CullState, by d: Int) {
        switch s.decision {
        case .undecided: counts.undecided += d
        case .reject: counts.reject += d
        case .keep: counts.keep += d
        }
        if s.inBasket { counts.basket += d }
    }
}
