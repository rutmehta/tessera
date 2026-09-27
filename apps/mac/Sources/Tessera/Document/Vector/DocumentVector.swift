import AppKit
import Observation
import TesseraCore

/// Live shapes, the Pen, Path / Direct Selection and vector masks on the canvas (WP B5-11). One
/// instance serves the workspace's current document; `DocumentTools` forwards the vector tools'
/// mouse and key events here and `ToolOverlayView` calls `draw(in:)`.
///
/// History: a shape drag, Pen path, anchor / handle drag or affine handle drag is ONE node. While the
/// pointer moves, edits go to the engine as interactive drafts (coalesced: one call in flight, the
/// newest waiting); mouse-up sends the final value, which records the net edit. Esc during a drag
/// cancels the draft (`cancelSourcePreview`) with no history change. Overlays are drawn from the
/// host's own copy of the geometry, so they track the pointer without waiting for the engine.
@MainActor @Observable
final class DocumentVector {
    static let shared = DocumentVector()

    @ObservationIgnored weak var workspace: DocumentWorkspace?
    var document: DocumentController? { workspace?.current }

    // MARK: Options

    var options = ShapeToolOptions()
    /// Path Selection moves the vector mask with the shape (the explicit linked-mask gesture: one
    /// Batch). Off: the mask stays where it is in the document.
    var moveMaskWithShape = false

    // MARK: Live state

    enum Gesture {
        case drawShape(start: CGPoint, current: CGPoint)
        case penHandle
        case anchor(AnchorRef, grab: ShapePoint, from: ShapePoint)
        case handle(AnchorRef, PathHandle, mirror: Bool)
        case affineHandle(FreeTransformModel.Handle)
        case affineMove(last: CGPoint)
        case affineRotate(start: CGPoint, angle: Double)
    }

    @ObservationIgnored private(set) var gesture: Gesture?
    /// The shape being dragged out (document pixels).
    private(set) var draft: LiveShape?
    private(set) var pen = PenDraft() { didSet { if pen.isEmpty != oldValue.isEmpty { DocumentTools.shared.publishHint() } } }
    /// B5-11b: the status hint while a Pen path is being drawn (nil idle: the tool's idle hint).
    var penHint: String? {
        pen.isEmpty ? nil : "Pen: click to add points, drag for curves (⌥ breaks handles); click the first point to close, Return finishes, Esc discards, ⌫ removes the last point"
    }
    /// B5-11b: Path Selection deselected this layer's path (a click on empty canvas): no box, no outline.
    @ObservationIgnored private(set) var deselectedPath: DocLayerID?
    @ObservationIgnored private(set) var penHover: ShapePoint?
    /// The selected shape layer as last read, and the geometry being edited (overlays).
    private(set) var info: ShapeLayerInfo?
    @ObservationIgnored private var infoKey: (String, DocLayerID, UInt64)?
    /// Direct Selection: the merged local path being edited and the selected anchors.
    private(set) var livePath: ShapePath?
    var selectedAnchors: Set<AnchorRef> = []
    /// Path Selection's affine box (document pixels) and the transform it started from.
    private(set) var affine: FreeTransformModel?
    @ObservationIgnored private var affineBase: AffineTransform2D = .identity
    /// The layer the box belongs to (the box is rebuilt when the primary layer changes).
    @ObservationIgnored private(set) var affineLayer: DocLayerID?

    // MARK: Engine plumbing

    @ObservationIgnored private let queue = DispatchQueue(label: "dev.tessera.document-vector", qos: .userInteractive)
    @ObservationIgnored private var inFlight = false
    @ObservationIgnored private var waiting: (@Sendable () throws -> DocumentChange)?
    /// Bumped by every final / cancel so a late preview completion never re-sends a stale draft.
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var busy = 0
    /// Preview latency: pointer event time for each preview epoch, and the measured latencies (ms)
    /// from the event to the first frame showing it.
    @ObservationIgnored private var pendingEpochs: [(epoch: UInt64, start: TimeInterval)] = []
    @ObservationIgnored private(set) var latencies: [Double] = []
    @ObservationIgnored private(set) var previewsSent = 0
    @ObservationIgnored private(set) var framesSeen = 0
    /// Engine call durations of previews (ms) and the frames' own render times (ms).
    @ObservationIgnored private(set) var previewCallMs: [Double] = []
    @ObservationIgnored private(set) var frameRenderMs: [Double] = []
    @ObservationIgnored var onFinished: ((String, Bool) -> Void)?
    /// B5-11b: engine rejections of an edit (the inspector's colour wells rebuild from the model on each).
    private(set) var rejections = 0
    /// B5-11b: keyboard steps of an inspector slider record one node this long after the last step.
    static let defaultKeyboardCommitDelay: TimeInterval = 0.6
    @ObservationIgnored var keyboardCommitDelay = DocumentVector.defaultKeyboardCommitDelay
    /// The inspector edit being stepped from the keyboard: its drafts are live, one node is recorded
    /// `keyboardCommitDelay` after the last step (or at once when another edit starts).
    private struct KeyboardEdit {
        weak var doc: DocumentController?
        let layer: DocLayerID
        var label: String
        var draft: @Sendable () throws -> DocumentChange
        var maskBase: VectorMaskInfo?
    }
    @ObservationIgnored private var keyboardEdit: KeyboardEdit?
    @ObservationIgnored private var keyboardTimer: Task<Void, Never>?
    var hasPendingKeyboardEdit: Bool { keyboardEdit != nil }

