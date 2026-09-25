import AppKit
import TesseraCore

/// Colour-grading wheel: the disc shows the engine's OkLab hue plane (angle = hue, radius =
/// saturation), so the puck sits on the colour the grade actually adds. Drag the puck (⇧ keeps
/// the hue, ⌥ drags finely), double-click to reset.
@MainActor
final class ColorWheelView: NSView {
    var hue: Double = 0 { didSet { needsDisplay = true } }
    var saturation: Double = 0 { didSet { needsDisplay = true } }
    var isEnabled = true { didSet { alphaValue = isEnabled ? 1 : 0.4 } }
    var title = "" { didSet { setAccessibilityLabel(title) } }
    /// (hue, saturation, final).
    var onChange: ((Double, Double, Bool) -> Void)?

    private var dragging = false
    private var dragStart: (p: CGPoint, hue: Double, sat: Double)?
    private static var discCache: [Int: CGImage] = [:]

    override var isFlipped: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.slider)
    }
    required init?(coder: NSCoder) { fatalError() }

    private var disc: NSRect {
        let d = min(bounds.width, bounds.height) - 4
        return NSRect(x: bounds.midX - d / 2, y: bounds.midY - d / 2, width: d, height: d)
    }

    /// Engine hue (OkLab a/b angle, counter-clockwise from +a) and saturation (0…100 over the radius).
    private func puck() -> CGPoint {
        let r = disc.width / 2 * CGFloat(saturation / 100), a = hue * .pi / 180
        return CGPoint(x: disc.midX + r * cos(a), y: disc.midY + r * sin(a))
    }

    override func draw(_ dirtyRect: NSRect) {
        let d = disc
        let px = Int(d.width * (window?.backingScaleFactor ?? 2))
        if let img = Self.discImage(px) {
            NSGraphicsContext.current?.cgContext.draw(img, in: d)
        }
        NSColor(calibratedWhite: 0, alpha: 0.5).setStroke()
        let ring = NSBezierPath(ovalIn: d.insetBy(dx: 0.5, dy: 0.5))
        ring.lineWidth = 1
        ring.stroke()
        // Crosshair at neutral.
        let cross = NSBezierPath()
        cross.move(to: CGPoint(x: d.midX - 4, y: d.midY)); cross.line(to: CGPoint(x: d.midX + 4, y: d.midY))
        cross.move(to: CGPoint(x: d.midX, y: d.midY - 4)); cross.line(to: CGPoint(x: d.midX, y: d.midY + 4))
        NSColor(calibratedWhite: 0.1, alpha: 0.6).setStroke()
        cross.stroke()
        let p = puck()
        if saturation > 0 {
            let line = NSBezierPath()
            line.move(to: CGPoint(x: d.midX, y: d.midY)); line.line(to: p)
            NSColor(calibratedWhite: 1, alpha: 0.6).setStroke()
            line.stroke()
        }
        let r = NSRect(x: p.x - 5, y: p.y - 5, width: 10, height: 10)
        let dot = NSBezierPath(ovalIn: r)
        let c = OkLab.srgb(l: 0.72, c: 0.14 * saturation / 100, hue: hue)
        NSColor(srgbRed: c.r, green: c.g, blue: c.b, alpha: 1).setFill()
        dot.fill()
        (dragging ? Theme.accent : NSColor.white).setStroke()
        dot.lineWidth = 2
        dot.stroke()
    }

    /// The disc in OkLCh at L = 0.72, chroma rising to the rim, rendered once per pixel size.
    private static func discImage(_ size: Int) -> CGImage? {
        guard size > 8 else { return nil }
        if let img = discCache[size] { return img }
        var px = [UInt8](repeating: 0, count: size * size * 4)
        let c = Double(size) / 2
        for y in 0..<size {
            for x in 0..<size {
                let dx = Double(x) + 0.5 - c, dy = c - (Double(y) + 0.5)
                let r = hypot(dx, dy) / c
                guard r <= 1 else { continue }
                var h = atan2(dy, dx) * 180 / .pi
                if h < 0 { h += 360 }
                let rgb = OkLab.srgb(l: 0.72 - 0.08 * r, c: 0.16 * r, hue: h)
                let a = min(max((1 - r) * Double(size) / 2, 0), 1)   // antialiased rim
                let i = (y * size + x) * 4
                px[i] = UInt8(rgb.r * 255 * a); px[i + 1] = UInt8(rgb.g * 255 * a)
                px[i + 2] = UInt8(rgb.b * 255 * a); px[i + 3] = UInt8(255 * a)
            }
        }
        let img = px.withUnsafeMutableBytes { buf -> CGImage? in
            CGContext(data: buf.baseAddress, width: size, height: size, bitsPerComponent: 8, bytesPerRow: size * 4,
                      space: CGColorSpace(name: CGColorSpace.sRGB)!,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)?.makeImage()
        }
        if discCache.count > 8 { discCache.removeAll() }
        discCache[size] = img
        return img
    }

    private func set(from p: CGPoint, event: NSEvent, final: Bool) {
        let d = disc
        var h = hue, s = saturation
        if event.modifierFlags.contains(.option), let start = dragStart {
            // Fine: move the puck a quarter of the pointer distance.
            let sp = CGPoint(x: d.midX + (d.width / 2) * CGFloat(start.sat / 100) * cos(start.hue * .pi / 180),
                             y: d.midY + (d.width / 2) * CGFloat(start.sat / 100) * sin(start.hue * .pi / 180))
            let q = CGPoint(x: sp.x + (p.x - start.p.x) * 0.25, y: sp.y + (p.y - start.p.y) * 0.25)
            (h, s) = polar(q, d)
        } else {
            (h, s) = polar(p, d)
        }
        if event.modifierFlags.contains(.shift), let start = dragStart { h = start.hue }
        hue = (h * 10).rounded() / 10
        saturation = s.rounded()
        onChange?(hue, saturation, final)
    }

    private func polar(_ p: CGPoint, _ d: NSRect) -> (Double, Double) {
        let dx = Double(p.x - d.midX), dy = Double(p.y - d.midY)
        var h = atan2(dy, dx) * 180 / .pi
        if h < 0 { h += 360 }
        return (h, min(hypot(dx, dy) / Double(d.width / 2) * 100, 100))
    }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        let p = convert(event.locationInWindow, from: nil)
        if event.clickCount == 2 {
            hue = 0; saturation = 0
            onChange?(0, 0, true)
            return
        }
        dragging = true
        dragStart = (p, hue, saturation)
        // Clicking away from the puck jumps there (then drags).
        if hypot(puck().x - p.x, puck().y - p.y) > 8 { set(from: p, event: event, final: false) }
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard dragging else { return }
        set(from: convert(event.locationInWindow, from: nil), event: event, final: false)
    }

    override func mouseUp(with event: NSEvent) {
        guard dragging else { return }
        dragging = false
        onChange?(hue, saturation, true)
        dragStart = nil
        needsDisplay = true
    }
}
