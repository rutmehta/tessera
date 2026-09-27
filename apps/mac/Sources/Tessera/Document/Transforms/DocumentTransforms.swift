import AppKit
import Observation
import TesseraCore

/// Edit ▸ Transform ▸ Warp, Perspective Warp, Puppet Warp and Content-Aware Scale (WP B5-12): a modal
/// session on the primary layer that edits one non-destructive `transform` stage of a smart object.
///
/// Engine contract (crates/tessera-ffi/src/document/transform.rs): `begin` returns a token; every drag
/// step sends the complete operation as a preview (scratch only, recorded nowhere), coalesced so at most
/// one call is in flight on a serial queue off the main thread; Apply records ONE history node; Esc
/// cancels with no history change. A layer that is not yet a smart object is wrapped on Apply only after
/// the user agrees (the alert in `TransformSheets`); cancelling never creates a smart object. Large
/// children preview on the engine's reduced proxy while editing (the exact render happens on Apply).
@MainActor @Observable
final class DocumentTransforms {
    static let shared = DocumentTransforms()

    @ObservationIgnored weak var workspace: DocumentWorkspace?
    var document: DocumentController? { workspace?.current }

    // MARK: Session

    struct Session {
        /// The document (by identity: ids are per engine and can repeat across engines).
        weak var doc: DocumentController?
        let start: AdvancedTransformStart
        var op: TransformOperationModel
        var kernel: TransformKernel
        /// The last operation the engine accepted (a rejected edit returns here).
        var accepted: TransformOperationModel
        /// Puppet: solved vertex positions of the accepted preview (child pixels).
        var deformed: [CGPoint]?
        /// A preview has been shown (Apply records something).
        var previewed = false
    }

    private(set) var session: Session?
    /// Perspective: Layout (fit planes to the image) or Warp (move the planes).
    var perspectiveLayout = true
    enum WarpSplit: String, CaseIterable { case none, vertical, horizontal, cross }
    /// Warp: the next click splits the net through the clicked point.
    var warpSplit: WarpSplit = .none
    var warpPreset = "Custom"
    /// Signed bend, percent (−100…100).
    var warpBend: Double = 50
    var showMesh = true
    var selectedPin: Int?
    var puppetDensity: PuppetDensityTag = .normal
    var puppetExpansion: UInt32 = 2
    private(set) var puppetNote: String?
    private(set) var channels: [ProtectionChannel] = []
    /// The Apply alert: wrapping a pixel / text / shape layer needs the user's consent.
    var consentPending = false
    private(set) var status: String?
    /// "Warp: median 48 ms · p95 70 ms (24 drags)" (drag step → presented frame).
    private(set) var latencyReadout: String?
    @ObservationIgnored private(set) var latencies: [Double] = []

    enum Gesture {
        case warpControl(row: Int, column: Int)
        case perspectiveVertex(row: Int, column: Int)
        case pin(Int)
        case pinRotate(Int, startAngle: Double, base: Double)
        case casHandle(ContentAwareScaleModel.Handle)
    }
    @ObservationIgnored private(set) var gesture: Gesture?

    @ObservationIgnored private let queue = DispatchQueue(label: "dev.tessera.document-transforms", qos: .userInteractive)
    @ObservationIgnored private var previewInFlight = false
    @ObservationIgnored private var previewDirty = false
    @ObservationIgnored private var busy = 0
    @ObservationIgnored private var generation = 0
    /// Called when a commit / cancel finished (self-test).
    @ObservationIgnored var onFinished: ((Result<DocumentChange?, Error>) -> Void)?

    private struct CopyOwner {
        weak var document: DocumentController?
        let slot: RasterizedPSDCopySlot
    }
    private var copies: [ObjectIdentifier: CopyOwner] = [:]
    private init() {}

    func isCopying(_ doc: DocumentController?) -> Bool {
        guard let doc else { return false }
        return copies[ObjectIdentifier(doc)]?.slot.id != nil
    }

