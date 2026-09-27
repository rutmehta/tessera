import AppKit
import Observation
import TesseraCore

/// Remove tool, Edit ▸ Content-Aware Fill, Remove Distractions review and Filter ▸ Neural Filters (WP B5-09).
///
/// The Remove tool shares the Healing Brush's slot and key (J; ⇧J switches between them). While it is on,
/// `DocumentTools` forwards the viewport's mouse and keys here (small marked hooks): a drag paints the
/// removal mask engine-side (`begin_remove_stroke` …), the release removes it as one history node.
/// "Remove Distractions" first shows the detector's suggestions over the canvas for review and removes only
/// the accepted ones. Long applies run off the main thread with a Cancel button; errors name the missing
/// model and where its file comes from, and nothing is ever downloaded or simulated.
@MainActor @Observable
final class DocumentRetouch {
    static let shared = DocumentRetouch()

    var document: DocumentController? { DocumentTools.shared.document }

    // MARK: State

    /// The Remove tool is the current tool (the document's tool reads Healing Brush meanwhile).
    private(set) var removeActive = false
    var options = RemoveToolOptions() { didSet { syncBrush() } }
    /// "Removing…" while an apply runs.
    private(set) var busy: String?
    @ObservationIgnored private var busyStarted = Date()
    /// Suggestions under review.
    private(set) var review: DistractionReview?
    /// The last error (options bar and sheets show it).
    private(set) var error: RetouchErrorPresentation?
    /// The open Neural Filters sheet.
    var neuralSheet: NeuralSheetModel?
    /// Installed models (refreshed when the tool or the sheet opens).
    private(set) var models: [RetouchModelInfo] = []
    /// The stroke being painted, canvas points (drawn by the overlay).
    @ObservationIgnored private(set) var strokePoints: [CanvasPoint] = []
    @ObservationIgnored private var strokeOpen = false
    @ObservationIgnored private var pending: [CanvasPoint] = []
    @ObservationIgnored private var inFlight = false
    @ObservationIgnored private var savedHeal: BrushOptions?
    @ObservationIgnored private let queue = DispatchQueue(label: "dev.tessera.document-retouch", qos: .userInitiated)
    /// Called after each finished apply (self-test).
    @ObservationIgnored var onFinished: ((Result<RetouchOutcome, Error>) -> Void)?

    private init() {}

    static func backend(_ doc: DocumentController?) -> (any DocumentRetouchBackend)? {
        doc?.backend as? any DocumentRetouchBackend
    }

    private func say(_ s: String) { document?.report?(s) }

    var lamaInstalled: Bool { models.first { $0.modelId == "remove/lama" }?.installed ?? false }

    func refreshModels() {
        models = (try? Self.backend(document)?.retouchModels()) ?? []
    }

    // MARK: Tool

    /// Palette slot, ⇧J: the Remove tool on.
    func activate() {
        guard let doc = document else { return }
        let tools = DocumentTools.shared
        if doc.tool != .heal { tools.select(.heal) }
        removeActive = true
        error = nil
        refreshModels()
        if savedHeal == nil { savedHeal = tools.brushes[.heal] }
        syncBrush()
        overlay(doc)?.needsDisplay = true
        say("Remove: paint over what to remove; release removes it. \(options.engine == .auto && !lamaInstalled ? "LaMa is not installed, so Auto uses PatchMatch." : "")")
    }

    /// Any other tool (or ⇧J back to the Healing Brush).
    func deactivate() {
        guard removeActive else { return }
        removeActive = false
        if strokeOpen { cancelStroke() }
        endReview()
        if let h = savedHeal { DocumentTools.shared.brushes[.heal] = h }
        savedHeal = nil
        if let doc = document { overlay(doc)?.needsDisplay = true }
    }

    /// `DocumentTools.select` hook: choosing a tool ends the Remove tool.
    func toolSelected(_ tool: DocumentTool) {
        if removeActive { deactivate() }
    }

