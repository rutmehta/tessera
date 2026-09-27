import SwiftUI
import TesseraCore

/// The options bar while an advanced transform is open (WP B5-12), after the tool icon and name: the
/// operation's controls (compact native fields with 11 pt captions, neutral `SegmentedPicker`s,
/// `MenuPicker` pop-ups, borderless actions), the interpolation pop-up, a limitations help glyph, then
/// borderless Cancel and bordered Apply (DESIGN.md §10). Errors and hints go to the status bar.
struct TransformOptionsBar: View {
    let document: DocumentController
    @Bindable var t: DocumentTransforms

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Height.small) }

    var body: some View {
        if let s = t.session {
            switch s.op {
            case .warp: warp(s)
            case .perspective: perspective
            case .puppet(let p): puppet(p)
            case .contentAwareScale(let c): scale(c)
            }
            if s.op.tag != .contentAwareScale {
                MenuPicker(selection: $t.kernel, options: TransformKernel.allCases.map { ($0, $0.title) })
                    .frame(width: Theme.Width.labelWide + Theme.Space.l)
                    .help("Interpolation")
            }
            Image(systemName: "info.circle")
                .font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                .help(s.start.limitations.joined(separator: "\n"))
                .accessibilityLabel("Limitations")
            if let r = t.latencyReadout {
                Text(r).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary).fixedSize()
                    .accessibilityIdentifier("document.transform.latency")
            }
            separator
            Button("Reset") { t.resetOperation() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            Button("Cancel") { t.cancel() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .help("Esc")
            Button(s.start.needsConversion ? "Apply…" : "Apply") { t.apply() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .help(s.start.needsConversion ? "Return · converts the layer to a smart object" : "Return")
                .accessibilityIdentifier("document.transform.apply")
        } else {
            Text("Opening…").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
        }
    }

    private var presets: [(value: String, title: String)] {
        [("Custom", "Custom")] + TransformBridge.presetNames().map { ($0, WarpPresetInfo.title($0)) }
    }

    @ViewBuilder private func warp(_ s: DocumentTransforms.Session) -> some View {
        MenuPicker(selection: Binding(get: { t.warpPreset }, set: { t.applyWarpPreset($0) }), options: presets)
            .frame(width: Theme.Width.labelWide + Theme.Space.l)
            .help("Warp preset")
        OptionField(title: "Bend", value: Binding(get: { t.warpBend }, set: { t.setWarpBend($0) }), range: -100...100, unit: "%")
            .disabled(t.warpPreset == "Custom")
        Menu {
            Button("Default (1 × 1)") { t.update { if case .warp = $0 { $0 = .warp(.identity(width: Double(s.start.childWidth), height: Double(s.start.childHeight))) } } }
            ForEach([UInt32(3), 4, 5], id: \.self) { n in Button("\(n) × \(n)") { t.warpGrid(n) } }
        } label: { Label("Grid", systemImage: "square.grid.3x3") }
            .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
            .fixedSize()
            .help("Grid: add splits (the warp does not move)")
        SegmentedPicker(selection: $t.warpSplit, segments: [
            .init(value: .none, title: "", symbol: "hand.point.up.left", help: "Drag points and handles"),
            .init(value: .vertical, title: "", symbol: "rectangle.split.2x1", help: "Split vertically: click the net"),
            .init(value: .horizontal, title: "", symbol: "rectangle.split.1x2", help: "Split horizontally: click the net"),
            .init(value: .cross, title: "", symbol: "rectangle.split.2x2", help: "Split crosswise: click the net (⇧ keeps splitting)"),
        ], height: Theme.Height.small, fill: false)
        .fixedSize()
    }

    @ViewBuilder private var perspective: some View {
        SegmentedPicker(selection: $t.perspectiveLayout, segments: [
            .init(value: true, title: "Layout", symbol: nil, help: "Fit the planes to the image"),
            .init(value: false, title: "Warp", symbol: nil, help: "Move the planes"),
        ], height: Theme.Height.small, fill: false)
        .fixedSize()
        .onChange(of: t.perspectiveLayout) { _, _ in t.redraw() }
        Button("Split Vertically") { t.splitPerspective(vertical: true) }
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            .help("Add a linked plane: the planes share their new edge")
        Button("Split Horizontally") { t.splitPerspective(vertical: false) }
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
    }

    @ViewBuilder private func puppet(_ p: PuppetModel) -> some View {
        MenuPicker(selection: Binding(get: { p.mode == "Rigid" }, set: { t.setPuppetRigid($0) }),
                   options: [(false, "Normal"), (true, "Rigid")])
            .frame(width: Theme.Width.labelWide)
            .help("Mode")
        SegmentedPicker(selection: Binding(get: { t.puppetDensity }, set: { t.puppetDensity = $0; t.remesh() }),
                        segments: PuppetDensityTag.allCases.map { .init(value: $0, title: $0.title, symbol: nil, help: "Density: \($0.title)") },
                        height: Theme.Height.small, fill: false)
            .fixedSize()
        OptionField(title: "Expansion", value: Binding(get: { Double(t.puppetExpansion) }, set: {
            t.puppetExpansion = UInt32(min(max($0, 0), 64)); t.remesh()
        }), range: 0...64, unit: "px")
        OptionToggle(title: "Show Mesh", on: Binding(get: { t.showMesh }, set: { t.showMesh = $0; t.redraw() }))
        OptionField(title: "Rotate", value: Binding(get: { t.selectedPinDegrees ?? 0 }, set: { t.setSelectedPinDegrees($0) }),
                    range: -360...360, unit: "°", fractionDigits: 1)
            .disabled(t.selectedPin == nil)
        Text("\(p.pins.count) pin\(p.pins.count == 1 ? "" : "s")").font(Theme.Fonts.captionNumeric)
            .foregroundStyle(Theme.textTertiary).fixedSize()
        if let note = t.puppetNote {
            Image(systemName: "exclamationmark.triangle").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.warning)
                .help(note)
        }
    }

    @ViewBuilder private func scale(_ c: ContentAwareScaleModel) -> some View {
        OptionField(title: "W", value: Binding(get: { Double(c.width) }, set: { t.setScale(width: UInt32(max($0, 1))) }),
                    range: 1...Double(c.canvasWidth * 4), unit: "px")
        OptionField(title: "H", value: Binding(get: { Double(c.height) }, set: { t.setScale(height: UInt32(max($0, 1))) }),
                    range: 1...Double(c.canvasHeight * 4), unit: "px")
        OptionField(title: "Amount", value: Binding(get: { Double(c.amount) * 100 }, set: { t.setScale(amount: Float($0 / 100)) }),
                    range: 0...100, unit: "%")
        MenuPicker(selection: Binding(get: { c.protectChannel }, set: { t.setScale(protect: .some($0)) }),
                   options: [(UInt64?.none, "Protect: None")] + t.channels.map { (UInt64?.some($0.id), "Protect: \($0.name)") })
            .frame(width: Theme.Width.labelWide + Theme.Space.xxl)
            .help("Protect a saved alpha channel (there is no automatic skin detection)")
    }
}

