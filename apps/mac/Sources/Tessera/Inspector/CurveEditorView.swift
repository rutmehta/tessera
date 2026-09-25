import AppKit
import TesseraCore
import TesseraFFI

/// Tone curve editor (docs/01 §2.3): the parametric curve with draggable regions and split points,
/// or a point curve per channel (monotone cubic, draggable knots, arrow-key nudge), drawn over the
/// histogram. Both axes are the engine's log tone axis. AppKit-drawn: a drag calls straight into
/// the develop session without SwiftUI (the same < 16 ms path as the sliders).
@MainActor
final class CurveEditorView: NSView, DevelopKeyHandling {
    enum Mode { case parametric, point }

    var mode: Mode = .parametric { didSet { if mode != oldValue { selected = nil; needsDisplay = true } } }
    var channel: CurveChannel = .rgb { didSet { if channel != oldValue { selected = nil; needsDisplay = true } } }
    var curve = PointCurve.identity { didSet { needsDisplay = true } }
    var parametric = ParametricCurveModel() { didSet { needsDisplay = true } }
    var histogram: Histogram? { didSet { needsDisplay = true } }
    var isEnabled = true { didSet { alphaValue = isEnabled ? 1 : 0.4 } }

    /// Point curve edits (`final` on mouse-up / key).
    var onCurve: ((PointCurve, Bool) -> Void)?
    /// Parametric region amount (index into shadows, darks, lights, highlights).
    var onRegion: ((Int, Double, Bool) -> Void)?
    var onSplits: ((ParametricCurveModel, Bool) -> Void)?

    private(set) var selected: Int?
    /// A drag is in progress (external reloads must not fight it).
    var isInteracting: Bool { dragKnot != nil || dragRegion != nil || dragSplit != nil }
    private var dragKnot: Int?
    private var dragRegion: (index: Int, start: Double, y: CGFloat)?
    private var dragSplit: (index: Int, start: Double, x: CGFloat)?
    private var hover: CGPoint?
    private var tracking: NSTrackingArea?

    private let splitStrip: CGFloat = 14

