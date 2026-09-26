import AppKit
import Observation
import TesseraCore

/// One open layered document: the observed model of the Layers, Properties and History panels
/// over a `DocumentBackend` (the stub now, M5-09's `DocumentSession` from M5-13b on). Every edit
/// goes through here so errors land in the status bar and the panels refresh once per change.
@MainActor @Observable
final class DocumentController: Identifiable {
    let backend: any DocumentBackend
    let id: String
    private(set) var info: DocumentSummary
    private(set) var layers: [LayerRecord] = []
    private(set) var outline = DocumentOutline()
    /// Selected layers; the last is the primary one (Properties, blend mode, opacity).
    var selection: [DocLayerID] = [] {
        didSet {
            guard selection != oldValue else { return }
            try? backend.setSelectedLayers(ids: selection)
            onSelectionChange?()
        }
    }
    private(set) var history: [DocHistoryEntry] = []
    private(set) var snapshots: [String] = []
    private(set) var memoryBytes: UInt64 = 0
    /// Bumped whenever `layers` is reloaded (panels refresh non-dragging controls on it).
    private(set) var revision = 0
    var tool: DocumentTool = .move
    /// The marquee selection in canvas pixels (from `info().selectionBounds`).
    var marquee: CanvasRect? { info.selectionBounds }
    /// Zoom of the viewport (screen pixels per canvas pixel) and when it last changed (HUD).
    var zoom: Double = 1
    var zoomChangedAt: Date?

    /// Reports a message (the status bar).
    @ObservationIgnored var report: ((String) -> Void)?
    /// The viewport presenting frames of this document.
    @ObservationIgnored weak var viewport: DocumentViewportView?
    /// Viewport state kept per document, so switching tabs keeps each document's zoom and position.
    @ObservationIgnored var viewState: DocumentViewportMath?
    /// The Layers outline (AppKit) refreshes itself from these.
    @ObservationIgnored var onLayersReload: ((_ old: DocumentOutline, _ new: DocumentOutline) -> Void)?
    @ObservationIgnored var onSelectionChange: (() -> Void)?
    @ObservationIgnored var onFrame: ((DocFrame) -> Void)?
    @ObservationIgnored private var bridge: ListenerBridge?
    /// Live adjustment models during a drag (so several fields of one drag compose).
    @ObservationIgnored private var liveAdjustment: [DocLayerID: AdjustmentModel] = [:]
    @ObservationIgnored private var fillDebounce: Task<Void, Never>?

    init(backend: any DocumentBackend) throws {
        self.backend = backend
        info = try backend.info()
        id = backend.id()
        let bridge = ListenerBridge()
        bridge.controller = self
        self.bridge = bridge
        backend.setListener(listener: bridge)
        reloadModel()
        reloadHistory()
        if let top = outline.children(of: DocumentOutline.root).first { selection = [top] }
    }

    var title: String { info.title }
    var isDirty: Bool { info.dirty }
    var primary: LayerRecord? { selection.last.flatMap { outline.node($0) } }
    func node(_ id: DocLayerID) -> LayerRecord? { outline.node(id) }

    // MARK: Model refresh

    func reloadModel() {
        do {
            let old = outline
            layers = try backend.layers()
            outline = DocumentOutline(layers)
            info = try backend.info()
            let kept = selection.filter { outline.contains($0) }
            if kept != selection { selection = kept }
            revision += 1
            onLayersReload?(old, outline)
            viewport?.selectionDidChange()
        } catch {
            report?("Layers: \(error.localizedDescription)")
        }
    }

    func reloadHistory() {
        do {
            history = try backend.historyItems()
            snapshots = try backend.snapshots()
            memoryBytes = try backend.historyMemoryBytes()
            info = try backend.info()
        } catch {
            report?("History: \(error.localizedDescription)")
        }
    }

    /// "render: L2 1368 × 912, 3.8 ms" of the latest frame, at most ten updates a second (Debug ▸ Show
    /// Render Timing).
    private(set) var renderReadout: String?
    /// The latest presented frame (the listener's `FrameInfo`).
    @ObservationIgnored private(set) var lastFrame: DocFrame?
    /// Called with every frame after the viewport (self-test timing).
    @ObservationIgnored var frameObserver: ((DocFrame) -> Void)?
    @ObservationIgnored private var readoutAt = Date.distantPast
    @ObservationIgnored private var readoutPending = false
    @ObservationIgnored private let logFrames = ProcessInfo.processInfo.environment["TESSERA_DOC_FRAME_LOG"] != nil

