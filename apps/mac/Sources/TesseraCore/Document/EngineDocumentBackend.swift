import Foundation
import TesseraFFI

// The engine's layered documents behind document mode (WP B5-03).
//
// `EngineDocumentEngine` opens documents through `Engine` (B5-01, crates/tessera-ffi/src/document.rs)
// and `EngineDocumentBackend` adapts one `DocumentSession` to `DocumentBackend`, converting every
// record field by field (the name table in tools/orchestrate/wp/B5-02/IMPLEMENTATION-STATUS.md).
// The listener adapter hops engine render-thread callbacks to the main queue and coalesces them:
// however many frames arrive while the main thread is busy, the UI sees the newest frame, the union
// of changed layer ids and the latest history head, once.

// MARK: - Record conversion (TesseraFFI ⇄ TesseraCore)

extension DocBitDepth {
    init(_ d: DocDepth) {
        switch d {
        case .u8: self = .u8
        case .u16: self = .u16
        case .f32: self = .f32
        }
    }
    var ffi: DocDepth {
        switch self {
        case .u8: .u8
        case .u16: .u16
        case .f32: .f32
        }
    }
}

extension LayerKindTag {
    init(_ k: DocLayerKind) {
        switch k {
        case .pixel: self = .pixel
        case .adjustment: self = .adjustment
        case .fill: self = .fill
        case .group: self = .group
        case .smartObject: self = .smartObject
        case .text: self = .text
        }
    }
    var ffi: DocLayerKind {
        switch self {
        case .pixel: .pixel
        case .adjustment: .adjustment
        case .fill: .fill
        case .group: .group
        case .smartObject: .smartObject
        case .text: .text
        }
    }
}

extension LayerGroupMode {
    init(_ m: DocGroupMode) {
        switch m {
        case .passThrough: self = .passThrough
        case .isolated: self = .isolated
        }
    }
    var ffi: DocGroupMode {
        switch self {
        case .passThrough: .passThrough
        case .isolated: .isolated
        }
    }
}

extension LayerLockFlags {
    init(_ l: LayerLocks) { self.init(transparency: l.transparency, pixels: l.pixels, position: l.position, all: l.all) }
    var ffi: LayerLocks { LayerLocks(transparency: transparency, pixels: pixels, position: position, all: all) }
}

extension CanvasRect {
    init(_ r: DocRect) { self.init(x: r.x, y: r.y, width: r.width, height: r.height) }
    var ffi: DocRect { DocRect(x: x, y: y, width: width, height: height) }
}

extension LayerRecord {
    init(_ n: LayerNode) {
        self.init(id: n.id, parent: n.parent, index: n.index, depth: n.depth, kind: LayerKindTag(n.kind), name: n.name,
                  visible: n.visible, opacity: n.opacity, fillOpacity: n.fillOpacity, blendMode: n.blendMode,
                  groupMode: n.groupMode.map(LayerGroupMode.init), clipped: n.clipped, locks: LayerLockFlags(n.locks),
                  knockout: n.knockout, background: n.background, hasMask: n.hasMask, maskEnabled: n.maskEnabled,
                  maskLinked: n.maskLinked, maskDensity: n.maskDensity, adjustmentJson: n.adjustmentJson,
                  fillJson: n.fillJson, bounds: n.bounds.map(CanvasRect.init), revision: n.revision)
    }
    var ffi: LayerNode {
        LayerNode(id: id, parent: parent, index: index, depth: depth, kind: kind.ffi, name: name, visible: visible,
                  opacity: opacity, fillOpacity: fillOpacity, blendMode: blendMode, groupMode: groupMode?.ffi,
                  clipped: clipped, locks: locks.ffi, knockout: knockout, background: background, hasMask: hasMask,
                  maskEnabled: maskEnabled, maskLinked: maskLinked, maskDensity: maskDensity,
                  adjustmentJson: adjustmentJson, fillJson: fillJson, bounds: bounds?.ffi, revision: revision)
    }
}

extension NewLayerKind {
    init(_ n: NewLayer) {
        switch n {
        case .pixel: self = .pixel
        case .group(let mode): self = .group(mode: LayerGroupMode(mode))
        case .adjustment(let json): self = .adjustment(json: json)
        case .fill(let json): self = .fill(json: json)
        }
    }
    var ffi: NewLayer {
        switch self {
        case .pixel: .pixel
        case .group(let mode): .group(mode: mode.ffi)
        case .adjustment(let json): .adjustment(json: json)
        case .fill(let json): .fill(json: json)
        }
    }
}

