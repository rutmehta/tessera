import AppKit
import Observation
import TesseraCore

/// Filter ▸ Liquify… (WP B5-13): a workspace sheet over the document window with its own canvas.
///
/// The mesh, its freeze plane and the stroke live engine-side (`begin_liquify` … `commit_liquify`); this model sends
/// pointer samples on one serial queue and shows what the engine renders: a proxy preview (≤ 2048 px on the long
/// side) in an IOSurface, and the mesh for the overlay. Pointer batches coalesce while one is in flight, so a slow
/// frame never queues stale work. Apply renders at full resolution off the main thread as one history node
/// (`RetouchJobs`: Cancel returns at once, a late result is discarded and the engine refuses to write it).
@MainActor @Observable
final class DocumentLiquify {
    static let shared = DocumentLiquify()

    /// The open workspace (the sheet's item).
    var workspace: LiquifyWorkspaceModel?
    /// "Opening Liquify…" while the engine snapshots the layer.
    private(set) var opening = false
    /// Called when a workspace is shown (self-test).
    @ObservationIgnored var onOpened: ((LiquifyWorkspaceModel) -> Void)?

    private init() {}

    static func backend(_ doc: DocumentController?) -> (any DocumentLiquifyBackend)? {
        doc?.backend as? any DocumentLiquifyBackend
    }

    /// Filter ▸ Liquify…: a new Liquify on the selected layer (appended as a smart filter on smart objects).
    func open(_ doc: DocumentController) {
        guard let l = doc.primary else { doc.report?("Liquify: select a layer"); return }
        guard l.kind == .pixel || l.kind == .smartObject else {
            doc.report?("Liquify works on pixel layers and smart objects, not \(l.kind.title.lowercased()) layers"); return
        }
        start(doc, layer: l, stage: nil)
    }

    /// Double-click on a Liquify smart filter row: re-edit it in place.
    func editSmartFilter(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        guard let node = doc.node(layer) else { return }
        start(doc, layer: node, stage: row.index)
    }

    private func start(_ doc: DocumentController, layer: LayerRecord, stage: UInt32?) {
        guard workspace == nil, !opening, let b = Self.backend(doc) else { return }
        opening = true
        doc.report?("Liquify: preparing \(layer.name)…")
        let hasSelection = doc.marquee != nil
        let (id, kind, name) = (layer.id, layer.kind, layer.name)
        Task { @MainActor in
            let r = await Task.detached(priority: .userInitiated) { Result { try b.beginLiquify(layer: id, stageIndex: stage) } }.value
            self.opening = false
            switch r {
            case .success(let info):
                let m = LiquifyWorkspaceModel(doc: doc, backend: b, info: info, layerName: name, layerKind: kind,
                                              hasSelection: hasSelection)
                self.workspace = m
                m.refresh(mesh: true)
                doc.report?(info.stageIndex == nil ? "Liquify: paint to deform; Apply adds one history step"
                            : "Liquify: re-editing the smart filter; Apply replaces it")
                self.onOpened?(m)
            case .failure(let e):
                doc.report?("Liquify: \(e.localizedDescription)")
            }
        }
    }

    /// The sheet went away (Apply finished or Cancel).
    func closed(_ m: LiquifyWorkspaceModel) {
        if workspace === m { workspace = nil }
    }
}

/// One open Liquify workspace.
@MainActor @Observable
final class LiquifyWorkspaceModel: Identifiable {
    enum MeshSize: String, CaseIterable, Identifiable {
        case small, medium, large
        var id: String { rawValue }
        var title: String { rawValue.capitalized }
        /// Minimum spacing between drawn mesh lines, view points.
        var spacing: Double {
            switch self {
            case .small: 10
            case .medium: 20
            case .large: 40
            }
        }
    }

    let id = UUID()
    let doc: DocumentController
    let backend: any DocumentLiquifyBackend
    let info: LiquifyWorkspaceInfo
    let layerName: String
    let layerKind: LayerKindTag
    let hasSelection: Bool

    var tool: LiquifyToolKind = .forwardWarp
    var brush = LiquifyBrushSettings()
    var showMesh = false { didSet { if showMesh { refresh(mesh: true) }; changed() } }
    var meshSize: MeshSize = .medium { didSet { changed() } }
    var showMask = true { didSet { changed() } }
    /// Before / after: the untouched source (no document or history change).
    var showOriginal = false { didSet { refresh(mesh: false) } }
    var output: LiquifyOutput
    /// Reconstruct (whole mesh) amount, percent.
    var reconstructAmount: Double = 100
    var view = LiquifyViewTransform(scale: 1, origin: .zero)
    private(set) var image: CGImage?
    private(set) var mesh: LiquifyMeshData?
    private(set) var error: String?
    /// Brush → preview on screen, per pointer batch (view points of the last ones).
    private(set) var latencies: [Double] = []
    /// Bumped on any overlay-relevant change (the canvas redraws).
    private(set) var revision = 0
    let jobs = RetouchJobs()
    var busy: String? { jobs.running?.title }

