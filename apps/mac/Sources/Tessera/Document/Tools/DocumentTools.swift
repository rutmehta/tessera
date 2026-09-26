import AppKit
import Observation
import TesseraCore

/// The layered editor's tools (WP B5-04): tool options, colours, the gestures of every tool on the
/// document viewport, stroke capture (tablet pressure and tilt, coalesced per frame), selections,
/// Free Transform, and the Select / Edit menu commands. One instance serves the workspace's current
/// document; the viewport forwards mouse events here and `ToolOverlayView` draws what this holds.
///
/// Engine calls that can take a while (strokes, transforms, image-driven selections) run in order on
/// one serial queue off the main thread; the main thread reloads the document model when they end.
@MainActor @Observable
final class DocumentTools {
    static let shared = DocumentTools()

    @ObservationIgnored weak var workspace: DocumentWorkspace?
    var document: DocumentController? { workspace?.current }

    // MARK: Options

    /// Brush options per painting tool (Photoshop keeps them per tool).
    var brushes: [DocumentTool: BrushOptions] = {
        var brush = BrushOptions()
        var eraser = BrushOptions()
        eraser.hardness = 1
        var clone = BrushOptions()
        clone.size = 60
        clone.hardness = 0.5
        var heal = clone
        heal.size = 40
        return [.brush: brush, .eraser: eraser, .cloneStamp: clone, .heal: heal]
    }()
    var colors = ToolColors()
    var pressure = PressureCurve()
    /// The options bar's selection mode (⇧ / ⌥ / ⇧⌥ override it per gesture).
    var selectionMode: SelectionCombine = .replace
    var feather: Float = 0
    var antialias = true
    var tolerance: Float = 32
    var contiguous = true
    /// Wand / Quick Selection sample the composite.
    var sampleAllLayers = true
    var quickSize: Float = 30
    /// Eyedropper: 0 point, 1 = 3 × 3, 2 = 5 × 5.
    var eyedropperRadius: UInt32 = 0
    var eyedropperAllLayers = true
    /// Paint the primary layer's mask instead of its pixels (layers with a mask).
    var paintMask = false
    var transformInterpolation: ResampleMode = .bicubic

    /// The brush options of the current painting tool (or the brush's).
    var currentBrush: BrushOptions {
        get { brushes[paintTool] ?? BrushOptions() }
        set { brushes[paintTool] = newValue; redraw() }
    }
    private var paintTool: DocumentTool {
        let t = document?.tool ?? .brush
        return t.paints ? t : .brush
    }

    // MARK: Live state (drawn by ToolOverlayView)

    /// Marching ants of the current document's selection (canvas pixels).
    private(set) var outline: [SelectionOutline] = []
    /// The pointer in view points (brush outline), nil outside the canvas.
    @ObservationIgnored private(set) var pointer: CGPoint?
    /// A gesture in progress.
    @ObservationIgnored private(set) var gesture: Gesture?
    /// Brush HUD readout while ⌃⌥-dragging.
    private(set) var hud: (size: Float, hardness: Float)?
    /// Free Transform in progress.
    private(set) var transform: FreeTransformModel?
    @ObservationIgnored private var transformLayers: [DocLayerID] = []
    /// Clone source: the point ⌥-clicked and its layer; `cloneOffset` once the first stroke aligned it.
    private(set) var cloneSource: (point: CanvasPoint, layer: DocLayerID)?
    @ObservationIgnored private(set) var cloneOffset: CGSize?
    /// Last stroke end (⇧-click draws a straight line from it).
    @ObservationIgnored private var lastStrokeEnd: CanvasPoint?
    /// Imported and built-in tips.
    private(set) var tips: [BrushTip] = []
    /// The latest stroke timing ("3.1 ms per frame, 42 dabs").
    private(set) var strokeReadout: String?
    /// Select and Mask preview mode while its sheet is open.
    var refinePreview: RefinePreview = .marchingAnts
    var refine = RefineEdgeSettings()
    var sheet: ToolSheet?

    enum RefinePreview: String, CaseIterable, Identifiable {
        case marchingAnts, overlay, onBlack, onWhite
        var id: String { rawValue }
        var title: String {
            switch self {
            case .marchingAnts: "Marching Ants"
            case .overlay: "Overlay"
            case .onBlack: "On Black"
            case .onWhite: "On White"
            }
        }
    }

    enum ToolSheet: Identifiable, Equatable {
        case refineEdge, colorRange, modify(SelectionModifyKind), fill, saveSelection
        var id: String {
            switch self {
            case .refineEdge: "refine"
            case .colorRange: "colorRange"
            case .modify(let k): "modify.\(k.rawValue)"
            case .fill: "fill"
            case .saveSelection: "save"
            }
        }
    }

