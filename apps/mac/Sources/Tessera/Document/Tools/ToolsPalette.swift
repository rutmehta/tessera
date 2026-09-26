import AppKit
import SwiftUI
import TesseraCore

/// The tools palette (WP B5-04): a vertical HUD column on the left of the canvas, one slot per tool
/// group in Photoshop's order (the slot shows the group's current tool; right-click lists the group,
/// ⇧ + its key cycles it), then the foreground / background swatches with swap (X) and default (D).
struct ToolsPalette: View {
    @Bindable var document: DocumentController
    @Bindable var tools: DocumentTools

    var body: some View {
        VStack(spacing: Theme.Space.xxs) {
            ForEach(DocumentTool.paletteSlots, id: \.self) { slot in
                let shown = slot.group.contains(document.tool) ? document.tool : slot
                IconButton(symbol: shown.symbol, help: "\(shown.title) (\(shown.key))", on: slot.group.contains(document.tool),
                           size: Theme.Height.large) { tools.select(shown) }
                    .contextMenu {
                        if slot.group.count > 1 {
                            ForEach(slot.group, id: \.self) { t in
                                Button("\(t.title)    (\(t.key))") { tools.select(t) }
                            }
                        }
                    }
                    .accessibilityIdentifier("document.tool.\(shown.rawValue)")
            }
            Hairline().frame(width: Theme.Height.large).padding(.vertical, Theme.Space.xxs)
            ColorSwatches(tools: tools)
        }
        .padding(Theme.Space.xxs)
        .background(HUDBackground())
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("document.tools")
    }
}

/// Foreground over background; a click opens the colour panel for that swatch.
struct ColorSwatches: View {
    @Bindable var tools: DocumentTools

    private func swatch(_ c: ToolColor) -> some View {
        RoundedRectangle(cornerRadius: Theme.Radius.chip)
            .fill(Color(red: Double(c.r), green: Double(c.g), blue: Double(c.b)))   // lint:allow (user colour)
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
            .frame(width: Theme.Height.small, height: Theme.Height.small)
    }

    var body: some View {
        VStack(spacing: Theme.Space.xxs) {
            ZStack(alignment: .topLeading) {
                swatch(tools.colors.background)
                    .offset(x: Theme.Space.s, y: Theme.Space.s)
                    .onTapGesture { ColorPanelBridge.shared.edit(tools.colors.background) { tools.colors.background = $0 } }
                    .help("Background colour \(tools.colors.background.hex)")
                swatch(tools.colors.foreground)
                    .onTapGesture { ColorPanelBridge.shared.edit(tools.colors.foreground) { tools.colors.foreground = $0 } }
                    .help("Foreground colour \(tools.colors.foreground.hex)")
            }
            .frame(width: Theme.Height.large, height: Theme.Height.large, alignment: .topLeading)
            .accessibilityIdentifier("document.colors")
            HStack(spacing: 0) {
                IconButton(symbol: "arrow.left.arrow.right", help: "Swap colours (X)", size: Theme.Height.small) { tools.colors.swap() }
                IconButton(symbol: "circle.lefthalf.filled", help: "Default colours (D)", size: Theme.Height.small) { tools.colors.reset() }
            }
        }
    }
}

/// Routes the shared colour panel to one swatch at a time.
@MainActor
final class ColorPanelBridge: NSObject {
    static let shared = ColorPanelBridge()
    private var apply: ((ToolColor) -> Void)?

    func edit(_ c: ToolColor, apply: @escaping (ToolColor) -> Void) {
        self.apply = apply
        let panel = NSColorPanel.shared
        panel.showsAlpha = false
        panel.setTarget(self)
        panel.setAction(#selector(changed(_:)))
        panel.color = NSColor(srgbRed: CGFloat(c.r), green: CGFloat(c.g), blue: CGFloat(c.b), alpha: 1)   // lint:allow (user colour)
        panel.orderFront(nil)
    }

    @objc private func changed(_ sender: NSColorPanel) {
        guard let c = sender.color.usingColorSpace(.sRGB) else { return }
        apply?(ToolColor(r: Float(c.redComponent), g: Float(c.greenComponent), b: Float(c.blueComponent)))
    }
}

/// A compact numeric field for the options bar: caption, value, unit.
struct OptionField: View {
    let title: String
    @Binding var value: Double
    var range: ClosedRange<Double>
    var unit = ""
    var fractionDigits = 0
    var width: CGFloat = Theme.Width.label - Theme.Space.l
    var identifier: String?