    @ObservationIgnored private let queue = DispatchQueue(label: "dev.tessera.liquify", qos: .userInteractive)
    @ObservationIgnored private var pending: [LiquifyInputPoint] = []
    @ObservationIgnored private var pendingSince: Date?
    @ObservationIgnored private var inFlight = false
    @ObservationIgnored private var strokeTool: LiquifyToolKind?
    @ObservationIgnored private var restTimer: Timer?
    @ObservationIgnored private var lastPoint: LiquifyInputPoint?
    @ObservationIgnored private var fitted = false
    @ObservationIgnored private var closed = false
    /// Called when Apply finishes or is discarded (self-test).
    @ObservationIgnored var onApplied: ((Result<DocumentChange, Error>) -> Void)?

    init(doc: DocumentController, backend: any DocumentLiquifyBackend, info: LiquifyWorkspaceInfo, layerName: String,
         layerKind: LayerKindTag, hasSelection: Bool) {
        self.doc = doc; self.backend = backend; self.info = info; self.layerName = layerName; self.layerKind = layerKind
        self.hasSelection = hasSelection
        self.output = info.stageIndex != nil ? .smartFilter : LiquifyOutput.preferred(layerKind: layerKind)
    }

    var title: String { "Liquify" }
    var subtitle: String {
        var s = "\(layerName) · \(info.width) × \(info.height) px"
        if let i = info.stageIndex { s += " · re-editing smart filter \(i + 1)" }
        return s
    }
    var outputs: [(LiquifyOutput, String?)] {
        LiquifyOutput.availability(layerKind: layerKind, hasSelection: hasSelection, reEditing: info.stageIndex != nil)
    }
    var previewNote: String {
        info.previewFactor == 1 ? "Preview at full resolution"
            : "Preview at 1/\(info.previewFactor) (\(info.previewWidth) × \(info.previewHeight)); Apply renders \(info.width) × \(info.height)"
    }
    var latencyText: String? {
        guard !latencies.isEmpty else { return nil }
        let s = latencies.sorted()
        return String(format: "Brush → preview: median %.0f ms · p95 %.0f ms", s[s.count / 2], s[min(s.count - 1, s.count * 95 / 100)])
    }

    private func changed() { revision &+= 1 }

    // MARK: View

    func fit(in size: CGSize) {
        view = .fit(source: info.sourceSize, in: size, margin: Double(Theme.Space.l))
        fitted = true
        changed()
    }

    func fitIfNeeded(in size: CGSize) { if !fitted, size.width > 0, size.height > 0 { fit(in: size) } }

    func actualPixels(in size: CGSize) {
        let c = CGPoint(x: Double(size.width) / 2, y: Double(size.height) / 2)
        view.zoom(by: 1 / view.scale, around: c)
        changed()
    }

    func zoom(by f: Double, around p: CGPoint) {
        view.zoom(by: f, around: p)
        changed()
    }

    func pan(dx: Double, dy: Double) {
        view.pan(dx: dx, dy: dy)
        changed()
    }

    // MARK: Brush

    func pointerDown(_ source: CGPoint, pressure: Float, option: Bool) {
        guard busy == nil, !closed else { return }
        error = nil
        strokeTool = tool.withOption(option)
        showOriginal = false
        add(source, pressure: pressure)
        restTimer?.invalidate()
        if strokeTool?.actsInPlace == true {
            // Tools that act in place keep working while the pointer rests (Rate scales them).
            restTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { _ in
                MainActor.assumeIsolated {
                    guard let m = DocumentLiquify.shared.workspace, let p = m.lastPoint else { return }
                    m.pending.append(p)
                    m.pump()
                }
            }
        }
    }

    func pointerDragged(_ source: CGPoint, pressure: Float) {
        guard strokeTool != nil else { return }
        add(source, pressure: pressure)
    }

    func pointerUp(_ source: CGPoint, pressure: Float) {
        guard strokeTool != nil else { return }
        add(source, pressure: pressure)
        restTimer?.invalidate()
        restTimer = nil
        strokeTool = nil
        lastPoint = nil
        let (b, t) = (backend, info.token)
        queue.async { try? b.liquifyEndStroke(token: t) }
        refresh(mesh: true)
    }

    private func add(_ p: CGPoint, pressure: Float) {
        let q = LiquifyInputPoint(p, pressure: pressure)
        lastPoint = q
        pending.append(q)
        if pendingSince == nil { pendingSince = Date() }
        pump()
    }

