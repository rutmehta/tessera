import AppKit
import Observation
import TesseraCore

/// Remove tool, Edit ▸ Content-Aware Fill, Remove Distractions review and Filter ▸ Neural Filters (WP B5-09).
///
/// The Remove tool shares the Healing Brush's slot and key (J; ⇧J switches between them). While it is on,
/// `DocumentTools` forwards the viewport's mouse and keys here (small marked hooks): a drag paints the
/// removal mask engine-side (`begin_remove_stroke` …), the release removes it as one history node.
/// "Remove Distractions" first shows the detector's suggestions over the canvas for review and removes only
/// the accepted ones. Long applies run off the main thread with a Cancel button (`RetouchJobs`, B5-09b: Cancel
/// returns to idle at once, the late result is discarded, a new job waits until the cancelled one stopped).
/// LaMa, DDColor and DRUNet are downloaded through the app's model downloads (`RetouchModelDownloads`, the
/// M2-51 flow) only when asked and only if Settings ▸ AI allows it; the operation that asked runs when the
/// download completes. Nothing is simulated.
@MainActor @Observable
final class DocumentRetouch {
    static let shared = DocumentRetouch()

    var document: DocumentController? { DocumentTools.shared.document }

    // MARK: State

    /// The Remove tool is the current tool (the document's tool reads Healing Brush meanwhile).
    private(set) var removeActive = false
    var options = RemoveToolOptions() { didSet { syncBrush() } }
    /// One apply at a time; cancel never waits (B5-09b).
    let jobs = RetouchJobs()
    /// LaMa / DDColor / DRUNet through the app's model downloads (B5-09b). Replaceable for the self-test.
    var downloads = RetouchModelDownloads(acquisition: .shared)
    /// "Removing…" while an apply runs.
    var busy: String? { jobs.running?.title }
    /// A refusal or a download note for the options bar (B5-09b).
    private(set) var notice: String?
    /// A Remove stroke painted with LaMa, waiting for the LaMa download (B5-09b).
    private(set) var strokeWaitingForModel = false
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
    /// Called when a cancelled (abandoned) apply finally returns (self-test).
    @ObservationIgnored var onDiscarded: ((Result<RetouchOutcome, Error>) -> Void)?

    private init() {}

    static func backend(_ doc: DocumentController?) -> (any DocumentRetouchBackend)? {
        doc?.backend as? any DocumentRetouchBackend
    }

    private func say(_ s: String) { document?.report?(s) }

    var lamaInstalled: Bool { models.first { $0.modelId == "remove/lama" }?.installed ?? false }

    func refreshModels() {
        models = (try? Self.backend(document)?.retouchModels()) ?? []
    }

    func model(_ id: String?) -> RetouchModelInfo? { id.flatMap { id in models.first { $0.modelId == id } } }

    // MARK: Model downloads (B5-09b)

    /// Runs `run` once the model `id` is installed: at once when it is (or none is needed); after the download
    /// when downloads are allowed (inline progress where the model is shown); never when they are off (the
    /// options bar / sheet says so and links to Settings ▸ AI) or when the download fails — then `abandon`.
    func withModel(_ id: String?, operation: String, run: @escaping @MainActor () -> Void,
                   abandon: @escaping @MainActor () -> Void = {}) {
        guard let m = model(id), !m.installed else { run(); return }
        let outcome = downloads.request(m, for: operation) { [weak self] r in
            guard let self else { return }
            self.refreshModels()
            switch r {
            case .success:
                self.notice = nil
                self.say("\(m.modelId) downloaded; running \(operation)")
                run()
            case .failure(let e):
                self.notice = e.localizedDescription
                self.say("\(operation): \(e.localizedDescription)")
                abandon()
            }
        }
        switch outcome {
        case .ready:
            refreshModels()
            run()
        case .started:
            notice = nil
            say("\(operation) needs \(m.modelId): downloading it first; \(operation) runs when it is ready")
        case .downloadsOff:
            let why = "\(operation) needs \(m.modelId), which is not installed, and model downloads are off (Settings ▸ AI). Nothing was downloaded."
            notice = why
            say(why)
            abandon()
        }
    }

