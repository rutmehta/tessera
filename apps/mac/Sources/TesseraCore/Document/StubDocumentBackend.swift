import CoreGraphics
import Foundation
import ImageIO
import IOSurface
import UniformTypeIdentifiers

/// `DocumentEngine` without the engine: documents are `StubDocumentBackend`s (WP M5-10).
public final class StubDocumentEngine: DocumentEngine, @unchecked Sendable {
    public static let shared = StubDocumentEngine()

    /// Resolves a library image id to its file (Library ▸ Edit in Layers). Ids of the form
    /// `file:<path>` resolve without it.
    public var imageURL: (@Sendable (String) -> URL?)?
    private let lock = NSLock()
    private var byPath: [String: Weak] = [:]
    private var untitled = 0
    private struct Weak { weak var value: StubDocumentBackend? }

    public init() {}

    /// A new document holding the six-layer sample artwork (the stub has no blank canvas to show).
    public func newDocument(width: UInt32, height: UInt32, depth: DocBitDepth, profile: String?) throws -> any DocumentBackend {
        guard width > 0, height > 0, width <= 30_000, height <= 30_000 else {
            throw DocumentError.invalid("Canvas size must be 1–30,000 pixels a side")
        }
        lock.lock(); untitled += 1; let n = untitled; lock.unlock()
        let state = StubDocumentBackend.sampleState(width: width, height: height, depth: depth,
                                                    profile: profile ?? "sRGB IEC61966-2.1")
        return StubDocumentBackend(state: state, title: "Untitled-\(n)", path: nil, engine: self)
    }

    public func openDocument(path: String) throws -> any DocumentBackend {
        let key = URL(fileURLWithPath: path).standardizedFileURL.path
        lock.lock()
        if let existing = byPath[key]?.value { lock.unlock(); return existing }
        lock.unlock()
        let url = URL(fileURLWithPath: key)
        let ext = url.pathExtension.lowercased()
        let backend: StubDocumentBackend
        if ext == "tessera-doc" {
            let data: Data
            do { data = try Data(contentsOf: url) } catch { throw DocumentError.io(error.localizedDescription) }
            guard let file = try? JSONDecoder().decode(StubDocumentBackend.File.self, from: data) else {
                throw DocumentError.invalid("\(url.lastPathComponent) is not a document this build can read")
            }
            backend = StubDocumentBackend(state: file.state, title: url.lastPathComponent, path: key, engine: self)
        } else if ["psd", "psb", "jpg", "jpeg", "png", "tif", "tiff", "heic"].contains(ext) {
            backend = try Self.imageDocument(url, engine: self, path: ext == "psd" || ext == "psb" ? key : nil, source: nil)
        } else {
            throw DocumentError.unsupported("Tessera opens .tessera-doc, .psd, .psb, JPEG, PNG and TIFF documents")
        }
        lock.lock(); byPath[key] = Weak(value: backend); lock.unlock()
        return backend
    }

    public func openDocumentFromImage(imageId: String, developed: Bool) throws -> any DocumentBackend {
        let url = imageId.hasPrefix("file:") ? URL(fileURLWithPath: String(imageId.dropFirst(5))) : imageURL?(imageId)
        guard let url else { throw DocumentError.notFound("image \(imageId)") }
        // The stub cannot develop RAW files: ImageIO's rendering (or the embedded preview) stands in.
        return try Self.imageDocument(url, engine: self, path: nil, source: imageId)
    }

    /// One pixel layer named after the file (PSD / PSB: the flattened composite; the stub has no PSD reader).
    static func imageDocument(_ url: URL, engine: StubDocumentEngine, path: String?, source: String?) throws -> StubDocumentBackend {
        guard let raster = StubImageCache.shared.raster(url.standardizedFileURL.path) else {
            throw DocumentError.io("Could not read \(url.lastPathComponent)")
        }
        let name = url.deletingPathExtension().lastPathComponent
        let layer = StubLayer(id: 1, props: StubProps(name: name), kind: .pixel(.image(path: url.standardizedFileURL.path)))
        let state = StubState(width: UInt32(raster.width), height: UInt32(raster.height), depth: .u8,
                              profile: "sRGB IEC61966-2.1", root: [layer], nextID: 2)
        let b = StubDocumentBackend(state: state, title: url.lastPathComponent, path: path, engine: engine)
        b.sourceImageId = source
        return b
    }

    func forget(_ path: String?) {
        guard let path else { return }
        lock.lock(); byPath[path] = nil; lock.unlock()
    }

    func remember(_ backend: StubDocumentBackend, path: String) {
        lock.lock(); byPath[path] = Weak(value: backend); lock.unlock()
    }
}

/// An in-memory layered document with snapshot history and a CPU compositor, implementing the
/// whole `DocumentBackend` surface so document mode runs (and is tested) without the engine.
/// Rendering runs on a serial queue, coalesced, one frame per request; listener calls arrive on
/// the main queue. Saves are JSON (`.tessera-doc` written by the stub is readable only by the stub).
public final class StubDocumentBackend: DocumentBackend, @unchecked Sendable {
    struct Entry {
        var id: DocHistoryID
        var label: String
        var parent: DocHistoryID?
        var state: StubState
        var author = "user"
    }