    enum Gesture {
        /// Canvas start / current, the op, the modifiers held at mouse-down.
        case marquee(start: CGPoint, current: CGPoint, op: SelectionCombine, ellipse: Bool, startShift: Bool, startOption: Bool)
        case lasso(points: [CanvasPoint], op: SelectionCombine)
        case polygon(points: [CanvasPoint], op: SelectionCombine, hover: CanvasPoint?)
        case magnetic(anchors: [CanvasPoint], path: [CanvasPoint], live: [CanvasPoint], op: SelectionCombine)
        case quick(points: [CanvasPoint], op: SelectionCombine)
        case paint(last: CanvasPoint)
        case hud(startSize: Float, startHardness: Float, start: CGPoint)
        case pan(last: CGPoint)
        case move(start: CGPoint)
        case transformHandle(FreeTransformModel.Handle)
        case transformMove(last: CGPoint)
        case transformRotate(start: CGPoint, angle: Double)
    }

    // MARK: Engine plumbing

    /// Strokes, transforms and image selections, in order, off the main thread.
    @ObservationIgnored private let queue = DispatchQueue(label: "dev.tessera.document-tools", qos: .userInteractive)
    @ObservationIgnored private let outlineQueue = DispatchQueue(label: "dev.tessera.document-outline", qos: .userInitiated)
    @ObservationIgnored private var coalescer = FrameStrokeCoalescer()
    @ObservationIgnored private var strokeOpen = false
    @ObservationIgnored private var strokeTimes: [Double] = []
    @ObservationIgnored private var opacityKeys = BrushHUDMath.OpacityKeys()
    @ObservationIgnored private var outlineToken = 0
    @ObservationIgnored private var outlineKey: (String, UInt64, Int)?
    @ObservationIgnored private var transformPush = (inFlight: false, dirty: false)
    @ObservationIgnored private var busy = 0
    @ObservationIgnored private var magneticToken = 0
    /// Frames observed during the stroke (self-test timing).
    @ObservationIgnored var strokeObserver: ((StrokeFrameResult) -> Void)?

    private init() {}

    func attach(_ workspace: DocumentWorkspace) {
        guard self.workspace !== workspace else { return }
        self.workspace = workspace
    }

    private func backend(_ doc: DocumentController) -> (any DocumentToolsBackend)? {
        doc.backend as? any DocumentToolsBackend
    }

    private func say(_ s: String) { document?.report?(s) }

    private func redraw() { document?.viewport?.toolOverlay.needsDisplay = true }

