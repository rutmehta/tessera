import AppKit
import TesseraCore

/// The advanced transforms' on-canvas editors (WP B5-12), a subview above `ToolOverlayView` that never
/// takes mouse events (`DocumentTools` forwards them to `DocumentTransforms`). Only the on-image set
/// (DESIGN.md §2.3, §10): lines are 1 pt `OnImage.guide` over a 3 pt `OnImage.shadow`; secondary lines
/// (tangents, puppet mesh, the layout planes while warping) `OnImage.guideFaint`; anchors and vertices
/// are the square transform handles (`OnImage.text` with `OnImage.ink`); warp tangents and puppet pins are
/// dots; the selected pin is the accent (content selection). Readouts are scrim chips.
@MainActor
final class TransformOverlayView: NSView {
    weak var viewport: DocumentViewportView?

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    private var t: DocumentTransforms { .shared }

    override func draw(_ dirtyRect: NSRect) {
        guard let v = viewport, let doc = v.controller, t.isActive(doc), let s = t.session else { return }
        NSGraphicsContext.current?.cgContext.setShouldAntialias(true)
        let map = { (p: CGPoint) in self.t.viewPoint(p, in: v) }
        // The fixed child canvas the stage clips to (dashed, faint).
        let canvas = [CGPoint(x: 0, y: 0), CGPoint(x: CGFloat(s.start.childWidth), y: 0),
                      CGPoint(x: CGFloat(s.start.childWidth), y: CGFloat(s.start.childHeight)),
                      CGPoint(x: 0, y: CGFloat(s.start.childHeight))].map(map)
        faint(polygon(canvas, closed: true), dashed: true)

        switch s.op {
        case .warp(let m):
            for line in m.gridLines() { guide(polygon(line.map(map), closed: false)) }
            for (a, b) in m.handleSegments() { faint(polygon([map(a), map(b)], closed: false), dashed: false) }
            for (r, c) in m.visibleControls() {
                let p = map(m.controlPoints[r][c])
                if WarpMeshModel.isAnchor(row: r, column: c) { handle(p) } else { dot(p, radius: 3) }
            }
            if t.warpSplit != .none { chip("Click to split \(t.warpSplit == .cross ? "both ways" : t.warpSplit.rawValue)", at: CGPoint(x: canvas[0].x, y: canvas[0].y - 20)) }
        case .perspective(let p):
            let layout = t.perspectiveLayout
            if !layout {
                // The layout planes stay visible, faint, while warping.
                for q in p.quads { faint(polygon(p.quad(p.source, q.row, q.column).map(map), closed: true), dashed: true) }
            }
            let g = layout ? p.source : p.destination
            for q in p.quads { guide(polygon(p.quad(g, q.row, q.column).map(map), closed: true)) }
            for row in g { for v in row { handle(map(v)) } }
            chip(layout ? "Layout" : "Warp", at: CGPoint(x: canvas[0].x, y: canvas[0].y - 20))
        case .puppet(let p):
            let pos = s.deformed ?? p.restVertices
            if t.showMesh, pos.count == p.restVertices.count {
                let mesh = NSBezierPath()
                for (a, b) in p.edges {
                    mesh.move(to: map(pos[a]))
                    mesh.line(to: map(pos[b]))
                }
                mesh.lineWidth = 0.5
                Theme.Palette.OnImage.guideFaint.setStroke()
                mesh.stroke()
            }
            for (i, pin) in p.pins.enumerated() {
                let c = map(pin.target)
                if let r = pin.rotation {
                    // The rotation ring with its angle.
                    let ring = NSBezierPath(ovalIn: CGRect(x: c.x - 22, y: c.y - 22, width: 44, height: 44))
                    faint(ring, dashed: false)
                    let tick = NSBezierPath()
                    tick.move(to: c)
                    tick.line(to: CGPoint(x: c.x + CGFloat(22 * cos(r)), y: c.y + CGFloat(22 * sin(r))))
                    guide(tick)
                }
                pinDot(c, selected: i == t.selectedPin)
            }
        case .contentAwareScale(let c):
            let r = [CGPoint(x: 0, y: 0), CGPoint(x: CGFloat(c.width), y: 0),
                     CGPoint(x: CGFloat(c.width), y: CGFloat(c.height)), CGPoint(x: 0, y: CGFloat(c.height))].map(map)
            guide(polygon(r, closed: true))
            for h in ContentAwareScaleModel.Handle.allCases { handle(map(c.point(h))) }
            let pct = c.percent
            chip(String(format: "%u × %u px · %.0f %% × %.0f %%", c.width, c.height, pct.w, pct.h),
                 at: CGPoint(x: r[2].x + 10, y: r[2].y + 6))
        }
    }

    // MARK: Drawing

    private func polygon(_ pts: [CGPoint], closed: Bool) -> NSBezierPath {
        let p = NSBezierPath()
        guard let first = pts.first else { return p }
        p.move(to: first)
        for q in pts.dropFirst() { p.line(to: q) }
        if closed { p.close() }
        return p
    }

    /// A guide line: 3 pt shadow under 1 pt guide.
    private func guide(_ p: NSBezierPath) {
        p.lineJoinStyle = .round
        p.lineWidth = 3
        Theme.Palette.OnImage.shadow.setStroke()
        p.stroke()
        p.lineWidth = 1
        Theme.Palette.OnImage.guide.setStroke()
        p.stroke()
    }

    private func faint(_ p: NSBezierPath, dashed: Bool) {
        p.lineWidth = 1
        if dashed { p.setLineDash([4, 3], count: 2, phase: 0) }
        Theme.Palette.OnImage.guideFaint.setStroke()
        p.stroke()
    }

    /// The square transform handle (7 pt, `OnImage.text` with an `OnImage.ink` outline).
    private func handle(_ p: CGPoint) {
        let r = CGRect(x: p.x - 3.5, y: p.y - 3.5, width: 7, height: 7)
        Theme.Palette.OnImage.text.setFill()
        r.fill()
        Theme.Palette.OnImage.ink.setStroke()
        let b = NSBezierPath(rect: r)
        b.lineWidth = 1
        b.stroke()
    }

    private func dot(_ p: CGPoint, radius: CGFloat) {
        let b = NSBezierPath(ovalIn: CGRect(x: p.x - radius, y: p.y - radius, width: 2 * radius, height: 2 * radius))
        Theme.Palette.OnImage.text.setFill()
        b.fill()
        b.lineWidth = 1
        Theme.Palette.OnImage.ink.setStroke()
        b.stroke()
    }

    private func pinDot(_ p: CGPoint, selected: Bool) {
        let b = NSBezierPath(ovalIn: CGRect(x: p.x - 5, y: p.y - 5, width: 10, height: 10))
        (selected ? Theme.Palette.accent : Theme.Palette.OnImage.text).setFill()
        b.fill()
        b.lineWidth = 1.5
        Theme.Palette.OnImage.ink.setStroke()
        b.stroke()
    }

    /// A scrim chip with on-image text (16 pt, radius 4).
    private func chip(_ text: String, at p: CGPoint) {
        let attrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.captionNumeric, .foregroundColor: Theme.Palette.OnImage.text]
        let s = NSAttributedString(string: text, attributes: attrs)
        let size = s.size()
        let r = CGRect(x: max(p.x, 2), y: max(p.y, 2), width: size.width + 12, height: 16)
        Theme.Palette.OnImage.scrim.setFill()
        NSBezierPath(roundedRect: r, xRadius: 4, yRadius: 4).fill()
        s.draw(at: CGPoint(x: r.minX + 6, y: r.minY + (16 - size.height) / 2))
    }
}