    struct File: Codable {
        var format = "tessera-doc-stub"
        var version = 1
        var state: StubState
    }

    private let lock = NSLock()
    private weak var engine: StubDocumentEngine?
    private static let counter = NSLock()
    nonisolated(unsafe) private static var nextDocID: UInt64 = 1

    private let docID: String
    private var title: String
    private var path: String?
    var sourceImageId: String?
    private var base: StubState
    private var live: StubState
    private var entries: [DocHistoryID: Entry] = [:]
    private var nextHistoryID: DocHistoryID = 1
    private var head: DocHistoryID?
    private var lastChild: [DocHistoryID: DocHistoryID] = [:]
    private var pending = false
    private var savedHead: DocHistoryID?
    private var snaps: [DocSnapshot] = []
    private var maxStates: UInt32 = 50
    private var selection: CanvasRect?

    // Presentation
    private weak var listener: (any DocumentBackendListener)?
    private var surfaces: [(id: UInt32, surface: IOSurfaceRef)] = []
    private var ring = 0
    private var viewport: (level: UInt8, rect: CanvasRect, zoom: Float)?
    private var renderRequest: (state: StubState, interactive: Bool)?
    private var rendering = false
    private var generation: UInt64 = 0
    private let renderQueue = DispatchQueue(label: "dev.tessera.stub-document.render", qos: .userInteractive)
    private var notifyLayers = false
    private var notifyHistory = false
    private var thumbnails: [String: IOSurfaceRef] = [:]
    /// Renders so far (tests).
    public private(set) var renderCount = 0

    /// Texel budgets per frame: the stub coarsens the level beyond these.
    static let finalBudget = 900_000
    static let interactiveBudget = 220_000

    init(state: StubState, title: String, path: String?, engine: StubDocumentEngine?) {
        Self.counter.lock()
        docID = String(Self.nextDocID)
        Self.nextDocID += 1
        Self.counter.unlock()
        self.title = title
        self.path = path
        base = state
        live = state
        self.engine = engine
    }

    /// A document with the six-layer sample (tests and New Document).
    public convenience init(sampleWidth: UInt32 = 1600, height: UInt32 = 1000) {
        self.init(state: Self.sampleState(width: sampleWidth, height: height, depth: .u8, profile: "sRGB IEC61966-2.1"),
                  title: "Sample", path: nil, engine: nil)
    }

    /// Paper, a masked landscape, a radial vignette fill and a "Grade" group of Curves and
    /// Hue/Saturation: six layers covering every row feature of the Layers panel.
    static func sampleState(width: UInt32, height: UInt32, depth: DocBitDepth, profile: String) -> StubState {
        let (w, h) = (Float(width), Float(height))
        let inset = StubPatterns.inset(width: w, height: h)
        let curves = AdjustmentModel.curves(master: [[0, 0], [0.25, 0.2], [0.75, 0.82], [1, 1]], rgb: [[], [], []]).json
        let hueSat = AdjustmentModel.hueSaturation(hue: 0, saturation: 15, lightness: 0, colorize: false).json
        let vignette = FillModel.gradient(radial: true, start: [Double(w) / 2, Double(h) / 2], end: [0, 0], stops: [
            .init(position: 0.45, color: [0.1, 0.06, 0.12, 0]), .init(position: 1, color: [0.1, 0.06, 0.12, 0.85]),
        ]).json
        let ellipse = CanvasRect(x: Int32(inset.minX) - Int32(w * 0.1), y: Int32(inset.minY) - Int32(h * 0.12),
                              width: UInt32(inset.width + CGFloat(w) * 0.2), height: UInt32(inset.height + CGFloat(h) * 0.24))
        let root: [StubLayer] = [
            StubLayer(id: 1, props: StubProps(name: "Paper", locks: LayerLockFlags(position: true)),
                      kind: .pixel(.pattern("paper", canvasWidth: w, canvasHeight: h))),
            StubLayer(id: 2, props: StubProps(name: "Landscape"), kind: .pixel(.pattern("landscape", canvasWidth: w, canvasHeight: h)),
                      mask: StubMask(shape: .ellipse(ellipse))),
            StubLayer(id: 3, props: StubProps(name: "Vignette", opacity: 0.6, blendMode: "multiply", clipped: true), kind: .fill(json: vignette)),
            StubLayer(id: 4, props: StubProps(name: "Grade"), kind: .group(mode: .passThrough, children: [
                StubLayer(id: 5, props: StubProps(name: "Curves 1"), kind: .adjustment(json: curves)),
                StubLayer(id: 6, props: StubProps(name: "Hue/Saturation 1", opacity: 0.8), kind: .adjustment(json: hueSat)),
            ])),
        ]
        return StubState(width: width, height: height, depth: depth, profile: profile, root: root, nextID: 7)
    }