    /// The brush outline `ToolOverlayView` draws (Healing Brush's) follows the Remove size, hard.
    private func syncBrush() {
        guard removeActive else { return }
        var b = DocumentTools.shared.brushes[.heal] ?? BrushOptions()
        b.size = options.size
        b.hardness = 1
        DocumentTools.shared.brushes[.heal] = b
    }

    // MARK: Mouse (from DocumentTools)

    func mouseDown(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard let doc = document, doc === v.controller else { return false }
        let c = CanvasPoint(v.canvasPoint(e))
        if var r = review {
            if let hit = r.hit(c) {
                r.toggle(hit.id)
                review = r
                overlay(doc)?.needsDisplay = true
            }
            return true
        }
        guard busy == nil else { say("\(busy ?? "") Esc or Cancel stops it"); return true }
        beginStroke(doc, at: c)
        return true
    }

    func mouseDragged(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard strokeOpen else { return review != nil }
        add(CanvasPoint(v.canvasPoint(e)))
        return true
    }

    func mouseUp(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard strokeOpen, let doc = document else { return review != nil }
        add(CanvasPoint(v.canvasPoint(e)))
        endStroke(doc)
        return true
    }

    // MARK: Stroke

    private func strokeLayer(_ doc: DocumentController) -> DocLayerID? {
        guard let l = doc.primary else { say("Remove: select a layer"); return nil }
        guard l.kind == .pixel || l.kind == .smartObject else {
            say("Remove: select a pixel layer or a smart object (not \(l.kind.rawValue.replacingOccurrences(of: "_", with: " ")))")
            return nil
        }
        return l.id
    }