extension LayerMaskInit {
    init(_ m: MaskInit) {
        switch m {
        case .revealAll: self = .revealAll
        case .hideAll: self = .hideAll
        case .fromSelection: self = .fromSelection
        }
    }
    var ffi: MaskInit {
        switch self {
        case .revealAll: .revealAll
        case .hideAll: .hideAll
        case .fromSelection: .fromSelection
        }
    }
}

extension LayerProperties {
    init(_ p: LayerPropsRecord) {
        self.init(name: p.name, visible: p.visible, opacity: p.opacity, fillOpacity: p.fillOpacity, blendMode: p.blendMode,
                  clipped: p.clipped, locks: LayerLockFlags(p.locks), knockout: p.knockout, colorTag: p.colorTag)
    }
    var ffi: LayerPropsRecord {
        LayerPropsRecord(name: name, visible: visible, opacity: opacity, fillOpacity: fillOpacity, blendMode: blendMode,
                         clipped: clipped, locks: locks.ffi, knockout: knockout, colorTag: colorTag)
    }
}

extension DocViewportPlan {
    init(_ p: DocSurfacePlan) { self.init(level: p.level, width: p.width, height: p.height) }
    var ffi: DocSurfacePlan { DocSurfacePlan(level: level, width: width, height: height) }
}

extension DocFrame {
    init(_ f: DocFrameInfo) {
        self.init(surfaceId: f.surfaceId, level: f.level, x: f.x, y: f.y, width: f.width, height: f.height,
                  canvasRect: CanvasRect(f.canvasRect), levelWidth: f.levelWidth, levelHeight: f.levelHeight, zoom: f.zoom,
                  epoch: f.epoch, renderMs: f.renderMs, fullRecomposite: f.fullRecomposite, blocks: f.blocks)
    }
    var ffi: DocFrameInfo {
        DocFrameInfo(surfaceId: surfaceId, level: level, x: x, y: y, width: width, height: height, canvasRect: canvasRect.ffi,
                     levelWidth: levelWidth, levelHeight: levelHeight, zoom: zoom, epoch: epoch, renderMs: renderMs,
                     fullRecomposite: fullRecomposite, blocks: blocks)
    }
}

extension DocExportFormat {
    init(_ f: ExportFormat) {
        switch f {
        case .png: self = .png
        case .jpeg: self = .jpeg
        case .tiff: self = .tiff
        }
    }
    var ffi: ExportFormat {
        switch self {
        case .png: .png
        case .jpeg: .jpeg
        case .tiff: .tiff
        }
    }
}

extension DocExportColor {
    init(_ c: ExportColor) {
        switch c {
        case .document: self = .document
        case .srgb: self = .srgb
        case .displayP3: self = .displayP3
        case .adobeRgb: self = .adobeRgb
        case .proPhoto: self = .proPhoto
        case .rec2020: self = .rec2020
        }
    }
    var ffi: ExportColor {
        switch self {
        case .document: .document
        case .srgb: .srgb
        case .displayP3: .displayP3
        case .adobeRgb: .adobeRgb
        case .proPhoto: .proPhoto
        case .rec2020: .rec2020
        }
    }
}

// MARK: - History ids

/// The UI's history ids: 0 is "as opened" (the Opened row, `checkoutHistory(id: 0)`, `historyHead == 0`),
/// every other id is the engine's history node id. The engine's base node (node 0 unless pruning removed
/// it, then the oldest retained parentless node) is the "as opened" state: it is not listed as a row, its
/// children list no parent, and its id reads as 0.
struct DocumentHistoryIDMap: Equatable, Sendable {
    /// The engine node shown as "Opened".
    var base: UInt64 = 0

    func ui(_ engineID: UInt64) -> DocHistoryID { engineID == base ? 0 : engineID }
    func engine(_ uiID: DocHistoryID) -> UInt64 { uiID == 0 ? base : uiID }

    /// Engine items (creation order) → the rows after "Opened"; updates `base`.
    mutating func rows(_ items: [DocHistoryItem]) -> [DocHistoryEntry] {
        if let root = items.filter({ $0.parent == nil }).map(\.id).min() { base = root }
        return items.filter { $0.id != base }.map {
            DocHistoryEntry(id: $0.id, label: $0.label, parent: $0.parent.flatMap { $0 == base ? nil : $0 },
                            isCurrent: $0.isCurrent, author: $0.author)
        }
    }
}

