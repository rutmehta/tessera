import Foundation

// `DocumentChannelsBackend` on the stub (WP B5-08; `--stub-library` runs and unit tests). The stub keeps
// rectangular selections, so its channels are rectangles too (empty, whole canvas, or a rectangle) and
// combine by bounding boxes like the stub's selection tools. They are session state: stub history does
// not record them, and they have no thumbnails.

/// A stub channel's coverage.
enum StubChannelCoverage: Equatable {
    case none, all, rect(CanvasRect)

    func inverted() -> StubChannelCoverage {
        switch self {
        case .none: .all
        case .all: .none
        case .rect: .all   // a rectangle's complement is not a rectangle; its bounds are the canvas
        }
    }

    func bounds(_ canvas: CanvasRect) -> CGRect? {
        switch self {
        case .none: nil
        case .all: Self.cg(canvas)
        case .rect(let r): Self.cg(r)
        }
    }

    static func cg(_ r: CanvasRect) -> CGRect {
        CGRect(x: Double(r.x), y: Double(r.y), width: Double(r.width), height: Double(r.height))
    }

    init(_ r: CGRect?, canvas: CanvasRect) {
        guard let r = r?.integral, !r.isNull, r.width >= 1, r.height >= 1 else { self = .none; return }
        let c = CanvasRect(x: Int64(r.minX), y: Int64(r.minY), width: Int64(r.width), height: Int64(r.height))
        self = c == canvas ? .all : .rect(c)
    }

    /// `self` (current) combined with `new` by `op`, by bounding boxes.
    func combined(with new: StubChannelCoverage, op: SelectionCombine, canvas: CanvasRect) -> StubChannelCoverage {
        let a = bounds(canvas), b = new.bounds(canvas)
        switch op {
        case .replace: return new
        case .add: return StubChannelCoverage(a.map { x in b.map { x.union($0) } ?? x } ?? b, canvas: canvas)
        case .subtract: return b.map { $0.contains(a ?? .null) } == true ? .none : self
        case .intersect: return StubChannelCoverage(a.flatMap { x in b.map { x.intersection($0) } }, canvas: canvas)
        }
    }
}

struct StubChannel {
    var id: UInt64
    var name: String
    var kind: SavedChannelKind
    var color: ToolColor
    var opacity: Float
    var visible: Bool
    var coverage: StubChannelCoverage
    var revision: UInt64
}

/// One stub document's channels and its id counter.
struct StubChannelState {
    var list: [StubChannel] = []
    private var counter: UInt64 = 1
    mutating func next() -> UInt64 {
        defer { counter += 1 }
        return counter
    }
}

/// Channels of every stub document, keyed by backend identity.
final class StubChannelStore: @unchecked Sendable {
    static let shared = StubChannelStore()
    private let lock = NSLock()
    private var states: [ObjectIdentifier: StubChannelState] = [:]

    func with<T>(_ owner: AnyObject, _ body: (inout StubChannelState) throws -> T) rethrows -> T {
        lock.lock(); defer { lock.unlock() }
        let key = ObjectIdentifier(owner)
        var state = states[key] ?? StubChannelState()
        defer { states[key] = state }
        return try body(&state)
    }
}

extension StubDocumentBackend: DocumentChannelsBackend {
    private var store: StubChannelStore { .shared }

    private func canvasRect() throws -> CanvasRect {
        let i = try info()
        return CanvasRect(x: 0, y: 0, width: Int64(i.width), height: Int64(i.height))
    }

    private func selectionCoverage() throws -> StubChannelCoverage? {
        guard let r = try info().selectionBounds else { return nil }
        return StubChannelCoverage(StubChannelCoverage.cg(r), canvas: try canvasRect())
    }

    /// Channel edits are not stub history nodes: the change reports the unchanged head.
    private func sessionChange() throws -> DocumentChange {
        let i = try info()
        return DocumentChange(layersChanged: [], created: [], historyHead: i.historyHead, dirtyRect: nil, epoch: i.epoch,
                              dirty: i.dirty)
    }

    private static func check(_ name: String) throws -> String {
        let n = name.trimmingCharacters(in: .whitespaces)
        if n.isEmpty { throw DocumentError.invalid("channel name is empty") }
        return n
    }

    private static func checkSpot(_ c: ToolColor, _ s: Float) throws {
        if [c.r, c.g, c.b, s].contains(where: { !$0.isFinite || $0 < 0 || $0 > 1 }) {
            throw DocumentError.invalid("spot colour and solidity must be finite values within 0…1")
        }
    }

    private func add(_ name: String, kind: SavedChannelKind, color: ToolColor, opacity: Float,
                     coverage: StubChannelCoverage) throws -> SavedChannelChange {
        let id = store.with(self) { st in
            let id = st.next()
            let rev = st.next()
            st.list.append(StubChannel(id: id, name: name, kind: kind, color: color, opacity: opacity, visible: false,
                                       coverage: coverage, revision: rev))
            return id
        }
        return SavedChannelChange(channelID: id, change: try sessionChange())
    }