    /// Runs `body` on the tools queue, then `done` on the main actor with the result.
    private func enqueue<T: Sendable>(_ what: String, _ body: @escaping @Sendable () throws -> T,
                                      done: (@MainActor (T) -> Void)? = nil) {
        busy += 1
        queue.async {
            let r = Result { try body() }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let tools = DocumentTools.shared
                    tools.busy -= 1
                    switch r {
                    case .success(let v): done?(v)
                    case .failure(let e): tools.say("\(what): \(e.localizedDescription)")
                    }
                }
            }
        }
    }

    /// After an engine edit: rows, history and the outline.
    private func reload(_ doc: DocumentController) {
        doc.reloadModel()
        doc.reloadHistory()
    }

    /// Waits until queued engine work has finished (self-test).
    func idle() async {
        while busy > 0 { try? await Task.sleep(for: .milliseconds(20)) }
    }

    // MARK: Tools

    func select(_ tool: DocumentTool) {
        guard let doc = document else { return }
        if transform != nil, tool != doc.tool { commitTransform() }
        if case .polygon = gesture { gesture = nil }
        if case .magnetic = gesture { gesture = nil }
        doc.tool = tool
        if tool.isPlaceholder { say("\(tool.title): a placeholder in this build (arrives with a later work package)") }
        doc.viewport?.cursorDidChange()
        redraw()
    }

    func cursor(for doc: DocumentController?) -> NSCursor {
        guard let doc else { return .arrow }
        if transform != nil { return .arrow }
        switch doc.tool {
        case .move: return .arrow
        case .hand: return .openHand
        case .brush, .eraser, .cloneStamp, .heal, .quickSelect: return .crosshair
        default: return .crosshair
        }
    }

    // MARK: Mouse

    private func combine(_ e: NSEvent) -> SelectionCombine {
        SelectionModifiers.combine(shift: e.modifierFlags.contains(.shift), option: e.modifierFlags.contains(.option),
                                   optionsBar: selectionMode)
    }

    func mouseMoved(_ e: NSEvent, in v: DocumentViewportView) {
        pointer = v.convert(e.locationInWindow, from: nil)
        let c = CanvasPoint(v.canvasPoint(e))
        switch gesture {
        case .polygon(let pts, let op, _): gesture = .polygon(points: pts, op: op, hover: c)
        case .magnetic(let anchors, let path, _, let op):
            gesture = .magnetic(anchors: anchors, path: path, live: [], op: op)
            updateMagnetic(to: c)
        default: break
        }
        v.toolOverlay.needsDisplay = true
    }

    func mouseExited(in v: DocumentViewportView) {
        pointer = nil
        v.toolOverlay.needsDisplay = true
    }

    /// ⌥-right-drag: the brush HUD.
    func rightMouseDown(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard let doc = document, doc.tool.paints || doc.tool == .quickSelect, e.modifierFlags.contains(.option) else { return false }
        let b = currentBrush
        gesture = .hud(startSize: doc.tool == .quickSelect ? quickSize : b.size, startHardness: b.hardness,
                       start: v.convert(e.locationInWindow, from: nil))
        hud = (b.size, b.hardness)
        return true
    }

    func mouseDown(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard let doc = document, doc === v.controller else { return false }
        let p = v.canvasPoint(e)
        let c = CanvasPoint(p)
        let flags = e.modifierFlags
        pointer = v.convert(e.locationInWindow, from: nil)
        if transform != nil { return transformMouseDown(e, in: v) }
        // ⌃-click with a painting tool: the brush HUD (size ↔, hardness ↕).
        if flags.contains(.control), doc.tool.paints || doc.tool == .quickSelect {
            let b = currentBrush
            gesture = .hud(startSize: doc.tool == .quickSelect ? quickSize : b.size, startHardness: b.hardness, start: pointer!)
            hud = (b.size, b.hardness)
            return true
        }
        switch doc.tool {
        case .move:
            beginMove(doc, at: p)
        case .marquee, .ellipseMarquee:
            gesture = .marquee(start: p, current: p, op: combine(e), ellipse: doc.tool == .ellipseMarquee,
                               startShift: flags.contains(.shift), startOption: flags.contains(.option))
        case .lasso:
            gesture = .lasso(points: [c], op: combine(e))
        case .polygonLasso:
            polygonClick(c, e)
        case .magneticLasso:
            magneticClick(c, e)
        case .quickSelect:
            let first = doc.marquee == nil
            gesture = .quick(points: [c], op: flags.contains(.option) ? .subtract
                : flags.contains(.shift) ? .add : (first ? .replace : (selectionMode == .replace ? .add : selectionMode)))
        case .wand:
            wand(doc, at: c, op: combine(e))
        case .objectSelect:
            objectSelect(doc, at: c, op: combine(e))
        case .brush, .eraser, .cloneStamp, .heal:
            if flags.contains(.option), doc.tool == .cloneStamp || doc.tool == .heal {
                setCloneSource(doc, at: c)
            } else {
                beginStroke(doc, at: c, event: e)
            }
        case .eyedropper:
            eyedrop(doc, at: c, background: flags.contains(.option))
        case .hand:
            gesture = .pan(last: pointer!)
        case .zoom:
            v.zoomStep(in: !flags.contains(.option), at: e)
        case .gradient:
            fillSelection(.color(colors.foreground), opacity: 1)
            say("Gradient: a placeholder that fills the selection with the foreground colour (gradients arrive later)")
        case .crop, .type:
            say("\(doc.tool.title): a placeholder in this build (arrives with a later work package)")
        }
        v.toolOverlay.needsDisplay = true
        return true
    }

    func mouseDragged(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = document else { return }
        let p = v.canvasPoint(e)
        let c = CanvasPoint(p)
        pointer = v.convert(e.locationInWindow, from: nil)
        switch gesture {
        case .marquee(let start, _, let op, let ellipse, let s, let o):
            gesture = .marquee(start: start, current: p, op: op, ellipse: ellipse, startShift: s, startOption: o)
        case .lasso(var pts, let op):
            if let l = pts.last, hypot(Double(l.x - c.x), Double(l.y - c.y)) * v.pointsPerPixel >= 1.5 { pts.append(c) }
            gesture = .lasso(points: pts, op: op)
        case .quick(var pts, let op):
            pts.append(c)
            gesture = .quick(points: pts, op: op)
        case .paint:
            addSample(e, at: c, in: v)
            gesture = .paint(last: c)
        case .hud(let size0, let hard0, let start):
            let d = (pointer!.x - start.x, pointer!.y - start.y)
            let r = BrushHUDMath.drag(startSize: size0, startHardness: hard0, dx: d.0, dy: d.1, zoom: v.pointsPerPixel)
            if doc.tool == .quickSelect { quickSize = r.size } else {
                var b = currentBrush
                b.size = r.size
                b.hardness = r.hardness
                currentBrush = b
            }
            hud = (r.size, r.hardness)
        case .pan(let last):
            v.panBy(dx: pointer!.x - last.x, dy: pointer!.y - last.y)
            gesture = .pan(last: pointer!)
        case .move(let start):
            guard var t = transform else { break }
            t.tx = p.x - start.x
            t.ty = p.y - start.y
            transform = t
            pushTransform()
        case .transformHandle(let h):
            transform?.drag(h, to: p, constrain: e.modifierFlags.contains(.shift), fromCenter: e.modifierFlags.contains(.option))
            pushTransform()
        case .transformMove(let last):
            transform?.move(by: CGSize(width: p.x - last.x, height: p.y - last.y))
            gesture = .transformMove(last: p)
            pushTransform()
        case .transformRotate(let start, let angle):
            transform?.rotate(from: start, to: p, startAngle: angle, snap: e.modifierFlags.contains(.shift))
            pushTransform()
        case .polygon(let pts, let op, _):
            gesture = .polygon(points: pts, op: op, hover: c)
        default: break
        }
        v.toolOverlay.needsDisplay = true
    }

    func mouseUp(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = document else { gesture = nil; return }
        let p = v.canvasPoint(e)
        switch gesture {
        case .marquee(let start, _, let op, let ellipse, let s, let o):
            gesture = nil
            let r = SelectionModifiers.marqueeRect(start: start, current: p,
                                                   square: e.modifierFlags.contains(.shift) && !s,
                                                   fromCenter: e.modifierFlags.contains(.option) && !o)
            if r.width * v.pointsPerPixel < 2 || r.height * v.pointsPerPixel < 2 {
                if op == .replace, doc.marquee != nil { run(doc, "Deselect") { try doc.backend.clearSelection() } }
            } else {
                select(doc, "Marquee") { t in
                    try t.selectMarquee(ellipse ? .ellipse : .rect, rect: r, feather: self.feather, antialias: self.antialias, op: op)
                }
            }
        case .lasso(let pts, let op):
            gesture = nil
            if pts.count >= 3 {
                select(doc, "Lasso") { t in try t.selectLasso(pts, mode: .free, feather: self.feather, antialias: self.antialias, op: op) }
            } else if op == .replace, doc.marquee != nil {
                run(doc, "Deselect") { try doc.backend.clearSelection() }
            }
        case .quick(let pts, let op):
            gesture = nil
            let radius = quickSize / 2, all = sampleAllLayers
            selectAsync(doc, "Quick Selection") { t in try t.selectQuick(stroke: pts, radius: radius, sampleAll: all, op: op) }
        case .paint:
            endStroke(doc, at: CanvasPoint(p))
            gesture = nil
        case .hud:
            gesture = nil
            hud = nil
        case .move:
            gesture = nil
            commitTransform()
        case .transformHandle, .transformMove, .transformRotate:
            gesture = nil
        case .pan:
            gesture = nil
        default: break
        }
        v.toolOverlay.needsDisplay = true
    }

    // MARK: Painting

    private func strokeLayer(_ doc: DocumentController) -> (DocLayerID, BrushStrokeTarget)? {
        guard let l = doc.primary else { say("Select a layer to paint on"); return nil }
        if l.kind == .pixel { return (l.id, paintMask && l.hasMask ? .mask : .pixels) }
        if l.kind == .group { say("Groups have no pixels: select a layer inside the group"); return nil }
        // Adjustment, fill and other layers paint their mask (created on the first stroke).
        return (l.id, .mask)
    }

    private func sample(_ e: NSEvent, at c: CanvasPoint) -> PenSample {
        let tablet = e.subtype == .tabletPoint || e.subtype == .tabletProximity
        let raw = tablet ? Double(e.pressure) : 1
        return PenSample(x: c.x, y: c.y, pressure: pressure.map(raw), tiltX: tablet ? Float(e.tilt.x) : 0,
                         tiltY: tablet ? Float(e.tilt.y) : 0, timestamp: e.timestamp)
    }

    private func beginStroke(_ doc: DocumentController, at c: CanvasPoint, event e: NSEvent) {
        guard let (layer, target) = strokeLayer(doc), let t = backend(doc), let kind = doc.tool.strokeKind else { return }
        var brush = currentBrush
        if brush.symmetry != .none, brush.symmetryX == 0, brush.symmetryY == 0 {
            brush.symmetryX = Float(doc.info.width) / 2
            brush.symmetryY = Float(doc.info.height) / 2
        }
        let color = colors.foreground
        var cloneOffset: (Float, Float, DocLayerID)?
        if kind == .clone || kind == .heal {
            guard let src = cloneSource else { say("\(doc.tool.title): ⌥-click to set the source first"); return }
            if self.cloneOffset == nil {
                self.cloneOffset = CGSize(width: Double(src.point.x - c.x), height: Double(src.point.y - c.y))
            }
            let o = self.cloneOffset!
            cloneOffset = (Float(o.width), Float(o.height), src.layer)
        }
        coalescer = FrameStrokeCoalescer(minSpacing: max(0.25, brush.size * brush.spacing * 0.25))
        strokeTimes.removeAll()
        strokeOpen = true
        var first = sample(e, at: c)
        // ⇧-click: a straight line from the last stroke's end.
        let line = e.modifierFlags.contains(.shift) ? lastStrokeEnd : nil
        let (source, options) = (cloneOffset, brush)
        enqueue("Paint") {
            if let o = source { try t.setCloneSource(layer: o.2, dx: o.0, dy: o.1) }
            try t.beginStroke(layer: layer, target: target, tool: kind, brush: options, color: color)
        } done: { _ in }
        if let l = line {
            first.x = l.x
            first.y = l.y
            coalescer.add(first)
            coalescer.add(sample(e, at: c))
        } else {
            coalescer.add(first)
        }
        gesture = .paint(last: c)
        pump(t)
    }

    private func addSample(_ e: NSEvent, at c: CanvasPoint, in v: DocumentViewportView) {
        guard strokeOpen, let doc = document, let t = backend(doc) else { return }
        coalescer.add(sample(e, at: c))
        pump(t)
    }

    /// Sends the pending samples unless a frame is in flight (they then join the next batch).
    private func pump(_ t: any DocumentToolsBackend) {
        guard let batch = coalescer.nextBatch() else { return }
        let start = Date()
        queue.async {
            let r = Result { try t.strokePoints(batch) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let tools = DocumentTools.shared
                    tools.coalescer.batchDone()
                    switch r {
                    case .success(let f):
                        tools.strokeTimes.append(Date().timeIntervalSince(start) * 1000)
                        tools.strokeObserver?(f)
                    case .failure(let e):
                        tools.strokeOpen = false
                        tools.say("Paint: \(e.localizedDescription)")
                    }
                    if tools.strokeOpen { tools.pump(t) }
                }
            }
        }
    }

    private func endStroke(_ doc: DocumentController, at c: CanvasPoint) {
        guard strokeOpen, let t = backend(doc) else { return }
        strokeOpen = false
        lastStrokeEnd = c
        let rest = coalescer.drain()
        let times = strokeTimes
        let label = doc.tool.strokeKind?.historyLabel ?? "Paint"
        enqueue(label) {
            if !rest.isEmpty { _ = try t.strokePoints(rest) }
            return try t.endStroke()
        } done: { [weak doc] _ in
            guard let doc else { return }
            let tools = DocumentTools.shared
            tools.reload(doc)
            if !times.isEmpty {
                let s = times.sorted()
                tools.strokeReadout = String(format: "%@: %d frames, median %.1f ms", label, s.count, s[s.count / 2])
            }
        }
    }

    private func setCloneSource(_ doc: DocumentController, at c: CanvasPoint) {
        guard let l = doc.primary else { return }
        cloneSource = (c, l.id)
        cloneOffset = nil
        say(String(format: "Clone source set at %.0f, %.0f on %@", c.x, c.y, l.name))
        redraw()
    }

    // MARK: Selections

    /// A quick selection call on the main thread.
    private func select(_ doc: DocumentController, _ what: String, _ body: (any DocumentToolsBackend) throws -> DocumentChange) {
        guard let t = backend(doc) else { return }
        doc.run(what) { try body(t) }
        refreshOutline(doc)
    }

    /// An image-driven selection (wand, quick, subject, …) off the main thread.
    func selectAsync(_ doc: DocumentController, _ what: String,
                     _ body: @escaping @Sendable (any DocumentToolsBackend) throws -> DocumentChange) {
        guard let t = backend(doc) else { return }
        say("\(what)…")
        let start = Date()
        enqueue(what) { try body(t) } done: { [weak doc] _ in
            guard let doc else { return }
            let tools = DocumentTools.shared
            tools.reload(doc)
            tools.refreshOutline(doc)
            let b = doc.marquee.map { " → \($0.width) × \($0.height)" } ?? " → nothing selected"
            tools.say(String(format: "%@%@ (%.2f s)", what, b, Date().timeIntervalSince(start)))
        }
    }

    private func run(_ doc: DocumentController, _ what: String, _ body: () throws -> DocumentChange) {
        doc.run(what, body)
        refreshOutline(doc)
    }

    private func wand(_ doc: DocumentController, at c: CanvasPoint, op: SelectionCombine) {
        let (tol, cont, all, aa) = (tolerance, contiguous, sampleAllLayers, antialias)
        selectAsync(doc, "Magic Wand") { t in try t.selectWand(at: c, tolerance: tol, contiguous: cont, sampleAll: all, antialias: aa, op: op) }
    }

    private func objectSelect(_ doc: DocumentController, at c: CanvasPoint, op: SelectionCombine) {
        selectAsync(doc, "Object Selection") { t in try t.selectObject(at: c, op: op) }
    }

    private func polygonClick(_ c: CanvasPoint, _ e: NSEvent) {
        guard let doc = document else { return }
        if case .polygon(var pts, let op, _) = gesture {
            let px = doc.viewport?.pointsPerPixel ?? 1
            let closes = pts.count >= 3 && (e.clickCount >= 2 || hypot(Double(pts[0].x - c.x), Double(pts[0].y - c.y)) * px < 8)
            if closes { finishPolygon(); return }
            pts.append(c)
            gesture = .polygon(points: pts, op: op, hover: c)
        } else {
            gesture = .polygon(points: [c], op: combine(e), hover: c)
        }
    }

    func finishPolygon() {
        guard let doc = document else { return }
        switch gesture {
        case .polygon(let pts, let op, _):
            gesture = nil
            guard pts.count >= 3 else { redraw(); return }
            select(doc, "Polygonal Lasso") { t in try t.selectLasso(pts, mode: .polygon, feather: feather, antialias: antialias, op: op) }
        case .magnetic(let anchors, _, _, let op):
            gesture = nil
            guard anchors.count >= 2 else { redraw(); return }
            let (f, aa) = (feather, antialias)
            selectAsync(doc, "Magnetic Lasso") { t in try t.selectLasso(anchors, mode: .magnetic, feather: f, antialias: aa, op: op) }
        default: break
        }
        redraw()
    }

    private func magneticClick(_ c: CanvasPoint, _ e: NSEvent) {
        if case .magnetic(var anchors, var path, let live, let op) = gesture {
            if e.clickCount >= 2 && anchors.count >= 2 { finishPolygon(); return }
            anchors.append(c)
            path += live
            gesture = .magnetic(anchors: anchors, path: path, live: [], op: op)
        } else {
            gesture = .magnetic(anchors: [c], path: [c], live: [], op: combine(e))
        }
    }

    private func updateMagnetic(to c: CanvasPoint) {
        guard let doc = document, let t = backend(doc), case .magnetic(let anchors, _, _, _) = gesture,
              let from = anchors.last else { return }
        magneticToken += 1
        let token = magneticToken
        queue.async {
            let path = (try? t.magneticPath(from: from, to: c)) ?? [from, c]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let tools = DocumentTools.shared
                    guard token == tools.magneticToken, case .magnetic(let a, let p, _, let op) = tools.gesture else { return }
                    tools.gesture = .magnetic(anchors: a, path: p, live: path, op: op)
                    tools.redraw()
                }
            }
        }
    }

    func selectionDidChange(in v: DocumentViewportView) {
        guard let doc = v.controller else { return }
        refreshOutline(doc, level: v.viewLevel)
    }

    /// Fetches the marching ants for the selection shown (off the main thread, newest wins).
    func refreshOutline(_ doc: DocumentController, level: Int? = nil) {
        guard let t = backend(doc) else { return }
        let lvl = level ?? doc.viewport?.viewLevel ?? 0
        let key = (doc.id, doc.info.epoch, lvl)
        if let k = outlineKey, k == key { return }
        outlineKey = key
        guard doc.marquee != nil else {
            if !outline.isEmpty { outline = [] }
            redraw()
            return
        }
        outlineToken += 1
        let token = outlineToken
        outlineQueue.async {
            let o = (try? t.selectionOutline(level: UInt8(max(0, min(lvl, 6))))) ?? []
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let tools = DocumentTools.shared
                    guard token == tools.outlineToken else { return }
                    tools.outline = o
                    tools.redraw()
                }
            }
        }
    }

    // MARK: Select menu

    func selectAll() { if let d = document { run(d, "Select All") { try backend(d)?.selectAll() ?? d.backend.clearSelection() } } }
    func deselect() {
        guard let d = document else { return }
        if case .polygon = gesture { gesture = nil }
        run(d, "Deselect") { try d.backend.clearSelection() }
    }
    func inverse() { if let d = document, let t = backend(d) { run(d, "Inverse") { try t.selectInverse() } } }
    func selectSubject() { if let d = document { selectAsync(d, "Select Subject") { t in try t.selectSubject(op: .replace) } } }
    func selectSky() { if let d = document { selectAsync(d, "Select Sky") { t in try t.selectSky(op: .replace) } } }

    func colorRange(_ color: ToolColor, fuzziness: Float) {
        guard let d = document else { return }
        selectAsync(d, "Color Range") { t in try t.selectColorRange(color, fuzziness: fuzziness, op: .replace) }
    }

    func modify(_ kind: SelectionModifyKind, px: Float) {
        guard let d = document else { return }
        selectAsync(d, kind.title) { t in try t.modifySelection(kind, px: px) }
    }

    /// Select and Mask: live preview (coalesced to the newest settings).
    @ObservationIgnored private var refinePush = (inFlight: false, dirty: false)
    func refineChanged() {
        guard let d = document, let t = backend(d) else { return }
        if refinePush.inFlight { refinePush.dirty = true; return }
        refinePush.inFlight = true
        let s = refine
        queue.async {
            let r = Result { try t.refineEdge(s, interactive: true) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let tools = DocumentTools.shared
                    tools.refinePush.inFlight = false
                    if case .failure(let e) = r { tools.say("Select and Mask: \(e.localizedDescription)") }
                    tools.outlineKey = nil
                    if let d = tools.document { d.reloadModel() }
                    if tools.refinePush.dirty { tools.refinePush.dirty = false; tools.refineChanged() }
                }
            }
        }
    }

    func refineFinish(apply: Bool) {
        guard let d = document, let t = backend(d) else { return }
        let s = refine
        refinePreview = .marchingAnts
        enqueue("Select and Mask") {
            apply ? try t.refineEdge(s, interactive: false) : try t.cancelRefineEdge()
        } done: { [weak d] _ in
            guard let d else { return }
            DocumentTools.shared.outlineKey = nil
            DocumentTools.shared.reload(d)
        }
    }

    func saveSelection(_ name: String) {
        guard let d = document, let t = backend(d) else { return }
        do { try t.saveSelection(name: name); say("Selection saved as \(name)") } catch { say("Save Selection: \(error.localizedDescription)") }
    }

    func loadSelection(_ name: String) {
        guard let d = document, let t = backend(d) else { return }
        run(d, "Load Selection") { try t.loadSelection(name: name, op: .replace) }
    }

    func channels() -> [String] {
        guard let d = document, let t = backend(d) else { return [] }
        return (try? t.selectionChannels()) ?? []
    }

    // MARK: Edit menu

    func fillSelection(_ fill: SelectionFillKind, opacity: Float) {
        guard let d = document, let t = backend(d), let l = d.primary else { say("Fill: select a pixel layer"); return }
        run(d, "Fill") { try t.fillSelection(layer: l.id, fill: fill, opacity: opacity) }
    }

    /// ⌫ with a selection: clear the selected pixels of the primary pixel layer.
    func clearSelection() -> Bool {
        guard let d = document, d.marquee != nil, let l = d.primary, l.kind == .pixel, let t = backend(d) else { return false }
        run(d, "Clear") { try t.deleteSelection(layer: l.id, background: colors.background) }
        return true
    }

    private func eyedrop(_ doc: DocumentController, at c: CanvasPoint, background: Bool) {
        guard let t = backend(doc) else { return }
        do {
            let color = try t.sampleColor(at: c, sampleAll: eyedropperAllLayers, layer: doc.primary?.id, radius: eyedropperRadius)
            if background { colors.background = color } else { colors.foreground = color }
            say("\(background ? "Background" : "Foreground") \(color.hex)")
        } catch { say("Eyedropper: \(error.localizedDescription)") }
    }

    // MARK: Free Transform

    private var transformable: [DocLayerID] {
        guard let d = document else { return [] }
        return d.selection.filter { d.node($0)?.kind == .pixel }
    }

    /// ⌘T.
    func beginFreeTransform() {
        guard let d = document, let t = backend(d), transform == nil else { return }
        let ids = transformable
        guard !ids.isEmpty else { say("Free Transform: select a pixel layer"); return }
        do {
            let start = try t.beginTransform(layers: ids)
            guard let b = start.bounds else {
                _ = try? t.cancelTransform()
                say("Free Transform: the layer is empty")
                return
            }
            transformLayers = ids
            transform = FreeTransformModel(bounds: CGRect(x: Double(b.x), y: Double(b.y), width: Double(b.width), height: Double(b.height)))
            say("Free Transform: drag handles (⇧ keeps proportions, ⌥ from the centre), outside to rotate; Return applies, Esc cancels")
        } catch { say("Free Transform: \(error.localizedDescription)") }
        d.viewport?.cursorDidChange()
        redraw()
    }

    /// Move tool: a drag is a transform translation, committed on release.
    private func beginMove(_ d: DocumentController, at p: CGPoint) {
        guard let t = backend(d) else { return }
        let ids = transformable
        guard !ids.isEmpty else { say("Move: select a pixel layer"); return }
        do {
            let start = try t.beginTransform(layers: ids)
            let b = start.bounds.map { CGRect(x: Double($0.x), y: Double($0.y), width: Double($0.width), height: Double($0.height)) }
            transformLayers = ids
            transform = FreeTransformModel(bounds: b ?? CGRect(x: 0, y: 0, width: 1, height: 1))
            gesture = .move(start: p)
        } catch { say("Move: \(error.localizedDescription)") }
    }

    /// Sends the newest matrix; drags coalesce to one call in flight.
    func pushTransform() {
        guard let d = document, let t = backend(d), let m = transform?.matrix else { return }
        redraw()
        if transformPush.inFlight { transformPush.dirty = true; return }
        transformPush.inFlight = true
        queue.async {
            let r = Result { try t.setTransform(m, interpolation: .bilinear) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let tools = DocumentTools.shared
                    tools.transformPush.inFlight = false
                    if case .failure(let e) = r { tools.say("Transform: \(e.localizedDescription)") }
                    if tools.transformPush.dirty, tools.transform != nil {
                        tools.transformPush.dirty = false
                        tools.pushTransform()
                    }
                }
            }
        }
    }

    /// Return (or tool change): one "Free Transform" history node.
    func commitTransform() {
        guard let d = document, let t = backend(d), let model = transform else { return }
        let m = model.matrix, interp = transformInterpolation, identity = model.isIdentity
        transform = nil
        transformPush.dirty = false
        enqueue("Free Transform") {
            if !identity { _ = try t.setTransform(m, interpolation: interp) }
            return try t.commitTransform()
        } done: { [weak d] _ in
            guard let d else { return }
            DocumentTools.shared.reload(d)
            d.viewport?.cursorDidChange()
        }
        redraw()
    }

    /// Esc.
    func cancelTransform() {
        guard let d = document, let t = backend(d), transform != nil else { return }
        transform = nil
        transformPush.dirty = false
        enqueue("Free Transform") { try t.cancelTransform() } done: { [weak d] _ in
            guard let d else { return }
            DocumentTools.shared.reload(d)
        }
        redraw()
    }

    /// Numeric edits from the options bar.
    func updateTransform(_ f: (inout FreeTransformModel) -> Void) {
        guard var m = transform else { return }
        f(&m)
        transform = m
        pushTransform()
    }

    /// Edit ▸ Transform ▸ Flip / Rotate: one node each.
    func quickTransform(_ title: String, _ make: (CGRect) -> AffineTransform2D) {
        guard let d = document, let t = backend(d) else { return }
        if transform != nil { commitTransform() }
        let ids = transformable
        guard !ids.isEmpty else { say("\(title): select a pixel layer"); return }
        do {
            let start = try t.beginTransform(layers: ids)
            guard let b = start.bounds else { _ = try t.cancelTransform(); return }
            let r = CGRect(x: Double(b.x), y: Double(b.y), width: Double(b.width), height: Double(b.height))
            let m = make(r)
            enqueue(title) {
                _ = try t.setTransform(m, interpolation: .bicubic)
                return try t.commitTransform()
            } done: { [weak d] _ in if let d { DocumentTools.shared.reload(d) } }
        } catch { say("\(title): \(error.localizedDescription)") }
    }

    private func handleHit(_ p: CGPoint, in v: DocumentViewportView) -> FreeTransformModel.Handle? {
        guard let t = transform else { return nil }
        let vp = v.viewPoint(canvas: p)
        return FreeTransformModel.Handle.allCases.first { h in
            let q = v.viewPoint(canvas: t.transformed(h))
            return hypot(q.x - vp.x, q.y - vp.y) <= 7
        }
    }

    private func transformMouseDown(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard let t = transform else { return false }
        let p = v.canvasPoint(e)
        if let h = handleHit(p, in: v) {
            gesture = .transformHandle(h)
        } else if t.contains(p) {
            gesture = .transformMove(last: p)
        } else {
            gesture = .transformRotate(start: p, angle: t.angle)
        }
        return true
    }

    // MARK: Keys

    /// Tool letters, [ ] and ⇧[ ⇧], digits, X / D, Return / Esc and ⌫ (with a selection). Returns
    /// whether the key was used.
    func handleKey(_ event: NSEvent) -> Bool {
        guard let doc = document else { return false }
        if event.keyCode == 51 || event.keyCode == 117 {
            let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            return mods.isEmpty && clearSelection()
        }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var mods: DocumentKeyMap.Mods = []
        if flags.contains(.shift) { mods.insert(.shift) }
        if flags.contains(.option) { mods.insert(.option) }
        if flags.contains(.command) { mods.insert(.command) }
        if flags.contains(.control) { mods.insert(.control) }
        guard let action = ToolKeyMap.action(keyCode: event.keyCode, characters: event.charactersIgnoringModifiers ?? "",
                                             mods: mods, current: doc.tool) else { return false }
        switch action {
        case .tool(let t): select(t)
        case .brushSize(let larger):
            if doc.tool == .quickSelect {
                quickSize = BrushHUDMath.bracket(size: quickSize, larger: larger)
            } else {
                var b = currentBrush
                b.size = BrushHUDMath.bracket(size: b.size, larger: larger)
                currentBrush = b
            }
        case .brushHardness(let harder):
            var b = currentBrush
            b.hardness = BrushHUDMath.bracket(hardness: b.hardness, harder: harder)
            currentBrush = b
        case .opacityDigit(let d):
            let v = opacityKeys.press(d, at: event.timestamp)
            if doc.tool.paints {
                var b = currentBrush
                b.opacity = v
                currentBrush = b
                say("Brush opacity \(Int((v * 100).rounded())) %")
            } else if doc.primary != nil {
                doc.setOpacity(Double(v * 100), final: true)
            }
        case .swapColors: colors.swap()
        case .defaultColors: colors.reset()
        case .commit:
            if transform != nil { commitTransform() } else if gesture != nil { finishPolygon() } else { return false }
        case .cancel:
            if transform != nil { cancelTransform() } else if gesture != nil { gesture = nil; redraw() } else { return false }
        }
        redraw()
        return true
    }

    // MARK: Brushes

    func reloadTips() {
        guard let d = document, let t = backend(d) else { return }
        tips = t.brushTips()
    }

    func importAbr(_ url: URL) {
        guard let d = document, let t = backend(d) else { return }
        do {
            let added = try t.importAbr(path: url.path)
            tips = t.brushTips()
            say("Imported \(added.count) brush\(added.count == 1 ? "" : "es") from \(url.lastPathComponent)")
        } catch { say("Import brushes: \(error.localizedDescription)") }
    }

    @ObservationIgnored private var previewCache: [String: NSImage] = [:]
    func tipImage(_ id: String, px: UInt32 = 48) -> NSImage? {
        let key = "\(id)@\(px)"
        if let i = previewCache[key] { return i }
        guard let d = document, let t = backend(d), let bmp = try? t.brushTipPreview(id: id, maxPx: px) else { return nil }
        // Paint = dark ink on a clear background (the Brushes panel list).
        var rgba = [UInt8](repeating: 0, count: Int(bmp.width * bmp.height) * 4)
        for (i, v) in bmp.pixels.enumerated() {
            rgba[i * 4 + 3] = v
        }
        guard let provider = CGDataProvider(data: Data(rgba) as CFData),
              let cg = CGImage(width: Int(bmp.width), height: Int(bmp.height), bitsPerComponent: 8, bitsPerPixel: 32,
                               bytesPerRow: Int(bmp.width) * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                               bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                               provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        let img = NSImage(cgImage: cg, size: NSSize(width: Int(bmp.width) / 2, height: Int(bmp.height) / 2))
        img.isTemplate = true
        previewCache[key] = img
        return img
    }
}