    private var formatter: NumberFormatter {
        let f = NumberFormatter()
        f.numberStyle = .decimal
        f.minimumFractionDigits = 0
        f.maximumFractionDigits = fractionDigits
        f.minimum = NSNumber(value: range.lowerBound)
        f.maximum = NSNumber(value: range.upperBound)
        return f
    }

    var body: some View {
        HStack(spacing: Theme.Space.xs) {
            Text(title).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            TextField(title, value: $value, formatter: formatter)
                .textFieldStyle(.roundedBorder)
                .controlSize(.small)
                .font(Theme.Fonts.captionNumeric)
                .multilineTextAlignment(.trailing)
                .frame(width: width)
                .labelsHidden()
                .accessibilityIdentifier(identifier ?? "document.option.\(title.lowercased())")
            if !unit.isEmpty { Text(unit).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary) }
        }
        .fixedSize()
    }
}

/// A small labelled checkbox for the options bar.
struct OptionToggle: View {
    let title: String
    @Binding var on: Bool

    var body: some View {
        Toggle(title, isOn: $on)
            .toggleStyle(.checkbox)
            .controlSize(.small)
            .font(Theme.Fonts.caption)
            .foregroundStyle(Theme.textSecondary)
            .fixedSize()
    }
}

/// The options bar (WP B5-04): a HUD bar across the top of the canvas with the current tool's options.
struct ToolOptionsBar: View {
    @Bindable var document: DocumentController
    @Bindable var tools: DocumentTools

    private func percent(_ get: @escaping () -> Float, _ set: @escaping (Float) -> Void) -> Binding<Double> {
        Binding(get: { Double(get() * 100) }, set: { set(Float($0 / 100)) })
    }

    private func brushField(_ key: WritableKeyPath<BrushOptions, Float>, scale: Double = 1) -> Binding<Double> {
        Binding(get: { Double(tools.currentBrush[keyPath: key]) * scale },
                set: { var b = tools.currentBrush; b[keyPath: key] = Float($0 / scale); tools.currentBrush = b })
    }