    private func modify(_ id: UInt64, _ body: (inout StubChannel) throws -> Void) throws {
        try store.with(self) { st in
            guard let i = st.list.firstIndex(where: { $0.id == id }) else { throw DocumentError.notFound("channel \(id)") }
            let rev = st.next()
            try body(&st.list[i])
            st.list[i].revision = rev
        }
    }

    public func documentChannels() throws -> [SavedChannel] {
        store.with(self) { st in
            st.list.enumerated().map { i, c in
                SavedChannel(id: c.id, kind: c.kind, name: c.name, color: c.color, opacity: c.opacity, visible: c.visible,
                             index: UInt32(i), revision: c.revision)
            }
        }
    }

    public func saveSelectionChannel(name: String, target: UInt64?, op: SelectionCombine) throws -> SavedChannelChange {
        guard let sel = try selectionCoverage() else { throw DocumentError.invalid("Save Selection: there is no selection") }
        guard let target else {
            return try add(try Self.check(name), kind: .alpha, color: ToolColor(r: 1, g: 0, b: 0), opacity: 0.5, coverage: sel)
        }
        let canvas = try canvasRect()
        try modify(target) { c in c.coverage = c.coverage.combined(with: sel, op: op, canvas: canvas) }
        return SavedChannelChange(channelID: target, change: try sessionChange())
    }

    public func loadSelectionChannel(id: UInt64, op: SelectionCombine, invert: Bool) throws -> DocumentChange {
        let coverage = try store.with(self) { st -> StubChannelCoverage in
            guard let c = st.list.first(where: { $0.id == id }) else { throw DocumentError.notFound("channel \(id)") }
            return invert ? c.coverage.inverted() : c.coverage
        }
        let canvas = try canvasRect()
        let current = try selectionCoverage() ?? StubChannelCoverage.none
        let out = op == .replace ? coverage : current.combined(with: coverage, op: op, canvas: canvas)
        switch out {
        case .none: return try clearSelection()
        case .all: return try setSelectionRect(x: 0, y: 0, width: canvas.width, height: canvas.height, feather: 0)
        case .rect(let r): return try setSelectionRect(x: r.x, y: r.y, width: r.width, height: r.height, feather: 0)
        }
    }

    public func newAlphaChannel(name: String, selected: Bool) throws -> SavedChannelChange {
        try add(try Self.check(name), kind: .alpha, color: ToolColor(r: 1, g: 0, b: 0), opacity: 0.5,
                coverage: selected ? .all : .none)
    }

    public func renameDocumentChannel(id: UInt64, name: String) throws -> DocumentChange {
        let n = try Self.check(name)
        try modify(id) { c in c.name = n }
        return try sessionChange()
    }

    public func deleteDocumentChannel(id: UInt64) throws -> DocumentChange {
        try store.with(self) { st in
            guard let i = st.list.firstIndex(where: { $0.id == id }) else { throw DocumentError.notFound("channel \(id)") }
            st.list.remove(at: i)
        }
        return try sessionChange()
    }

    public func duplicateDocumentChannel(id: UInt64) throws -> SavedChannelChange {
        let newID = try store.with(self) { st -> UInt64 in
            guard var c = st.list.first(where: { $0.id == id }) else { throw DocumentError.notFound("channel \(id)") }
            c.id = st.next()
            c.name += " copy"
            c.visible = false
            st.list.append(c)
            return c.id
        }
        return SavedChannelChange(channelID: newID, change: try sessionChange())
    }

    public func setSpotChannel(id: UInt64, color: ToolColor, solidity: Float) throws -> DocumentChange {
        try Self.checkSpot(color, solidity)
        try modify(id) { c in c.kind = .spot; c.color = color; c.opacity = solidity }
        return try sessionChange()
    }

    public func newSpotChannel(name: String, color: ToolColor, solidity: Float, fromSelection: Bool) throws
        -> SavedChannelChange {
        try Self.checkSpot(color, solidity)
        let n = try Self.check(name)
        var coverage = StubChannelCoverage.none
        if fromSelection {
            guard let sel = try selectionCoverage() else {
                throw DocumentError.invalid("New Spot Channel: there is no selection")
            }
            coverage = sel
        }
        return try add(n, kind: .spot, color: color, opacity: solidity, coverage: coverage)
    }

    public func channelThumbnail(id: UInt64, maxPx: UInt32) throws -> UInt32 {
        throw DocumentError.unsupported("Channel thumbnails need the engine (the stub backend has no pixels)")
    }

    public func setChannelVisible(id: UInt64, visible: Bool) throws {
        try modify(id) { c in c.visible = visible }
    }
}