    private init() {}

    func attach(_ workspace: DocumentWorkspace) {
        guard self.workspace !== workspace else { return }
        self.workspace = workspace
    }

    private func backend(_ doc: DocumentController) -> (any DocumentVectorBackend)? { doc.backend as? any DocumentVectorBackend }
    private func say(_ s: String) { document?.report?(s) }
    private func redraw() { document?.viewport?.toolOverlay.needsDisplay = true }

    /// Waits for queued engine calls (self-test).
    func idle() async {
        while busy > 0 || inFlight { try? await Task.sleep(for: .milliseconds(10)) }
    }

    func resetLatencies() { latencies.removeAll(); pendingEpochs.removeAll(); previewsSent = 0; framesSeen = 0; previewCallMs.removeAll(); frameRenderMs.removeAll() }

    /// Frame arrival (self-test hook through `DocumentController.frameObserver`).
    func frameArrived(_ f: DocFrame) {
        let now = ProcessInfo.processInfo.systemUptime
        framesSeen += 1
        frameRenderMs.append(f.renderMs)
        let done = pendingEpochs.filter { $0.epoch <= f.epoch }
        guard !done.isEmpty else { return }
        pendingEpochs.removeAll { $0.epoch <= f.epoch }
        if let first = done.map(\.start).min() { latencies.append((now - first) * 1000) }
    }

    // MARK: Selected shape

    /// The primary layer's shape record, re-read when its revision changes.
    func info(for doc: DocumentController, layer: DocLayerID) -> ShapeLayerInfo? {
        guard let node = doc.node(layer), node.kind == .shape, let b = backend(doc) else { return nil }
        if let k = infoKey, k.0 == doc.id, k.1 == layer, k.2 == node.revision, let info, info.layer == layer { return info }
        do {
            let i = try b.shapeLayer(layer)
            info = i
            infoKey = (doc.id, layer, node.revision)
            if gesture == nil { livePath = nil }
            return i
        } catch {
            info = nil
            infoKey = nil
            return nil
        }
    }

    /// Forces the next read (after an edit).
    func invalidate() { infoKey = nil }

    private var selectedShape: ShapeLayerInfo? {
        guard let doc = document, let p = doc.primary, p.kind == .shape else { return nil }
        return info(for: doc, layer: p.id)
    }

    /// The local path Direct Selection addresses (merged coincident anchors, as the engine does).
    private func editablePath(_ i: ShapeLayerInfo) -> ShapePath { livePath ?? i.source.path.mergingCoincidentAnchors }

    // MARK: Engine calls