    // MARK: Reads

    public func info() -> DocumentSummary {
        lock.lock(); defer { lock.unlock() }
        return DocumentSummary(id: docID, path: path, title: title, width: live.width, height: live.height, depth: live.depth,
                            profileName: live.profile, dirty: pending || head != savedHead, historyHead: head,
                            selectedLayerIds: [], sourceImageId: sourceImageId)
    }

    public func layers() -> [LayerRecord] {
        lock.lock(); defer { lock.unlock() }
        return live.nodes
    }

    public func layerThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 {
        lock.lock()
        guard let l = live.layer(id) else { lock.unlock(); throw DocumentError.notFound("layer \(id)") }
        let rev = live.nodes.first { $0.id == id }?.revision ?? l.revision
        let (w, h) = (live.width, live.height)
        lock.unlock()
        var solo = l
        solo.props.visible = true; solo.props.opacity = 1; solo.props.fillOpacity = 1
        solo.props.blendMode = "normal"; solo.props.clipped = false; solo.mask = nil
        if case .group(_, let kids) = solo.kind { solo.kind = .group(mode: .isolated, children: kids) }
        let comp = StubCompositor(layers: [solo], width: Float(w), height: Float(h))
        return try thumbnail(key: "l\(id):\(rev):\(maxPx)", width: w, height: h, maxPx: maxPx) { comp.sample($0, $1) }
    }

    public func maskThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 {
        lock.lock()
        guard let l = live.layer(id) else { lock.unlock(); throw DocumentError.notFound("layer \(id)") }
        let (w, h) = (live.width, live.height)
        lock.unlock()
        guard let mask = l.mask else { throw DocumentError.invalid("Layer has no mask") }
        return try thumbnail(key: "m\(id):\(l.revision):\(maxPx)", width: w, height: h, maxPx: maxPx) { x, y in
            let v = mask.value(x, y)
            return RGBA(v, v, v, 1)
        }
    }

    public func compositeThumbnail(maxPx: UInt32) throws -> UInt32 {
        lock.lock()
        let state = live
        lock.unlock()
        let comp = StubCompositor(state)
        return try thumbnail(key: "c:\(state.revision):\(maxPx)", width: state.width, height: state.height, maxPx: maxPx) {
            comp.sample($0, $1)
        }
    }

    private func thumbnail(key: String, width: UInt32, height: UInt32, maxPx: UInt32,
                           sample: (Float, Float) -> RGBA) throws -> UInt32 {
        lock.lock()
        if let s = thumbnails[key] { lock.unlock(); return IOSurfaceGetID(s) }
        lock.unlock()
        let scale = Float(max(maxPx, 1)) / Float(max(width, height))
        let tw = max(Int((Float(width) * scale).rounded()), 1), th = max(Int((Float(height) * scale).rounded()), 1)
        guard let surface = DocumentSurfaces.make(width: tw, height: th) else { throw DocumentError.io("surface") }
        IOSurfaceLock(surface, [], nil)
        let base = IOSurfaceGetBaseAddress(surface).assumingMemoryBound(to: UInt8.self)
        let stride = IOSurfaceGetBytesPerRow(surface)
        for ty in 0..<th {
            for tx in 0..<tw {
                let c = sample((Float(tx) + 0.5) / scale, (Float(ty) + 0.5) / scale)
                Self.store(c, base + ty * stride + tx * 4)
            }
        }
        IOSurfaceUnlock(surface, [], nil)
        lock.lock()
        renderCount += 1
        if thumbnails.count > 400 { thumbnails.removeAll() }
        thumbnails[key] = surface
        lock.unlock()
        return IOSurfaceGetID(surface)
    }

    @inline(__always)
    static func store(_ c: RGBA, _ p: UnsafeMutablePointer<UInt8>) {
        let q = { (v: Float) -> UInt8 in UInt8(min(max(v, 0), 1) * 255 + 0.5) }
        p[0] = q(c.x); p[1] = q(c.y); p[2] = q(c.z); p[3] = q(c.w)
    }

    // MARK: Edits

    private func label(_ l: StubLayer?) -> String { l?.props.name ?? "Layer" }

    /// Applies one edit to the live state; non-interactive edits become a history entry.
    @discardableResult
    private func edit(_ label: String, interactive: Bool = false, _ body: (inout StubState) throws -> [DocLayerID]) throws -> DocumentChange {
        lock.lock()
        if pending, !interactive { appendEntry("Change", live) }
        var s = live
        let changed: [DocLayerID]
        do { changed = try body(&s) } catch { lock.unlock(); throw error }
        s.revision += 1
        live = s
        if interactive { pending = true } else { appendEntry(label, s) }
        let update = DocumentChange(layersChanged: changed, historyHead: head,
                                    dirtyRect: CanvasRect(x: 0, y: 0, width: s.width, height: s.height))
        lock.unlock()
        post(layers: true, history: !interactive)
        scheduleRender(interactive: interactive)
        return update
    }