    func beginStroke(_ doc: DocumentController, at c: CanvasPoint) {
        guard let layer = strokeLayer(doc), let b = Self.backend(doc) else { return }
        error = nil
        strokeOpen = true
        strokePoints = [c]
        pending = [c]
        inFlight = false
        let (size, engine) = (options.size, options.engine)
        queue.async {
            let r = Result { try b.beginRemoveStroke(layer: layer, size: size, engine: engine) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    if case .failure(let e) = r { DocumentRetouch.shared.fail("Remove", e) }
                }
            }
        }
        pump(b)
        overlay(doc)?.needsDisplay = true
    }

    private func add(_ c: CanvasPoint) {
        guard strokeOpen, let doc = document, let b = Self.backend(doc) else { return }
        if let l = strokePoints.last, hypotf(l.x - c.x, l.y - c.y) < 0.5 { return }
        strokePoints.append(c)
        pending.append(c)
        pump(b)
        overlay(doc)?.needsDisplay = true
    }

    /// Sends pending points unless a batch is in flight (they join the next one).
    private func pump(_ b: any DocumentRetouchBackend) {
        guard !inFlight, !pending.isEmpty else { return }
        let batch = pending
        pending.removeAll()
        inFlight = true
        queue.async {
            _ = try? b.removeStrokePoints(batch)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let r = DocumentRetouch.shared
                    r.inFlight = false
                    if r.strokeOpen { r.pump(b) }
                }
            }
        }
    }

    func cancelStroke() {
        strokeOpen = false
        strokePoints = []
        pending = []
        let b = Self.backend(document)
        queue.async { b?.cancelRemoveStroke() }
        if let doc = document { overlay(doc)?.needsDisplay = true }
    }

    private func endStroke(_ doc: DocumentController) {
        guard let b = Self.backend(doc) else { return }
        strokeOpen = false
        let rest = pending
        pending.removeAll()
        let params = options.paramsJson
        let q = queue
        start("Removing…", doc) {
            try b.sync(on: q) {
                if !rest.isEmpty { _ = try b.removeStrokePoints(rest) }
                return try b.endRemoveStroke(paramsJson: params)
            }
        } done: { [weak self] _ in
            self?.strokePoints = []
        }
    }

    // MARK: Selection commands

    /// Options bar ▸ Remove Selection: Remove inside the selection.
    func removeSelection() {
        guard let doc = document, let layer = strokeLayer(doc), let b = Self.backend(doc) else { return }
        guard doc.marquee != nil else { say("Remove Selection: make a selection first"); return }
        let (engine, params) = (options.engine, options.paramsJson)
        start("Removing…", doc) { try b.removeSelection(layer: layer, engine: engine, paramsJson: params) }
    }

    /// Edit ▸ Content-Aware Fill.
    func contentAwareFill() {
        guard let doc = document, let b = Self.backend(doc) else { return }
        guard let l = doc.primary, l.kind == .pixel || l.kind == .smartObject else {
            say("Content-Aware Fill: select a pixel layer"); return
        }
        guard doc.marquee != nil else { say("Content-Aware Fill: make a selection first"); return }
        start("Filling…", doc, operation: "Content-Aware Fill") { try b.contentAwareFill(layer: l.id, paramsJson: "{}") }
    }

    // MARK: Remove Distractions

    /// Scans and shows the suggestions for review (no history).
    func scanDistractions() {
        guard let doc = document, let b = Self.backend(doc) else { return }
        guard let l = doc.primary, l.kind == .pixel else { say("Remove Distractions: select a pixel layer"); return }
        guard doc.marquee == nil else { say("Remove Distractions works on the whole layer: deselect first (⌘D)"); return }
        guard busy == nil else { return }
        error = nil
        busy = "Finding distractions…"
        busyStarted = Date()
        let layer = l.id
        Task { @MainActor [weak self] in
            let r = await Task.detached(priority: .userInitiated) {
                Result { try b.detectDistractions(layer: layer, paramsJson: "{}") }
            }.value
            guard let self else { return }
            self.busy = nil
            switch r {
            case .success(let scan):
                self.review = DistractionReview(layer: layer, scan: scan)
                let n = scan.candidates.count
                self.say(n == 0 ? "Remove Distractions: nothing found (faces: \(scan.faces))"
                         : "Remove Distractions: \(n) suggestion\(n == 1 ? "" : "s"). Click one to keep it; Remove Selected removes the rest.")
            case .failure(let e):
                self.fail("Remove Distractions", e)
            }
            if let doc = self.document { self.overlay(doc)?.needsDisplay = true }
        }
    }

    func toggleSuggestion(_ id: UInt32) {
        review?.toggle(id)
        if let doc = document { overlay(doc)?.needsDisplay = true }
    }

    func setAllSuggestions(_ on: Bool) {
        review?.setAll(on)
        if let doc = document { overlay(doc)?.needsDisplay = true }
    }

    func endReview() {
        guard review != nil else { return }
        review = nil
        let b = Self.backend(document)
        b?.clearDistractions()
        if let doc = document { overlay(doc)?.needsDisplay = true }
    }

    /// Removes only the accepted suggestions (one node).
    func applyReview() {
        guard let doc = document, let b = Self.backend(doc), let r = review, r.canApply else { return }
        let (ids, engine, params, layer) = (r.acceptedIds, options.engine, options.paramsJson, r.layer)
        start("Removing distractions…", doc, operation: "Remove Distractions") {
            try b.removeDistractions(layer: layer, accepted: ids, engine: engine, paramsJson: params)
        } done: { [weak self] ok in
            if ok { self?.review = nil }
        }
    }

    // MARK: Neural Filters

    /// Filter ▸ Neural Filters….
    func openNeuralFilters() {
        guard let doc = document, let b = Self.backend(doc) else { return }
        guard let l = doc.primary, l.kind == .pixel || l.kind == .smartObject else {
            say("Neural Filters: select a pixel layer or a smart object"); return
        }
        refreshModels()
        let state = NeuralSheetState(specs: b.neuralFilterSpecs(), layerKind: l.kind, hasSelection: doc.marquee != nil)
        neuralSheet = NeuralSheetModel(doc: doc, layer: l, state: state, owner: self)
    }

    /// Double-click on a neural smart filter row (SmartFilterRows hook).
    func editNeuralSmartFilter(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        guard let b = Self.backend(doc), let node = doc.node(layer), let kind = NeuralKind(filterId: row.filterId) else { return }
        refreshModels()
        let state = NeuralSheetState(specs: b.neuralFilterSpecs(), layerKind: node.kind, hasSelection: doc.marquee != nil, kind: kind,
                                     smartIndex: row.index, filterJson: row.filterJson)
        neuralSheet = NeuralSheetModel(doc: doc, layer: node, state: state, owner: self)
    }

    // MARK: Running applies

    /// Runs a blocking apply off the main thread; one history node on success.
    func start(_ what: String, _ doc: DocumentController, operation: String = "Remove",
               _ body: @escaping @Sendable () throws -> RetouchOutcome,
               done: (@MainActor (Bool) -> Void)? = nil) {
        guard busy == nil else { return }
        busy = what
        busyStarted = Date()
        error = nil
        say(what)
        Task { @MainActor [weak self] in
            let r = await Task.detached(priority: .userInitiated) { Result { try body() } }.value
            guard let self else { return }
            self.busy = nil
            switch r {
            case .success(let o):
                doc.run(operation) { o.change }
                var msg = String(format: "%@: %@, %.1f s", operation, o.backend, o.millis / 1000)
                if let n = o.note { msg += " (\(n))" }
                self.say(msg)
                done?(true)
            case .failure(let e):
                self.fail(operation, e)
                done?(false)
            }
            self.overlay(doc)?.needsDisplay = true
            self.onFinished?(r)
        }
    }

    /// Seconds the running apply has taken.
    var busySeconds: Double { Date().timeIntervalSince(busyStarted) }

    /// Cancel button, Esc.
    func cancel() {
        if strokeOpen { cancelStroke(); return }
        guard busy != nil else { return }
        Self.backend(document)?.cancelRetouch()
        say("Cancelling…")
    }

    func fail(_ operation: String, _ e: Error) {
        let p = RetouchErrorPresentation(operation: operation, message: e.localizedDescription)
        error = p.isCancel ? nil : p
        say(p.statusLine)
    }

    func clearError() { error = nil }

    // MARK: Keys (from DocumentTools)

    /// ⇧J toggles Remove / Healing Brush; with Remove on: [ ] size, Esc cancels, Return applies a review.
    func handleKey(_ event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let ch = (event.charactersIgnoringModifiers ?? "").lowercased()
        if ch == "j", flags == .shift, document != nil {
            if removeActive { deactivate() } else { activate() }
            return true
        }
        guard removeActive, flags.subtracting(.shift).isEmpty else { return false }
        switch event.keyCode {
        case 53:
            if busy != nil || strokeOpen { cancel(); return true }
            if review != nil { endReview(); return true }
            return false
        case 36, 76:
            if review != nil { applyReview(); return true }
            return false
        default: break
        }
        switch ch {
        case "[", "{": options.bracket(larger: false); return true
        case "]", "}": options.bracket(larger: true); return true
        default: return false
        }
    }

    // MARK: Overlay

    private func overlay(_ doc: DocumentController) -> RemoveOverlayView? {
        guard let v = doc.viewport else { return nil }
        if let o = v.subviews.compactMap({ $0 as? RemoveOverlayView }).first { return o }
        let o = RemoveOverlayView()
        o.frame = v.bounds
        o.autoresizingMask = [.width, .height]
        o.viewport = v
        v.addSubview(o, positioned: .above, relativeTo: v.toolOverlay)
        return o
    }
}