    func cancelCopy(_ doc: DocumentController) {
        guard let slot = copies[ObjectIdentifier(doc)]?.slot else { return }
        if slot.cancel() { doc.report?("Cancelling rasterized copy…") }
        else { doc.report?("Rasterized copy is finishing its file replacement") }
    }

    func closeCopies(for doc: DocumentController) {
        copies.removeValue(forKey: ObjectIdentifier(doc))?.slot.close()
    }

    func attach(_ workspace: DocumentWorkspace) {
        guard self.workspace !== workspace else { return }
        self.workspace = workspace
        TransformSelfTest.startIfRequested(workspace)
    }

    var isActive: Bool { session != nil }
    func isActive(_ doc: DocumentController?) -> Bool { doc != nil && session?.doc === doc }
    var tag: AdvancedTransformTag? { session?.op.tag }

    private func backend(_ doc: DocumentController) -> (any DocumentTransformsBackend)? { doc.backend as? any DocumentTransformsBackend }
    private func say(_ s: String) { status = s; document?.report?(s) }
    func redraw() { overlay(document)?.needsDisplay = true }

    /// Waits until queued engine work has finished (self-test).
    func idle() async {
        while busy > 0 || previewInFlight { try? await Task.sleep(for: .milliseconds(10)) }
    }