    /// Caller holds the lock.
    private func appendEntry(_ label: String, _ state: StubState) {
        let id = nextHistoryID
        nextHistoryID += 1
        entries[id] = Entry(id: id, label: label, parent: head, state: state)
        lastChild[head ?? 0] = id
        head = id
        pending = false
        prune()
    }

    /// Drops the oldest entries beyond `maxStates` (their state becomes the base).
    private func prune() {
        while entries.count > Int(maxStates), let oldest = entries.keys.min(), oldest != head {
            let e = entries.removeValue(forKey: oldest)!
            base = e.state
            for (k, v) in entries where v.parent == oldest { entries[k]?.parent = nil }
            if lastChild[oldest] != nil { lastChild[0] = lastChild[oldest]; lastChild[oldest] = nil }
            if savedHead == oldest { savedHead = nil }
            snaps.removeAll { $0.head == oldest }
        }
    }

    private var committed: StubState { head.flatMap { entries[$0]?.state } ?? base }

    private static func newName(_ state: StubState, _ stem: String) -> String {
        var n = 1
        let names = Set(state.nodes.map(\.name))
        while names.contains("\(stem) \(n)") { n += 1 }
        return "\(stem) \(n)"
    }

    public func addLayer(kind: NewLayerKind, name: String, parent: DocLayerID?, index: UInt32?) throws -> DocumentChange {
        try edit("New \(kind.historyNoun)") { s in
            let id = s.nextID
            s.nextID += 1
            let stubKind: StubKind
            switch kind {
            case .pixel: stubKind = .pixel(.empty)
            case .group: stubKind = .group(mode: .passThrough, children: [])
            case .adjustment(let json):
                guard AdjustmentModel(json: json) != nil else { throw DocumentError.invalid("Unknown adjustment") }
                stubKind = .adjustment(json: json)
            case .fill(let json):
                guard FillModel(json: json) != nil else { throw DocumentError.invalid("Unknown fill") }
                stubKind = .fill(json: json)
            }
            let n = name.isEmpty ? Self.newName(s, kind.defaultStem) : name
            let layer = StubLayer(id: id, props: StubProps(name: n), kind: stubKind, revision: s.revision + 1)
            try s.withChildren(of: parent) { kids in kids.insert(layer, at: min(Int(index ?? UInt32(kids.count)), kids.count)) }
            return [id]
        }
    }

    public func duplicateLayer(id: DocLayerID) throws -> DocumentChange {
        try edit("Duplicate Layer") { s in
            guard let l = s.layer(id), let parent = s.parent(of: id) else { throw DocumentError.notFound("layer \(id)") }
            var copy = s.renumbered(l)
            copy.props.name = l.props.name + " copy"
            copy.props.background = false
            let newID = copy.id
            try s.withChildren(of: parent) { kids in
                kids.insert(copy, at: kids.firstIndex { $0.id == id }! + 1)
            }
            return [newID]
        }
    }

    public func removeLayer(id: DocLayerID) throws -> DocumentChange {
        try edit("Delete Layer") { s in _ = try s.take(id); return [id] }
    }

    public func moveLayer(id: DocLayerID, parent: DocLayerID?, index: UInt32) throws -> DocumentChange {
        try edit("Move Layer") { s in
            if let parent {
                guard s.layer(parent) != nil else { throw DocumentError.notFound("layer \(parent)") }
                if parent == id || s.path(of: parent).map({ p in s.path(of: id).map { p.starts(with: $0) } ?? false }) == true {
                    throw DocumentError.invalid("A group cannot move into itself")
                }
            }
            let l = try s.take(id)
            try s.withChildren(of: parent) { kids in kids.insert(l, at: min(Int(index), kids.count)) }
            return [id]
        }
    }

    public func setProps(id: DocLayerID, props: LayerProperties) throws -> DocumentChange {
        try edit("Layer Properties") { s in
            try s.modify(id) { l in
                l.props.name = props.name; l.props.visible = props.visible; l.props.opacity = props.opacity
                l.props.fillOpacity = props.fillOpacity; l.props.blendMode = props.blendMode; l.props.clipped = props.clipped
                l.props.locks = props.locks; l.props.colorTag = props.colorTag
            }
            return [id]
        }
    }

    public func setVisible(id: DocLayerID, visible: Bool) throws -> DocumentChange {
        try edit(visible ? "Show Layer" : "Hide Layer") { s in try s.modify(id) { $0.props.visible = visible }; return [id] }
    }

    public func setOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange {
        try edit("Opacity", interactive: interactive) { s in try s.modify(id) { $0.props.opacity = min(max(value, 0), 1) }; return [id] }
    }

    public func setFillOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange {
        try edit("Fill Opacity", interactive: interactive) { s in
            try s.modify(id) { $0.props.fillOpacity = min(max(value, 0), 1) }; return [id]
        }
    }