extension DocumentSummary {
    init(_ i: DocumentInfo, history: DocumentHistoryIDMap = DocumentHistoryIDMap()) {
        self.init(id: i.id, path: i.path, title: i.title, width: i.width, height: i.height, depth: DocBitDepth(i.depth),
                  profileName: i.profileName, dirty: i.dirty, historyHead: history.ui(i.historyHead), canUndo: i.canUndo,
                  canRedo: i.canRedo, selectedLayerIds: i.selectedLayerIds, selectionBounds: i.selectionBounds.map(CanvasRect.init),
                  sourceImageId: i.sourceImageId, layerCount: i.layerCount, epoch: i.epoch, backend: i.backend)
    }
    func ffi(history: DocumentHistoryIDMap = DocumentHistoryIDMap()) -> DocumentInfo {
        DocumentInfo(id: id, path: path, title: title, width: width, height: height, depth: depth.ffi, profileName: profileName,
                     dirty: dirty, historyHead: history.engine(historyHead), canUndo: canUndo, canRedo: canRedo,
                     selectedLayerIds: selectedLayerIds, selectionBounds: selectionBounds?.ffi, sourceImageId: sourceImageId,
                     layerCount: layerCount, epoch: epoch, backend: backend)
    }
}

extension DocumentChange {
    init(_ u: DocumentUpdate, history: DocumentHistoryIDMap = DocumentHistoryIDMap()) {
        self.init(layersChanged: u.layersChanged, created: u.created, historyHead: history.ui(u.historyHead),
                  dirtyRect: u.dirtyRect.map(CanvasRect.init), epoch: u.epoch, dirty: u.dirty)
    }
    func ffi(history: DocumentHistoryIDMap = DocumentHistoryIDMap()) -> DocumentUpdate {
        DocumentUpdate(layersChanged: layersChanged, created: created, historyHead: history.engine(historyHead),
                       dirtyRect: dirtyRect?.ffi, epoch: epoch, dirty: dirty)
    }
}

extension DocumentError {
    /// `BridgeError.Failure` messages (the engine's `EngineError` text) by kind.
    init(bridge: BridgeError) {
        let message: String
        switch bridge {
        case .Failure(let m): message = m
        }
        let lower = message.lowercased()
        if lower.contains("not found") || lower.contains("unknown layer") || lower.contains("no such layer")
            || lower.contains("no layer") {
            self = .notFound(message)
        } else if lower.contains("unsupported") || lower.contains("not supported") || lower.contains("cannot save")
            || lower.contains("not yet") {
            self = .unsupported(message)
        } else if lower.contains("os error") || lower.contains("no such file") || lower.contains("permission denied")
            || lower.contains("i/o") {
            self = .io(message)
        } else {
            self = .invalid(message)
        }
    }
}

/// Runs an FFI call, turning `BridgeError` into `DocumentError`.
@inline(__always)
func bridged<T>(_ body: () throws -> T) throws -> T {
    do { return try body() } catch let e as BridgeError { throw DocumentError(bridge: e) }
}

// MARK: - Listener adapter

/// `DocumentListener` (engine render thread) → `DocumentBackendListener` on the main queue, coalesced:
/// callbacks arriving before the main queue drains merge into one delivery (the newest frame, the union
/// of changed layers in first-seen order, the latest head, every failure).
public final class EngineDocumentListenerAdapter: DocumentListener, @unchecked Sendable {
    private struct Pending {
        var frame: DocFrame?
        var layers: [DocLayerID] = []
        var seen = Set<DocLayerID>()
        var head: DocHistoryID?
        var failures: [String] = []
        var scheduled = false
    }

    private let lock = NSLock()
    private var target: (any DocumentBackendListener)?
    private var pending = Pending()
    private let schedule: @Sendable (@escaping @Sendable () -> Void) -> Void
    private let mapHead: @Sendable (UInt64) -> DocHistoryID
    /// Engine callbacks received and main-queue deliveries made (coalescing statistics, tests).
    public private(set) var received = 0
    public private(set) var deliveries = 0