    /// Sends the pending points and renders, unless a batch is in flight (they join the next one).
    private func pump() {
        guard !inFlight, !pending.isEmpty, let tool = strokeTool else { return }
        let batch = pending
        let since = pendingSince ?? Date()
        pending.removeAll()
        pendingSince = nil
        inFlight = true
        let (b, t, brush) = (backend, info.token, brush)
        // The mesh overlay follows each batch when shown; the freeze tools change the mask overlay.
        let wantMesh = showMesh || (showMask && (tool == .freeze || tool == .thaw))
        queue.async {
            let r = Result { () -> (LiquifyPreviewFrame, LiquifyMeshData?) in
                _ = try b.liquifyBrush(token: t, tool: tool, brush: brush, points: batch)
                let f = try b.previewLiquify(token: t, original: false)
                let m = wantMesh ? try b.liquifyMesh(token: t) : nil
                return (f, m)
            }
            let image = (try? r.get()).flatMap { Self.image($0.0) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.inFlight = false
                    switch r {
                    case .success(let (_, m)):
                        if let image { self.image = image }
                        if let m { self.mesh = m }
                        self.latencies.append(Date().timeIntervalSince(since) * 1000)
                        if self.latencies.count > 240 { self.latencies.removeFirst(self.latencies.count - 240) }
                        self.changed()
                    case .failure(let e):
                        self.error = e.localizedDescription
                    }
                    self.pump()
                }
            }
        }
    }

    /// A fresh preview (and optionally the mesh), in order after any queued brush work.
    func refresh(mesh wantMesh: Bool) {
        guard !closed else { return }
        let (b, t, original) = (backend, info.token, showOriginal)
        queue.async {
            let f = Result { try b.previewLiquify(token: t, original: original) }
            let m = wantMesh ? try? b.liquifyMesh(token: t) : nil
            let image = (try? f.get()).flatMap { Self.image($0) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    if let image { self.image = image }
                    if let m { self.mesh = m }
                    if case .failure(let e) = f, !self.closed { self.error = e.localizedDescription }
                    self.changed()
                }
            }
        }
    }

    nonisolated static func image(_ f: LiquifyPreviewFrame) -> CGImage? {
        IOSurfaceLookup(f.surfaceId).flatMap { FilterSheetModel.image($0, width: Int(f.width), height: Int(f.height)) }
    }

    /// Waits until queued engine work has run (self-test).
    func idle() async {
        while inFlight || !pending.isEmpty { try? await Task.sleep(for: .milliseconds(10)) }
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in queue.async { c.resume() } }
        try? await Task.sleep(for: .milliseconds(30))
    }

    // MARK: Whole-mesh commands

    private func command(_ what: String, _ body: @escaping @Sendable (any DocumentLiquifyBackend, UInt64) throws -> Void) {
        guard busy == nil else { return }
        let (b, t) = (backend, info.token)
        queue.async {
            let r = Result { try body(b, t) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    if case .failure(let e) = r { self.error = "\(what): \(e.localizedDescription)" }
                }
            }
        }
        refresh(mesh: true)
    }

    func reconstructAll() {
        let a = reconstructAmount / 100
        command("Reconstruct") { b, t in try b.liquifyReconstructAll(token: t, amount: a) }
    }

    func restoreAll() { command("Restore All") { b, t in try b.liquifyReset(token: t, keepFreeze: true) } }
    func freezeAll() { command("Freeze All") { b, t in try b.liquifyFreezeAll(token: t, frozen: true) } }
    func thawAll() { command("Thaw All") { b, t in try b.liquifyFreezeAll(token: t, frozen: false) } }

    // MARK: Keys (canvas first responder)

    func key(_ e: NSEvent) -> Bool {
        guard let c = e.charactersIgnoringModifiers?.first, !e.modifierFlags.contains(.command) else { return false }
        if c == "[" || c == "]" { brush.step(larger: c == "]"); changed(); return true }
        if c == "p" { showOriginal.toggle(); return true }
        if let t = LiquifyToolKind.forKey(c) { tool = t; return true }
        return false
    }

    // MARK: Apply / Cancel

    func apply() {
        guard busy == nil, !closed else { return }
        if let why = outputs.first(where: { $0.0 == output })?.1 { error = why; return }
        let (b, t, out, doc, q) = (backend, info.token, output, doc, queue)
        restTimer?.invalidate()
        let refused = jobs.start("Applying Liquify…", operation: "Liquify", {
            // In order after any queued brush work.
            try q.sync { try b.commitLiquify(token: t, output: out) }
        }) { [weak self] end in
            guard let self else { return }
            switch end {
            case .finished(.success(let change)):
                doc.run("Liquify") { change }
                doc.reloadModel()
                doc.report?("Liquify applied (\(out.title.lowercased()))")
                self.close()
                self.onApplied?(.success(change))
            case .finished(.failure(let e)):
                self.error = e.localizedDescription
                self.onApplied?(.failure(e))
            case .discarded(let r):
                // Cancelled while rendering: the engine refused to write; nothing reaches history.
                self.onApplied?(r.flatMap { _ in .failure(DocumentError.invalid("discarded")) })
            }
        }
        if let refused { error = refused }
    }

    /// Cancel (Esc): closes without changing the document. During Apply, the render is abandoned at once.
    func cancel() {
        let (b, t) = (backend, info.token)
        if jobs.isBusy {
            jobs.cancel { b.cancelLiquify(token: t) }
        } else {
            queue.async { b.cancelLiquify(token: t) }
        }
        doc.report?("Liquify cancelled; the layer is unchanged")
        close()
    }

    private func close() {
        closed = true
        restTimer?.invalidate()
        restTimer = nil
        DocumentLiquify.shared.closed(self)
    }
}