    public func setBlendMode(id: DocLayerID, mode: String) throws -> DocumentChange {
        guard DocBlendMode(backendName: mode) != nil else { throw DocumentError.invalid("Unknown blend mode \(mode)") }
        return try edit("Blend Mode") { s in try s.modify(id) { $0.props.blendMode = mode }; return [id] }
    }

    public func setGroupMode(id: DocLayerID, mode: LayerGroupMode) throws -> DocumentChange {
        try edit("Group Mode") { s in
            try s.modify(id) { l in
                guard case .group(_, let kids) = l.kind else { throw DocumentError.invalid("Not a group") }
                l.kind = .group(mode: mode, children: kids)
            }
            return [id]
        }
    }

    public func setAdjustmentJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange {
        guard let model = AdjustmentModel(json: json) else { throw DocumentError.invalid("Unknown adjustment") }
        return try edit(model.kind.title, interactive: interactive) { s in
            try s.modify(id) { l in
                guard case .adjustment = l.kind else { throw DocumentError.invalid("Not an adjustment layer") }
                l.kind = .adjustment(json: json)
            }
            return [id]
        }
    }

    public func setFillJson(id: DocLayerID, json: String) throws -> DocumentChange {
        guard FillModel(json: json) != nil else { throw DocumentError.invalid("Unknown fill") }
        return try edit("Fill Content") { s in
            try s.modify(id) { l in
                guard case .fill = l.kind else { throw DocumentError.invalid("Not a fill layer") }
                l.kind = .fill(json: json)
            }
            return [id]
        }
    }

    public func setMaskEnabled(id: DocLayerID, enabled: Bool) throws -> DocumentChange {
        try edit(enabled ? "Enable Layer Mask" : "Disable Layer Mask") { s in
            try s.modify(id) { l in
                guard l.mask != nil else { throw DocumentError.invalid("Layer has no mask") }
                l.mask?.enabled = enabled
            }
            return [id]
        }
    }

    public func setMaskLinked(id: DocLayerID, linked: Bool) throws -> DocumentChange {
        try edit(linked ? "Link Layer Mask" : "Unlink Layer Mask") { s in
            try s.modify(id) { l in
                guard l.mask != nil else { throw DocumentError.invalid("Layer has no mask") }
                l.mask?.linked = linked
            }
            return [id]
        }
    }

    public func removeMask(id: DocLayerID) throws -> DocumentChange {
        try edit("Delete Layer Mask") { s in
            try s.modify(id) { l in
                guard l.mask != nil else { throw DocumentError.invalid("Layer has no mask") }
                l.mask = nil
            }
            return [id]
        }
    }

    public func addMask(id: DocLayerID, initial: LayerMaskInit) throws -> DocumentChange {
        lock.lock(); let sel = selection; lock.unlock()
        if initial == .fromSelection, sel == nil { throw DocumentError.invalid("Make a selection first (M, then drag)") }
        return try edit("Add Layer Mask") { s in
            try s.modify(id) { l in
                guard !l.props.background else { throw DocumentError.invalid("The Background layer cannot have a mask") }
                let shape: StubMask.Shape = switch initial {
                case .revealAll: .reveal
                case .hideAll: .hide
                case .fromSelection: .rect(sel!, feather: 0)
                }
                l.mask = StubMask(shape: shape)
            }
            return [id]
        }
    }

    public func setClipped(id: DocLayerID, clipped: Bool) throws -> DocumentChange {
        try edit(clipped ? "Create Clipping Mask" : "Release Clipping Mask") { s in
            try s.modify(id) { $0.props.clipped = clipped }; return [id]
        }
    }

    public func mergeDown(id: DocLayerID) throws -> DocumentChange {
        try edit("Merge Down") { s in
            guard let parent = s.parent(of: id), let kids = s.children(of: parent),
                  let i = kids.firstIndex(where: { $0.id == id }) else { throw DocumentError.notFound("layer \(id)") }
            guard i > 0 else { throw DocumentError.invalid("There is no layer below “\(kids[i].props.name)” to merge into") }
            let lower = kids[i - 1], upper = kids[i]
            var merged = lower
            merged.kind = .pixel(.merged([lower, upper]))
            merged.props.opacity = 1; merged.props.fillOpacity = 1; merged.props.blendMode = "normal"; merged.mask = nil
            merged.revision = s.revision + 1
            try s.withChildren(of: parent) { list in
                list.remove(at: i)
                list[i - 1] = merged
            }
            return [lower.id, upper.id]
        }
    }

    public func flatten() throws -> DocumentChange {
        try edit("Flatten Image") { s in
            let all = s.root
            let id = s.nextID
            s.nextID += 1
            s.root = [StubLayer(id: id, props: StubProps(name: "Background", locks: LayerLockFlags(position: true), background: true),
                                kind: .pixel(.merged(all)), revision: s.revision + 1)]
            return [id]
        }
    }