    /// Options bar ▸ Download (no operation waits).
    func downloadModel(_ id: String) {
        withModel(id, operation: "Download") { [weak self] in self?.say("\(id) is installed") }
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
        notice = nil
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
        if let why = jobs.refusal { notice = why; say(why); return true }
        if strokeWaitingForModel { say("Remove: waiting for the LaMa download (Esc forgets the stroke)"); return true }
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
        if strokeWaitingForModel { downloads.forgetWaiting("remove/lama") }
        strokeWaitingForModel = false
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
        if !rest.isEmpty { queue.async { _ = try? b.removeStrokePoints(rest) } }
        if let why = jobs.refusal {
            notice = why
            say(why)
            cancelStroke()
            return
        }
        // LaMa not installed: keep the stroke (engine-side mask and band) until the download completes.
        strokeWaitingForModel = true
        withModel(RetouchModelDownloads.modelId(for: options.engine), operation: "Remove", run: { [weak self] in
            guard let self, self.strokeWaitingForModel else { return }
            self.strokeWaitingForModel = false
            self.finishStroke(doc, b)
        }, abandon: { [weak self] in
            guard let self, self.strokeWaitingForModel else { return }
            self.cancelStroke()
        })
    }

    private func finishStroke(_ doc: DocumentController, _ b: any DocumentRetouchBackend) {
        let params = options.paramsJson
        let q = queue
        start("Removing…", doc) {
            try b.sync(on: q) { try b.endRemoveStroke(paramsJson: params) }
        } done: { [weak self] _ in
            self?.strokePoints = []
            if let doc = self?.document { self?.overlay(doc)?.needsDisplay = true }
        }
    }

    // MARK: Selection commands

