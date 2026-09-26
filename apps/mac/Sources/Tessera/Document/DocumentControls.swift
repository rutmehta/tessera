import AppKit
import SwiftUI
import TesseraCore

/// A `ValueSlider` for document panels: the drag calls `onChange(value, final)` directly (no
/// SwiftUI state on the hot path); SwiftUI only pushes a new value when `revision` changes and
/// the slider is not being dragged.
struct DocSlider: NSViewRepresentable {
    let title: String
    let value: Double
    let range: ClosedRange<Double>
    var defaultValue: Double
    var format = "%.0f"
    var step: Double = 1
    var enabled = true
    var identifier: String
    let revision: Int
    let onChange: (Double, Bool) -> Void

    final class Coordinator { var onChange: ((Double, Bool) -> Void)? }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        let c = context.coordinator
        s.onChange = { v, final in c.onChange?(v, final) }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        _ = revision
        context.coordinator.onChange = onChange
        s.title = title
        s.minValue = range.lowerBound
        s.maxValue = range.upperBound
        s.defaultValue = defaultValue
        s.valueFormat = format
        s.step = step
        s.isEnabled = enabled
        s.setAccessibilityIdentifier(identifier)
        if !s.isDragging { s.doubleValue = value }
        s.needsDisplay = true
    }
}

/// `NSColorWell` for fill colours (straight RGB in the document's encoding).
struct DocColorWell: NSViewRepresentable {
    let rgb: [Double]
    var identifier: String
    let onChange: ([Double]) -> Void

    final class Coordinator: NSObject {
        var onChange: (([Double]) -> Void)?
        var updating = false
        @MainActor @objc func changed(_ sender: NSColorWell) {
            guard !updating, let c = sender.color.usingColorSpace(.sRGB) else { return }
            onChange?([Double(c.redComponent), Double(c.greenComponent), Double(c.blueComponent)])
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSColorWell {
        let w = NSColorWell(style: .minimal)
        w.target = context.coordinator
        w.action = #selector(Coordinator.changed(_:))
        return w
    }

    func updateNSView(_ w: NSColorWell, context: Context) {
        context.coordinator.onChange = onChange
        w.setAccessibilityIdentifier(identifier)
        guard rgb.count >= 3 else { return }
        let color = NSColor(srgbRed: rgb[0], green: rgb[1], blue: rgb[2], alpha: 1)   // lint:allow (fill colour is document data)
        if w.color.usingColorSpace(.sRGB) != color {
            context.coordinator.updating = true
            w.color = color
            context.coordinator.updating = false
        }
    }
}

/// Swatch of a document colour (gradient stops).
func documentColor(_ rgba: [Double]) -> Color {
    guard rgba.count >= 3 else { return Theme.clear }
    let ns = NSColor(srgbRed: rgba[0], green: rgba[1], blue: rgba[2], alpha: rgba.count > 3 ? rgba[3] : 1)   // lint:allow (document data)
    return Color(nsColor: ns)
}

/// Point curve editor for a Curves adjustment channel (reuses the develop `CurveEditorView` in
/// point mode; the axes are the document's encoded 0…1 values).
struct DocCurveEditor: NSViewRepresentable {
    let points: [[Double]]
    let channel: CurveChannel
    let revision: Int
    let onChange: ([[Double]], Bool) -> Void

    final class Coordinator { var onChange: (([[Double]], Bool) -> Void)? }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> CurveEditorView {
        let v = CurveEditorView(frame: .zero)
        v.mode = .point
        let c = context.coordinator
        v.onCurve = { curve, final in c.onChange?(curve.knots.map { [$0.x, $0.y] }, final) }
        v.setAccessibilityIdentifier("document.properties.curves.editor")
        return v
    }

    func updateNSView(_ v: CurveEditorView, context: Context) {
        _ = revision
        context.coordinator.onChange = onChange
        v.channel = channel
        guard !v.isInteracting else { return }
        v.curve = PointCurve(json: points.map { ["x": $0[0], "y": $0[1]] })
    }
}