    /// `schedule` runs a drain on the main queue (tests pass their own); `mapHead` converts engine
    /// history ids (see `DocumentHistoryIDMap`).
    public init(target: any DocumentBackendListener,
                schedule: @escaping @Sendable (@escaping @Sendable () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) },
                mapHead: @escaping @Sendable (UInt64) -> DocHistoryID = { $0 }) {
        self.target = target
        self.schedule = schedule
        self.mapHead = mapHead
    }

    /// Stops deliveries (pending ones are dropped).
    public func cancel() {
        lock.lock()
        target = nil
        pending = Pending()
        lock.unlock()
    }

    private func enqueue(_ update: (inout Pending) -> Void) {
        lock.lock()
        guard target != nil else { lock.unlock(); return }
        received += 1
        update(&pending)
        let needsDrain = !pending.scheduled
        pending.scheduled = true
        lock.unlock()
        if needsDrain { schedule { [weak self] in self?.drain() } }
    }

    /// Delivers everything pending (main queue).
    func drain() {
        lock.lock()
        let p = pending
        pending = Pending()
        let target = self.target
        if target != nil { deliveries += 1 }
        lock.unlock()
        guard let target else { return }
        if !p.layers.isEmpty { target.onLayersChanged(layerIds: p.layers) }
        if let h = p.head { target.onHistoryChanged(head: h) }
        if let f = p.frame { target.onFrame(frame: f) }
        for m in p.failures { target.onRenderFailed(message: m) }
    }

    public func onFrame(frame: DocFrameInfo) {
        let f = DocFrame(frame)
        enqueue { $0.frame = f }
    }

    public func onLayersChanged(layerIds: [UInt64]) {
        enqueue { p in
            for id in layerIds where p.seen.insert(id).inserted { p.layers.append(id) }
        }
    }

    public func onHistoryChanged(head: UInt64) {
        let h = mapHead(head)
        enqueue { $0.head = h }
    }

    public func onRenderFailed(message: String) {
        enqueue { $0.failures.append(message) }
    }
}

// MARK: - Engine entry points

/// `Engine.newDocument / openDocument / openDocumentFromImage` as a `DocumentEngine`. One adapter per
/// `Engine` (`for(_:)`); a session opened twice (same path or image) returns the same backend object.
public final class EngineDocumentEngine: DocumentEngine, @unchecked Sendable {
    public let engine: Engine
    private let lock = NSLock()
    private var sessions: [String: WeakBackend] = [:]

    private final class WeakBackend { weak var value: EngineDocumentBackend?; init(_ v: EngineDocumentBackend) { value = v } }
    private final class WeakEngine { weak var value: EngineDocumentEngine?; init(_ v: EngineDocumentEngine) { value = v } }
    nonisolated(unsafe) private static var registry: [ObjectIdentifier: WeakEngine] = [:]
    private static let registryLock = NSLock()

    public init(engine: Engine) { self.engine = engine }

    /// The adapter of `engine` (created once, kept while documents or the app use it).
    public static func `for`(_ engine: Engine) -> EngineDocumentEngine {
        registryLock.lock(); defer { registryLock.unlock() }
        let key = ObjectIdentifier(engine)
        if let e = registry[key]?.value, e.engine === engine { return e }
        registry = registry.filter { $0.value.value != nil }
        let e = EngineDocumentEngine(engine: engine)
        registry[key] = WeakEngine(e)
        return e
    }

    /// Wraps a session, reusing the backend of a session already open (the engine returns the same
    /// session for the same path or image; its id is stable).
    func backend(for session: DocumentSession) -> EngineDocumentBackend {
        let id = session.id()
        lock.lock(); defer { lock.unlock() }
        if let b = sessions[id]?.value, !b.isClosed { return b }
        sessions = sessions.filter { $0.value.value.map { !$0.isClosed } ?? false }
        let b = EngineDocumentBackend(session: session)
        sessions[id] = WeakBackend(b)
        return b
    }

    public func newDocument(width: UInt32, height: UInt32, depth: DocBitDepth, profile: String?) throws -> any DocumentBackend {
        let s = try bridged { try engine.newDocument(width: width, height: height, depth: depth.ffi, profile: profile) }
        return backend(for: s)
    }

    /// Blocking (decodes the file): call off the main thread for large files.
    public func openDocument(path: String) throws -> any DocumentBackend {
        let s = try bridged { try engine.openDocument(path: path) }
        return backend(for: s)
    }

    /// Blocking (renders the image at full resolution): call off the main thread.
    public func openDocumentFromImage(imageId: String, developed: Bool) throws -> any DocumentBackend {
        let s = try bridged { try engine.openDocumentFromImage(imageId: imageId, developed: developed) }
        return backend(for: s)
    }

    /// The engine's blend mode names (`compositor::BlendMode` serde names, menu order).
    public static var blendModeNames: [String] { TesseraFFI.blendModeNames() }
}

