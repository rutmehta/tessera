import AppKit
import TesseraCore

/// Draws the layered editor's on-canvas feedback over the document viewport (WP M5-11): marching ants
/// from the engine's selection outline, gestures in progress (marquee, lassos, quick-selection
/// stroke), the brush outline at brush size with its hardness ring, symmetry guides, the clone
/// source, the brush HUD readout, the Free Transform box, and Select and Mask preview modes. It never
/// takes mouse events: the viewport forwards them to `DocumentTools`.
@MainActor
final class ToolOverlayView: NSView {
    weak var viewport: DocumentViewportView?
    private var phase: CGFloat = 0
    private var timer: Timer?

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    /// Ants march at 30 fps while there is an outline.
    private func updateTimer() {
        let animate = !DocumentTools.shared.outline.isEmpty || DocumentTools.shared.gesture != nil
        if animate, timer == nil {
            timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.phase = (self.phase + 0.5).truncatingRemainder(dividingBy: 8)
                    self.needsDisplay = true
                    if DocumentTools.shared.outline.isEmpty && DocumentTools.shared.gesture == nil {
                        self.timer?.invalidate()
                        self.timer = nil
                    }
                }
            }
        }
    }

    private func path(_ pts: [CanvasPoint], closed: Bool, in v: DocumentViewportView) -> NSBezierPath? {
        guard pts.count > 1 else { return nil }
        let p = NSBezierPath()
        p.move(to: v.viewPoint(canvas: pts[0].cgPoint))
        for q in pts.dropFirst() { p.line(to: v.viewPoint(canvas: q.cgPoint)) }
        if closed { p.close() }
        return p
    }

    /// The on-image ant pair: light line, dark dashes.
    private func ants(_ p: NSBezierPath) {
        p.lineWidth = Theme.Space.hairline
        Theme.Palette.OnImage.text.setStroke()
        p.stroke()
        p.setLineDash([4, 4], count: 2, phase: phase)
        Theme.Palette.OnImage.ink.setStroke()
        p.stroke()
        p.setLineDash([], count: 0, phase: 0)
    }

    private func guide(_ p: NSBezierPath, width: CGFloat = 1) {
        p.lineWidth = width + 1
        Theme.Palette.OnImage.shadow.setStroke()
        p.stroke()
        p.lineWidth = width
        Theme.Palette.OnImage.guide.setStroke()
        p.stroke()
    }

    override func draw(_ dirtyRect: NSRect) {
        updateTimer()
        guard let v = viewport, let doc = v.controller, DocumentTools.shared.document === doc else { return }
        let tools = DocumentTools.shared
        NSGraphicsContext.current?.cgContext.setShouldAntialias(true)

        // Select and Mask preview modes: the canvas outside the selection tinted, black or white.
        if tools.sheet == .refineEdge, tools.refinePreview != .marchingAnts {
            let canvas = CGRect(x: 0, y: 0, width: Double(doc.info.width), height: Double(doc.info.height))
            let a = v.viewPoint(canvas: canvas.origin), b = v.viewPoint(canvas: CGPoint(x: canvas.maxX, y: canvas.maxY))
            let fill = NSBezierPath(rect: CGRect(x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y))
            fill.windingRule = .evenOdd
            for o in tools.outline { if let p = path(o.points, closed: true, in: v) { fill.append(p) } }
            switch tools.refinePreview {
            case .overlay: Theme.Palette.OnImage.reject.withAlphaComponent(0.5).setFill()
            case .onBlack: Theme.Palette.OnImage.ink.setFill()
            case .onWhite: Theme.Palette.OnImage.text.setFill()
            case .marchingAnts: break
            }
            fill.fill()
        }

        // Marching ants.
        if tools.transform == nil {
            for o in tools.outline { if let p = path(o.points, closed: o.closed, in: v) { ants(p) } }
        }

        // Gestures in progress.
        switch tools.gesture {
        case .marquee(let s, let c, _, let ellipse, let s0, let o0):
            let mods = NSEvent.modifierFlags
            let r = SelectionModifiers.marqueeRect(start: s, current: c, square: mods.contains(.shift) && !s0,
                                                   fromCenter: mods.contains(.option) && !o0)
            let a = v.viewPoint(canvas: r.origin), b = v.viewPoint(canvas: CGPoint(x: r.maxX, y: r.maxY))
            let vr = CGRect(x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y).integral.insetBy(dx: 0.5, dy: 0.5)
            ants(ellipse ? NSBezierPath(ovalIn: vr) : NSBezierPath(rect: vr))
        case .lasso(let pts, _):
            if let p = path(pts, closed: false, in: v) { ants(p) }
        case .polygon(let pts, _, let hover):
            if let p = path(pts + (hover.map { [$0] } ?? []), closed: false, in: v) { ants(p) }
            for q in pts { knob(v.viewPoint(canvas: q.cgPoint), small: true) }
        case .magnetic(let anchors, let pathPts, let live, _):
            if let p = path(pathPts + live, closed: false, in: v) { ants(p) }
            for q in anchors { knob(v.viewPoint(canvas: q.cgPoint), small: true) }
        case .quick(let pts, _):
            let r = CGFloat(Double(tools.quickSize) / 2 * v.pointsPerPixel)
            if let p = path(pts, closed: false, in: v) {
                p.lineCapStyle = .round
                p.lineJoinStyle = .round
                p.lineWidth = max(2 * r, 1)
                Theme.Palette.OnImage.guideFaint.setStroke()
                p.stroke()
            }
        default: break
        }

        if let t = tools.transform { drawTransform(t, in: v) }

        // Symmetry guides.
        if doc.tool.paints, tools.currentBrush.symmetry != .none { drawSymmetry(tools.currentBrush, doc: doc, in: v) }

        // Clone source crosshair: the source point, or where the pointer samples from.
        if doc.tool == .cloneStamp || doc.tool == .heal, let src = tools.cloneSource {
            var c = v.viewPoint(canvas: src.point.cgPoint)
            if let o = tools.cloneOffset, let ptr = tools.pointer {
                let k = v.pointsPerPixel
                c = CGPoint(x: ptr.x + o.width * k, y: ptr.y + o.height * k)
            }
            let p = NSBezierPath()
            p.move(to: CGPoint(x: c.x - 7, y: c.y)); p.line(to: CGPoint(x: c.x + 7, y: c.y))
            p.move(to: CGPoint(x: c.x, y: c.y - 7)); p.line(to: CGPoint(x: c.x, y: c.y + 7))
            guide(p)
        }

        // Brush outline at the pointer (and the quick-selection brush).
        if let ptr = tools.pointer, tools.transform == nil, doc.tool.paints || doc.tool == .quickSelect {
            let b = tools.currentBrush
            let size = doc.tool == .quickSelect ? tools.quickSize : b.size
            let center: CGPoint = {
                if case .hud(_, _, let s) = tools.gesture { return s }
                return ptr
            }()
            let r = CGFloat(Double(size) / 2 * v.pointsPerPixel)
            if r >= 2 {
                let o = NSBezierPath(ovalIn: CGRect(x: center.x - r, y: center.y - r, width: 2 * r, height: 2 * r))
                guide(o)
                if doc.tool != .quickSelect, b.hardness < 0.95, r * CGFloat(b.hardness) > 2 {
                    let ri = r * CGFloat(b.hardness)
                    let inner = NSBezierPath(ovalIn: CGRect(x: center.x - ri, y: center.y - ri, width: 2 * ri, height: 2 * ri))
                    inner.setLineDash([2, 3], count: 2, phase: 0)
                    inner.lineWidth = 1
                    Theme.Palette.OnImage.guideFaint.setStroke()
                    inner.stroke()
                }
            }
            if r < 6 {
                let p = NSBezierPath()
                p.move(to: CGPoint(x: center.x - 4, y: center.y)); p.line(to: CGPoint(x: center.x + 4, y: center.y))
                p.move(to: CGPoint(x: center.x, y: center.y - 4)); p.line(to: CGPoint(x: center.x, y: center.y + 4))
                guide(p)
            }
            if let hud = tools.hud {
                let text = String(format: "%.0f px  ·  %.0f %% hard", hud.size, hud.hardness * 100)
                chip(text, at: CGPoint(x: center.x + r + Theme.Space.s, y: center.y - Theme.Height.chip / 2))
            }
        }
    }

    private func chip(_ text: String, at p: CGPoint) {
        let attrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.captionNumeric, .foregroundColor: Theme.Palette.OnImage.text]
        let s = NSAttributedString(string: text, attributes: attrs)
        let size = s.size()
        let r = CGRect(x: p.x, y: p.y, width: size.width + 2 * Theme.Space.s, height: Theme.Height.chip + Theme.Space.xs)
        Theme.Palette.OnImage.scrim.setFill()
        NSBezierPath(roundedRect: r, xRadius: Theme.Radius.chip, yRadius: Theme.Radius.chip).fill()
        s.draw(at: CGPoint(x: r.minX + Theme.Space.s, y: r.midY - size.height / 2))
    }

    private func knob(_ p: CGPoint, small: Bool = false) {
        let r: CGFloat = small ? 3 : 4
        let o = NSBezierPath(rect: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r))
        Theme.Palette.OnImage.text.setFill()
        o.fill()
        o.lineWidth = 1
        Theme.Palette.OnImage.ink.setStroke()
        o.stroke()
    }

    private func drawTransform(_ t: FreeTransformModel, in v: DocumentViewportView) {
        let corners = t.corners.map { v.viewPoint(canvas: $0) }
        let box = NSBezierPath()
        box.move(to: corners[0])
        corners.dropFirst().forEach { box.line(to: $0) }
        box.close()
        guide(box)
        for h in FreeTransformModel.Handle.allCases { knob(v.viewPoint(canvas: t.transformed(h))) }
        let c = v.viewPoint(canvas: t.referenceNow)
        let ring = NSBezierPath(ovalIn: CGRect(x: c.x - 4, y: c.y - 4, width: 8, height: 8))
        guide(ring)
    }

    private func drawSymmetry(_ b: BrushOptions, doc: DocumentController, in v: DocumentViewportView) {
        let (w, h) = (Double(doc.info.width), Double(doc.info.height))
        let x = b.symmetryX == 0 && b.symmetryY == 0 ? w / 2 : Double(b.symmetryX)
        let y = b.symmetryX == 0 && b.symmetryY == 0 ? h / 2 : Double(b.symmetryY)
        let p = NSBezierPath()
        let seg = { (a: CGPoint, c: CGPoint) in p.move(to: v.viewPoint(canvas: a)); p.line(to: v.viewPoint(canvas: c)) }
        switch b.symmetry {
        case .none: return
        case .vertical: seg(CGPoint(x: x, y: 0), CGPoint(x: x, y: h))
        case .horizontal: seg(CGPoint(x: 0, y: y), CGPoint(x: w, y: y))
        case .dual:
            seg(CGPoint(x: x, y: 0), CGPoint(x: x, y: h))
            seg(CGPoint(x: 0, y: y), CGPoint(x: w, y: y))
        case .diagonal:
            let r = max(w, h)
            seg(CGPoint(x: x - r, y: y - r), CGPoint(x: x + r, y: y + r))
        case .radial, .mandala:
            let n = max(Int(b.symmetryCount), 1) * (b.symmetry == .mandala ? 2 : 1)
            let r = hypot(w, h)
            for k in 0..<n {
                let a = Double(k) * 2 * .pi / Double(n) - .pi / 2
                seg(CGPoint(x: x, y: y), CGPoint(x: x + cos(a) * r, y: y + sin(a) * r))
            }
        }
        p.setLineDash([6, 4], count: 2, phase: 0)
        p.lineWidth = 1
        Theme.Palette.OnImage.guideFaint.setStroke()
        p.stroke()
    }
}
