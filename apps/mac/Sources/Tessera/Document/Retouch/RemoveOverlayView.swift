import AppKit
import TesseraCore

/// The Remove tool's on-canvas feedback (WP B5-09), a subview above `ToolOverlayView` that never takes
/// mouse events: the stroke being painted as a translucent `OnImage.reject` band at brush width (what will
/// be removed), and Remove Distractions suggestions under review as boxes: accepted ones solid
/// `OnImage.guide` over `OnImage.shadow` with a faint reject tint inside, kept ones dashed in
/// `OnImage.guideFaint`, each with a scrim chip naming it. Only the on-image set (DESIGN.md §2.3).
@MainActor
final class RemoveOverlayView: NSView {
    weak var viewport: DocumentViewportView?

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        guard let v = viewport, let doc = v.controller, DocumentTools.shared.document === doc else { return }
        let r = DocumentRetouch.shared
        guard r.removeActive || r.review != nil else { return }
        NSGraphicsContext.current?.cgContext.setShouldAntialias(true)

        // The stroke: the dabs' union as a round-capped band.
        let pts = r.strokePoints
        if let first = pts.first {
            let w = max(CGFloat(Double(r.options.size) * v.pointsPerPixel), 1)
            let p = NSBezierPath()
            p.move(to: v.viewPoint(canvas: first.cgPoint))
            if pts.count == 1 {
                p.line(to: v.viewPoint(canvas: first.cgPoint))
            } else {
                for q in pts.dropFirst() { p.line(to: v.viewPoint(canvas: q.cgPoint)) }
            }
            p.lineWidth = w
            p.lineCapStyle = .round
            p.lineJoinStyle = .round
            Theme.Palette.OnImage.reject.withAlphaComponent(0.55).setStroke()
            p.stroke()
        }

        // Suggestions under review.
        guard let review = r.review else { return }
        for c in review.candidates {
            let b = c.bounds
            let a = v.viewPoint(canvas: CGPoint(x: Double(b.x), y: Double(b.y)))
            let z = v.viewPoint(canvas: CGPoint(x: Double(b.x + b.width), y: Double(b.y + b.height)))
            let rect = CGRect(x: a.x, y: a.y, width: max(z.x - a.x, 3), height: max(z.y - a.y, 3)).insetBy(dx: -2, dy: -2)
            let box = NSBezierPath(rect: rect)
            let on = review.isAccepted(c.id)
            if on {
                Theme.Palette.OnImage.reject.withAlphaComponent(0.22).setFill()
                box.fill()
                box.lineWidth = 3
                Theme.Palette.OnImage.shadow.setStroke()
                box.stroke()
                box.lineWidth = 1.5
                Theme.Palette.OnImage.guide.setStroke()
                box.stroke()
            } else {
                box.lineWidth = 1
                box.setLineDash([4, 3], count: 2, phase: 0)
                Theme.Palette.OnImage.guideFaint.setStroke()
                box.stroke()
            }
            chip("\(c.kind.title)\(on ? "" : " (kept)")", at: CGPoint(x: rect.minX, y: rect.minY - 18))
        }
    }

    /// A scrim chip with on-image text (DESIGN.md chips: 16 pt on images, radius 4).
    private func chip(_ text: String, at p: CGPoint) {
        let attrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.caption, .foregroundColor: Theme.Palette.OnImage.text]
        let s = NSAttributedString(string: text, attributes: attrs)
        let size = s.size()
        let r = CGRect(x: p.x, y: max(p.y, 2), width: size.width + 12, height: 16)
        Theme.Palette.OnImage.scrim.setFill()
        NSBezierPath(roundedRect: r, xRadius: 4, yRadius: 4).fill()
        s.draw(at: CGPoint(x: r.minX + 6, y: r.minY + (16 - size.height) / 2))
    }
}