    override var isFlipped: Bool { false }
    override var acceptsFirstResponder: Bool { isEnabled && mode == .point }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Tone curve")
    }
    required init?(coder: NSCoder) { fatalError() }

    private var plot: NSRect {
        let strip = mode == .parametric ? splitStrip : 0
        return NSRect(x: bounds.minX + 4, y: bounds.minY + 4 + strip,
                      width: bounds.width - 8, height: bounds.height - 8 - strip)
    }

    private func point(_ x: Double, _ y: Double) -> CGPoint {
        CGPoint(x: plot.minX + plot.width * x, y: plot.minY + plot.height * y)
    }

    private func value(_ p: CGPoint) -> CurveKnot {
        CurveKnot(Double((p.x - plot.minX) / plot.width), Double((p.y - plot.minY) / plot.height))
    }

    private var channelColor: NSColor {
        switch channel {
        case .red: Theme.Palette.channelRed
        case .green: Theme.Palette.channelGreen
        case .blue: Theme.Palette.channelBlue
        default: Theme.Palette.plotLine
        }
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        Theme.Palette.plotWell.setFill()
        NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: Theme.Radius.chip, yRadius: Theme.Radius.chip).fill()
        let plot = self.plot
        drawHistogram(in: plot)
        // Quarter grid and the identity diagonal.
        let grid = NSBezierPath()
        for i in 1..<4 {
            let f = CGFloat(i) / 4
            grid.move(to: CGPoint(x: plot.minX + plot.width * f, y: plot.minY))
            grid.line(to: CGPoint(x: plot.minX + plot.width * f, y: plot.maxY))
            grid.move(to: CGPoint(x: plot.minX, y: plot.minY + plot.height * f))
            grid.line(to: CGPoint(x: plot.maxX, y: plot.minY + plot.height * f))
        }
        Theme.Palette.plotGrid.setStroke()
        grid.lineWidth = 1
        grid.stroke()
        let diagonal = NSBezierPath()
        diagonal.move(to: point(0, 0)); diagonal.line(to: point(1, 1))
        Theme.Palette.plotGuide.setStroke()
        diagonal.setLineDash([3, 3], count: 2, phase: 0)
        diagonal.stroke()

        switch mode {
        case .parametric: drawParametric(in: plot)
        case .point: drawPoint(in: plot)
        }
        if let h = hover, plot.contains(h) {
            let v = value(h)
            let out = mode == .point ? curve.evaluate(v.x) : parametric.evaluate(v.x)
            let text = String(format: "%.0f → %.0f", v.x * 100, out * 100) as NSString
            let attrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.captionNumeric,
                                                        .foregroundColor: Theme.Palette.plotText]
            text.draw(at: NSPoint(x: plot.minX + Theme.Space.xs, y: plot.maxY - Theme.Space.l), withAttributes: attrs)
        }
    }

    private func drawHistogram(in plot: NSRect) {
        guard let h = histogram, h.luminance.count == 256 else { return }
        let bins: [UInt32]
        switch (mode, channel) {
        case (.point, .red): bins = h.red
        case (.point, .green): bins = h.green
        case (.point, .blue): bins = h.blue
        default: bins = h.luminance
        }
        let peak = sqrt(Double(bins[1..<255].max() ?? 1))
        let p = NSBezierPath()
        p.move(to: CGPoint(x: plot.minX, y: plot.minY))
        for (i, v) in bins.enumerated() {
            p.line(to: CGPoint(x: plot.minX + plot.width * CGFloat(i) / 255,
                               y: plot.minY + plot.height * CGFloat(min(sqrt(Double(v)) / max(peak, 1), 1)) * 0.9))
        }
        p.line(to: CGPoint(x: plot.maxX, y: plot.minY))
        p.close()
        (mode == .point ? channelColor : Theme.Palette.plotLine).withAlphaComponent(0.13).setFill()
        p.fill()
    }

    private func curvePath(_ f: (Double) -> Double) -> NSBezierPath {
        let p = NSBezierPath()
        let n = max(Int(plot.width), 64)
        for i in 0...n {
            let x = Double(i) / Double(n)
            let q = point(x, min(max(f(x), 0), 1))
            i == 0 ? p.move(to: q) : p.line(to: q)
        }
        p.lineWidth = 1.5
        return p
    }

    private func drawParametric(in plot: NSRect) {
        let edges = [0] + parametric.splits.map { $0 / 100 } + [1]
        // Region under the pointer or being dragged.
        let active = dragRegion?.index ?? hover.flatMap { plot.contains($0) ? region(at: $0) : nil }
        if let i = active {
            Theme.Palette.plotGrid.setFill()
            NSRect(x: plot.minX + plot.width * edges[i], y: plot.minY,
                   width: plot.width * (edges[i + 1] - edges[i]), height: plot.height).fill()
        }
        Theme.Palette.plotLine.setStroke()
        curvePath(parametric.evaluate).stroke()
        // Split handles below the plot.
        for (i, s) in parametric.splits.enumerated() {
            let x = plot.minX + plot.width * s / 100
            let tri = NSBezierPath()
            tri.move(to: CGPoint(x: x, y: bounds.minY + splitStrip - 1))
            tri.line(to: CGPoint(x: x - 5, y: bounds.minY + 4))
            tri.line(to: CGPoint(x: x + 5, y: bounds.minY + 4))
            tri.close()
            (dragSplit?.index == i ? Theme.Palette.accent : Theme.Palette.plotText).setFill()
            tri.fill()
            let tick = NSBezierPath()
            tick.move(to: CGPoint(x: x, y: plot.minY)); tick.line(to: CGPoint(x: x, y: plot.maxY))
            Theme.Palette.plotGrid.setStroke()
            tick.stroke()
        }
    }

    private func drawPoint(in plot: NSRect) {
        if parametric.amounts.contains(where: { $0 != 0 }) {
            Theme.Palette.plotGuide.setStroke()
            curvePath(parametric.evaluate).stroke()
        }
        channelColor.setStroke()
        curvePath(curve.evaluate).stroke()
        for (i, k) in curve.knots.enumerated() {
            let c = point(k.x, k.y)
            let r = NSRect(x: c.x - 4, y: c.y - 4, width: 8, height: 8)
            let dot = NSBezierPath(ovalIn: r)
            if i == selected {
                Theme.Palette.accent.setFill(); dot.fill()
            } else {
                Theme.Palette.plotWell.setFill(); dot.fill()
                channelColor.setStroke(); dot.lineWidth = 1.5; dot.stroke()
            }
        }
    }

    private func region(at p: CGPoint) -> Int {
        let x = Double((p.x - plot.minX) / plot.width) * 100
        return parametric.splits.firstIndex { x < $0 } ?? 3
    }

    // MARK: Mouse

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let t = NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                               owner: self, userInfo: nil)
        addTrackingArea(t)
        tracking = t
    }

    override func mouseMoved(with event: NSEvent) {
        hover = convert(event.locationInWindow, from: nil)
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        hover = nil
        needsDisplay = true
    }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        let p = convert(event.locationInWindow, from: nil)
        switch mode {
        case .parametric:
            if p.y < plot.minY {
                let x = Double((p.x - plot.minX) / plot.width) * 100
                if let i = parametric.splits.indices.min(by: { abs(parametric.splits[$0] - x) < abs(parametric.splits[$1] - x) }),
                   abs(parametric.splits[i] - x) < 6 {
                    dragSplit = (i, parametric.splits[i], p.x)
                }
            } else if plot.contains(p) {
                let i = region(at: p)
                if event.clickCount == 2 {
                    parametric.amounts[i] = 0
                    onRegion?(i, 0, true)
                } else {
                    dragRegion = (i, parametric.amounts[i], p.y)
                }
            }
        case .point:
            window?.makeFirstResponder(self)
            let v = value(p)
            let radius = Double(7 / plot.width)
            if let i = curve.hit(v, radius: radius) {
                if event.clickCount == 2 {
                    curve.remove(i)
                    selected = nil
                    onCurve?(curve, true)
                    return
                }
                selected = i
                dragKnot = i
            } else if plot.insetBy(dx: -4, dy: -4).contains(p) {
                let onCurve = abs(point(v.x, curve.evaluate(v.x)).y - p.y) < 12
                if let i = curve.insert(x: v.x, y: onCurve ? nil : v.y) {
                    selected = i
                    dragKnot = i
                    self.onCurve?(curve, false)
                }
            }
        }
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        hover = p
        let fine: CGFloat = event.modifierFlags.contains(.option) ? 0.25 : 1
        if let d = dragRegion {
            let v = min(max((d.start + Double((p.y - d.y) * fine * 200 / plot.height)).rounded(), -100), 100)
            parametric.amounts[d.index] = v
            onRegion?(d.index, v, false)
        } else if let d = dragSplit {
            parametric.setSplit(d.index, d.start + Double((p.x - d.x) * fine / plot.width * 100))
            onSplits?(parametric, false)
        } else if let i = dragKnot {
            let v = value(p)
            curve.move(i, to: CurveKnot(v.x, v.y))
            onCurve?(curve, false)
        }
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        if let d = dragRegion { onRegion?(d.index, parametric.amounts[d.index], true) }
        if dragSplit != nil { onSplits?(parametric, true) }
        if dragKnot != nil { onCurve?(curve, true) }
        dragRegion = nil
        dragSplit = nil
        dragKnot = nil
        needsDisplay = true
    }

    // MARK: Keys (point nudge)

    private var keyCommit: DispatchWorkItem?

    /// Key edits render at once; a pause of 0.6 s makes the burst one undo step.
    private func keyEdit() {
        onCurve?(curve, false)
        keyCommit?.cancel()
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated { guard let self else { return }; self.onCurve?(self.curve, true) }
        }
        keyCommit = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6, execute: work)
    }

    func handleDevelopKey(_ event: NSEvent) -> Bool {
        guard mode == .point, isEnabled else { return false }
        let step = event.modifierFlags.contains(.shift) ? 10.0 / 255 : 1.0 / 255
        switch event.keyCode {
        case 123, 124, 125, 126:
            guard let i = selected else { return false }
            let (dx, dy): (Double, Double) = switch event.keyCode {
            case 123: (-step, 0); case 124: (step, 0); case 125: (0, -step); default: (0, step)
            }
            curve.nudge(i, dx: dx, dy: dy)
            keyEdit()
            return true
        case 51, 117:
            guard let i = selected else { return false }
            curve.remove(i)
            selected = nil
            keyEdit()
            return true
        case 48:   // Tab: next knot
            selected = ((selected ?? -1) + 1) % curve.knots.count
            needsDisplay = true
            return true
        case 53:
            selected = nil
            window?.makeFirstResponder(nil)
            needsDisplay = true
            return true
        default:
            return false
        }
    }
}