// MARK: - Session adapter

/// One engine `DocumentSession` as a `DocumentBackend`: each call is the session call of the same
/// name, records converted field by field, history ids mapped (`DocumentHistoryIDMap`).
public final class EngineDocumentBackend: DocumentBackend, @unchecked Sendable {
    public let session: DocumentSession
    private let lock = NSLock()
    private var historyMap = DocumentHistoryIDMap()
    private var listener: EngineDocumentListenerAdapter?
    private var closed = false

    public init(session: DocumentSession) { self.session = session }

    var isClosed: Bool { lock.lock(); defer { lock.unlock() }; return closed }
    private var map: DocumentHistoryIDMap { lock.lock(); defer { lock.unlock() }; return historyMap }
    func change(_ body: () throws -> DocumentUpdate) throws -> DocumentChange {
        let u = try bridged(body)
        return DocumentChange(u, history: map)
    }

    public func id() -> String { session.id() }

    // Model reads

    public func info() throws -> DocumentSummary { DocumentSummary(try bridged { try session.info() }, history: map) }
    public func layers() throws -> [LayerRecord] { try bridged { try session.layers() }.map(LayerRecord.init) }
    public func layer(id: DocLayerID) throws -> LayerRecord { LayerRecord(try bridged { try session.layer(id: id) }) }
    public func setSelectedLayers(ids: [DocLayerID]) throws { try bridged { try session.setSelectedLayers(ids: ids) } }
    public func layerThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 {
        try bridged { try session.layerThumbnail(id: id, maxPx: maxPx) }
    }
    public func maskThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 {
        try bridged { try session.maskThumbnail(id: id, maxPx: maxPx) }
    }
    public func compositeThumbnail(maxPx: UInt32) throws -> UInt32 { try bridged { try session.compositeThumbnail(maxPx: maxPx) } }

    // Edits

    public func addLayer(kind: NewLayerKind, name: String, parent: DocLayerID?, index: UInt32?) throws -> DocumentChange {
        try change { try session.addLayer(kind: kind.ffi, name: name, parent: parent, index: index) }
    }
    public func duplicateLayer(id: DocLayerID) throws -> DocumentChange { try change { try session.duplicateLayer(id: id) } }
    public func removeLayer(id: DocLayerID) throws -> DocumentChange { try change { try session.removeLayer(id: id) } }
    public func moveLayer(id: DocLayerID, parent: DocLayerID?, index: UInt32) throws -> DocumentChange {
        try change { try session.moveLayer(id: id, parent: parent, index: index) }
    }
    public func setProps(id: DocLayerID, props: LayerProperties) throws -> DocumentChange {
        try change { try session.setProps(id: id, props: props.ffi) }
    }
    public func renameLayer(id: DocLayerID, name: String) throws -> DocumentChange {
        try change { try session.renameLayer(id: id, name: name) }
    }
    public func setVisible(id: DocLayerID, visible: Bool) throws -> DocumentChange {
        try change { try session.setVisible(id: id, visible: visible) }
    }
    public func setOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange {
        try change { try session.setOpacity(id: id, value: value, interactive: interactive) }
    }
    public func setFillOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange {
        try change { try session.setFillOpacity(id: id, value: value, interactive: interactive) }
    }
    public func setBlendMode(id: DocLayerID, mode: String) throws -> DocumentChange {
        try change { try session.setBlendMode(id: id, mode: mode) }
    }
    public func setGroupMode(id: DocLayerID, mode: LayerGroupMode) throws -> DocumentChange {
        try change { try session.setGroupMode(id: id, mode: mode.ffi) }
    }
    public func setLocks(id: DocLayerID, locks: LayerLockFlags) throws -> DocumentChange {
        try change { try session.setLocks(id: id, locks: locks.ffi) }
    }
    public func setAdjustmentJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange {
        try change { try session.setAdjustmentJson(id: id, json: json, interactive: interactive) }
    }
    public func setFillJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange {
        try change { try session.setFillJson(id: id, json: json, interactive: interactive) }
    }
    public func addMask(id: DocLayerID, mask: LayerMaskInit) throws -> DocumentChange {
        try change { try session.addMask(id: id, mask: mask.ffi) }
    }
    public func removeMask(id: DocLayerID) throws -> DocumentChange { try change { try session.removeMask(id: id) } }
    public func setMaskEnabled(id: DocLayerID, enabled: Bool) throws -> DocumentChange {
        try change { try session.setMaskEnabled(id: id, enabled: enabled) }
    }
    public func setMaskDensity(id: DocLayerID, density: Float) throws -> DocumentChange {
        try change { try session.setMaskDensity(id: id, density: density) }
    }
    public func setMaskLinked(id: DocLayerID, linked: Bool) throws { try bridged { try session.setMaskLinked(id: id, linked: linked) } }
    public func setClipped(id: DocLayerID, clipped: Bool) throws -> DocumentChange {
        try change { try session.setClipped(id: id, clipped: clipped) }
    }
    public func mergeDown(id: DocLayerID) throws -> DocumentChange { try change { try session.mergeDown(id: id) } }
    public func flatten() throws -> DocumentChange { try change { try session.flatten() } }
    public func groupLayers(ids: [DocLayerID], name: String) throws -> DocumentChange {
        try change { try session.groupLayers(ids: ids, name: name) }
    }
    public func ungroupLayer(id: DocLayerID) throws -> DocumentChange { try change { try session.ungroupLayer(id: id) } }
    public func setSelectionRect(x: Int64, y: Int64, width: Int64, height: Int64, feather: Float) throws -> DocumentChange {
        try change { try session.setSelectionRect(x: x, y: y, width: width, height: height, feather: feather) }
    }
    public func clearSelection() throws -> DocumentChange { try change { try session.clearSelection() } }
    public func commit(label: String) throws -> DocumentChange { try change { try session.commit(label: label) } }

