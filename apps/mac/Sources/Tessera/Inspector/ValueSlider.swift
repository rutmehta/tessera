import AppKit
import QuartzCore

/// Custom-drawn slider control for the develop panels. It is an `NSControl` so each drag step is a
/// synchronous target/action call straight into the model / renderer: no SwiftUI state change, no
/// view-graph diff, which keeps slider → pixels well inside the 16 ms budget (docs/08 §1).
///
/// Drag anywhere on the track (relative, not jump-to-click), ⌥ for fine control, double-click to reset.
/// Value text is drawn by the control itself so it can update every frame without layout.
///
/// Anatomy (DESIGN.md §4.3): 32 pt row; label (11 pt, secondary) and value (11 pt tabular, right
/// aligned) on the first line; a 2 pt track with the fill running from the default to the value;
/// a 12 pt round thumb with an accent ring while dragging.
@MainActor
final class ValueSlider: NSControl, KeyOwningControl {
    var title = "" { didSet { needsDisplay = true } }
    var minValue: Double = -100
    var maxValue: Double = 100
    var defaultValue: Double = 0
    var valueFormat = "%+.0f"
    var step: Double = 1
    /// Called for every value change, with `isFinal` true on mouse-up (commit / history point).
    var onChange: ((Double, Bool) -> Void)?
    /// Track painted as a gradient (HSL hue/saturation/luminance sliders) instead of grey.
    var trackColors: [NSColor]? { didSet { needsDisplay = true } }
    /// Drag lifecycle with the modifiers at mouse-down (⌥ previews on the Masking slider).
    var onDragBegan: ((NSEvent.ModifierFlags) -> Void)?
    var onDragEnded: (() -> Void)?

    private var value: Double = 0
    private var dragStartValue: Double = 0
    private var dragStartX: CGFloat = 0
    private(set) var isDragging = false
    private var keyboardEditing = false

    override var isEnabled: Bool {
        didSet { alphaValue = isEnabled ? 1 : CGFloat(Theme.Opacity.disabled) }
    }

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