    private func brushFlag(_ key: WritableKeyPath<BrushOptions, Bool>) -> Binding<Bool> {
        Binding(get: { tools.currentBrush[keyPath: key] },
                set: { var b = tools.currentBrush; b[keyPath: key] = $0; tools.currentBrush = b })
    }

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Height.small) }

    var body: some View {
        // Scrolls sideways when the window is narrower than the options (never widens the canvas).
        ScrollView(.horizontal, showsIndicators: false) { bar }
            .frame(height: Theme.Height.large + Theme.Space.xs)
            .background(HUDBackground())
            .fixedSize(horizontal: false, vertical: true)
    }

    private var bar: some View {
        HStack(spacing: Theme.Space.s) {
            Image(systemName: tools.transform != nil ? "arrow.up.left.and.arrow.down.right" : document.tool.symbol)
                .font(Theme.Fonts.icon).foregroundStyle(Theme.textSecondary)
            Text(tools.transform != nil ? "Free Transform" : document.tool.title)
                .font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary).fixedSize()
            separator
            if tools.transform != nil {
                transformOptions
            } else if document.tool.selects {
                selectionOptions
            } else if document.tool.paints {
                paintOptions
            } else {
                otherOptions
            }
        }
        .padding(.horizontal, Theme.Space.m)
        .frame(height: Theme.Height.large + Theme.Space.xs)
        .tint(Theme.accent)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("document.optionsBar")
    }

    @ViewBuilder private var selectionOptions: some View {
        SegmentedPicker(selection: $tools.selectionMode, segments: SelectionCombine.allCases.map {
            .init(value: $0, title: "", symbol: $0.symbol, help: $0.title + ($0 == .add ? " (⇧)" : $0 == .subtract ? " (⌥)" : $0 == .intersect ? " (⇧⌥)" : ""))
        }, height: Theme.Height.small, fill: false)
        .fixedSize()
        separator
        switch document.tool {
        case .wand:
            OptionField(title: "Tolerance", value: Binding(get: { Double(tools.tolerance) }, set: { tools.tolerance = Float($0) }),
                        range: 0...255)
            OptionToggle(title: "Anti-alias", on: $tools.antialias)
            OptionToggle(title: "Contiguous", on: $tools.contiguous)
            OptionToggle(title: "Sample All Layers", on: $tools.sampleAllLayers)
        case .quickSelect:
            OptionField(title: "Size", value: Binding(get: { Double(tools.quickSize) }, set: { tools.quickSize = Float($0) }),
                        range: 1...2000, unit: "px")
            OptionToggle(title: "Sample All Layers", on: $tools.sampleAllLayers)
        case .objectSelect:
            Text("Click an object").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
        default:
            OptionField(title: "Feather", value: Binding(get: { Double(tools.feather) }, set: { tools.feather = Float($0) }),
                        range: 0...250, unit: "px", fractionDigits: 1)
            OptionToggle(title: "Anti-alias", on: $tools.antialias)
            if document.tool == .polygonLasso || document.tool == .magneticLasso {
                Text("Click points · double-click or Return closes · Esc cancels")
                    .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
            }
        }
        separator
        Button("Select Subject") { tools.selectSubject() }
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
        Button("Select and Mask…") { tools.sheet = .refineEdge }
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            .disabled(document.marquee == nil)
    }

    @ViewBuilder private var paintOptions: some View {
        BrushPresetButton(tools: tools)
        OptionField(title: "Size", value: brushField(\.size), range: 1...5000, unit: "px", identifier: "document.option.size")
        OptionField(title: "Hardness", value: brushField(\.hardness, scale: 100), range: 0...100, unit: "%")
        if document.tool == .brush {
            MenuPicker(selection: brushBlend, options: DocBlendMode.allCases.map { ($0.backendName, $0.title) })
                .frame(width: Theme.Width.labelWide + Theme.Space.xxl)
                .help("Mode")
        }
        OptionField(title: "Opacity", value: brushField(\.opacity, scale: 100), range: 1...100, unit: "%")
        IconButton(symbol: "hand.point.up.left", help: "Pressure controls opacity", on: tools.currentBrush.pressureOpacity,
                   size: Theme.Height.small) { var b = tools.currentBrush; b.pressureOpacity.toggle(); tools.currentBrush = b }
        OptionField(title: "Flow", value: brushField(\.flow, scale: 100), range: 1...100, unit: "%")
        OptionField(title: "Smoothing", value: brushField(\.smoothing), range: 0...200, unit: "px")
        IconButton(symbol: "circle.circle", help: "Pressure controls size", on: tools.currentBrush.pressureSize,
                   size: Theme.Height.small) { var b = tools.currentBrush; b.pressureSize.toggle(); tools.currentBrush = b }
        Menu {
            Picker("Symmetry", selection: brushSymmetry) {
                ForEach(BrushSymmetry.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.inline)
        } label: {
            Label("Symmetry", systemImage: "square.split.2x1")
        }
        .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
        .fixedSize()
        .help("Paint Symmetry: \(tools.currentBrush.symmetry.title)")
        if document.tool == .cloneStamp || document.tool == .heal {
            OptionToggle(title: "Sample All Layers", on: brushFlag(\.sampleAllLayers))
            Text(tools.cloneSource == nil ? "⌥-click sets the source" : "Aligned").font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textTertiary).fixedSize()
        }
        if document.primary?.hasMask == true, document.primary?.kind == .pixel {
            OptionToggle(title: "Paint Mask", on: $tools.paintMask)
        }
    }

    private var brushBlend: Binding<String> {
        Binding(get: { tools.currentBrush.blendMode }, set: { var b = tools.currentBrush; b.blendMode = $0; tools.currentBrush = b })
    }

    private var brushSymmetry: Binding<BrushSymmetry> {
        Binding(get: { tools.currentBrush.symmetry }, set: { var b = tools.currentBrush; b.symmetry = $0; tools.currentBrush = b })
    }

    @ViewBuilder private var transformOptions: some View {
        let t = tools.transform
        OptionField(title: "X", value: Binding(get: { Double(t?.referenceNow.x ?? 0) }, set: { v in
            tools.updateTransform { m in m.tx += v - m.referenceNow.x }
        }), range: -100_000...100_000, unit: "px", fractionDigits: 1)
        OptionField(title: "Y", value: Binding(get: { Double(t?.referenceNow.y ?? 0) }, set: { v in
            tools.updateTransform { m in m.ty += v - m.referenceNow.y }
        }), range: -100_000...100_000, unit: "px", fractionDigits: 1)
        OptionField(title: "W", value: Binding(get: { t?.widthPercent ?? 100 }, set: { v in tools.updateTransform { $0.sx = v / 100 } }),
                    range: -10_000...10_000, unit: "%", fractionDigits: 1)
        OptionField(title: "H", value: Binding(get: { t?.heightPercent ?? 100 }, set: { v in tools.updateTransform { $0.sy = v / 100 } }),
                    range: -10_000...10_000, unit: "%", fractionDigits: 1)
        OptionField(title: "Angle", value: Binding(get: { t?.angle ?? 0 }, set: { v in tools.updateTransform { $0.angle = v } }),
                    range: -180...180, unit: "°", fractionDigits: 1)
        OptionField(title: "H skew", value: Binding(get: { t?.skewX ?? 0 }, set: { v in tools.updateTransform { $0.skewX = v } }),
                    range: -89...89, unit: "°", fractionDigits: 1)
        OptionField(title: "V skew", value: Binding(get: { t?.skewY ?? 0 }, set: { v in tools.updateTransform { $0.skewY = v } }),
                    range: -89...89, unit: "°", fractionDigits: 1)
        MenuPicker(selection: $tools.transformInterpolation, options: ResampleMode.allCases.map { ($0, $0.title) })
            .frame(width: Theme.Width.labelWide + Theme.Space.l)
            .help("Interpolation")
        separator
        Button("Cancel") { tools.cancelTransform() }
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            .help("Esc")
        Button("Commit") { tools.commitTransform() }
            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            .help("Return")
            .accessibilityIdentifier("document.transform.commit")
    }

    @ViewBuilder private var otherOptions: some View {
        switch document.tool {
        case .eyedropper:
            MenuPicker(selection: $tools.eyedropperRadius, options: [(UInt32(0), "Point Sample"), (1, "3 by 3 Average"), (2, "5 by 5 Average")])
                .frame(width: Theme.Width.labelWide + Theme.Space.xl)
            OptionToggle(title: "Sample All Layers", on: $tools.eyedropperAllLayers)
            Text("⌥-click sets the background colour").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
        case .move:
            Text("Drag to move the selected layers · ⌘T Free Transform").font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textTertiary).fixedSize()
        case .hand:
            Button("Fit on Screen") { document.viewport?.zoomToFit() }.buttonStyle(.theme(.borderless, height: Theme.Height.small))
            Button("100 %") { document.viewport?.zoomActual() }.buttonStyle(.theme(.borderless, height: Theme.Height.small))
        case .zoom:
            Text("Click zooms in · ⌥-click zooms out").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
        case .gradient:
            Text("Placeholder: a click fills the selection with the foreground colour").font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textTertiary).fixedSize()
        default:
            Text("Placeholder in this build").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
        }
    }
}

/// The brush tip in the options bar; the popover is the brush preset picker.
struct BrushPresetButton: View {
    @Bindable var tools: DocumentTools
    @State private var open = false

    var body: some View {
        Button { open.toggle() } label: {
            HStack(spacing: Theme.Space.xs) {
                if let img = tools.tipImage(tools.currentBrush.tipId ?? "round:\(tools.currentBrush.hardness)", px: 32) {
                    Image(nsImage: img).foregroundStyle(Theme.textPrimary)
                        .frame(width: Theme.Height.chip, height: Theme.Height.chip)
                }
                Text(String(format: "%.0f", tools.currentBrush.size)).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                Image(systemName: "chevron.down").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
            }
        }
        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
        .help("Brush presets")
        .popover(isPresented: $open, arrowEdge: .bottom) {
            BrushesList(tools: tools)
                .frame(width: Theme.Width.inspectorMin)
                .padding(Theme.Space.m)
                .background(Theme.panel)
        }
    }
}