    private func enqueue<T: Sendable>(_ what: String, _ body: @escaping @Sendable () throws -> T,
                                      done: (@MainActor (T) -> Void)? = nil, failed: (@MainActor (Error) -> Void)? = nil,
                                      reportFailure: Bool = true) {
        busy += 1
        queue.async {
            let r = Result { try body() }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let me = DocumentTransforms.shared
                    me.busy -= 1
                    switch r {
                    case .success(let v): done?(v)
                    case .failure(let e):
                        if reportFailure { me.say("\(what): \(e.localizedDescription)") }
                        failed?(e)
                    }
                }
            }
        }
    }

    // MARK: Begin / end

    /// Whether Edit ▸ Transform ▸ `tag` can start on the document's primary layer.
    func canBegin(_ doc: DocumentController?) -> Bool {
        guard let doc, let l = doc.primary, backend(doc) != nil else { return false }
        return l.kind != .adjustment
    }

    /// Edit ▸ Transform ▸ Warp / Perspective Warp / Puppet Warp / Content-Aware Scale on the primary layer.
    func begin(_ tag: AdvancedTransformTag) {
        guard let doc = document, let layer = doc.primary else { say("\(tag.title): select a layer"); return }
        begin(tag, doc: doc, layer: layer.id, index: nil)
    }

    /// Re-edits transform stage `index` of smart object `layer` (double-click on its smart filter row).
    func editStage(_ doc: DocumentController, layer: DocLayerID, index: UInt32) {
        guard let t = backend(doc) else { return }
        do {
            guard let stage = try t.transformStages(layer: layer).first(where: { $0.index == index }) else { return }
            guard stage.kind.editable else {
                say("\(stage.kind.title) stages are edited where they were made (not here)")
                return
            }
            begin(stage.kind, doc: doc, layer: layer, index: index)
        } catch { say("Transform: \(error.localizedDescription)") }
    }

    private func begin(_ tag: AdvancedTransformTag, doc: DocumentController, layer: DocLayerID, index: UInt32?) {
        guard let t = backend(doc) else { say("\(tag.title) needs the engine document backend"); return }
        // One gesture owns the canvas: finish text, Free Transform and any older session first.
        if DocumentText.shared.isEditing(doc) {
            if DocumentText.shared.isComposing { DocumentText.shared.cancel() } else { DocumentText.shared.apply() }
        }
        if DocumentTools.shared.transform != nil { DocumentTools.shared.commitTransform() }
        if session != nil { cancel() }
        status = nil
        selectedPin = nil
        warpSplit = .none
        generation += 1
        let gen = generation
        let (density, expansion) = (puppetDensity, puppetExpansion)
        enqueue(tag.title) { () -> (AdvancedTransformStart, PuppetMeshInfo?, [ProtectionChannel]) in
            let start = try t.beginAdvancedTransform(layer: layer, index: index, kind: tag)
            var mesh: PuppetMeshInfo?
            if tag == .puppet, start.existing == nil {
                do {
                    mesh = try t.puppetMesh(layer: layer, density: density, expansion: expansion)
                } catch {
                    _ = try? t.cancelAdvancedTransform(token: start.token)
                    throw error
                }
            }
            return (start, mesh, tag == .contentAwareScale ? t.protectionChannels() : [])
        } done: { [weak doc] (start, mesh, channels) in
            let me = DocumentTransforms.shared
            guard let doc, me.generation == gen, me.document === doc else {
                _ = try? t.cancelAdvancedTransform(token: start.token)
                return
            }
            me.channels = channels
            me.started(start, mesh: mesh, doc: doc)
        }
    }

    private func started(_ start: AdvancedTransformStart, mesh: PuppetMeshInfo?, doc: DocumentController) {
        var kernel: TransformKernel = start.kind == .contentAwareScale ? .bilinear : .bicubic
        var op: TransformOperationModel
        if let existing = start.existing,
           let (parsed, k) = TransformOperationModel.parse(existing.json, canvasWidth: start.childWidth, canvasHeight: start.childHeight) {
            op = parsed
            kernel = k
            if case .puppet(let p) = parsed, let d = PuppetDensityTag(rawValue: p.density) {
                puppetDensity = d
                puppetExpansion = p.expansion
            }
        } else {
            switch start.kind {
            case .warp:
                op = .warp(.identity(width: Double(start.childWidth), height: Double(start.childHeight)))
            case .perspective:
                op = .perspective(PerspectiveModel(rect: start.contentRect))
            case .puppet:
                guard let m = mesh?.mesh else { say("Puppet Warp: no mesh"); return }
                op = .puppet(m)
                puppetNote = mesh?.note
            default:
                op = .contentAwareScale(ContentAwareScaleModel(canvasWidth: start.childWidth, canvasHeight: start.childHeight))
            }
        }
        if case .puppet = op, mesh == nil { puppetNote = nil }
        session = Session(doc: doc, start: start, op: op, kernel: kernel, accepted: op)
        watchDocument()
        warpPreset = "Custom"
        _ = ensureOverlay(doc)
        doc.viewport?.cursorDidChange()
        let hint: String = switch start.kind {
        case .warp: "Warp: drag the net's points and handles; Split, then click to add lines"
        case .perspective: "Perspective Warp: Layout fits planes to the image, Warp moves them"
        case .puppet: "Puppet Warp: click adds a pin, drag moves it, ⌥-click removes, ⌥-drag beside a pin rotates"
        default: "Content-Aware Scale: drag the right or bottom handle, or type the size"
        }
        say(hint + (start.needsConversion ? " · Apply converts the layer to a smart object" : "") + " · Return applies, Esc cancels")
        redraw()
    }

    /// Return / Apply. A layer that is not a smart object asks first.
    func apply() {
        guard let s = session else { return }
        if s.start.needsConversion, s.previewed { consentPending = true; return }
        commit(convert: false)
    }

    /// The Apply alert's "Convert and Apply".
    func confirmConversion() {
        consentPending = false
        commit(convert: true)
    }

    private func commit(convert: Bool) {
        guard let s = session, let doc = document, s.doc === doc, let t = backend(doc) else { return }
        flushPreviewThen { [weak self] in
            guard let self else { return }
            let token = s.start.token
            let title = s.op.tag.title
            self.end()
            self.enqueue(title) {
                try t.commitAdvancedTransform(token: token, convert: convert)
            } done: { [weak doc] c in
                guard let doc else { return }
                let me = DocumentTransforms.shared
                me.say("\(title) applied")
                me.reloadAfterFrame(doc, epoch: c.epoch) { me.onFinished?(.success(c)) }
            } failed: { e in
                DocumentTransforms.shared.onFinished?(.failure(e))
                // The session stays open in the engine only when consent was missing; end it cleanly.
                _ = try? t.cancelAdvancedTransform(token: token)
                doc.reloadModel()
            }
        }
    }

    /// Esc / Cancel: no history change.
    func cancel() {
        guard let s = session, let doc = s.doc, let t = backend(doc) else { end(); return }
        let token = s.start.token
        end()
        enqueue("Cancel") {
            try t.cancelAdvancedTransform(token: token)
        } done: { [weak doc] c in
            guard let doc else { return }
            DocumentTransforms.shared.reloadAfterFrame(doc, epoch: c.epoch) { DocumentTransforms.shared.onFinished?(.success(c)) }
        } failed: { e in
            DocumentTransforms.shared.onFinished?(.failure(e))
        }
    }

    private func end() {
        session = nil
        gesture = nil
        consentPending = false
        previewDirty = false
        selectedPin = nil
        generation += 1
        document?.viewport?.cursorDidChange()
        redraw()
    }

    /// Another tool was chosen: apply what does not need consent, otherwise cancel.
    func toolSelected() {
        guard let s = session else { return }
        if s.start.needsConversion { say("\(s.op.tag.title) cancelled: applying it converts the layer (use Apply)"); cancel() } else { apply() }
    }

    /// Cancels the session when the workspace switches to another document (tab, open, close).
    private func watchDocument() {
        withObservationTracking {
            _ = workspace?.current
        } onChange: {
            Task { @MainActor in
                let me = DocumentTransforms.shared
                guard let s = me.session else { return }
                if me.workspace?.current !== s.doc { me.cancel() } else { me.watchDocument() }
            }
        }
    }

    /// The document is switching or closing: never apply into another document.
    func documentWillChange() { if session != nil { cancel() } }

    /// Reloads rows and history once the engine has presented `epoch` (the render thread holds the
    /// session while it renders an exact stage, so reloading at once would block the main thread).
    private func reloadAfterFrame(_ doc: DocumentController, epoch: UInt64, then: (@MainActor () -> Void)? = nil) {
        Task { @MainActor [weak doc] in
            let end = Date().addingTimeInterval(20)
            while let d = doc, (d.lastFrame?.epoch ?? 0) < epoch, d.viewport != nil, Date() < end {
                try? await Task.sleep(for: .milliseconds(15))
            }
            guard let doc else { return }
            doc.reloadModel()
            doc.reloadHistory()
            then?()
        }
    }

    // MARK: Previews

    /// The operation changed: preview it (coalesced).
    func changed() {
        guard session != nil else { return }
        previewDirty = true
        redraw()
        pumpPreview()
    }

    /// Replaces the operation (options bar, self-test) and previews it.
    func update(_ f: (inout TransformOperationModel) -> Void) {
        guard var s = session else { return }
        f(&s.op)
        session = s
        changed()
    }

    private func pumpPreview() {
        guard previewDirty, !previewInFlight, let s = session, let doc = document, s.doc === doc, let t = backend(doc) else { return }
        previewDirty = false
        previewInFlight = true
        let op = s.op, kernel = s.kernel, token = s.start.token
        let draft = s.start.draftLevel > 0
        let gen = generation
        let started = Date()
        queue.async {
            let r = Result { () -> AdvancedTransformPreviewResult in
                if case .contentAwareScale(let c) = op {
                    return AdvancedTransformPreviewResult(
                        change: try t.contentAwareScale(token: token, width: c.width, height: c.height, amount: c.amount,
                                                        protect: c.protectChannel, draft: draft),
                        deformed: nil)
                }
                return try t.previewAdvancedTransform(token: token, json: op.json(kernel: kernel), draft: draft)
            }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let me = DocumentTransforms.shared
                    me.previewInFlight = false
                    guard me.generation == gen, var s = me.session else { return }
                    switch r {
                    case .success(let p):
                        s.accepted = op
                        s.previewed = true
                        if let d = p.deformed { s.deformed = d }
                        me.session = s
                        me.status = nil
                        me.track(epoch: p.change.epoch, since: started, doc: doc)
                    case .failure(let e):
                        // The engine kept the previous preview; so does the editor (unless a newer edit is queued).
                        if !me.previewDirty { s.op = s.accepted; me.session = s }
                        me.status = "\(op.tag.title): \(e.localizedDescription)"
                        doc.report?(me.status ?? "")
                    }
                    me.redraw()
                    me.pumpPreview()
                }
            }
        }
    }

    /// Runs `then` once the latest edit has been previewed (Apply commits what was shown).
    private func flushPreviewThen(_ then: @escaping @MainActor () -> Void) {
        Task { @MainActor in
            pumpPreview()
            while previewInFlight || previewDirty {
                try? await Task.sleep(for: .milliseconds(5))
                pumpPreview()
            }
            then()
        }
    }

    /// Drag step → presented frame latency (the frame whose epoch covers the preview).
    private func track(epoch: UInt64, since: Date, doc: DocumentController) {
        Task { @MainActor [weak doc] in
            let end = Date().addingTimeInterval(10)
            while let d = doc, (d.lastFrame?.epoch ?? 0) < epoch, Date() < end, d.viewport != nil {
                try? await Task.sleep(for: .milliseconds(2))
            }
            guard let d = doc, (d.lastFrame?.epoch ?? 0) >= epoch else { return }
            let me = DocumentTransforms.shared
            me.latencies.append(Date().timeIntervalSince(since) * 1000)
            let sorted = me.latencies.suffix(200).sorted()
            let median = sorted[sorted.count / 2], p95 = sorted[min(sorted.count - 1, Int(Double(sorted.count) * 0.95))]
            me.latencyReadout = String(format: "Preview median %.0f ms · p95 %.0f ms (%d)", median, p95, sorted.count)
            if ProcessInfo.processInfo.environment["TESSERA_TRANSFORM_LATENCY_LOG"] != nil {
                FileHandle.standardError.write(Data(String(format: "transform-latency: %.2f ms epoch %llu\n",
                                                           me.latencies.last ?? 0, epoch).utf8))
            }
        }
    }

    func resetLatencies() { latencies.removeAll(); latencyReadout = nil }

    // MARK: Options bar actions

    var kernel: TransformKernel {
        get { session?.kernel ?? .bicubic }
        set { guard session != nil else { return }; session?.kernel = newValue; changed() }
    }

    /// Warp ▸ preset (or Custom) with the current bend.
    func applyWarpPreset(_ name: String) {
        warpPreset = name
        guard name != "Custom", let s = session, case .warp = s.op else { return }
        do {
            let m = try TransformBridge.preset(width: Double(s.start.childWidth), height: Double(s.start.childHeight), name: name,
                                               bend: warpBend / 100)
            update { $0 = .warp(m) }
        } catch { say("Warp: \(error.localizedDescription)") }
    }

    func setWarpBend(_ percent: Double) {
        warpBend = min(max(percent, -100), 100)
        if warpPreset != "Custom" { applyWarpPreset(warpPreset) }
    }

    /// Warp ▸ Grid: a regular net (the current surface kept).
    func warpGrid(_ n: UInt32) {
        guard let s = session, case .warp(let m) = s.op else { return }
        do {
            let g = try TransformBridge.subdivide(m, columns: n, rows: n)
            update { $0 = .warp(g) }
        } catch { say("Warp: \(error.localizedDescription)") }
    }

    /// Warp split through a child point (exact: the image does not move).
    func splitWarp(at child: CGPoint, _ mode: WarpSplit) {
        guard let s = session, case .warp(let m) = s.op, mode != .none else { return }
        do {
            let next = try TransformBridge.split(m, at: child, vertical: mode == .vertical || mode == .cross,
                                                 horizontal: mode == .horizontal || mode == .cross)
            update { $0 = .warp(next) }
            warpPreset = "Custom"
        } catch { say("Warp split: \(error.localizedDescription)") }
    }

    func resetOperation() {
        guard let s = session else { return }
        switch s.op {
        case .warp: update { $0 = .warp(.identity(width: Double(s.start.childWidth), height: Double(s.start.childHeight))) }
        case .perspective(var p): p.destination = p.source; update { $0 = .perspective(p) }
        case .puppet(var p): p.pins = []; selectedPin = nil; update { $0 = .puppet(p) }
        case .contentAwareScale(var c):
            c.width = c.canvasWidth; c.height = c.canvasHeight
            update { $0 = .contentAwareScale(c) }
        }
        warpPreset = "Custom"
    }

    /// Perspective ▸ split the plane grid (column or row through the middle of cell `cell`).
    func splitPerspective(vertical: Bool, cell: (row: Int, column: Int)? = nil) {
        guard let s = session, case .perspective(var p) = s.op else { return }
        let c = cell ?? (p.rows / 2, p.columns / 2)
        if vertical { p.splitColumn(c.column) } else { p.splitRow(c.row) }
        update { $0 = .perspective(p) }
    }

    /// Puppet ▸ density / expansion: a new mesh; pins keep their places.
    func remesh() {
        guard let s = session, let doc = document, let t = backend(doc), case .puppet(let old) = s.op else { return }
        let (layer, density, expansion) = (s.start.layer, puppetDensity, puppetExpansion)
        enqueue("Puppet Warp") {
            try t.puppetMesh(layer: layer, density: density, expansion: expansion)
        } done: { info in
            let me = DocumentTransforms.shared
            guard me.session?.start.token == s.start.token else { return }
            me.puppetNote = info.note
            me.selectedPin = nil
            me.session?.deformed = nil
            me.update { $0 = .puppet(old.rebased(onto: info.mesh)) }
        }
    }

    func setPuppetRigid(_ rigid: Bool) {
        guard let s = session, case .puppet(var p) = s.op else { return }
        p.mode = rigid ? "Rigid" : "Normal"
        update { $0 = .puppet(p) }
    }

    /// The selected pin's rotation in degrees (nil: automatic).
    var selectedPinDegrees: Double? {
        guard let s = session, case .puppet(let p) = s.op, let i = selectedPin, i < p.pins.count else { return nil }
        return p.pins[i].rotation.map { $0 * 180 / .pi }
    }

    func setSelectedPinDegrees(_ degrees: Double?) {
        guard let s = session, case .puppet(var p) = s.op, let i = selectedPin, i < p.pins.count else { return }
        p.pins[i].rotation = degrees.map { $0 * .pi / 180 }
        update { $0 = .puppet(p) }
    }

    func removeSelectedPin() {
        guard let s = session, case .puppet(var p) = s.op, let i = selectedPin, i < p.pins.count else { return }
        p.pins.remove(at: i)
        selectedPin = nil
        update { $0 = .puppet(p) }
    }

    func setScale(width: UInt32? = nil, height: UInt32? = nil, amount: Float? = nil, protect: UInt64?? = nil) {
        guard let s = session, case .contentAwareScale(var c) = s.op else { return }
        if let width { c.width = max(width, 1) }
        if let height { c.height = max(height, 1) }
        if let amount { c.amount = min(max(amount, 0), 1) }
        if let protect { c.protectChannel = protect }
        update { $0 = .contentAwareScale(c) }
    }

    /// File ▸ Save Rasterized PSD Copy…: PSD refuses native-only stacks; this writes a copy with them applied.
    func saveRasterizedPSD(_ doc: DocumentController) {
        guard !doc.isClosed, !isCopying(doc), let t = backend(doc) else { return }
        let panel = NSSavePanel()
        panel.title = "Save Rasterized PSD Copy"
        panel.message = "Smart objects with transform stages or smart filters are rasterized in the copy; this document is unchanged."
        panel.allowedContentTypes = [.init(filenameExtension: "psd")].compactMap { $0 }
        panel.nameFieldStringValue = (doc.title as NSString).deletingPathExtension + " (rasterized).psd"
        let handle: @MainActor (NSApplication.ModalResponse) -> Void = { [weak doc] r in
            guard r == .OK, let url = panel.url, let doc, !doc.isClosed else { return }
            let me = DocumentTransforms.shared
            guard !me.isCopying(doc) else { return }
            do {
                // Retain a cancel handle on MainActor before enqueuing blocking Rust work.
                let operation = try t.prepareRasterizedPSDCopy()
                me.enqueueCopy(operation, for: doc, url: url)
            } catch { doc.report?("Save Rasterized PSD Copy: \(error.localizedDescription)") }
        }
        if let window = doc.viewport?.window {
            panel.beginSheetModal(for: window) { r in MainActor.assumeIsolated { handle(r) } }
        } else {
            handle(panel.runModal())
        }
    }

    /// Separate from the save panel so ownership/cancellation can be tested with tiny fakes.
    func enqueueCopy(_ operation: any RasterizedPSDCopyOperation, for doc: DocumentController, url: URL) {
        guard !doc.isClosed, !isCopying(doc) else { _ = operation.cancel(); return }
        let slot = RasterizedPSDCopySlot()
        guard let id = slot.install(operation) else { return }
        let owner = ObjectIdentifier(doc)
        copies[owner] = CopyOwner(document: doc, slot: slot)
        doc.report?("Saving rasterized copy… Use File > Cancel Rasterized PSD Copy to cancel.")
        enqueue("Save Rasterized PSD Copy", {
            try operation.run(path: url.path)
        }, done: { [weak doc] outcome in
            guard self.finishCopy(owner, id: id), let doc, !doc.isClosed else { return }
            doc.report?(outcome == .saved
                ? "Saved a rasterized copy as \(url.lastPathComponent)"
                : "Rasterized copy cancelled")
        }, failed: { [weak doc] error in
            guard self.finishCopy(owner, id: id), let doc, !doc.isClosed else { return }
            doc.report?("Save Rasterized PSD Copy: \(error.localizedDescription)")
        }, reportFailure: false)
    }

    private func finishCopy(_ owner: ObjectIdentifier, id: UUID) -> Bool {
        guard let copy = copies[owner], copy.slot.finish(id) else { return false }
        copies.removeValue(forKey: owner)
        return copy.document != nil
    }

    // MARK: Canvas

    private let hitRadius = 7.0

    private func mapping() -> ChildMapping? { session.map { $0.start.mapping } }

    /// Child pixels → viewport points.
    func viewPoint(_ child: CGPoint, in v: DocumentViewportView) -> CGPoint {
        v.viewPoint(canvas: mapping()?.document(child) ?? child)
    }

    private func childPoint(_ e: NSEvent, in v: DocumentViewportView) -> CGPoint {
        let p = v.canvasPoint(e)
        return mapping()?.child(p) ?? p
    }

    func mouseDown(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard var s = session else { return false }
        let viewP = v.convert(e.locationInWindow, from: nil)
        let child = childPoint(e, in: v)
        let option = e.modifierFlags.contains(.option)
        let map = { (p: CGPoint) in self.viewPoint(p, in: v) }
        switch s.op {
        case .warp(let m):
            if warpSplit != .none {
                splitWarp(at: child, warpSplit)
                if !e.modifierFlags.contains(.shift) { warpSplit = .none }
            } else if let h = m.hit(viewP, radius: hitRadius, map: map) {
                gesture = .warpControl(row: h.row, column: h.column)
                warpPreset = "Custom"
            }
        case .perspective(let p):
            if let h = p.hit(viewP, radius: hitRadius, layout: perspectiveLayout, map: map) {
                gesture = .perspectiveVertex(row: h.row, column: h.column)
            }
        case .puppet(var p):
            let positions = s.deformed ?? p.restVertices
            if let i = p.pinHit(viewP, radius: hitRadius, map: map) {
                if option, e.clickCount >= 1, !e.modifierFlags.contains(.shift) {
                    // ⌥-click on a pin removes it.
                    p.pins.remove(at: i)
                    selectedPin = nil
                    update { $0 = .puppet(p) }
                } else {
                    selectedPin = i
                    gesture = .pin(i)
                }
            } else if option, let i = p.pinHit(viewP, radius: hitRadius * 5, map: map) {
                // ⌥-drag beside a pin rotates the mesh around it.
                let c = map(p.pins[i].target)
                selectedPin = i
                gesture = .pinRotate(i, startAngle: atan2(Double(viewP.y - c.y), Double(viewP.x - c.x)),
                                     base: p.pins[i].rotation ?? 0)
            } else if let i = p.addPin(near: child, positions: positions,
                                       radius: hitRadius * 3 / max(v.pointsPerPixel * scale(s), 1e-6)) {
                selectedPin = i
                s.op = .puppet(p)
                session = s
                gesture = .pin(i)
                changed()
            }
        case .contentAwareScale(let c):
            if let h = ContentAwareScaleModel.Handle.allCases.first(where: {
                let q = map(c.point($0))
                return hypot(Double(q.x - viewP.x), Double(q.y - viewP.y)) <= hitRadius
            }) {
                gesture = .casHandle(h)
            }
        }
        redraw()
        return true
    }

    /// Document pixels per child pixel (placement scale).
    private func scale(_ s: Session) -> Double {
        let m = s.start.childToDocument
        return sqrt(abs(m.determinant))
    }

    func mouseDragged(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard let s = session, let g = gesture else { return session != nil }
        let child = childPoint(e, in: v)
        let viewP = v.convert(e.locationInWindow, from: nil)
        switch (g, s.op) {
        case (.warpControl(let r, let c), .warp(var m)):
            m.move(row: r, column: c, to: child)
            update { $0 = .warp(m) }
        case (.perspectiveVertex(let r, let c), .perspective(var p)):
            var next = p
            next.move(row: r, column: c, to: child, layout: perspectiveLayout)
            if next.isValid {
                p = next
                update { $0 = .perspective(p) }
            } else {
                status = "Perspective Warp: planes must stay convex (the previous shape is kept)"
            }
        case (.pin(let i), .puppet(var p)) where i < p.pins.count:
            p.pins[i].target = child
            update { $0 = .puppet(p) }
        case (.pinRotate(let i, let start, let base), .puppet(var p)) where i < p.pins.count:
            let c = viewPoint(p.pins[i].target, in: v)
            let a = atan2(Double(viewP.y - c.y), Double(viewP.x - c.x))
            p.pins[i].rotation = base + (a - start)
            update { $0 = .puppet(p) }
        case (.casHandle(let h), .contentAwareScale(var c)):
            c.drag(h, to: child, proportional: e.modifierFlags.contains(.shift))
            update { $0 = .contentAwareScale(c) }
        default: break
        }
        return true
    }

    func mouseUp(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard session != nil else { return false }
        gesture = nil
        redraw()
        return true
    }

    /// Return applies, Esc cancels, ⌫ removes the selected pin. Returns whether the key was used.
    func handleKey(_ e: NSEvent) -> Bool {
        guard session != nil, isActive(document) else { return false }
        let mods = e.modifierFlags.intersection(.deviceIndependentFlagsMask).subtracting([.numericPad, .function, .capsLock])
        switch e.keyCode {
        case 36, 76 where mods.isEmpty || mods == [.command]: apply(); return true
        case 53 where mods.isEmpty: cancel(); return true
        case 51, 117 where mods.isEmpty:
            if case .puppet = session?.op, selectedPin != nil { removeSelectedPin(); return true }
            return false
        default: return false
        }
    }

    // MARK: Overlay

    func overlay(_ doc: DocumentController?) -> TransformOverlayView? {
        doc?.viewport?.subviews.compactMap { $0 as? TransformOverlayView }.first
    }

    private func ensureOverlay(_ doc: DocumentController) -> TransformOverlayView? {
        guard let v = doc.viewport else { return nil }
        if let o = overlay(doc) { return o }
        let o = TransformOverlayView()
        o.frame = v.bounds
        o.autoresizingMask = [.width, .height]
        o.viewport = v
        v.addSubview(o, positioned: .above, relativeTo: v.toolOverlay)
        return o
    }
}