extension DocumentRetouchBackend {
    /// Runs `body` on `queue` and waits (the stroke's points and its end stay in order).
    func sync<T>(on queue: DispatchQueue, _ body: () throws -> T) throws -> T { try queue.sync { try body() } }
}

// MARK: - Neural Filters sheet model

@MainActor @Observable
final class NeuralSheetModel: Identifiable {
    let id = UUID()
    let doc: DocumentController
    let layer: LayerRecord
    var state: NeuralSheetState
    private(set) var revision = 0
    private(set) var busy = false
    private(set) var error: RetouchErrorPresentation?
    @ObservationIgnored private weak var owner: DocumentRetouch?

    init(doc: DocumentController, layer: LayerRecord, state: NeuralSheetState, owner: DocumentRetouch) {
        self.doc = doc
        self.layer = layer
        self.state = state
        self.owner = owner
    }

    var title: String { state.smartIndex == nil ? "Neural Filters" : "Edit \(state.spec?.name ?? "Neural Filter")" }
    var subtitle: String {
        state.smartIndex == nil ? "Layer “\(layer.name)”" : "Smart filter of “\(layer.name)”"
    }

    /// The model the chosen filter needs, when it is not installed.
    var missingModel: RetouchModelInfo? {
        let id: String? = switch state.kind {
        case .colorize: "filters/ddcolor"
        case .jpegArtifactRemoval: "enhance/drunet-color"
        case .skinSmoothing: nil
        }
        guard let id else { return nil }
        return owner?.models.first { $0.modelId == id && !$0.installed }
    }