    override var intrinsicContentSize: NSSize { NSSize(width: NSView.noIntrinsicMetric, height: Theme.Height.slider) }
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { isEnabled }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        needsDisplay = true
        return accepted
    }

    override func resignFirstResponder() -> Bool {
        commitKeyboardEdit()
        let accepted = super.resignFirstResponder()
        needsDisplay = true
        return accepted
    }

    private func commitKeyboardEdit() {
        guard keyboardEditing else { return }
        keyboardEditing = false
        setValue(value, final: true)
    }

    override func keyDown(with event: NSEvent) {
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard isEnabled, !mods.contains(.command), !mods.contains(.control) else {
            super.keyDown(with: event); return
        }
        let amount = step * (mods.contains(.option) ? 0.1 : mods.contains(.shift) ? 10 : 1)
        let next: Double
        switch event.keyCode {
        case 123, 125: next = value - amount
        case 124, 126: next = value + amount
        case 115: next = minValue   // Home
        case 119: next = maxValue   // End
        case 36, 76, 53:
            commitKeyboardEdit()
            window?.makeFirstResponder(nil)
            return
        default: super.keyDown(with: event); return
        }
        let before = value
        setValue(next, final: false)
        if value != before { keyboardEditing = true }
    }

    private static let thumb = Theme.Height.thumb
    /// Track: full width minus half a thumb each side, centred on the thumb row.
    private var trackRect: NSRect {
        let inset = Self.thumb / 2
        let midY = bounds.height - Self.thumb / 2 - Theme.Space.xxs
        return NSRect(x: inset, y: midY - 1, width: max(bounds.width - 2 * inset, 1), height: Theme.Space.xxs)
    }

    private func fraction(_ v: Double) -> CGFloat { CGFloat((v - minValue) / (maxValue - minValue)) }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        if window?.firstResponder === self {
            let focus = NSBezierPath(roundedRect: bounds.insetBy(dx: Theme.Space.hairline, dy: Theme.Space.hairline),
                                     xRadius: Theme.Radius.control, yRadius: Theme.Radius.control)
            focus.lineWidth = Theme.Space.hairline
            Theme.Palette.accent.setStroke()
            focus.stroke()
        }
        let changed = isDragging || value != defaultValue
        let titleAttrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.caption,
                                                         .foregroundColor: Theme.Palette.textSecondary]
        let valueAttrs: [NSAttributedString.Key: Any] = [
            .font: changed ? Theme.NSFonts.captionNumericMedium : Theme.NSFonts.captionNumeric,
            .foregroundColor: changed ? Theme.Palette.textPrimary : Theme.Palette.textTertiary]
        (title as NSString).draw(at: NSPoint(x: 0, y: 0), withAttributes: titleAttrs)
        let text = String(format: valueFormat, abs(value) < step / 2 ? 0 : value) as NSString
        let tw = ceil(text.size(withAttributes: valueAttrs).width)
        text.draw(at: NSPoint(x: bounds.width - tw, y: 0), withAttributes: valueAttrs)

        let track = trackRect
        let x0 = track.minX + track.width * fraction(defaultValue)
        let x1 = track.minX + track.width * fraction(value)
        let radius = track.height / 2
        if let colors = trackColors, colors.count >= 2, let gradient = NSGradient(colors: colors) {
            gradient.draw(in: NSBezierPath(roundedRect: track.insetBy(dx: 0, dy: -0.5), xRadius: radius + 0.5, yRadius: radius + 0.5), angle: 0)
        } else {
            Theme.Palette.hairlineStrong.setFill()
            NSBezierPath(roundedRect: track, xRadius: radius, yRadius: radius).fill()
            // Fill from the default (centre for bipolar controls) to the value.
            (isDragging ? Theme.Palette.accent : Theme.Palette.textSecondary).setFill()
            NSRect(x: min(x0, x1), y: track.minY, width: abs(x1 - x0), height: track.height).fill()
        }
        // Default tick for bipolar controls, so "zero" is findable.
        if defaultValue > minValue, defaultValue < maxValue {
            Theme.Palette.textTertiary.setFill()
            NSRect(x: round(x0) - 0.5, y: track.minY - Theme.Space.xxs, width: 1, height: track.height + Theme.Space.xs).fill()
        }

        // Thumb: 12 pt disc with a hairline; accent ring while dragging.
        let d = Self.thumb
        let thumb = NSRect(x: x1 - d / 2, y: track.midY - d / 2, width: d, height: d)
        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow()
        shadow.shadowColor = Theme.Palette.OnImage.shadow
        shadow.shadowBlurRadius = Theme.Space.xxs
        shadow.shadowOffset = NSSize(width: 0, height: -0.5)
        shadow.set()
        Theme.Palette.thumb.setFill()
        NSBezierPath(ovalIn: thumb.insetBy(dx: 0.5, dy: 0.5)).fill()
        NSGraphicsContext.restoreGraphicsState()
        let ring = NSBezierPath(ovalIn: thumb.insetBy(dx: 0.5, dy: 0.5))
        ring.lineWidth = isDragging ? 2 : Theme.Space.hairline
        (isDragging ? Theme.Palette.accent : Theme.Palette.hairlineStrong).setStroke()
        ring.stroke()
    }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        window?.makeFirstResponder(self)
        commitKeyboardEdit()
        if event.clickCount == 2 {
            setValue(defaultValue, final: true)
            return
        }
        isDragging = true
        onDragBegan?(event.modifierFlags)
        dragStartValue = value
        dragStartX = convert(event.locationInWindow, from: nil).x
        // Clicking on the track (not near the thumb) jumps there, then drags relatively.
        let p = convert(event.locationInWindow, from: nil)
        let thumbX = trackRect.minX + trackRect.width * fraction(value)
        if p.y > bounds.height - Theme.Height.small, abs(p.x - thumbX) > Self.thumb / 2 {
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
        onDragEnded?()
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
