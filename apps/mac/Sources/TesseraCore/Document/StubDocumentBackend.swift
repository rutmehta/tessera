import CoreGraphics
import Foundation
import ImageIO
import IOSurface
import UniformTypeIdentifiers

/// `DocumentEngine` without the engine: documents are `StubDocumentBackend`s (WP M5-13).
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

    typealias Viewport = (level: UInt8, x: UInt32, y: UInt32, width: UInt32, height: UInt32, zoom: Double)

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
    /// nil = as opened (reported as 0).
    private var head: DocHistoryID?
    /// Redo target per state (0 = as opened).
    private var lastChild: [DocHistoryID: DocHistoryID] = [:]
    private var pending = false
    private var savedHead: DocHistoryID?
    private var snaps: [(name: String, head: DocHistoryID?)] = []
    private var maxStates: UInt32 = 50
    private var selectedLayers: [DocLayerID] = []
    /// Mask links are session state (not history), as in the engine.
    private var linkOverrides: [DocLayerID: Bool] = [:]
    private var epoch: UInt64 = 1

    // Presentation
    private weak var listener: (any DocumentBackendListener)?
    private var surfaces: [(id: UInt32, surface: IOSurfaceRef)] = []
    private var ring = 0
    private var viewport: Viewport?
    private var renderRequest: (state: StubState, interactive: Bool)?
    private var rendering = false
    private var notifyLayers: Set<DocLayerID>?
    private var notifyHistory = false
    private var thumbnails: [String: IOSurfaceRef] = [:]
    private let renderQueue = DispatchQueue(label: "dev.tessera.stub-document.render", qos: .userInteractive)
    /// Renders so far (tests: thumbnails are cached per revision).
    public private(set) var renderCount = 0

    /// Texels computed per frame: beyond these the stub samples every 2nd (4th…) texel.
    static let finalBudget = 900_000
    static let interactiveBudget = 220_000

    init(state: StubState, title: String, path: String?, engine: StubDocumentEngine?) {
        Self.counter.lock()
        docID = "doc#\(Self.nextDocID)"
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

    /// Paper, a masked landscape, a clipped radial vignette fill and a "Grade" group of Curves and
    /// Hue/Saturation: six layers covering every row feature of the Layers panel.
    static func sampleState(width: UInt32, height: UInt32, depth: DocBitDepth, profile: String) -> StubState {
        let (w, h) = (Float(width), Float(height))
        let inset = StubPatterns.inset(width: w, height: h)
        let curves = AdjustmentModel.curves(master: [[0, 0], [0.25, 0.2], [0.75, 0.82], [1, 1]], rgb: [[], [], []]).json
        let hueSat = AdjustmentModel.hueSaturation(hue: 0, saturation: 15, lightness: 0, colorize: false).json
        let vignette = FillModel.gradient(radial: true, start: [Double(w) / 2, Double(h) / 2], end: [0, 0], stops: [
            .init(position: 0.45, color: [0.1, 0.06, 0.12, 0]), .init(position: 1, color: [0.1, 0.06, 0.12, 0.85]),
        ]).json
        let ellipse = CanvasRect(x: Int64(inset.minX) - Int64(w * 0.1), y: Int64(inset.minY) - Int64(h * 0.12),
                                 width: Int64(inset.width + CGFloat(w) * 0.2), height: Int64(inset.height + CGFloat(h) * 0.24))
        let root: [StubLayer] = [
            StubLayer(id: 1, props: StubProps(name: "Paper", locks: LayerLockFlags(position: true)),
                      kind: .pixel(.pattern("paper", canvasWidth: w, canvasHeight: h))),
            StubLayer(id: 2, props: StubProps(name: "Landscape"), kind: .pixel(.pattern("landscape", canvasWidth: w, canvasHeight: h)),
                      mask: StubMask(shape: .ellipse(ellipse))),
            StubLayer(id: 3, props: StubProps(name: "Vignette", opacity: 0.6, blendMode: "multiply", clipped: true),
                      kind: .fill(json: vignette)),
            StubLayer(id: 4, props: StubProps(name: "Grade"), kind: .group(mode: .passThrough, children: [
                StubLayer(id: 5, props: StubProps(name: "Curves 1"), kind: .adjustment(json: curves)),
                StubLayer(id: 6, props: StubProps(name: "Hue/Saturation 1", opacity: 0.8), kind: .adjustment(json: hueSat)),
            ])),
        ]
        return StubState(width: width, height: height, depth: depth, profile: profile, root: root, nextID: 7)
    }

    // MARK: Reads

    public func id() -> String { docID }

    public func info() throws -> DocumentSummary {
        lock.lock(); defer { lock.unlock() }
        return DocumentSummary(id: docID, path: path, title: title, width: live.width, height: live.height, depth: live.depth,
                               profileName: live.profile, dirty: isDirty, historyHead: head ?? 0,
                               canUndo: head != nil || pending, canRedo: !pending && lastChild[head ?? 0].flatMap { entries[$0] } != nil,
                               selectedLayerIds: selectedLayers.filter { live.layer($0) != nil }, selectionBounds: live.selection,
                               sourceImageId: sourceImageId, layerCount: UInt32(live.nodes.count), epoch: epoch, backend: "Stub (CPU)")
    }

    private var isDirty: Bool { pending || head != savedHead }

    public func layers() throws -> [LayerRecord] {
        lock.lock(); defer { lock.unlock() }
        return live.nodes.map { n in
            var n = n
            if let l = linkOverrides[n.id] { n.maskLinked = l }
            return n
        }
    }

    public func layer(id: DocLayerID) throws -> LayerRecord {
        guard let n = try layers().first(where: { $0.id == id }) else { throw DocumentError.notFound("layer \(id)") }
        return n
    }

    public func setSelectedLayers(ids: [DocLayerID]) throws {
        lock.lock(); selectedLayers = ids; lock.unlock()
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
            let v = mask.raw(x, y)
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
                Self.store(sample((Float(tx) + 0.5) / scale, (Float(ty) + 0.5) / scale), base + ty * stride + tx * 4)
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

    // MARK: Edit plumbing

    /// Applies one edit to the live state; non-interactive edits become a history node.
    /// `body` returns (rows changed, layers created).
    @discardableResult
    private func edit(_ label: String, interactive: Bool = false,
                      _ body: (inout StubState) throws -> (changed: [DocLayerID], created: [DocLayerID])) throws -> DocumentChange {
        lock.lock()
        if pending, !interactive { appendEntry("Change", live) }
        var s = live
        let result: (changed: [DocLayerID], created: [DocLayerID])
        do { result = try body(&s) } catch { lock.unlock(); throw error }
        s.revision += 1
        live = s
        epoch += 1
        if interactive { pending = true } else { appendEntry(label, s) }
        let change = changeLocked(result.changed, created: result.created)
        lock.unlock()
        post(layers: result.changed + result.created, history: !interactive)
        scheduleRender(interactive: interactive)
        return change
    }

    /// Caller holds the lock.
    private func changeLocked(_ changed: [DocLayerID], created: [DocLayerID] = [], dirtyRect: Bool = true) -> DocumentChange {
        DocumentChange(layersChanged: changed, created: created, historyHead: head ?? 0,
                       dirtyRect: dirtyRect ? CanvasRect(x: 0, y: 0, width: Int64(live.width), height: Int64(live.height)) : nil,
                       epoch: epoch, dirty: isDirty)
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

    /// Drops the oldest states beyond `maxStates` (their state becomes the base). The current
    /// state and snapshotted states are kept.
    private func prune() {
        let protected = Set(snaps.compactMap(\.head))
        while entries.count > Int(maxStates),
              let oldest = entries.keys.filter({ $0 != head && !protected.contains($0) }).min(),
              entries[oldest]?.parent == nil {
            let e = entries.removeValue(forKey: oldest)!
            base = e.state
            for (k, v) in entries where v.parent == oldest { entries[k]?.parent = nil }
            lastChild[0] = lastChild[oldest]
            lastChild[oldest] = nil
            if savedHead == oldest { savedHead = nil }
        }
    }

    private var committed: StubState { head.flatMap { entries[$0]?.state } ?? base }

    private static func newName(_ state: StubState, _ stem: String) -> String {
        var n = 1
        let names = Set(state.nodes.map(\.name))
        while names.contains("\(stem) \(n)") { n += 1 }
        return "\(stem) \(n)"
    }

    private func modifyOne(_ label: String, _ id: DocLayerID, interactive: Bool = false,
                           _ body: @escaping (inout StubLayer) throws -> Void) throws -> DocumentChange {
        try edit(label, interactive: interactive) { s in
            try s.modify(id, body)
            return ([id], [])
        }
    }

    // MARK: Edits

    public func addLayer(kind: NewLayerKind, name: String, parent: DocLayerID?, index: UInt32?) throws -> DocumentChange {
        try edit("New \(kind.historyNoun)") { s in
            let id = s.nextID
            s.nextID += 1
            let stubKind: StubKind
            switch kind {
            case .pixel: stubKind = .pixel(.empty)
            case .group(let mode): stubKind = .group(mode: mode, children: [])
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
            return ([id], [id])
        }
    }

    public func duplicateLayer(id: DocLayerID) throws -> DocumentChange {
        try edit("Duplicate Layer") { s in
            guard let l = s.layer(id), let parent = s.parent(of: id) else { throw DocumentError.notFound("layer \(id)") }
            var copy = s.renumbered(l)
            copy.props.name = l.props.name + " copy"
            copy.props.background = false
            let newID = copy.id
            try s.withChildren(of: parent) { kids in kids.insert(copy, at: kids.firstIndex { $0.id == id }! + 1) }
            return ([newID], [newID])
        }
    }

    public func removeLayer(id: DocLayerID) throws -> DocumentChange {
        try edit("Delete Layer") { s in _ = try s.take(id); return ([id], []) }
    }

    public func moveLayer(id: DocLayerID, parent: DocLayerID?, index: UInt32) throws -> DocumentChange {
        try edit("Move Layer") { s in
            if let parent {
                guard s.layer(parent) != nil else { throw DocumentError.notFound("layer \(parent)") }
                if let pp = s.path(of: parent), let ip = s.path(of: id), pp.starts(with: ip) {
                    throw DocumentError.invalid("A group cannot move into itself")
                }
            }
            let l = try s.take(id)
            try s.withChildren(of: parent) { kids in kids.insert(l, at: min(Int(index), kids.count)) }
            return ([id], [])
        }
    }

    public func setProps(id: DocLayerID, props: LayerProperties) throws -> DocumentChange {
        try modifyOne("Layer Properties", id) { l in
            l.props.name = props.name; l.props.visible = props.visible; l.props.opacity = props.opacity
            l.props.fillOpacity = props.fillOpacity; l.props.clipped = props.clipped
            l.props.locks = props.locks; l.props.colorTag = props.colorTag
            if props.blendMode != "pass_through" { l.props.blendMode = props.blendMode }
        }
    }

    public func renameLayer(id: DocLayerID, name: String) throws -> DocumentChange {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { throw DocumentError.invalid("Layer names cannot be empty") }
        return try modifyOne("Rename Layer", id) { $0.props.name = trimmed }
    }

    public func setVisible(id: DocLayerID, visible: Bool) throws -> DocumentChange {
        try modifyOne(visible ? "Show Layer" : "Hide Layer", id) { $0.props.visible = visible }
    }

    public func setOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange {
        try modifyOne("Opacity", id, interactive: interactive) { $0.props.opacity = min(max(value, 0), 1) }
    }

    public func setFillOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange {
        try modifyOne("Fill Opacity", id, interactive: interactive) { $0.props.fillOpacity = min(max(value, 0), 1) }
    }

    /// `pass_through` switches a group to pass-through; any mode on a group makes it isolated.
    public func setBlendMode(id: DocLayerID, mode: String) throws -> DocumentChange {
        if mode == "pass_through" { return try setGroupMode(id: id, mode: .passThrough) }
        guard DocBlendMode(backendName: mode) != nil else { throw DocumentError.invalid("Unknown blend mode \(mode)") }
        return try modifyOne("Blend Mode", id) { l in
            l.props.blendMode = mode
            if case .group(_, let kids) = l.kind { l.kind = .group(mode: .isolated, children: kids) }
        }
    }

    public func setGroupMode(id: DocLayerID, mode: LayerGroupMode) throws -> DocumentChange {
        try modifyOne("Group Mode", id) { l in
            guard case .group(_, let kids) = l.kind else { throw DocumentError.invalid("Not a group") }
            l.kind = .group(mode: mode, children: kids)
        }
    }

    public func setLocks(id: DocLayerID, locks: LayerLockFlags) throws -> DocumentChange {
        try modifyOne("Lock", id) { $0.props.locks = locks }
    }

    public func setAdjustmentJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange {
        guard let model = AdjustmentModel(json: json) else { throw DocumentError.invalid("Unknown adjustment") }
        return try modifyOne(model.kind.title, id, interactive: interactive) { l in
            guard case .adjustment = l.kind else { throw DocumentError.invalid("Not an adjustment layer") }
            l.kind = .adjustment(json: json)
        }
    }

    public func setFillJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange {
        guard FillModel(json: json) != nil else { throw DocumentError.invalid("Unknown fill") }
        return try modifyOne("Fill Content", id, interactive: interactive) { l in
            guard case .fill = l.kind else { throw DocumentError.invalid("Not a fill layer") }
            l.kind = .fill(json: json)
        }
    }

    public func addMask(id: DocLayerID, mask initial: LayerMaskInit) throws -> DocumentChange {
        lock.lock(); let sel = live.selection; lock.unlock()
        if initial == .fromSelection, sel == nil { throw DocumentError.invalid("Make a selection first (M, then drag)") }
        return try modifyOne("Add Layer Mask", id) { l in
            guard !l.props.background else { throw DocumentError.invalid("The Background layer cannot have a mask") }
            guard l.mask == nil else { throw DocumentError.invalid("The layer already has a mask") }
            let shape: StubMask.Shape = switch initial {
            case .revealAll: .reveal
            case .hideAll: .hide
            case .fromSelection: .rect(sel!, feather: 0)
            }
            l.mask = StubMask(shape: shape)
        }
    }

    public func removeMask(id: DocLayerID) throws -> DocumentChange {
        try modifyOne("Delete Layer Mask", id) { l in
            guard l.mask != nil else { throw DocumentError.invalid("Layer has no mask") }
            l.mask = nil
        }
    }

    public func setMaskEnabled(id: DocLayerID, enabled: Bool) throws -> DocumentChange {
        try modifyOne(enabled ? "Enable Layer Mask" : "Disable Layer Mask", id) { l in
            guard l.mask != nil else { throw DocumentError.invalid("Layer has no mask") }
            l.mask?.enabled = enabled
        }
    }

    public func setMaskDensity(id: DocLayerID, density: Float) throws -> DocumentChange {
        try modifyOne("Mask Density", id) { l in
            guard l.mask != nil else { throw DocumentError.invalid("Layer has no mask") }
            l.mask?.density = min(max(density, 0), 1)
        }
    }

    public func setMaskLinked(id: DocLayerID, linked: Bool) throws {
        lock.lock()
        guard live.layer(id)?.mask != nil else { lock.unlock(); throw DocumentError.invalid("Layer has no mask") }
        linkOverrides[id] = linked
        lock.unlock()
        post(layers: [id], history: false)
    }

    public func setClipped(id: DocLayerID, clipped: Bool) throws -> DocumentChange {
        try modifyOne(clipped ? "Create Clipping Mask" : "Release Clipping Mask", id) { $0.props.clipped = clipped }
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
            return ([lower.id, upper.id], [lower.id])
        }
    }

    public func flatten() throws -> DocumentChange {
        try edit("Flatten Image") { s in
            let (w, h) = (Float(s.width), Float(s.height))
            let white = StubLayer(id: 0, props: StubProps(name: "White"), kind: .fill(json: FillModel.solid(color: [1, 1, 1]).json))
            let id = s.nextID
            s.nextID += 1
            s.root = [StubLayer(id: id, props: StubProps(name: "Background", locks: LayerLockFlags(position: true), background: true),
                                kind: .pixel(.merged([white] + s.root)), revision: s.revision + 1)]
            _ = (w, h)
            return ([id], [id])
        }
    }

    public func groupLayers(ids: [DocLayerID], name: String) throws -> DocumentChange {
        try edit("Group Layers") { s in
            guard let first = ids.first, let parent = s.parent(of: first), let kids = s.children(of: parent) else {
                throw DocumentError.invalid("Select layers to group")
            }
            let members = kids.filter { ids.contains($0.id) }
            guard members.count == Set(ids).count else { throw DocumentError.invalid("Grouped layers must share a parent") }
            let top = kids.lastIndex { ids.contains($0.id) }!
            let insertAt = top - kids[..<top].filter { ids.contains($0.id) }.count
            let gid = s.nextID
            s.nextID += 1
            let group = StubLayer(id: gid, props: StubProps(name: name.isEmpty ? Self.newName(s, "Group") : name),
                                  kind: .group(mode: .passThrough, children: members), revision: s.revision + 1)
            try s.withChildren(of: parent) { list in
                list.removeAll { ids.contains($0.id) }
                list.insert(group, at: insertAt)
            }
            return (ids + [gid], [gid])
        }
    }

    public func ungroupLayer(id: DocLayerID) throws -> DocumentChange {
        try edit("Ungroup Layers") { s in
            guard let g = s.layer(id), case .group(_, let children) = g.kind, let parent = s.parent(of: id) else {
                throw DocumentError.invalid("Not a group")
            }
            try s.withChildren(of: parent) { list in
                let i = list.firstIndex { $0.id == id }!
                list.remove(at: i)
                list.insert(contentsOf: children, at: i)
            }
            return ([id] + children.map(\.id), [])
        }
    }

    public func setSelectionRect(x: Int64, y: Int64, width: Int64, height: Int64, feather: Float) throws -> DocumentChange {
        guard width > 0, height > 0 else { throw DocumentError.invalid("Empty selection") }
        return try edit("Rectangular Marquee") { s in
            s.selection = CanvasRect(x: x, y: y, width: width, height: height)
            return ([], [])
        }
    }

    public func clearSelection() throws -> DocumentChange {
        try edit("Deselect") { s in s.selection = nil; return ([], []) }
    }

    public func commit(label: String) throws -> DocumentChange {
        lock.lock()
        guard pending else {
            let c = changeLocked([], dirtyRect: false)
            lock.unlock()
            return c
        }
        appendEntry(label, live)
        epoch += 1
        let c = changeLocked([])
        lock.unlock()
        post(layers: [], history: true)
        scheduleRender(interactive: false)
        return c
    }

    // MARK: History

    public func undo() throws -> DocumentChange {
        lock.lock()
        if pending { appendEntry("Change", live) }
        guard let h = head else { let c = changeLocked([], dirtyRect: false); lock.unlock(); return c }
        head = entries[h]?.parent
        lastChild[head ?? 0] = h
        let rev = live.revision
        live = committed
        live.revision = max(live.revision, rev) + 1
        return historyMovedLocked()
    }

    public func redo() throws -> DocumentChange {
        lock.lock()
        guard !pending, let next = lastChild[head ?? 0], let e = entries[next] else {
            let c = changeLocked([], dirtyRect: false); lock.unlock(); return c
        }
        let rev = live.revision
        head = next
        live = e.state
        live.revision = max(live.revision, rev) + 1
        return historyMovedLocked()
    }

    /// Caller holds the lock; this releases it.
    private func historyMovedLocked() -> DocumentChange {
        epoch += 1
        let ids = live.nodes.map(\.id)
        let c = changeLocked(ids)
        lock.unlock()
        post(layers: ids, history: true)
        scheduleRender(interactive: false)
        return c
    }

    public func historyItems() throws -> [DocHistoryEntry] {
        lock.lock(); defer { lock.unlock() }
        return entries.values.sorted { $0.id < $1.id }.map {
            DocHistoryEntry(id: $0.id, label: $0.label, parent: $0.parent, isCurrent: $0.id == head, author: $0.author)
        }
    }

    public func checkoutHistory(id: DocHistoryID) throws -> DocumentChange {
        lock.lock()
        if pending { appendEntry("Change", live) }
        let rev = live.revision
        if id == 0 {
            if let h = head {
                var c = h
                while let p = entries[c]?.parent { c = p }
                lastChild[0] = c
            }
            head = nil
            live = base
            live.revision = rev + 1
            return historyMovedLocked()
        }
        guard let e = entries[id] else { lock.unlock(); throw DocumentError.notFound("history entry \(id)") }
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
        return historyMovedLocked()
    }

    public func snapshot(name: String) throws {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { throw DocumentError.invalid("Name the snapshot") }
        lock.lock()
        if pending { appendEntry("Change", live) }
        snaps.removeAll { $0.name == trimmed }
        snaps.append((trimmed, head))
        lock.unlock()
        post(layers: [], history: true)
    }

    public func snapshots() throws -> [String] {
        lock.lock(); defer { lock.unlock() }
        return snaps.map(\.name)
    }

    /// Restoring is itself an undoable step.
    public func restoreSnapshot(name: String) throws -> DocumentChange {
        lock.lock()
        guard let snap = snaps.first(where: { $0.name == name }) else { lock.unlock(); throw DocumentError.notFound("snapshot \(name)") }
        if pending { appendEntry("Change", live) }
        var s = snap.head.flatMap { entries[$0]?.state } ?? base
        s.revision = live.revision + 1
        live = s
        appendEntry("Snapshot “\(name)”", s)
        return historyMovedLocked()
    }

    public func setMaxStates(maxStates count: UInt32) throws {
        lock.lock(); maxStates = max(count, 2); prune(); lock.unlock()
    }

    public func historyMemoryBytes() throws -> UInt64 {
        lock.lock(); defer { lock.unlock() }
        // A stub layer record is a few hundred bytes; the engine counts distinct tile buffers.
        let layers = entries.values.reduce(0) { $0 + $1.state.nodes.count }
        return UInt64(layers * 320 + entries.count * 96)
    }

    // MARK: Presentation

    public func setListener(listener: (any DocumentBackendListener)?) {
        lock.lock(); self.listener = listener; lock.unlock()
    }

    /// The coarsest level whose extent covers a `width × height` fit-to-window viewport.
    public func planSurface(width: UInt32, height: UInt32) throws -> DocViewportPlan {
        lock.lock(); let (cw, ch) = (live.width, live.height); lock.unlock()
        var level = 0
        while level < 15, (cw >> (level + 1)) >= max(width, 1), (ch >> (level + 1)) >= max(height, 1) { level += 1 }
        return DocViewportPlan(level: UInt8(level), width: max(cw >> level, 1), height: max(ch >> level, 1))
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

    public func setViewport(level: UInt8, x: UInt32, y: UInt32, width: UInt32, height: UInt32, zoom: Double) throws {
        lock.lock()
        viewport = (level, x, y, width, height, zoom)
        lock.unlock()
        scheduleRender(interactive: false)
    }

    /// Stub frames are SDR.
    public func setDisplayHeadroom(headroom: Float) throws {}

    public func refresh() throws { scheduleRender(interactive: false) }

    public func detachSurfaces() {
        lock.lock(); surfaces.removeAll(); viewport = nil; lock.unlock()
    }

    /// Coalesces listener calls into one main-queue turn.
    private func post(layers: [DocLayerID], history: Bool) {
        lock.lock()
        let first = notifyLayers == nil && !notifyHistory
        notifyLayers = (notifyLayers ?? []).union(layers)
        notifyHistory = notifyHistory || history
        lock.unlock()
        guard first else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.lock.lock()
            let (l, h, target, head) = (self.notifyLayers, self.notifyHistory, self.listener, self.head ?? 0)
            self.notifyLayers = nil
            self.notifyHistory = false
            self.lock.unlock()
            if let l { target?.onLayersChanged(layerIds: Array(l)) }
            if h { target?.onHistoryChanged(head: head) }
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
            let ep = epoch
            lock.unlock()
            let frame = render(req.state, viewport: vp, interactive: req.interactive, into: target, epoch: ep)
            lock.lock()
            renderCount += 1
            let l = listener
            lock.unlock()
            if let frame { DispatchQueue.main.async { l?.onFrame(frame: frame) } }
        }
    }

    /// Renders the viewport region, clipped to the level and the surface (the engine's contract).
    private func render(_ state: StubState, viewport vp: Viewport, interactive: Bool,
                        into target: (id: UInt32, surface: IOSurfaceRef), epoch: UInt64) -> DocFrame? {
        let t0 = CFAbsoluteTimeGetCurrent()
        let sw = IOSurfaceGetWidth(target.surface), sh = IOSurfaceGetHeight(target.surface)
        let level = Int(vp.level)
        let lw = max(Int(state.width) >> level, 1), lh = max(Int(state.height) >> level, 1)
        let x0 = min(Int(vp.x), lw), y0 = min(Int(vp.y), lh)
        let rw = min(Int(vp.width), lw - x0, sw), rh = min(Int(vp.height), lh - y0, sh)
        guard rw > 0, rh > 0 else { return nil }
        // The CPU stub computes every `step`-th texel beyond its budget and repeats it.
        let budget = interactive ? Self.interactiveBudget : Self.finalBudget
        var step = 1
        while (rw / step) * (rh / step) > budget { step *= 2 }
        let scale = Float(1 << level)
        let comp = StubCompositor(state)
        IOSurfaceLock(target.surface, [], nil)
        let stride = IOSurfaceGetBytesPerRow(target.surface)
        nonisolated(unsafe) let bytes = IOSurfaceGetBaseAddress(target.surface).assumingMemoryBound(to: UInt8.self)
        let rows = (rh + step - 1) / step
        let bands = min(16, rows)
        DispatchQueue.concurrentPerform(iterations: bands) { band in
            for r in (rows * band / bands)..<(rows * (band + 1) / bands) {
                let ty = r * step
                let cy = (Float(y0 + ty) + 0.5 * Float(step)) * scale
                var tx = 0
                while tx < rw {
                    let c = comp.sample((Float(x0 + tx) + 0.5 * Float(step)) * scale, cy)
                    for dy in 0..<min(step, rh - ty) {
                        let line = bytes + (ty + dy) * stride
                        for dx in 0..<min(step, rw - tx) { Self.store(c, line + (tx + dx) * 4) }
                    }
                    tx += step
                }
            }
        }
        IOSurfaceUnlock(target.surface, [], nil)
        let cx = Int64(x0) << level, cy = Int64(y0) << level
        let canvas = CanvasRect(x: cx, y: cy, width: min(Int64(rw) << level, Int64(state.width) - cx),
                                height: min(Int64(rh) << level, Int64(state.height) - cy))
        return DocFrame(surfaceId: target.id, level: vp.level, x: UInt32(x0), y: UInt32(y0), width: UInt32(rw), height: UInt32(rh),
                        canvasRect: canvas, levelWidth: UInt32(lw), levelHeight: UInt32(lh), zoom: vp.zoom, epoch: epoch,
                        renderMs: (CFAbsoluteTimeGetCurrent() - t0) * 1000, fullRecomposite: true, blocks: UInt32(rows))
    }

    /// Composite of the whole canvas at level 0 (scaled to `maxEdge`) as straight RGBA8.
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
        case "psd", "psb": throw DocumentError.unsupported("Saving as PSD / PSB needs the engine (M5-13b); save as .tessera-doc")
        default: throw DocumentError.invalid("Documents save as .tessera-doc")
        }
        lock.lock()
        if pending { appendEntry("Change", live) }
        let state = live
        lock.unlock()
        do {
            try JSONEncoder().encode(File(state: state)).write(to: url, options: .atomic)
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
        post(layers: [], history: true)
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
        case .document, .srgb: srgb
        case .displayP3: CGColorSpace(name: CGColorSpace.displayP3)!
        case .adobeRgb: CGColorSpace(name: CGColorSpace.adobeRGB1998)!
        case .rec2020: CGColorSpace(name: CGColorSpace.itur_2020)!
        case .proPhoto: CGColorSpace(name: CGColorSpace.rommrgb) ?? srgb
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
