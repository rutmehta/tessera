import AppKit
import Observation
import TesseraCore

/// Filter ▸ Adaptive Wide Angle… (⌥⇧⌘A, WP B5-20): a workspace sheet with its own canvas.
///
/// The engine snapshots the layer (`begin_adaptive_wide_angle`) and renders proxy previews of the recipe this model
/// edits (`AdaptiveWideAngleDraft`): a camera model (Perspective or Fisheye) with a focal length, a scale, and
/// constraint lines drawn on the source. Each line is traced by the engine along the camera model's image of a
/// straight edge (`adaptive_wide_angle_curve`), as Photoshop's Constraint tool bends its lines. Previews run off the
/// main thread, latest wins. OK renders at full resolution as one history node (`RetouchJobs`): the pixels of a
/// pixel layer, or a smart filter on a smart object; double-clicking that smart filter re-opens it here.
@MainActor @Observable
final class DocumentAdaptiveWideAngle {
    static let shared = DocumentAdaptiveWideAngle()

    /// The open workspace (the sheet's item).
    var workspace: AdaptiveWideAngleWorkspaceModel?
    /// While the engine snapshots the layer.
    private(set) var opening = false
    /// Called when a workspace is shown (self-test).
    @ObservationIgnored var onOpened: ((AdaptiveWideAngleWorkspaceModel) -> Void)?

    private init() {}

    static func backend(_ doc: DocumentController?) -> (any DocumentAdaptiveWideAngleBackend)? {
        doc?.backend as? any DocumentAdaptiveWideAngleBackend
    }

    /// Smart filter rows whose double-click opens this workspace.
    static func handles(filterId: String) -> Bool { filterId == AdaptiveWideAngleFilter.id }

    /// Filter ▸ Adaptive Wide Angle…: a new filter on the selected layer.
    func open(_ doc: DocumentController) {
        if let why = AdaptiveWideAngleFilter.refusal(kind: doc.primary?.kind) { doc.report?(why); return }
        guard let l = doc.primary else { return }
        start(doc, layer: l, stage: nil)
    }

    /// Double-click on an Adaptive Wide Angle smart filter row: re-edit it in place.
    func edit(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        guard let node = doc.node(layer) else { return }
        start(doc, layer: node, stage: row.index)
    }

    private func start(_ doc: DocumentController, layer: LayerRecord, stage: UInt32?) {
        let title = AdaptiveWideAngleFilter.title
        guard workspace == nil, !opening, let b = Self.backend(doc) else { return }
        opening = true
        doc.report?("\(title): preparing \(layer.name)…")
        let (id, name) = (layer.id, layer.name)
        Task { @MainActor in
            let r = await Task.detached(priority: .userInitiated) {
                Result { try b.beginAdaptiveWideAngle(layer: id, stageIndex: stage) }
            }.value
            self.opening = false
            do {
                let info = try r.get()
                let draft: AdaptiveWideAngleDraft
                do {
                    draft = try AdaptiveWideAngleDraft(recipeJson: info.recipeJson)
                } catch {
                    b.cancelAdaptiveWideAngle(token: info.token)
                    throw error
                }
                let m = AdaptiveWideAngleWorkspaceModel(doc: doc, backend: b, info: info, draft: draft, layerName: name)
                self.workspace = m
                m.start()
                doc.report?(info.stageIndex == nil
                            ? "\(title): drag along edges that should be straight (⇧ for horizontal or vertical)"
                            : "\(title): re-editing the smart filter; OK replaces it")
                self.onOpened?(m)
            } catch {
                doc.report?("\(title): \(error.localizedDescription)")
            }
        }
    }

    /// The sheet went away (OK finished or Cancel).
    func closed(_ m: AdaptiveWideAngleWorkspaceModel) {
        if workspace === m { workspace = nil }
    }
}

/// One open Adaptive Wide Angle workspace.
@MainActor @Observable
final class AdaptiveWideAngleWorkspaceModel: Identifiable {
    let id = UUID()
    let doc: DocumentController
    let backend: any DocumentAdaptiveWideAngleBackend
    let info: AdaptiveWideAngleWorkspaceInfo
    let layerName: String

    private(set) var draft: AdaptiveWideAngleDraft
    /// The corrected result; off shows the source with the constraint lines (drawing turns it off).
    var preview = true { didSet { if preview != oldValue { changed() } } }
    var showConstraints = true { didSet { changed() } }
    /// The selected constraint (Delete removes it; the Orientation picker edits it).
    var selected: Int? { didSet { changed() } }
    var view = LiquifyViewTransform(scale: 1, origin: .zero)
    /// The line being drawn (source pixels).
    private(set) var rubberBand: (from: CGPoint, to: CGPoint)?
    private(set) var original: CGImage?
    private(set) var corrected: CGImage?
    private(set) var error: String?
    private(set) var previewMillis: Double?
    /// Bumped on any canvas-relevant change.
    private(set) var revision = 0
    let jobs = RetouchJobs()
    var busy: String? { jobs.running?.title }