    public func setSelectionRect(x: Int32, y: Int32, width: UInt32, height: UInt32, feather: Float) throws {
        guard width > 0, height > 0 else { throw DocumentError.invalid("Empty selection") }
        lock.lock(); selection = CanvasRect(x: x, y: y, width: width, height: height); lock.unlock()
    }

    public func clearSelection() throws {
        lock.lock(); selection = nil; lock.unlock()
    }

    public func commit(label: String) throws {
        lock.lock()
        guard pending else { lock.unlock(); return }
        appendEntry(label, live)
        lock.unlock()
        post(layers: false, history: true)
        scheduleRender(interactive: false)
    }

    // MARK: History

    public func undo() throws -> Bool {
        lock.lock()
        if pending { appendEntry("Change", live) }
        guard let h = head else { lock.unlock(); return false }
        head = entries[h]?.parent
        lastChild[head ?? 0] = h
        live = committed
        live.revision = max(live.revision, entries[h]?.state.revision ?? 0) + 1
        lock.unlock()
        historyMoved()
        return true
    }

    public func redo() throws -> Bool {
        lock.lock()
        guard !pending, let next = lastChild[head ?? 0], let e = entries[next] else { lock.unlock(); return false }
        let rev = live.revision
        head = next
        live = e.state
        live.revision = max(live.revision, rev) + 1
        lock.unlock()
        historyMoved()
        return true
    }

    public func historyItems() -> [DocHistoryEntry] {
        lock.lock(); defer { lock.unlock() }
        return entries.values.sorted { $0.id < $1.id }.map {
            DocHistoryEntry(id: $0.id, label: $0.label, parent: $0.parent, isCurrent: $0.id == head, author: $0.author)
        }
    }

    public func checkoutHistory(id: DocHistoryID) throws {
        lock.lock()
        if pending { appendEntry("Change", live) }
        if id == 0 {
            // 0 = as opened (engine-api: head None).
            let rev = live.revision
            if let h = head { lastChild[0] = rootAncestor(h) }
            head = nil
            live = base
            live.revision = rev + 1
            lock.unlock()
            historyMoved()
            return
        }
        guard let e = entries[id] else { lock.unlock(); throw DocumentError.notFound("history entry \(id)") }
        let rev = live.revision
        head = id
        live = e.state
        live.revision = max(live.revision, rev) + 1
        var child = id
        var p = e.parent
        while true {
            lastChild[p ?? 0] = child
            guard let q = p else { break }
            child = q
            p = entries[q]?.parent
        }
        lock.unlock()
        historyMoved()
    }

    private func rootAncestor(_ id: DocHistoryID) -> DocHistoryID {
        var c = id
        while let p = entries[c]?.parent { c = p }
        return c
    }

    private func historyMoved() {
        post(layers: true, history: true)
        scheduleRender(interactive: false)
    }

    public func snapshot(name: String) throws {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { throw DocumentError.invalid("Name the snapshot") }
        lock.lock()
        if pending { appendEntry("Change", live) }
        snaps.removeAll { $0.name == trimmed }
        snaps.append(DocSnapshot(name: trimmed, head: head))
        lock.unlock()
        post(layers: false, history: true)
    }

    public func snapshots() -> [DocSnapshot] {
        lock.lock(); defer { lock.unlock() }
        return snaps
    }

    /// Restoring is itself an undoable step.
    public func restoreSnapshot(name: String) throws {
        lock.lock()
        guard let snap = snaps.first(where: { $0.name == name }) else { lock.unlock(); throw DocumentError.notFound("snapshot \(name)") }
        if pending { appendEntry("Change", live) }
        var s = snap.head.flatMap { entries[$0]?.state } ?? base
        s.revision = live.revision + 1
        live = s
        appendEntry("Snapshot “\(name)”", s)
        lock.unlock()
        historyMoved()
    }

    public func setMaxStates(_ count: UInt32) {
        lock.lock(); maxStates = max(count, 1); prune(); lock.unlock()
    }

    public func historyMemoryBytes() -> UInt64 {
        lock.lock(); defer { lock.unlock() }
        // A layer record is a few hundred bytes in the stub; the engine counts copied tiles instead.
        let layers = entries.values.reduce(0) { $0 + $1.state.nodes.count }
        return UInt64(layers * 320 + entries.count * 96)
    }

    // MARK: Presentation

    public func setListener(_ listener: (any DocumentBackendListener)?) {
        lock.lock(); self.listener = listener; lock.unlock()
    }

    public func planSurface(width: UInt32, height: UInt32) -> DocViewportPlan {
        DocViewportPlan(level: 0, width: min(max(width, 1), 8192), height: min(max(height, 1), 8192))
    }