    /// Skin Smoothing without the face detector: the selection's bounds are the face box.
    var faceHint: String? {
        guard state.kind == .skinSmoothing, state.faces == nil else { return nil }
        let detector = owner?.models.first { $0.modelId == "opencv/yunet" }?.installed ?? false
        if detector { return "Faces are found by the face detector (YuNet)." }
        return doc.marquee != nil ? "The face detector is not installed: the selection's bounds are used as the face box."
            : "The face detector (opencv/yunet) is not installed: select a face first; its bounds are used as the face box."
    }

    func choose(_ k: NeuralKind) {
        guard state.smartIndex == nil else { return }
        state.kind = k
        error = nil
        revision += 1
    }

    func set(_ c: NeuralControl, _ v: Double) { state.set(c, v) }

    func reset() {
        state.reset()
        revision += 1
    }

    func cancel() {
        if busy { owner?.cancelBusy(); return }
        owner?.neuralSheet = nil
    }

    func apply() {
        guard !busy else { return }
        let st = state, layer = layer.id, doc = doc
        let name = st.spec?.name ?? "Neural Filter"
        busy = true
        error = nil
        let smart = st.smartIndex
        let filters = doc.backend as? any DocumentFiltersBackend
        let retouch = DocumentRetouch.backend(doc)
        doc.report?("Applying \(name)…")
        Task { @MainActor [weak self] in
            let r: Result<(DocumentChange, String), Error> = await Task.detached(priority: .userInitiated) {
                Result {
                    if let i = smart {
                        guard let f = filters else { throw DocumentError.unsupported("smart filters need the engine") }
                        return (try f.setSmartFilter(layer: layer, index: i, change: .params(json: st.filterJson)), "Edit Smart Filter")
                    }
                    guard let b = retouch else { throw DocumentError.unsupported("neural filters need the engine") }
                    let o = try b.neuralFilter(layer: layer, kind: st.kind, paramsJson: st.paramsJson, output: st.output)
                    return (o.change, String(format: "%@ applied (%@, %.1f s)%@", name, o.backend, o.millis / 1000,
                                             o.note.map { " — \($0)" } ?? ""))
                }
            }.value
            guard let self else { return }
            self.busy = false
            switch r {
            case .success(let (change, message)):
                doc.run(name) { change }
                doc.report?(message)
                self.owner?.neuralSheet = nil
            case .failure(let e):
                let p = RetouchErrorPresentation(operation: name, message: e.localizedDescription)
                self.error = p
                doc.report?(p.statusLine)
            }
        }
    }
}

extension DocumentRetouch {
    /// Cancel from a sheet.
    func cancelBusy() { Self.backend(document)?.cancelRetouch() }
}