    // History

    public func undo() throws -> DocumentChange { try change { try session.undo() } }
    public func redo() throws -> DocumentChange { try change { try session.redo() } }
    public func historyItems() throws -> [DocHistoryEntry] {
        let items = try bridged { try session.historyItems() }
        lock.lock(); defer { lock.unlock() }
        return historyMap.rows(items)
    }
    public func checkoutHistory(id: DocHistoryID) throws -> DocumentChange {
        let target = map.engine(id)
        return try change { try session.checkoutHistory(id: target) }
    }
    public func snapshot(name: String) throws { try bridged { try session.snapshot(name: name) } }
    public func snapshots() throws -> [String] { try bridged { try session.snapshots() } }
    public func restoreSnapshot(name: String) throws -> DocumentChange { try change { try session.restoreSnapshot(name: name) } }
    public func setMaxStates(maxStates: UInt32) throws { try bridged { try session.setMaxStates(maxStates: maxStates) } }
    public func historyMemoryBytes() throws -> UInt64 { try bridged { try session.historyMemoryBytes() } }

    // Presentation

    public func setListener(listener target: (any DocumentBackendListener)?) {
        lock.lock()
        let old = listener
        let adapter = target.map { t in
            EngineDocumentListenerAdapter(target: t, mapHead: { [weak self] in self?.map.ui($0) ?? $0 })
        }
        listener = adapter
        lock.unlock()
        old?.cancel()
        session.setListener(listener: adapter)
    }
    public func planSurface(width: UInt32, height: UInt32) throws -> DocViewportPlan {
        DocViewportPlan(try bridged { try session.planSurface(width: width, height: height) })
    }
    public func attachSurface(iosurfaceId: UInt32, width: UInt32, height: UInt32) throws {
        try bridged { try session.attachSurface(iosurfaceId: iosurfaceId, width: width, height: height) }
    }
    public func setViewport(level: UInt8, x: UInt32, y: UInt32, width: UInt32, height: UInt32, zoom: Double) throws {
        try bridged { try session.setViewport(level: level, x: x, y: y, width: width, height: height, zoom: zoom) }
    }
    public func setDisplayHeadroom(headroom: Float) throws { try bridged { try session.setDisplayHeadroom(headroom: headroom) } }
    public func refresh() throws { try bridged { try session.refresh() } }
    public func detachSurfaces() { session.detachSurfaces() }

    // Output

    public func save() throws { try bridged { try session.save() } }
    public func saveAs(path: String) throws { try bridged { try session.saveAs(path: path) } }
    public func exportFlat(path: String, format: DocExportFormat, quality: UInt8, color: DocExportColor) throws {
        try bridged { try session.exportFlat(path: path, format: format.ffi, quality: quality, color: color.ffi) }
    }
    public func close() {
        lock.lock()
        closed = true
        let l = listener
        listener = nil
        lock.unlock()
        l?.cancel()
        session.setListener(listener: nil)
        session.close()
    }
}