    @ObservationIgnored private var gate = LatestRequestBuffer<String>()
    @ObservationIgnored private var fitted = false
    @ObservationIgnored private var closed = false
    @ObservationIgnored private var tracedKey: String
    /// Called when a preview lands (self-test) and when OK finishes or is discarded.
    @ObservationIgnored var onPreview: (() -> Void)?
    @ObservationIgnored var onApplied: ((Result<DocumentChange, Error>) -> Void)?

    init(doc: DocumentController, backend: any DocumentAdaptiveWideAngleBackend, info: AdaptiveWideAngleWorkspaceInfo,
         draft: AdaptiveWideAngleDraft, layerName: String) {
        self.doc = doc; self.backend = backend; self.info = info; self.draft = draft; self.layerName = layerName
        tracedKey = draft.curveKey
    }

    var title: String { AdaptiveWideAngleFilter.title }
    var subtitle: String {
        var s = "\(layerName) · \(info.width) × \(info.height) px"
        if let i = info.stageIndex { s += " · re-editing smart filter \(i + 1)" }
        else { s += info.smartObject ? " · adds a smart filter" : " · changes the layer's pixels" }
        return s
    }
    var previewNote: String {
        info.previewFactor == 1 ? "Preview at full resolution"
            : "Preview at 1/\(info.previewFactor) (\(info.previewWidth) × \(info.previewHeight)); OK renders \(info.width) × \(info.height)"
    }
    var image: CGImage? { preview ? (corrected ?? original) : original }
    var overlayVisible: Bool { !preview && showConstraints }

    private func changed() { revision &+= 1 }

    func start() {
        let (b, t) = (backend, info.token)
        Task { @MainActor [weak self] in
            let r = await Task.detached(priority: .userInitiated) { Result { try b.previewAdaptiveWideAngle(token: t, recipeJson: nil) } }.value
            guard let self, !self.closed else { return }
            if case .success(let f) = r { self.original = Self.image(f) }
            self.changed()
        }
        schedulePreview()
    }

    // MARK: Recipe controls

    var projection: AdaptiveProjection {
        get { draft.projection }
        set { draft.projection = newValue; retrace(); schedulePreview() }
    }

    func setFocal(_ mm: Double) {
        draft.setFocal35(mm)
        retrace()
        schedulePreview()
    }

    func setScale(_ percent: Double) {
        draft.setScalePercent(percent)
        schedulePreview()
    }

    func useExifFocal() { if let f = info.exifFocal35mm { setFocal(f) } }

    func setOrientation(_ o: AdaptiveLineOrientation) {
        guard let i = selected else { return }
        draft.setOrientation(o, at: i)
        schedulePreview()
    }

    func removeSelected() {
        guard let i = selected else { return }
        draft.removeLine(at: i)
        selected = nil
        schedulePreview()
    }

    func removeAll() {
        draft.removeAllLines()
        selected = nil
        schedulePreview()
    }

    // MARK: Drawing

    func fit(in size: CGSize) {
        view = .fit(source: info.sourceSize, in: size, margin: Double(Theme.Space.l))
        fitted = true
        changed()
    }

    func fitIfNeeded(in size: CGSize) { if !fitted, size.width > 0, size.height > 0 { fit(in: size) } }

    func zoom(by f: Double, around p: CGPoint) { view.zoom(by: f, around: p); changed() }
    func pan(dx: Double, dy: Double) { view.pan(dx: dx, dy: dy); changed() }

    /// Pointer down on the canvas (source pixels): selects the line under it, or starts a new one.
    func pointerDown(_ source: CGPoint, tolerance: Double) {
        guard busy == nil, !closed else { return }
        if preview { preview = false }
        showConstraints = true
        if let i = draft.line(near: source, tolerance: tolerance) {
            selected = i
            return
        }
        selected = nil
        let p = draft.clamp(source)
        rubberBand = (p, p)
        changed()
    }

    func pointerDragged(_ source: CGPoint) {
        guard let r = rubberBand else { return }
        rubberBand = (r.from, draft.clamp(source))
        changed()
    }

    func pointerUp(_ source: CGPoint, constrain: Bool) {
        guard let r = rubberBand else { return }
        rubberBand = nil
        addLine(from: r.from, to: draft.clamp(source), orientation: .forDrag(from: r.from, to: source, constrain: constrain))
    }