    fileprivate func listenerFrame(_ f: DocFrame) {
        lastFrame = f
        onFrame?(f)
        frameObserver?(f)
        if logFrames {
            FileHandle.standardError.write(Data(String(format: "doc-frame: epoch %llu L%u %u×%u render %.2f ms%@\n", f.epoch,
                                                       UInt32(f.level), f.width, f.height, f.renderMs,
                                                       f.fullRecomposite ? " full" : "").utf8))
        }
        let wait = 0.1 - Date().timeIntervalSince(readoutAt)
        if wait <= 0 {
            publishReadout()
        } else if !readoutPending {
            readoutPending = true
            Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(Int(wait * 1000) + 1))
                self?.readoutPending = false
                self?.publishReadout()
            }
        }
    }

    private func publishReadout() {
        guard let f = lastFrame else { return }
        readoutAt = Date()
        renderReadout = String(format: "render: L%u %u × %u, %.1f ms", UInt32(f.level), f.width, f.height, f.renderMs)
    }

    // MARK: Running edits

    /// Runs one backend edit; reports failures. Returns the change on success.
    @discardableResult
    func run(_ what: String, _ body: () throws -> DocumentChange) -> DocumentChange? {
        do {
            let c = try body()
            reloadModel()
            reloadHistory()
            return c
        } catch {
            report?("\(what): \(error.localizedDescription)")
            return nil
        }
    }

    private func attempt(_ what: String, _ body: () throws -> Void) {
        do { try body() } catch { report?("\(what): \(error.localizedDescription)") }
    }

    /// Where a new layer goes: above the primary selection, in its parent.
    private var insertionPoint: (parent: DocLayerID?, index: UInt32?) {
        guard let p = primary else { return (nil, nil) }
        return (p.parent, p.index + 1)
    }

    func select(_ id: DocLayerID) { selection = [id] }

    // MARK: Layer edits

    func addLayer(_ kind: NewLayerKind, name: String = "") {
        let at = insertionPoint
        if let c = run("New layer", { try backend.addLayer(kind: kind, name: name, parent: at.parent, index: at.index) }),
           let id = c.created.first ?? c.layersChanged.first {
            selection = [id]
        }
    }

    func addAdjustment(_ kind: AdjustmentModel.Kind) { addLayer(.adjustment(json: kind.neutral.json)) }

    func addFill(_ kind: FillModel.Kind) {
        addLayer(.fill(json: FillModel.neutral(kind, width: Double(info.width), height: Double(info.height)).json))
    }

    func duplicateSelection() {
        var added: [DocLayerID] = []
        for id in selection {
            if let c = run("Duplicate", { try backend.duplicateLayer(id: id) }), let n = c.created.first { added.append(n) }
        }
        if !added.isEmpty { selection = added }
    }

    func deleteSelection() {
        guard !selection.isEmpty else { return }
        let below = primary.flatMap { p in
            outline.position(of: p.id).flatMap { pos in
                let kids = outline.children(of: pos.parent)
                return kids.dropFirst(pos.index + 1).first { !selection.contains($0) } ?? kids.first { !selection.contains($0) }
            }
        }
        for id in selection where outline.contains(id) { run("Delete layer") { try backend.removeLayer(id: id) } }
        selection = below.map { [$0] } ?? []
    }

    func rename(_ id: DocLayerID, to name: String) {
        guard let n = node(id), !name.isEmpty, name != n.name else { return }
        run("Rename") { try backend.renameLayer(id: id, name: name) }
    }

    func setVisible(_ id: DocLayerID, _ visible: Bool) { run("Visibility") { try backend.setVisible(id: id, visible: visible) } }

    /// ⌥-click on an eye: show only this layer among its siblings (or all of them again).
    func soloVisibility(_ id: DocLayerID) {
        let others = layers.filter { $0.id != id && $0.parent == node(id)?.parent }
        let allHidden = others.allSatisfy { !$0.visible }
        for o in others where o.visible != allHidden { run("Visibility") { try backend.setVisible(id: o.id, visible: allHidden) } }
        if node(id)?.visible == false { setVisible(id, true) }
    }

    /// The Opacity slider: live while dragging (no history), one node on release.
    func setOpacity(_ value: Double, final: Bool) {
        guard let id = primary?.id else { return }
        run("Opacity") { try backend.setOpacity(id: id, value: Float(value / 100), interactive: true) }
        if final { commit("Opacity \(Int(value.rounded())) %") }
    }

    func setFillOpacity(_ value: Double, final: Bool) {
        guard let id = primary?.id else { return }
        run("Fill") { try backend.setFillOpacity(id: id, value: Float(value / 100), interactive: true) }
        if final { commit("Fill \(Int(value.rounded())) %") }
    }

    private func commit(_ label: String) {
        run(label) { try backend.commit(label: label) }
    }

    /// A blend mode, or nil for Pass Through (groups only).
    func setBlendMode(_ mode: DocBlendMode?) {
        for id in selection {
            guard let n = node(id) else { continue }
            if let mode {
                run("Blend mode") { try backend.setBlendMode(id: id, mode: mode.backendName) }
            } else if n.kind == .group {
                run("Blend mode") { try backend.setGroupMode(id: id, mode: .passThrough) }
            }
        }
    }

    func setGroupMode(_ mode: LayerGroupMode) {
        guard let id = primary?.id else { return }
        run("Group mode") { try backend.setGroupMode(id: id, mode: mode) }
    }

    enum Lock: String, CaseIterable { case transparency, pixels, position, all }

    func toggleLock(_ lock: Lock) {
        let on = !isLocked(lock)
        for id in selection {
            guard var locks = node(id)?.locks else { continue }
            switch lock {
            case .transparency: locks.transparency = on
            case .pixels: locks.pixels = on
            case .position: locks.position = on
            case .all: locks.all = on
            }
            run("Lock") { try backend.setLocks(id: id, locks: locks) }
        }
    }

    func isLocked(_ lock: Lock) -> Bool {
        guard let l = primary?.locks else { return false }
        return switch lock {
        case .transparency: l.transparency
        case .pixels: l.pixels
        case .position: l.position
        case .all: l.all
        }
    }

    func toggleClipping() {
        guard let n = primary else { return }
        run("Clipping mask") { try backend.setClipped(id: n.id, clipped: !n.clipped) }
    }

    // MARK: Groups and merging

    /// ⌘G: the selected siblings into a new group (one history node), or an empty group.
    func groupSelection() {
        let chosen = outline.flattened.filter { selection.contains($0) }
        guard let first = chosen.first else { addLayer(.group(mode: .passThrough)); return }
        let parent = outline.position(of: first)?.parent
        let siblings = chosen.filter { outline.position(of: $0)?.parent == parent }
        if siblings.count < chosen.count { report?("Group: only layers with the same parent are grouped") }
        if let c = run("Group", { try backend.groupLayers(ids: siblings, name: "") }), let g = c.created.first {
            selection = [g]
        }
    }

    /// ⇧⌘G: the group's children take its place.
    func ungroupSelection() {
        guard let g = primary, g.kind == .group else {
            report?("Ungroup: select a group")
            return
        }
        let kids = outline.children(of: g.id)
        if run("Ungroup", { try backend.ungroupLayer(id: g.id) }) != nil { selection = kids.reversed() }
    }

    func mergeDown() {
        guard let id = primary?.id else { return }
        if let c = run("Merge Down", { try backend.mergeDown(id: id) }), let merged = c.created.first { selection = [merged] }
    }

    func flatten() {
        if let c = run("Flatten", { try backend.flatten() }), let id = c.created.first { selection = [id] }
    }

    /// Drag and drop: `ids` into `parent` before the row at top-first `index`.
    @discardableResult
    func moveLayers(_ ids: [DocLayerID], into parent: DocLayerID, at index: Int) -> Bool {
        guard let target = outline.moving(ids, into: parent, at: index), let moves = outline.backendMoves(to: target) else {
            return false
        }
        for m in moves { run("Move layer") { try backend.moveLayer(id: m.id, parent: m.parent, index: m.index) } }
        return true
    }

    // MARK: Masks

    func addMask(_ initial: LayerMaskInit) {
        guard let id = primary?.id else { return }
        run("Layer mask") { try backend.addMask(id: id, mask: initial) }
    }

    func deleteMask() {
        guard let id = primary?.id else { return }
        run("Layer mask") { try backend.removeMask(id: id) }
    }

    func toggleMaskEnabled(_ id: DocLayerID? = nil) {
        guard let n = (id.flatMap { node($0) }) ?? primary, n.hasMask else { return }
        run("Layer mask") { try backend.setMaskEnabled(id: n.id, enabled: !n.maskEnabled) }
    }

    func toggleMaskLinked(_ id: DocLayerID) {
        guard let n = node(id), n.hasMask else { return }
        attempt("Layer mask") { try backend.setMaskLinked(id: id, linked: !n.maskLinked) }
        reloadModel()
    }

    // MARK: Adjustments and fills

    func adjustment(of id: DocLayerID) -> AdjustmentModel? {
        liveAdjustment[id] ?? AdjustmentModel(json: node(id)?.adjustmentJson)
    }

    /// Adjustment editors: live while dragging, one history node on release.
    func setAdjustment(_ id: DocLayerID, _ model: AdjustmentModel, final: Bool) {
        liveAdjustment[id] = final ? nil : model
        run(model.kind.title) { try backend.setAdjustmentJson(id: id, json: model.json, interactive: true) }
        if final { commit(model.kind.title) }
    }

    func fill(of id: DocLayerID) -> FillModel? { FillModel(json: node(id)?.fillJson) }

    /// Fill editors. Colour wells send a stream of changes: they apply live and commit one
    /// history node after a short pause.
    func setFill(_ id: DocLayerID, _ model: FillModel, debounce: Bool = false) {
        fillDebounce?.cancel()
        run("Fill") { try backend.setFillJson(id: id, json: model.json, interactive: true) }
        guard debounce else { commit(model.kind.title); return }
        fillDebounce = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(400))
            guard !Task.isCancelled else { return }
            self?.commit(model.kind.title)
        }
    }

    // MARK: Selection (marquee)

    func setMarquee(_ rect: CanvasRect?) {
        if let r = rect, !r.isEmpty {
            run("Selection") { try backend.setSelectionRect(x: r.x, y: r.y, width: r.width, height: r.height, feather: 0) }
        } else if marquee != nil {
            run("Selection") { try backend.clearSelection() }
        }
        viewport?.selectionDidChange()
    }

    func selectAll() { setMarquee(CanvasRect(x: 0, y: 0, width: Int64(info.width), height: Int64(info.height))) }
    func deselect() { setMarquee(nil) }

    // MARK: History

    func undo() { historyMove("Undo") { try backend.undo() } }
    func redo() { historyMove("Redo") { try backend.redo() } }

    private func historyMove(_ verb: String, _ body: () throws -> DocumentChange) {
        let before = info.historyHead
        let label = history.first { $0.id == before }?.label
        do {
            let c = try body()
            reloadAfterHistoryMove()
            if c.historyHead == before {
                report?("Nothing to \(verb.lowercased())")
            } else {
                let now = history.first { $0.id == c.historyHead }?.label
                report?("\(verb): " + ((verb == "Undo" ? label : now) ?? "done"))
            }
        } catch { report?("\(verb): \(error.localizedDescription)") }
    }

    /// `id` 0 = as opened.
    func checkout(_ id: DocHistoryID) {
        attempt("History") { _ = try backend.checkoutHistory(id: id) }
        reloadAfterHistoryMove()
    }

    func snapshot(named name: String) {
        attempt("Snapshot") { try backend.snapshot(name: name) }
        reloadHistory()
    }

    func restoreSnapshot(_ name: String) {
        attempt("Snapshot") { _ = try backend.restoreSnapshot(name: name) }
        reloadAfterHistoryMove()
    }

    private func reloadAfterHistoryMove() {
        liveAdjustment.removeAll()
        reloadModel()
        reloadHistory()
    }

    // MARK: Viewport

    func zoomDidChange(_ z: Double) {
        guard abs(z - zoom) > 1e-9 else { return }
        zoom = z
        zoomChangedAt = Date()
    }

    func close() {
        fillDebounce?.cancel()
        backend.setListener(listener: nil)
        backend.detachSurfaces()
        backend.close()
        bridge = nil
    }
}

/// Engine render thread → main actor. Delivered synchronously when already on the main thread.
private final class ListenerBridge: DocumentBackendListener, @unchecked Sendable {
    nonisolated(unsafe) weak var controller: DocumentController?

    private func onMain(_ body: @escaping @MainActor (DocumentController) -> Void) {
        if Thread.isMainThread {
            MainActor.assumeIsolated { if let c = controller { body(c) } }
        } else {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { if let c = self?.controller { body(c) } }
            }
        }
    }

    func onFrame(frame: DocFrame) { onMain { $0.listenerFrame(frame) } }
    func onLayersChanged(layerIds: [DocLayerID]) { onMain { $0.reloadModel() } }
    func onHistoryChanged(head: DocHistoryID) { onMain { $0.reloadHistory() } }
    func onRenderFailed(message: String) { onMain { $0.report?("Render: \(message)") } }
}
