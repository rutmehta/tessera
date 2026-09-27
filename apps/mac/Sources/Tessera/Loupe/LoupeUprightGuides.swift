import AppKit
import TesseraCore

/// A Guided Upright gesture on the loupe (M2-48).
enum GuideDrag {
    /// Drawing a new guide from `start` (sensor-normalised).
    case new(start: (x: Double, y: Double), current: (x: Double, y: Double))
    /// Dragging one end of guide `index`.
    case endpoint(index: Int, end: Bool)
    /// A click on a guide's body (selects it).
    case select
}

/// Guided Upright in the loupe: up to four guides drawn over the photo in the on-image set, the
/// selected one in the accent, square end handles. Points map view → displayed uv
/// (`MetalLoupeView.displayUV`) → sensor-normalised image space (`MaskSpace`, the space of the
/// engine's `GuideLine`). Each finished gesture is one history step (`UprightGuideTool`).
extension LoupeToolOverlay {
    private var guideSpace: MaskSpace? {
        guard let d = guideTool.develop else { return nil }
        // The uncorrected view (M2-51) shows the whole uncropped frame.
        return MaskSpace(orientation: Int(d.info.orientation), crop: guideTool.showsUncorrected ? nil : tools.storedCrop())
    }

    private func toImage(_ p: CGPoint) -> (x: Double, y: Double)? {
        guard let uv = loupe?.displayUV(p), let space = guideSpace else { return nil }
        return space.toMask(min(max(uv.u, 0), 1), min(max(uv.v, 0), 1))
    }

    private func toView(_ m: (x: Double, y: Double)) -> CGPoint? {
        guard let space = guideSpace else { return nil }
        let uv = space.fromMask(m.x, m.y)
        return loupe?.viewPoint(u: uv.u, v: uv.v)
    }

    private static let handleRadius: CGFloat = 8

    /// The guide end under `p` (view points), nearest first.
    private func endpoint(at p: CGPoint) -> (index: Int, end: Bool)? {
        var best: (Int, Bool, CGFloat)?
        for (i, g) in guideTool.guides.guides.enumerated() {
            for (end, m) in [(false, g.start), (true, g.end)] {
                guard let v = toView(m) else { continue }
                let d = hypot(v.x - p.x, v.y - p.y)
                if d < Self.handleRadius, d < (best?.2 ?? .infinity) { best = (i, end, d) }
            }
        }
        return best.map { ($0.0, $0.1) }
    }

    /// The guide whose line passes within a few points of `p`.
    private func guideBody(at p: CGPoint) -> Int? {
        for (i, g) in guideTool.guides.guides.enumerated() {
            guard let a = toView(g.start), let b = toView(g.end) else { continue }
            let (dx, dy) = (b.x - a.x, b.y - a.y)
            let len2 = dx * dx + dy * dy
            guard len2 > 0 else { continue }
            let t = min(max(((p.x - a.x) * dx + (p.y - a.y) * dy) / len2, 0), 1)
            if hypot(a.x + t * dx - p.x, a.y + t * dy - p.y) < Theme.Space.xs + Theme.Space.xxs { return i }
        }
        return nil
    }

    // MARK: Drawing

    func drawGuides() {
        let selected = guideTool.guides.selected
        for (i, g) in guideTool.guides.guides.enumerated() {
            guard let a = toView(g.start), let b = toView(g.end) else { continue }
            strokeGuide(a, b, color: i == selected ? Theme.Palette.accent : Theme.Palette.OnImage.guide, dashed: false)
            for c in [a, b] { drawHandle(c) }
        }
        if case .new(let s, let c) = guideDrag, let a = toView(s), let b = toView(c) {
            strokeGuide(a, b, color: Theme.Palette.OnImage.guide, dashed: true)
        }
        let n = guideTool.guides.guides.count
        let status = n < UprightGuides.minimum
            ? "Guided Upright · draw \(UprightGuides.minimum - n) more guide\(UprightGuides.minimum - n == 1 ? "" : "s") along verticals or horizontals"
            : "Guided Upright · \(n) of \(UprightGuides.maximum) guides · ⌫ removes the selected one · Return when done"
        hint(status)
    }

    private func strokeGuide(_ a: CGPoint, _ b: CGPoint, color: NSColor, dashed: Bool) {
        let shadow = NSBezierPath()
        shadow.move(to: a); shadow.line(to: b)
        shadow.lineWidth = 3
        shadow.lineCapStyle = .round
        Theme.Palette.OnImage.shadow.setStroke()
        shadow.stroke()
        let line = NSBezierPath()
        line.move(to: a); line.line(to: b)
        line.lineWidth = 1.5
        if dashed { line.setLineDash([4, 3], count: 2, phase: 0) }
        color.setStroke()
        line.stroke()
    }

    private func drawHandle(_ c: CGPoint) {
        let r = NSRect(x: c.x - 3.5, y: c.y - 3.5, width: 7, height: 7)
        Theme.Palette.OnImage.text.setFill()
        NSBezierPath(rect: r).fill()
        Theme.Palette.OnImage.ink.setStroke()
        let outline = NSBezierPath(rect: r)
        outline.lineWidth = 1
        outline.stroke()
    }

    // MARK: Cursor

    func guideCursor(_ p: CGPoint) {
        if endpoint(at: p) != nil { NSCursor.openHand.set() }
        else if guideBody(at: p) != nil { NSCursor.pointingHand.set() }
        else { NSCursor.crosshair.set() }
    }

    // MARK: Mouse

    func guideMouseDown(_ p: CGPoint, _ event: NSEvent) {
        if let (i, end) = endpoint(at: p) {
            guideTool.guides.selected = i
            guideDrag = .endpoint(index: i, end: end)
            NSCursor.closedHand.set()
        } else if let i = guideBody(at: p) {
            guideTool.guides.selected = i
            guideDrag = .select
        } else if let m = toImage(p), loupe?.pictureLocation(p) != nil {
            guard !guideTool.guides.isFull else {
                tools.model.statusMessage = "Four guides is the most Guided Upright uses; drag an end or ⌫ one first"
                return
            }
            guideTool.guides.selected = nil
            guideDrag = .new(start: m, current: m)
        }
        needsDisplay = true
    }

    func guideMouseDragged(_ p: CGPoint) {
        guard let m = toImage(p) else { return }
        switch guideDrag {
        case .new(let s, _): guideDrag = .new(start: s, current: m)
        case .endpoint(let i, let end): guideTool.guides.move(i, end: end, to: m)
        default: break
        }
        needsDisplay = true
    }

    func guideMouseUp(_ p: CGPoint) {
        switch guideDrag {
        case .new(let s, _):
            if let m = toImage(p), guideTool.guides.add(UprightGuide(start: s, end: m)) != nil {
                guideTool.commitGuides()
            }
        case .endpoint:
            guideTool.commitGuides()
        default: break
        }
        guideDrag = nil
        needsDisplay = true
    }
}