    /// Sends a draft (coalesced) or a final edit (ordered after any draft in flight).
    private func send(_ doc: DocumentController, final: Bool, label: String, start: TimeInterval? = nil,
                      _ body: @escaping @Sendable () throws -> DocumentChange) {
        if final {
            // B5-11b: a keyboard adjustment in progress is its own node, recorded first.
            if keyboardEdit != nil { commitKeyboardEdit() }
            generation += 1
            waiting = nil
            busy += 1
            let gen = generation
            queue.async {
                let r = Result { try body() }
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        let v = DocumentVector.shared
                        v.busy -= 1
                        v.finished(doc, label: label, r, generation: gen)
                    }
                }
            }
            return
        }
        if inFlight { waiting = body; return }
        inFlight = true
        previewsSent += 1
        let gen = generation
        let t0 = start ?? ProcessInfo.processInfo.systemUptime
        queue.async {
            let c0 = ProcessInfo.processInfo.systemUptime
            let r = Result { try body() }
            let callMs = (ProcessInfo.processInfo.systemUptime - c0) * 1000
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let v = DocumentVector.shared
                    v.inFlight = false
                    v.previewCallMs.append(callMs)
                    switch r {
                    case .success(let c): v.pendingEpochs.append((c.epoch, t0))
                    case .failure(let e): v.say("\(label): \(e.localizedDescription)")
                    }
                    if gen == v.generation, let next = v.waiting {
                        v.waiting = nil
                        v.send(doc, final: false, label: label, next)
                    }
                }
            }
        }
    }

    private func finished(_ doc: DocumentController, label: String, _ r: Result<DocumentChange, Error>, generation gen: Int) {
        invalidate()
        doc.reloadModel()
        doc.reloadHistory()
        switch r {
        case .success(let c):
            if let id = c.created.first { doc.select(id) }
            onFinished?(label, true)
        case .failure(let e):
            say("\(label): \(e.localizedDescription)")
            rejections += 1   // B5-11b: the inspector's colour wells rebuild from the model
            onFinished?(label, false)
        }
        if gen == generation { livePath = nil }
        redraw()
    }

    /// Esc: drop the draft, no history change.
    func cancelDraft() {
        guard let doc = document, let b = backend(doc) else { return }
        commitKeyboardEdit()
        gesture = nil
        draft = nil
        affine = affine.map { FreeTransformModel(bounds: $0.bounds) }
        livePath = nil
        send(doc, final: true, label: "Cancel") { try b.cancelSourcePreview() }
        say("Cancelled: the shape and history are unchanged")
        redraw()
    }

    // MARK: Tool changes

    func toolSelected(_ tool: DocumentTool) {
        commitKeyboardEdit()
        if tool != .pen, !pen.isEmpty { finishPen() }
        if tool != .pathSelect { affine = nil }
        if tool != .directSelect && tool != .pen { selectedAnchors.removeAll() }
        if tool == .pathSelect, let i = selectedShape, i.layer != deselectedPath { beginAffine(i) }
        redraw()
    }

    /// ⌘T on a shape layer: its affine handles (Path Selection).
    func beginFreeTransform() -> Bool {
        guard let doc = document, let i = selectedShape else { return false }
        deselectedPath = nil
        DocumentTools.shared.select(.pathSelect)
        beginAffine(i)
        doc.report?("Transform shape: drag handles (⇧ keeps proportions, ⌥ from the centre), inside to move, outside to rotate; each drag is one undo step, Esc cancels a drag")
        return true
    }

    private func beginAffine(_ i: ShapeLayerInfo) {
        guard let b = i.bounds, b.width > 0 || b.height > 0 else { affine = nil; return }
        affine = FreeTransformModel(bounds: b.insetBy(dx: b.width == 0 ? -0.5 : 0, dy: b.height == 0 ? -0.5 : 0))
        affineBase = i.transform
        affineLayer = i.layer
    }

    // MARK: Mouse

    private func localRadius(_ i: ShapeLayerInfo, in v: DocumentViewportView, points: Double = 6) -> Double {
        let scale = sqrt(abs(i.transform.determinant))
        return points / max(v.pointsPerPixel * scale, 1e-9)
    }

    func mouseMoved(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = document, doc.tool == .pen else { return }
        penHover = ShapePoint(v.canvasPoint(e))
        v.toolOverlay.needsDisplay = true
    }

    func mouseDown(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = document, backend(doc) != nil else {
            say("Shapes need the engine document backend")
            return
        }
        commitKeyboardEdit()
        let p = v.canvasPoint(e)
        switch doc.tool {
        case .rectangleShape, .ellipseShape, .polygonShape, .lineShape:
            gesture = .drawShape(start: p, current: p)
            draft = nil
        case .pen:
            penDown(doc, e, at: p, in: v)
        case .directSelect:
            directDown(doc, e, at: p, in: v)
        case .pathSelect:
            pathSelectDown(doc, e, at: p, in: v)
        default: break
        }
        v.toolOverlay.needsDisplay = true
    }

    func mouseDragged(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = document, let b = backend(doc) else { return }
        let p = v.canvasPoint(e)
        let flags = e.modifierFlags
        switch gesture {
        case .drawShape(let start, _):
            gesture = .drawShape(start: start, current: p)
            draft = liveShape(doc.tool, start: start, current: p, shift: flags.contains(.shift), option: flags.contains(.option))
        case .penHandle:
            pen.drag(to: ShapePoint(p), independent: flags.contains(.option))
        case .anchor(let r, let grab, let from):
            guard let i = info, let inv = i.transform.inverse else { break }
            let local = inv.apply(ShapePoint(p))
            let to = from + (local - grab)
            livePath = editablePath(i).movingAnchor(r, to: to)
            let id = i.layer
            send(doc, final: false, label: "Move Anchor Point", start: e.timestamp) {
                try b.editShapePath(id, commands: [.moveAnchor(r, to: to)], interactive: true)
            }
        case .handle(let r, let h, let mirror):
            guard let i = info, let inv = i.transform.inverse else { break }
            let to = inv.apply(ShapePoint(p))
            let mirrorNow = mirror && !flags.contains(.option)
            livePath = editablePath(i).settingHandle(r, h, to: to, mirror: mirrorNow)
            let id = i.layer
            send(doc, final: false, label: "Move Direction Point", start: e.timestamp) {
                try b.editShapePath(id, commands: [.setHandle(r, h, to: to, mirror: mirrorNow)], interactive: true)
            }
        case .affineHandle(let h):
            affine?.drag(h, to: p, constrain: flags.contains(.shift), fromCenter: flags.contains(.option))
            pushAffine(doc, final: false, start: e.timestamp)
        case .affineMove(let last):
            affine?.move(by: CGSize(width: p.x - last.x, height: p.y - last.y))
            gesture = .affineMove(last: p)
            pushAffine(doc, final: false, start: e.timestamp)
        case .affineRotate(let start, let angle):
            affine?.rotate(from: start, to: p, startAngle: angle, snap: flags.contains(.shift))
            pushAffine(doc, final: false, start: e.timestamp)
        case nil: break
        }
        v.toolOverlay.needsDisplay = true
    }

    func mouseUp(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = document, let b = backend(doc) else { gesture = nil; return }
        let g = gesture
        gesture = nil
        switch g {
        case .drawShape(let start, _):
            let p = v.canvasPoint(e)
            let live = liveShape(doc.tool, start: start, current: p, shift: e.modifierFlags.contains(.shift),
                                 option: e.modifierFlags.contains(.option))
            draft = nil
            guard ShapeToolMath.isShape(live) else {
                say("\(doc.tool.title): drag on the canvas to draw a shape")
                break
            }
            addShape(doc, source: options.source(for: live))
        case .penHandle:
            if pen.closed { finishPen() }
        case .anchor(let r, let grab, let from):
            guard let i = info, let inv = i.transform.inverse else { break }
            let to = from + (inv.apply(ShapePoint(v.canvasPoint(e))) - grab)
            let id = i.layer
            if to == from {
                send(doc, final: true, label: "Select Anchor") { try b.cancelSourcePreview() }
            } else {
                send(doc, final: true, label: "Move Anchor Point") { try b.editShapePath(id, commands: [.moveAnchor(r, to: to)], interactive: false) }
            }
        case .handle(let r, let h, let mirror):
            guard let i = info, let inv = i.transform.inverse else { break }
            let to = inv.apply(ShapePoint(v.canvasPoint(e)))
            let m = mirror && !e.modifierFlags.contains(.option)
            let id = i.layer
            send(doc, final: true, label: "Move Direction Point") {
                try b.editShapePath(id, commands: [.setHandle(r, h, to: to, mirror: m)], interactive: false)
            }
        case .affineRotate where affine?.isIdentity ?? true:
            // B5-11b: a click (no drag) on empty canvas deselects the path, as in Photoshop.
            if let a = affine, a.isIdentity { send(doc, final: true, label: "Transform") { try b.cancelSourcePreview() } }
            deselectPath(doc)
        case .affineHandle, .affineMove, .affineRotate:
            pushAffine(doc, final: true, start: nil)
        case nil: break
        }
        v.toolOverlay.needsDisplay = true
    }

    private func liveShape(_ tool: DocumentTool, start: CGPoint, current: CGPoint, shift: Bool, option: Bool) -> LiveShape {
        switch tool {
        case .ellipseShape:
            return ShapeToolMath.ellipse(ShapeToolMath.box(start: start, current: current, square: shift, fromCenter: option))
        case .polygonShape:
            return ShapeToolMath.polygon(center: start, current: current, sides: options.sides, star: options.star,
                                         inset: options.starInset, snap: shift)
        case .lineShape:
            return ShapeToolMath.line(start: start, current: current, snap: shift)
        default:
            return ShapeToolMath.rectangle(ShapeToolMath.box(start: start, current: current, square: shift, fromCenter: option),
                                           cornerRadius: options.cornerRadius)
        }
    }

    /// One "<Kind> Tool" / "Pen" node above the primary layer; the new layer is selected.
    func addShape(_ doc: DocumentController, source: ShapeSource, name: String = "") {
        guard let b = backend(doc) else { return }
        if let problem = source.strokeAlignmentProblem { say(problem); return }
        let parent = doc.primary?.parent
        let index = doc.primary.map { $0.index + 1 }
        send(doc, final: true, label: source.liveShape.map { _ in "Shape" } ?? "Pen") {
            try b.addShapeLayer(name: name, parent: parent, index: index, source: source, transform: .identity)
        }
    }

    // MARK: Pen

    private func penDown(_ doc: DocumentController, _ e: NSEvent, at p: CGPoint, in v: DocumentViewportView) {
        let q = ShapePoint(p)
        let radius = 6 / max(v.pointsPerPixel, 1e-9)
        // With no path in progress, the Pen adds / deletes anchors on the selected shape.
        if pen.isEmpty, let i = selectedShape, let inv = i.transform.inverse, let b = backend(doc) {
            let local = inv.apply(q)
            let path = editablePath(i)
            let lr = localRadius(i, in: v)
            if case .anchor(let r)? = path.target(at: local, radius: lr) {
                let id = i.layer
                send(doc, final: true, label: "Delete Anchor Point") { try b.editShapePath(id, commands: [.deleteAnchor(r)], interactive: false) }
                return
            }
            if case .segment(let s, let seg, let t)? = path.target(at: local, radius: lr) {
                let id = i.layer
                send(doc, final: true, label: "Add Anchor Point") {
                    try b.editShapePath(id, commands: [.insertAnchor(subpath: s, segment: seg, t: t)], interactive: false)
                }
                return
            }
        }
        pen.click(q, radius: radius)
        gesture = .penHandle
        if pen.closed { say("Pen: path closed") }
    }

    /// Return / tool change: the Pen path becomes one shape layer ("Pen").
    func finishPen() {
        guard let doc = document else { pen = PenDraft(); return }
        let path = pen.path
        let ok = pen.isCommittable
        pen = PenDraft()
        penHover = nil
        if ok { addShape(doc, source: options.source(for: path)) } else if path.anchorCount > 0 { say("Pen: a shape needs two or more points") }
        redraw()
    }

    // MARK: Direct Selection

    private func directDown(_ doc: DocumentController, _ e: NSEvent, at p: CGPoint, in v: DocumentViewportView) {
        if doc.primary?.kind != .shape || selectedShape == nil, let hit = hitLayer(doc, p, in: v) {
            doc.select(hit.layer)
            selectedAnchors.removeAll()
        }
        guard let i = selectedShape, let inv = i.transform.inverse, let b = backend(doc) else { return }
        let local = inv.apply(ShapePoint(p))
        let path = editablePath(i)
        switch path.target(at: local, radius: localRadius(i, in: v), selected: selectedAnchors) {
        case .handle(let r, let h)?:
            gesture = .handle(r, h, mirror: true)
        case .anchor(let r)?:
            if e.modifierFlags.contains(.shift) { selectedAnchors.formSymmetricDifference([r]) } else { selectedAnchors = [r] }
            gesture = .anchor(r, grab: local, from: path.anchor(r)?.point ?? local)
        case .segment(let s, let seg, let t)?:
            if e.modifierFlags.contains(.option) {
                let id = i.layer
                send(doc, final: true, label: "Add Anchor Point") {
                    try b.editShapePath(id, commands: [.insertAnchor(subpath: s, segment: seg, t: t)], interactive: false)
                }
                selectedAnchors = [AnchorRef(subpath: s, anchor: seg + 1)]
            } else {
                selectedAnchors.removeAll()
            }
        case nil:
            selectedAnchors.removeAll()
            if let hit = hitLayer(doc, p, in: v), hit.layer != i.layer { doc.select(hit.layer) }
        }
    }

    private func hitLayer(_ doc: DocumentController, _ p: CGPoint, in v: DocumentViewportView) -> ShapeHit? {
        guard let b = backend(doc) else { return nil }
        return try? b.shapeHitTest(p, includeStroke: true, tolerance: 4 / max(v.pointsPerPixel, 1e-9))
    }

    /// ⌫ with anchors selected (Direct Selection): delete them (one node).
    func deleteSelectedAnchors() -> Bool {
        guard let doc = document, doc.tool == .directSelect || doc.tool == .pen, !selectedAnchors.isEmpty,
              let i = selectedShape, let b = backend(doc) else { return false }
        // Highest index first so earlier indexes stay valid.
        let refs = selectedAnchors.sorted { ($0.subpath, $0.anchor) > ($1.subpath, $1.anchor) }
        selectedAnchors.removeAll()
        let id = i.layer
        send(doc, final: true, label: "Delete Anchor Point") { try b.editShapePath(id, commands: refs.map(PathCommand.deleteAnchor), interactive: false) }
        return true
    }

    // MARK: Path Selection (affine)

    private func pathSelectDown(_ doc: DocumentController, _ e: NSEvent, at p: CGPoint, in v: DocumentViewportView) {
        if let t = affine, let id = affineLayer, id == doc.primary?.id, info(for: doc, layer: id) != nil {
            let vp = v.viewPoint(canvas: p)
            if let h = FreeTransformModel.Handle.allCases.first(where: { h in
                let q = v.viewPoint(canvas: t.transformed(h))
                return hypot(q.x - vp.x, q.y - vp.y) <= 7
            }) {
                gesture = .affineHandle(h)
                return
            }
            if t.contains(p) { gesture = .affineMove(last: p); return }
        }
        if let hit = hitLayer(doc, p, in: v) {
            if hit.layer != doc.primary?.id { doc.select(hit.layer) }
            deselectedPath = nil
            if let i = info(for: doc, layer: hit.layer) { beginAffine(i) }
            gesture = .affineMove(last: p)
            return
        }
        if let t = affine, let id = affineLayer, id == doc.primary?.id, info(for: doc, layer: id) != nil {
            // Outside the box: a drag rotates; a click without a drag deselects (mouse-up).
            gesture = .affineRotate(start: p, angle: t.angle)
        } else {
            deselectPath(doc)
        }
    }

    /// Path Selection: no path selected (the layer stays selected in Layers; clicking the shape selects
    /// its path again).
    private func deselectPath(_ doc: DocumentController) {
        deselectedPath = doc.primary?.id
        affine = nil
        affineLayer = nil
        selectedAnchors.removeAll()
        redraw()
    }

    private func pushAffine(_ doc: DocumentController, final: Bool, start: TimeInterval?) {
        guard let b = backend(doc), let id = affineLayer, let i = info, i.layer == id, let t = affine else { return }
        if final, t.isIdentity {
            send(doc, final: true, label: "Transform") { try b.cancelSourcePreview() }
            return
        }
        let m = t.matrix.concatenating(after: affineBase)
        guard m.isFiniteAndInvertible else { return }
        let source = i.source, linked = moveMaskWithShape && i.vectorMask != nil
        let label = linked ? "Transform Shape and Vector Mask" : "Transform Shape"
        send(doc, final: final, label: label, start: start) {
            linked ? try b.transformShapeWithMask(id, transform: m, interactive: !final)
                : try b.setShapeLayer(id, source: source, transform: m, interactive: !final)
        }
        if final { affine = nil }
    }

    // MARK: Inspector edits

    /// A live-parameter / paint edit of the selected shape: drafts while dragging, one node on release.
    ///
    /// B5-11b: steps from the keyboard (a focused slider's arrows; `keyboard` nil = no mouse button
    /// down) stay drafts, and their final values too while more may follow: ONE node is recorded
    /// `keyboardCommitDelay` after the last step, or at once when another edit starts.
    func setSource(_ doc: DocumentController, layer: DocLayerID, _ source: ShapeSource, final: Bool, keyboard: Bool? = nil) {
        guard let b = backend(doc), let i = info(for: doc, layer: layer) else { return }
        if let problem = source.strokeAlignmentProblem { say(problem); return }
        let t = i.transform
        let deferred = defersToKeyboard(final: final, keyboard: keyboard)
        if deferred {
            keyboardStep(doc, layer: layer, label: "Edit Shape", maskBase: nil) {
                try b.setShapeLayer(layer, source: source, transform: t, interactive: true)
            }
        } else {
            settleKeyboardEdit(doc, layer: layer, mask: false)
            send(doc, final: final, label: "Edit Shape") { try b.setShapeLayer(layer, source: source, transform: t, interactive: !final) }
        }
        if !final || deferred, var cached = info, cached.layer == layer { cached.source = source; info = cached }
    }

    /// B5-11b: the paint a stroke enabled from None takes: the foreground colour, or a colour that
    /// contrasts with the fill when they are the same (never the fill colour, which hides dashes).
    func newStrokePaint(for info: ShapeLayerInfo) -> ShapePaint {
        let fg = DocumentTools.shared.colors.foreground
        return .solid(ShapeToolOptions.newStrokeColor(fill: info.source.fill, foreground: [Double(fg.r), Double(fg.g), Double(fg.b), 1]))
    }

    /// B5-11b: the shape's document bounds as Properties reports them (stroke included), following a
    /// Path Selection drag or a Direct Selection edit live.
    func displayBounds(_ doc: DocumentController, layer: DocLayerID) -> CGRect? {
        if doc.tool == .pathSelect, affineLayer == layer, let t = affine, !t.isIdentity {
            return Self.boundingBox(t.corners)
        }
        guard let i = info(for: doc, layer: layer) else { return nil }
        if gesture != nil, let p = livePath, let b = p.bounds {
            return Self.boundingBox([CGPoint(x: b.minX, y: b.minY), CGPoint(x: b.maxX, y: b.minY), CGPoint(x: b.maxX, y: b.maxY),
                                     CGPoint(x: b.minX, y: b.maxY)].map { i.transform.apply($0) })
        }
        return i.bounds
    }

    private static func boundingBox(_ c: [CGPoint]) -> CGRect? {
        guard let x0 = c.map(\.x).min(), let x1 = c.map(\.x).max(), let y0 = c.map(\.y).min(), let y1 = c.map(\.y).max() else { return nil }
        return CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
    }

    // MARK: Keyboard-stepped inspector edits (B5-11b)

    /// Whether an inspector edit joins / starts a keyboard adjustment instead of recording now.
    private func defersToKeyboard(final: Bool, keyboard: Bool?) -> Bool {
        let key = keyboard ?? (NSEvent.pressedMouseButtons & 1 == 0)
        guard key else { return false }
        return !final || keyboardEdit != nil
    }

    private func keyboardStep(_ doc: DocumentController, layer: DocLayerID, label: String, maskBase: VectorMaskInfo?,
                              _ draft: @escaping @Sendable () throws -> DocumentChange) {
        if let k = keyboardEdit, k.layer != layer || k.doc !== doc { commitKeyboardEdit() }
        send(doc, final: false, label: label, draft)
        keyboardEdit = KeyboardEdit(doc: doc, layer: layer, label: label, draft: draft, maskBase: keyboardEdit?.maskBase ?? maskBase)
        keyboardTimer?.cancel()
        let delay = keyboardCommitDelay
        keyboardTimer = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(Int(delay * 1000)))
            guard !Task.isCancelled else { return }
            DocumentVector.shared.commitKeyboardEdit()
        }
    }

    /// A mouse edit of the same control continues the keyboard adjustment (its final records the net
    /// change as one node); any other edit records the adjustment first.
    private func settleKeyboardEdit(_ doc: DocumentController, layer: DocLayerID, mask: Bool) {
        guard let k = keyboardEdit else { return }
        if k.doc === doc, k.layer == layer, (k.maskBase != nil || k.label.hasPrefix("Vector Mask")) == mask {
            keyboardTimer?.cancel()
            keyboardTimer = nil
            keyboardEdit = nil
        } else {
            commitKeyboardEdit()
        }
    }

    /// Records the keyboard adjustment in progress as one node (its newest value), if any.
    func commitKeyboardEdit() {
        keyboardTimer?.cancel()
        keyboardTimer = nil
        guard let k = keyboardEdit else { return }
        keyboardEdit = nil
        guard let doc = k.doc else { return }
        // A coalesced draft still waiting is sent first (the final path drops it); `commit` records the
        // pending draft, or nothing when another edit (an undo) already flushed it.
        let base = doc.backend, label = k.label, newest = waiting != nil ? k.draft : nil
        send(doc, final: true, label: label) {
            if let newest { _ = try newest() }
            return try base.commit(label: label)
        }
    }

    func setTransform(_ doc: DocumentController, layer: DocLayerID, _ t: AffineTransform2D) {
        guard let b = backend(doc), let i = info(for: doc, layer: layer), t.isFiniteAndInvertible else { return }
        commitKeyboardEdit()
        let source = i.source
        send(doc, final: true, label: "Transform Shape") { try b.setShapeLayer(layer, source: source, transform: t, interactive: false) }
    }

    func setMask(_ doc: DocumentController, layer: DocLayerID, _ mask: VectorMaskInfo?, final: Bool, keyboard: Bool? = nil) {
        guard let b = backend(doc) else { return }
        let deferred = defersToKeyboard(final: final, keyboard: keyboard)
        if deferred {
            // The node's label compares with the mask before the adjustment (as the engine's own final call).
            let base = keyboardEdit?.layer == layer ? keyboardEdit?.maskBase : info(for: doc, layer: layer)?.vectorMask
            let label = switch (base, mask) {
            case (let a?, let m?) where a.density != m.density: "Vector Mask Density"
            case (let a?, let m?) where a.feather != m.feather: "Vector Mask Feather"
            default: "Vector Mask"
            }
            keyboardStep(doc, layer: layer, label: label, maskBase: base) { try b.setVectorMask(layer, mask: mask, interactive: true) }
        } else {
            settleKeyboardEdit(doc, layer: layer, mask: true)
            send(doc, final: final, label: "Vector Mask") { try b.setVectorMask(layer, mask: mask, interactive: !final) }
        }
        if !final || deferred, var cached = info, cached.layer == layer { cached.vectorMask = mask; info = cached }
    }

    func setFillRule(_ doc: DocumentController, layer: DocLayerID, _ rule: ShapeFillRule) {
        guard let b = backend(doc) else { return }
        commitKeyboardEdit()
        send(doc, final: true, label: "Fill Rule") { try b.editShapePath(layer, commands: [.setFillRule(rule)], interactive: false) }
    }

    /// Layer ▸ Vector Mask ▸ Reveal All / Current Selection: a mask rectangle (document pixels).
    func addVectorMask(fromSelection: Bool) {
        guard let doc = document, let p = doc.primary else { return }
        let r: CGRect
        if fromSelection, let s = doc.marquee {
            r = CGRect(x: Double(s.x), y: Double(s.y), width: Double(s.width), height: Double(s.height))
        } else {
            r = CGRect(x: 0, y: 0, width: Double(doc.info.width), height: Double(doc.info.height))
        }
        let path = ShapePrimitives.rectangle(ShapeRect(r), radii: [0, 0, 0, 0]).mergingCoincidentAnchors
        setMask(doc, layer: p.id, VectorMaskInfo(path: path), final: true, keyboard: false)
    }

    func deleteVectorMask() {
        guard let doc = document, let p = doc.primary else { return }
        setMask(doc, layer: p.id, nil, final: true, keyboard: false)
    }

    /// Layer ▸ Combine Shapes: the bottom selected shape is the target, the others apply in stacking order.
    func combineSelected(_ op: ShapeOperation) {
        guard let doc = document, let b = backend(doc) else { return }
        let shapes = doc.outline.flattened.reversed().filter { id in doc.selection.contains(id) && doc.node(id)?.kind == .shape }
        guard shapes.count >= 2, let target = shapes.first else { say("\(op.title): select two or more shape layers"); return }
        let operands = Array(shapes.dropFirst())
        send(doc, final: true, label: op.title) { try b.booleanShapes(target, operands: operands, operation: op) }
    }

    var canCombine: Bool {
        guard let doc = document else { return false }
        return doc.selection.filter { doc.node($0)?.kind == .shape }.count >= 2
    }

    /// Layer ▸ Rasterize ▸ Shape (one node; undo restores the live shape).
    func convertToPixels() {
        guard let doc = document, let p = doc.primary, p.kind == .shape, let b = backend(doc) else { return }
        send(doc, final: true, label: "Rasterize Shape") { try b.convertToPixels(id: p.id) }
    }

    // MARK: Keys

    /// Return / Esc / ⌫ for the vector tools. Returns whether the key was used.
    func handleKey(_ event: NSEvent) -> Bool {
        guard let doc = document, doc.tool.isVector else { return false }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard mods.subtracting([.shift, .function, .numericPad]).isEmpty else { return false }
        switch event.keyCode {
        case 53:   // Esc
            if gesture != nil { cancelDraft(); return true }
            if !pen.isEmpty { pen = PenDraft(); say("Pen path discarded"); redraw(); return true }
            if !selectedAnchors.isEmpty { selectedAnchors.removeAll(); redraw(); return true }
            return false
        case 36, 76:   // Return / Enter
            if !pen.isEmpty { finishPen(); return true }
            return false
        case 51, 117:   // ⌫
            if doc.tool == .pen, !pen.isEmpty { pen.removeLast(); redraw(); return true }
            return deleteSelectedAnchors()
        default:
            return false
        }
    }

    var cursor: NSCursor {
        guard let t = document?.tool else { return .arrow }
        return t == .pathSelect || t == .directSelect ? .arrow : .crosshair
    }

    // MARK: Drawing

    /// Path Selection's box follows the primary shape (rebuilt after a committed drag or a new selection).
    func refreshAffine() {
        guard let doc = document, doc.tool == .pathSelect, gesture == nil, busy == 0, !inFlight else { return }
        if let i = selectedShape, i.layer != deselectedPath {
            if affine == nil || affineLayer != i.layer { beginAffine(i) }
        } else {
            affine = nil
        }
    }

    func draw(in v: DocumentViewportView) {
        guard let doc = v.controller, doc === document else { return }
        refreshAffine()
        let tool = doc.tool
        // The shape being dragged out.
        if let d = draft { stroke(ShapePrimitives.path(d), .identity, in: v, width: 1) }
        // The selected shape's path, anchors and handles.
        if tool.isVector, let i = selectedShape, !(tool == .pathSelect && i.layer == deselectedPath) {
            let path = editablePath(i)
            stroke(path, i.transform, in: v, width: 1)
            if let m = i.vectorMask { stroke(m.path, .identity, in: v, width: 1, faint: true) }
            if tool == .directSelect || tool == .pen {
                for (s, sub) in path.subpaths.enumerated() {
                    for (k, a) in sub.anchors.enumerated() {
                        let r = AnchorRef(subpath: s, anchor: k)
                        let c = v.viewPoint(canvas: i.transform.apply(a.point).cgPoint)
                        if selectedAnchors.contains(r) {
                            for q in [a.incoming, a.outgoing] where q != a.point {
                                let h = v.viewPoint(canvas: i.transform.apply(q).cgPoint)
                                let line = NSBezierPath()
                                line.move(to: c)
                                line.line(to: h)
                                guide(line)
                                dot(h)
                            }
                        }
                        knob(c, filled: selectedAnchors.contains(r))
                    }
                }
            }
        }
        if tool == .pathSelect, let t = affine, affineLayer == doc.primary?.id { drawAffine(t, in: v) }
        // The Pen path in progress, with the rubber band to the pointer.
        if tool == .pen, !pen.isEmpty {
            var path = pen.path
            if !pen.closed, let h = penHover, var last = path.subpaths.first {
                last.anchors.append(.corner(h))
                path.subpaths = [last]
            }
            stroke(path, .identity, in: v, width: 1)
            let withHandles = Set(pen.handleAnchors)
            for (k, a) in pen.anchors.enumerated() {
                let c = v.viewPoint(canvas: a.point.cgPoint)
                if withHandles.contains(k) {
                    for q in [a.incoming, a.outgoing] where q != a.point {
                        let h = v.viewPoint(canvas: q.cgPoint)
                        let line = NSBezierPath()
                        line.move(to: c)
                        line.line(to: h)
                        guide(line)
                        dot(h)
                    }
                }
                knob(c, filled: k == 0)
            }
        }
    }

    private func stroke(_ path: ShapePath, _ t: AffineTransform2D, in v: DocumentViewportView, width: CGFloat, faint: Bool = false) {
        let tolerance = 0.5 / max(v.pointsPerPixel * sqrt(abs(t.determinant)), 1e-6)
        for (pts, closed) in path.flattened(tolerance: tolerance) where pts.count > 1 {
            let b = NSBezierPath()
            b.move(to: v.viewPoint(canvas: t.apply(pts[0]).cgPoint))
            for q in pts.dropFirst() { b.line(to: v.viewPoint(canvas: t.apply(q).cgPoint)) }
            if closed { b.close() }
            if faint {
                b.setLineDash([4, 3], count: 2, phase: 0)
                b.lineWidth = width
                Theme.Palette.OnImage.guideFaint.setStroke()
                b.stroke()
            } else {
                guide(b, width: width)
            }
        }
    }

    private func guide(_ p: NSBezierPath, width: CGFloat = 1) {
        p.lineWidth = width + 1
        Theme.Palette.OnImage.shadow.setStroke()
        p.stroke()
        p.lineWidth = width
        Theme.Palette.OnImage.guide.setStroke()
        p.stroke()
    }

    /// Anchor square: filled when selected (Photoshop), hollow otherwise.
    private func knob(_ c: CGPoint, filled: Bool) {
        let r: CGFloat = 3.5
        let o = NSBezierPath(rect: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
        (filled ? Theme.Palette.OnImage.guide : Theme.Palette.OnImage.text).setFill()
        o.fill()
        o.lineWidth = 1
        Theme.Palette.OnImage.ink.setStroke()
        o.stroke()
    }

    /// Direction point (handle end).
    private func dot(_ c: CGPoint) {
        let r: CGFloat = 3
        let o = NSBezierPath(ovalIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
        Theme.Palette.OnImage.text.setFill()
        o.fill()
        o.lineWidth = 1
        Theme.Palette.OnImage.ink.setStroke()
        o.stroke()
    }

    private func drawAffine(_ t: FreeTransformModel, in v: DocumentViewportView) {
        let corners = t.corners.map { v.viewPoint(canvas: $0) }
        let box = NSBezierPath()
        box.move(to: corners[0])
        corners.dropFirst().forEach { box.line(to: $0) }
        box.close()
        guide(box)
        for h in FreeTransformModel.Handle.allCases { knob(v.viewPoint(canvas: t.transformed(h)), filled: false) }
    }
}