/// Edit ▸ Transform additions (WP B5-12).
struct AdvancedTransformMenuItems: View {
    let doc: DocumentController?
    private var t: DocumentTransforms { DocumentTransforms.shared }

    var body: some View {
        let on = t.canBegin(doc)
        Divider()
        Button("Content-Aware Scale") { t.begin(.contentAwareScale) }
            .keyboardShortcut("c", modifiers: [.command, .option, .shift])
            .disabled(!on)
        Button("Puppet Warp") { t.begin(.puppet) }.disabled(!on)
        Button("Perspective Warp") { t.begin(.perspective) }.disabled(!on)
        Button("Warp") { t.begin(.warp) }.disabled(!on)
    }
}

/// The Apply alert: converting a pixel / text / shape layer to a smart object is explicit.
struct TransformSheets: ViewModifier {
    @Bindable var t: DocumentTransforms

    func body(content: Content) -> some View {
        content.alert("Convert to Smart Object?", isPresented: $t.consentPending) {
            Button("Cancel", role: .cancel) {}
            Button("Convert and Apply") { t.confirmConversion() }
        } message: {
            Text("\(t.session?.op.tag.title ?? "The transform") is kept as an editable stage of a smart object. The layer's pixels, "
                 + "text or shape stay inside it unchanged, with its masks and style on the smart object. Undo restores the layer.")
        }
    }
}