    /// Options bar ▸ Remove Selection: Remove inside the selection.
    func removeSelection() {
        guard let doc = document, let layer = strokeLayer(doc), let b = Self.backend(doc) else { return }
        guard doc.marquee != nil else { say("Remove Selection: make a selection first"); return }
        let (engine, params) = (options.engine, options.paramsJson)
        withModel(RetouchModelDownloads.modelId(for: engine), operation: "Remove") { [weak self] in
            self?.start("Removing…", doc) { try b.removeSelection(layer: layer, engine: engine, paramsJson: params) }
        }
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
        error = nil
        let layer = l.id
        let refused = jobs.start("Finding distractions…", operation: "Remove Distractions",
                                 { try b.detectDistractions(layer: layer, paramsJson: "{}") }) { [weak self] end in
            guard let self else { return }
            switch end {
            case .finished(.success(let scan)):
                self.review = DistractionReview(layer: layer, scan: scan)
                let n = scan.candidates.count
                self.say(n == 0 ? "Remove Distractions: nothing found (faces: \(scan.faces))"
                         : "Remove Distractions: \(n) suggestion\(n == 1 ? "" : "s"). Click one to keep it; Remove Selected removes the rest.")
            case .finished(.failure(let e)):
                self.fail("Remove Distractions", e)
            case .discarded:
                b.clearDistractions()
                self.notice = nil
                self.say("Remove Distractions cancelled")
            }
            if let doc = self.document { self.overlay(doc)?.needsDisplay = true }
        }
        if let refused { notice = refused; say(refused) }
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
        withModel(RetouchModelDownloads.modelId(for: engine), operation: "Remove Distractions") { [weak self] in
            self?.start("Removing distractions…", doc, operation: "Remove Distractions") {
                try b.removeDistractions(layer: layer, accepted: ids, engine: engine, paramsJson: params)
            } done: { [weak self] ok in
                if ok { self?.review = nil }
            }
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

    /// Runs a blocking apply off the main thread; one history node on success. Refused (with a message)
    /// while another apply runs or a cancelled one is still stopping.
    func start(_ what: String, _ doc: DocumentController, operation: String = "Remove",
               _ body: @escaping @Sendable () throws -> RetouchOutcome,
               done: (@MainActor (Bool) -> Void)? = nil) {
        error = nil
        let refused = jobs.start(what, operation: operation, body) { [weak self] end in
            guard let self else { return }
            switch end {
            case .finished(let r):
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
                self.onFinished?(r)
            case .discarded(let r):
                self.discard(r, operation: operation, doc: doc)
                done?(false)
                self.onDiscarded?(r)
            }
            self.overlay(doc)?.needsDisplay = true
        }
        if let refused {
            notice = refused
            say(refused)
            done?(false)
        } else {
            notice = nil
            say(what)
        }
    }

    /// A cancelled apply returned: nothing of it is kept. When the engine had already committed it before it
    /// saw the cancel, that step is undone so the document is what the user saw when they cancelled.
    private func discard(_ r: Result<RetouchOutcome, Error>, operation: String, doc: DocumentController) {
        if case .success = r {
            _ = doc.run("Undo cancelled \(operation)") { try doc.backend.undo() }
        }
        if notice?.hasPrefix("The cancelled") == true { notice = nil }
        say("\(operation) cancelled; the engine job has stopped")
    }

    /// Seconds the running apply has taken.
    var busySeconds: Double { jobs.seconds }

    /// Cancel button, Esc: back to idle at once (the engine stops in the background).
    func cancel() {
        if strokeOpen || strokeWaitingForModel { cancelStroke(); return }
        let b = Self.backend(document)
        guard let job = jobs.cancel(stop: { b?.cancelRetouch() }) else { return }
        error = nil
        say("\(job.operation) cancelled")
    }

    func fail(_ operation: String, _ e: Error) {
        let p = RetouchErrorPresentation(operation: operation, message: e.localizedDescription)
        error = p.isCancel ? nil : p
        say(p.statusLine)
    }

    func clearError() { error = nil }
    func clearNotice() { notice = nil }

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
    /// This sheet's apply is running (B5-09b: through `RetouchJobs`).
    private(set) var applying = false
    var busy: Bool { applying }
    /// Apply was asked for and waits for the model download.
    private(set) var waitingForModel = false
    private(set) var error: RetouchErrorPresentation?
    @ObservationIgnored private(set) weak var owner: DocumentRetouch?

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
        guard let id = RetouchModelDownloads.modelId(for: state.kind) else { return nil }
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

    /// Footer Cancel: while applying, stops the job and returns the sheet to idle at once (B5-09b); while
    /// waiting for a model download, forgets the pending apply (the download continues); otherwise closes.
    func cancel() {
        if applying {
            applying = false
            owner?.cancel()
            error = nil
            doc.report?("\(state.spec?.name ?? "Neural Filter") cancelled")
            return
        }
        if waitingForModel, let id = requiredModelId {
            owner?.downloads.forgetWaiting(id)
            waitingForModel = false
            return
        }
        owner?.neuralSheet = nil
    }

    /// The model the chosen filter needs (installed or not).
    var requiredModelId: String? { RetouchModelDownloads.modelId(for: state.kind) }

    /// Apply: first the model when it is missing (download when allowed, then apply automatically), then the
    /// filter off the main thread through `RetouchJobs`.
    func apply() {
        guard !busy, !waitingForModel, let owner else { return }
        let name = state.spec?.name ?? "Neural Filter"
        if let why = owner.jobs.refusal {
            error = RetouchErrorPresentation(operation: name, message: why)
            return
        }
        error = nil
        waitingForModel = owner.model(requiredModelId).map { !$0.installed } ?? false
        owner.withModel(requiredModelId, operation: name, run: { [weak self] in
            guard let self, self.owner?.neuralSheet === self else { return }
            self.waitingForModel = false
            self.run()
        }, abandon: { [weak self] in
            self?.waitingForModel = false
        })
    }

    private func run() {
        guard let owner else { return }
        let st = state, layer = layer.id, doc = doc
        let name = st.spec?.name ?? "Neural Filter"
        let smart = st.smartIndex
        let filters = doc.backend as? any DocumentFiltersBackend
        let retouch = DocumentRetouch.backend(doc)
        let refused = owner.jobs.start("Applying \(name)…", operation: name, { () throws -> NeuralApplied in
            if let i = smart {
                guard let f = filters else { throw DocumentError.unsupported("smart filters need the engine") }
                return NeuralApplied(change: try f.setSmartFilter(layer: layer, index: i, change: .params(json: st.filterJson)),
                                     message: "Edit Smart Filter")
            }
            guard let b = retouch else { throw DocumentError.unsupported("neural filters need the engine") }
            let o = try b.neuralFilter(layer: layer, kind: st.kind, paramsJson: st.paramsJson, output: st.output)
            return NeuralApplied(change: o.change, message: String(format: "%@ applied (%@, %.1f s)%@", name, o.backend, o.millis / 1000,
                                                                   o.note.map { " — \($0)" } ?? ""))
        }) { [weak self] end in
            switch end {
            case .finished(.success(let a)):
                doc.run(name) { a.change }
                doc.report?(a.message)
                self?.applying = false
                if self?.owner?.neuralSheet === self { self?.owner?.neuralSheet = nil }
            case .finished(.failure(let e)):
                self?.applying = false
                let p = RetouchErrorPresentation(operation: name, message: e.localizedDescription)
                self?.error = p.isCancel ? nil : p
                doc.report?(p.statusLine)
            case .discarded(let r):
                // Cancelled from the sheet: nothing of it is kept.
                if case .success = r { _ = doc.run("Undo cancelled \(name)") { try doc.backend.undo() } }
                doc.report?("\(name) cancelled; the engine job has stopped")
            }
        }
        if let refused {
            error = RetouchErrorPresentation(operation: name, message: refused)
        } else {
            applying = true
            doc.report?("Applying \(name)…")
        }
    }
}

/// What a neural apply returns to the main actor.
struct NeuralApplied: Sendable {
    let change: DocumentChange
    let message: String
}

extension DocumentRetouch {
    /// Cancel from a sheet.
    func cancelBusy() { cancel() }
}
