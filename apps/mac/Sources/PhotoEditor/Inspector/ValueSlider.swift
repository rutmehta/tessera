import AppKit
import QuartzCore

/// Custom-drawn slider control for the develop panels. It is an `NSControl` so each drag step is a
/// synchronous target/action call straight into the model / renderer: no SwiftUI state change, no
/// view-graph diff, which keeps slider → pixels well inside the 16 ms budget (docs/08 §1).
///
/// Drag anywhere on the track (relative, not jump-to-click), ⌥ for fine control, double-click to reset.
/// Value text is drawn by the control itself so it can update every frame without layout.
@MainActor
final class ValueSlider: NSControl {
    var title = "" { didSet { needsDisplay = true } }
    var minValue: Double = -100
    var maxValue: Double = 100
    var defaultValue: Double = 0
    var valueFormat = "%+.0f"
    var step: Double = 1
    /// Called for every value change, with `isFinal` true on mouse-up (commit / history point).
    var onChange: ((Double, Bool) -> Void)?

    private var value: Double = 0
    private var dragStartValue: Double = 0
    private var dragStartX: CGFloat = 0
    private var isDragging = false

    override var doubleValue: Double {
        get { value }
        set {
            let v = min(max(newValue, minValue), maxValue)
            if v != value { value = v; needsDisplay = true; updateAccessibility() }
        }
    }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        isContinuous = true
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        setAccessibilityElement(true)
        setAccessibilityRole(.slider)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var intrinsicContentSize: NSSize { NSSize(width: NSView.noIntrinsicMetric, height: 30) }
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { false }   // culling keys stay global
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private var trackRect: NSRect { NSRect(x: 0, y: bounds.height - 9, width: bounds.width, height: 3) }

    private func fraction(_ v: Double) -> CGFloat { CGFloat((v - minValue) / (maxValue - minValue)) }

    private static let titleAttrs: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: 11), .foregroundColor: Theme.textPrimary]
    private static let valueAttrs: [NSAttributedString.Key: Any] = [
        .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .regular), .foregroundColor: Theme.textSecondary]
    private static let valueAttrsActive: [NSAttributedString.Key: Any] = [
        .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium), .foregroundColor: Theme.textPrimary]

    override func draw(_ dirtyRect: NSRect) {
        (title as NSString).draw(at: NSPoint(x: 0, y: 1), withAttributes: Self.titleAttrs)
        let text = String(format: valueFormat, abs(value) < step / 2 ? 0 : value) as NSString
        let attrs = (isDragging || value != defaultValue) ? Self.valueAttrsActive : Self.valueAttrs
        let tw = text.size(withAttributes: attrs).width
        text.draw(at: NSPoint(x: bounds.width - tw, y: 1), withAttributes: attrs)

        let track = trackRect
        NSColor(calibratedWhite: 0.26, alpha: 1).setFill()
        NSBezierPath(roundedRect: track, xRadius: 1.5, yRadius: 1.5).fill()

        // Fill from the default (centre for bipolar controls) to the value.
        let x0 = track.minX + track.width * fraction(defaultValue)
        let x1 = track.minX + track.width * fraction(value)
        NSColor(calibratedWhite: isDragging ? 0.85 : 0.62, alpha: 1).setFill()
        NSRect(x: min(x0, x1), y: track.minY, width: abs(x1 - x0), height: track.height).fill()

        // Thumb: a slim vertical bar, not a knob.
        let thumb = NSRect(x: x1 - 1.5, y: track.midY - 5, width: 3, height: 10)
        (isDragging ? Theme.accent : NSColor(calibratedWhite: 0.9, alpha: 1)).setFill()
        NSBezierPath(roundedRect: thumb, xRadius: 1, yRadius: 1).fill()
    }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        if event.clickCount == 2 {
            setValue(defaultValue, final: true)
            return
        }
        isDragging = true
        dragStartValue = value
        dragStartX = convert(event.locationInWindow, from: nil).x
        // Clicking on the track (not near the thumb) jumps there, then drags relatively.
        let p = convert(event.locationInWindow, from: nil)
        let thumbX = trackRect.minX + trackRect.width * fraction(value)
        if p.y > bounds.height - 16, abs(p.x - thumbX) > 6 {
            let v = minValue + Double((p.x - trackRect.minX) / trackRect.width) * (maxValue - minValue)
            dragStartValue = quantize(v)
            setValue(dragStartValue, final: false)
        }
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard isDragging else { return }
        let x = convert(event.locationInWindow, from: nil).x
        let fine = event.modifierFlags.contains(.option) ? 0.1 : 1.0
        let dv = Double((x - dragStartX) / max(trackRect.width, 1)) * (maxValue - minValue) * fine
        setValue(quantize(dragStartValue + dv), final: false)
    }

    override func mouseUp(with event: NSEvent) {
        guard isDragging else { return }
        isDragging = false
        setValue(value, final: true)
        needsDisplay = true
    }

    private func quantize(_ v: Double) -> Double { (v / step).rounded() * step }

    private func setValue(_ v: Double, final: Bool) {
        let old = value
        doubleValue = v
        if value != old || final {
            onChange?(value, final)
            if let action { sendAction(action, to: target) }
            // Present now rather than at the end of the runloop turn.
            if !final { displayIfNeeded() }
        }
    }

    private func updateAccessibility() {
        setAccessibilityValue(String(format: valueFormat, value))
        setAccessibilityLabel(title)
    }

    override func accessibilityPerformIncrement() -> Bool { setValue(value + step * 10, final: true); return true }
    override func accessibilityPerformDecrement() -> Bool { setValue(value - step * 10, final: true); return true }
}