    public func attachSurface(iosurfaceId: UInt32, width: UInt32, height: UInt32) throws {
        guard let s = IOSurfaceLookup(iosurfaceId) else { throw DocumentError.notFound("surface \(iosurfaceId)") }
        lock.lock()
        if let first = surfaces.first, IOSurfaceGetWidth(first.surface) != IOSurfaceGetWidth(s)
            || IOSurfaceGetHeight(first.surface) != IOSurfaceGetHeight(s) { surfaces.removeAll() }
        surfaces.removeAll { $0.id == iosurfaceId }
        surfaces.append((iosurfaceId, s))
        if surfaces.count > 3 { surfaces.removeFirst(surfaces.count - 3) }
        lock.unlock()
    }

    public func setViewport(level: UInt8, x: Int32, y: Int32, width: UInt32, height: UInt32, zoom: Float) {
        lock.lock()
        viewport = (level, CanvasRect(x: x, y: y, width: width, height: height), zoom)
        lock.unlock()
        scheduleRender(interactive: false)
    }

    public func setDisplayHeadroom(_ headroom: Float) {}

    public func refresh() { scheduleRender(interactive: false) }

    public func detachSurfaces() {
        lock.lock(); surfaces.removeAll(); viewport = nil; lock.unlock()
    }

    private func post(layers: Bool, history: Bool) {
        lock.lock()
        let first = !notifyLayers && !notifyHistory
        notifyLayers = notifyLayers || layers
        notifyHistory = notifyHistory || history
        lock.unlock()
        guard first else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.lock.lock()
            let (l, h, target) = (self.notifyLayers, self.notifyHistory, self.listener)
            self.notifyLayers = false
            self.notifyHistory = false
            self.lock.unlock()
            if l { target?.onLayersChanged() }
            if h { target?.onHistoryChanged() }
        }
    }

    private func scheduleRender(interactive: Bool) {
        lock.lock()
        guard viewport != nil, !surfaces.isEmpty else { lock.unlock(); return }
        renderRequest = (live, interactive)
        let start = !rendering
        rendering = true
        lock.unlock()
        if start { renderQueue.async { [weak self] in self?.renderLoop() } }
    }

    private func renderLoop() {
        while true {
            lock.lock()
            guard let req = renderRequest, let vp = viewport, !surfaces.isEmpty else {
                rendering = false
                lock.unlock()
                return
            }
            renderRequest = nil
            ring = (ring + 1) % surfaces.count
            let target = surfaces[ring]
            generation += 1
            let gen = generation
            lock.unlock()
            let frame = render(req.state, viewport: vp, interactive: req.interactive, into: target, generation: gen)
            lock.lock()
            renderCount += 1
            let l = listener
            lock.unlock()
            if let frame { DispatchQueue.main.async { l?.onFrame(frame) } }
        }
    }

    private func render(_ state: StubState, viewport vp: (level: UInt8, rect: CanvasRect, zoom: Float), interactive: Bool,
                        into target: (id: UInt32, surface: IOSurfaceRef), generation: UInt64) -> DocFrame? {
        let t0 = CFAbsoluteTimeGetCurrent()
        let rect = vp.rect
        guard !rect.isEmpty else { return nil }
        let sw = IOSurfaceGetWidth(target.surface), sh = IOSurfaceGetHeight(target.surface)
        var level = Int(vp.level)
        let budget = interactive ? Self.interactiveBudget : Self.finalBudget
        var size = DocumentViewportMath.levelSize(rect, level: level)
        while (Int(size.width) * Int(size.height) > budget || Int(size.width) > sw || Int(size.height) > sh), level < 15 {
            level += 1
            size = DocumentViewportMath.levelSize(rect, level: level)
        }
        let (w, h) = (Int(size.width), Int(size.height))
        let comp = StubCompositor(state)
        let sx = Float(rect.width) / Float(w), sy = Float(rect.height) / Float(h)
        let (ox, oy) = (Float(rect.x), Float(rect.y))
        IOSurfaceLock(target.surface, [], nil)
        let base = IOSurfaceGetBaseAddress(target.surface)
        let stride = IOSurfaceGetBytesPerRow(target.surface)
        nonisolated(unsafe) let bytes = base.assumingMemoryBound(to: UInt8.self)
        let bands = 16
        DispatchQueue.concurrentPerform(iterations: bands) { band in
            let y0 = h * band / bands, y1 = h * (band + 1) / bands
            for ty in y0..<y1 {
                let cy = oy + (Float(ty) + 0.5) * sy
                let row = bytes + ty * stride
                for tx in 0..<w {
                    Self.store(comp.sample(ox + (Float(tx) + 0.5) * sx, cy), row + tx * 4)
                }
            }
        }
        IOSurfaceUnlock(target.surface, [], nil)
        return DocFrame(surfaceId: target.id, level: UInt8(level), rect: rect, width: UInt32(w), height: UInt32(h),
                            renderMs: (CFAbsoluteTimeGetCurrent() - t0) * 1000, generation: generation, isFinal: !interactive)
    }

    /// Composite of the whole canvas at level 0 as straight RGBA8 (tests, export).
    public func renderCanvas(maxEdge: Int = 8192) -> (width: Int, height: Int, rgba: [UInt8]) {
        lock.lock(); let state = live; lock.unlock()
        let scale = min(Float(maxEdge) / Float(max(state.width, state.height)), 1)
        let w = max(Int(Float(state.width) * scale), 1), h = max(Int(Float(state.height) * scale), 1)
        let comp = StubCompositor(state)
        var out = [UInt8](repeating: 0, count: w * h * 4)
        out.withUnsafeMutableBufferPointer { buf in
            nonisolated(unsafe) let p = buf.baseAddress!
            DispatchQueue.concurrentPerform(iterations: 16) { band in
                for y in (h * band / 16)..<(h * (band + 1) / 16) {
                    for x in 0..<w {
                        Self.store(comp.sample((Float(x) + 0.5) / scale, (Float(y) + 0.5) / scale), p + (y * w + x) * 4)
                    }
                }
            }
        }
        return (w, h, out)
    }

    // MARK: Output

    public func save() throws {
        lock.lock(); let p = path; lock.unlock()
        guard let p, p.hasSuffix(".tessera-doc") else {
            throw DocumentError.invalid("Choose File ▸ Save As… to save this document as .tessera-doc")
        }
        try saveAs(path: p)
    }

    public func saveAs(path newPath: String) throws {
        let url = URL(fileURLWithPath: newPath).standardizedFileURL
        switch url.pathExtension.lowercased() {
        case "tessera-doc": break
        case "psd", "psb": throw DocumentError.unsupported("Saving as PSD / PSB needs the engine (M5-10b); save as .tessera-doc")
        default: throw DocumentError.invalid("Documents save as .tessera-doc")
        }
        lock.lock()
        if pending { appendEntry("Change", live) }
        let state = live
        lock.unlock()
        do {
            let data = try JSONEncoder().encode(File(state: state))
            try data.write(to: url, options: .atomic)
        } catch {
            throw DocumentError.io("Could not save \(url.lastPathComponent): \(error.localizedDescription)")
        }
        lock.lock()
        let old = path
        path = url.path
        title = url.lastPathComponent
        savedHead = head
        lock.unlock()
        if old != url.path { engine?.forget(old); engine?.remember(self, path: url.path) }
        post(layers: false, history: true)
    }

    public func exportFlat(path: String, format: DocExportFormat, quality: UInt8, color: DocExportColor) throws {
        let (w, h, rgba) = renderCanvas(maxEdge: 30_000)
        var bytes = rgba
        if format == .jpeg {
            // No alpha in JPEG: composite over white.
            for i in stride(from: 0, to: bytes.count, by: 4) {
                let a = Float(bytes[i + 3]) / 255
                for c in 0..<3 { bytes[i + c] = UInt8(Float(bytes[i + c]) * a + 255 * (1 - a) + 0.5) }
                bytes[i + 3] = 255
            }
        }
        let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
        guard let provider = CGDataProvider(data: Data(bytes) as CFData),
              let image = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4, space: srgb,
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider,
                                  decode: nil, shouldInterpolate: false, intent: .defaultIntent) else {
            throw DocumentError.io("Could not build the image")
        }
        let target: CGColorSpace = switch color {
        case .srgb: srgb
        case .displayP3: CGColorSpace(name: CGColorSpace.displayP3)!
        case .rec2020: CGColorSpace(name: CGColorSpace.itur_2020)!
        case .prophoto: CGColorSpace(name: CGColorSpace.rommrgb) ?? srgb
        }
        // CoreGraphics converts to the export space when drawing.
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: target,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { throw DocumentError.io("colour") }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        guard let converted = ctx.makeImage() else { throw DocumentError.io("colour") }
        let type: UTType = switch format { case .jpeg: .jpeg; case .png: .png; case .tiff: .tiff }
        guard let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, type.identifier as CFString, 1, nil) else {
            throw DocumentError.io("Could not write \(path)")
        }
        CGImageDestinationAddImage(dest, converted, [kCGImageDestinationLossyCompressionQuality: Double(quality) / 100] as CFDictionary)
        guard CGImageDestinationFinalize(dest) else { throw DocumentError.io("Could not write \(path)") }
    }

    public func close() {
        lock.lock()
        surfaces.removeAll()
        viewport = nil
        listener = nil
        thumbnails.removeAll()
        let p = path
        lock.unlock()
        engine?.forget(p)
    }
}

extension NewLayerKind {
    var historyNoun: String {
        switch self {
        case .pixel: "Layer"
        case .group: "Group"
        case .adjustment(let json): AdjustmentModel(json: json)?.kind.title ?? "Adjustment"
        case .fill(let json): (FillModel(json: json)?.kind.title ?? "Fill") + " Fill"
        }
    }
    var defaultStem: String {
        switch self {
        case .pixel: "Layer"
        case .group: "Group"
        case .adjustment(let json): AdjustmentModel(json: json)?.kind.title ?? "Adjustment"
        case .fill(let json): FillModel(json: json)?.kind.title ?? "Fill"
        }
    }
}