    /// Adds a constraint traced along the camera model; refuses lines outside the camera's field of view.
    @discardableResult
    func addLine(from a: CGPoint, to b: CGPoint, orientation: AdaptiveLineOrientation) -> Bool {
        var d = draft
        guard let i = d.addLine(from: a, to: b, orientation: orientation) else { changed(); return false }
        do {
            d.setCurve(try backend.adaptiveWideAngleCurve(recipeJson: d.recipeJson, from: d.lines[i].from, to: d.lines[i].to), at: i)
        } catch {
            self.error = error.localizedDescription
            changed()
            return false
        }
        draft = d
        selected = i
        error = nil
        schedulePreview()
        return true
    }

    /// Re-traces every line after a camera change (pure maths in the engine: synchronous).
    private func retrace() {
        guard draft.curveKey != tracedKey else { return }
        tracedKey = draft.curveKey
        var d = draft
        for i in d.lines.indices {
            do {
                d.setCurve(try backend.adaptiveWideAngleCurve(recipeJson: d.recipeJson, from: d.lines[i].from, to: d.lines[i].to), at: i)
            } catch {
                self.error = "Constraint \(i + 1): \(error.localizedDescription)"
            }
        }
        draft = d
    }

    // MARK: Preview

    private func schedulePreview() {
        changed()
        guard !closed, let r = gate.submit(draft.recipeJson) else { return }
        run(r)
    }

    private func run(_ request: LatestRequestBuffer<String>.Request) {
        let (b, t, json) = (backend, info.token, request.value)
        Task { @MainActor [weak self] in
            let r = await Task.detached(priority: .userInitiated) {
                Result { () -> (AdaptiveWideAnglePreviewFrame, CGImage?) in
                    let f = try b.previewAdaptiveWideAngle(token: t, recipeJson: json)
                    return (f, Self.image(f))
                }
            }.value
            guard let self else { return }
            let (accept, next) = self.gate.finish(request.generation)
            if accept, !self.closed {
                switch r {
                case .success(let (f, image)):
                    self.corrected = image
                    self.previewMillis = f.millis
                    self.error = nil
                case .failure(let e):
                    self.corrected = nil
                    self.error = e.localizedDescription
                }
                self.changed()
                self.onPreview?()
            }
            if let next, !self.closed { self.run(next) }
        }
    }

    nonisolated static func image(_ f: AdaptiveWideAnglePreviewFrame) -> CGImage? {
        IOSurfaceLookup(f.surfaceId).flatMap { FilterSheetModel.image($0, width: Int(f.width), height: Int(f.height)) }
    }

    // MARK: Keys (canvas first responder)

    func key(_ e: NSEvent) -> Bool {
        if e.keyCode == 51 || e.keyCode == 117 { removeSelected(); return true }
        guard let c = e.charactersIgnoringModifiers?.first, !e.modifierFlags.contains(.command) else { return false }
        if c == "p" { preview.toggle(); return true }
        return false
    }

    // MARK: OK / Cancel

    func ok() {
        guard busy == nil, !closed else { return }
        let (b, t, json, doc, title) = (backend, info.token, draft.recipeJson, doc, title)
        let refused = jobs.start("Applying \(title)…", operation: title, {
            try b.commitAdaptiveWideAngle(token: t, recipeJson: json)
        }) { [weak self] end in
            guard let self else { return }
            switch end {
            case .finished(.success(let change)):
                doc.run(title) { change }
                doc.reloadModel()
                doc.report?("\(title) applied")
                self.close()
                self.onApplied?(.success(change))
            case .finished(.failure(let e)):
                self.error = e.localizedDescription
                self.changed()
                self.onApplied?(.failure(e))
            case .discarded(let r):
                // Cancelled while rendering: the engine refuses to write once it sees the cancel; a step committed
                // before that is undone so engine history and the panels agree (as Liquify does).
                if case .success = r { _ = doc.run("Undo cancelled \(title)") { try doc.backend.undo() } }
                self.onApplied?(r.flatMap { _ in .failure(DocumentError.invalid("discarded")) })
            }
        }
        if let refused { error = refused; changed() }
    }

    /// Cancel (Esc): closes without changing the document; during OK the render is abandoned at once.
    func cancel() {
        let (b, t) = (backend, info.token)
        if jobs.isBusy {
            jobs.cancel { b.cancelAdaptiveWideAngle(token: t) }
        } else {
            b.cancelAdaptiveWideAngle(token: t)
        }
        doc.report?("\(title) cancelled; the layer is unchanged")
        close()
    }

    private func close() {
        closed = true
        gate.invalidate()
        DocumentAdaptiveWideAngle.shared.closed(self)
    }
}
