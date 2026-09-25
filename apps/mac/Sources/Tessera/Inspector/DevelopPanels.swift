import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

// MARK: - Shared pieces

/// A develop slider bound to a `DevelopControl`. Like the Basic sliders, a drag calls straight into
/// the session (coalesced per display frame) and SwiftUI only refreshes the value when the
/// develop revision changes (open, undo, preset, history) or a linked control posts a change.
struct ControlSlider: NSViewRepresentable {
    let control: DevelopControl
    var title: String?
    var trackColors: [NSColor]?
    var onDragBegan: ((NSEvent.ModifierFlags) -> Void)?
    var onDragEnded: (() -> Void)?
    @Environment(\.developReady) private var ready
    @Environment(\.developRevision) private var revision

    @MainActor final class Coordinator {
        var ready = false
        nonisolated(unsafe) var token: NSObjectProtocol?
        weak var slider: ValueSlider?
        let control: DevelopControl
        init(control: DevelopControl) {
            self.control = control
            token = NotificationCenter.default.addObserver(forName: DevelopTools.valuesChanged, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, let s = self.slider, !s.isDragging, self.ready else { return }
                    s.doubleValue = DevelopTools.shared.value(self.control)
                }
            }
        }
        deinit { if let token { NotificationCenter.default.removeObserver(token) } }
    }

    func makeCoordinator() -> Coordinator { Coordinator(control: control) }

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        s.title = title ?? control.title
        s.minValue = control.range.lowerBound
        s.maxValue = control.range.upperBound
        s.defaultValue = control.defaultValue
        s.valueFormat = control.format
        s.step = control.step
        s.trackColors = trackColors
        s.onDragBegan = onDragBegan
        s.onDragEnded = onDragEnded
        let coordinator = context.coordinator
        coordinator.slider = s
        let control = control
        s.onChange = { value, isFinal in
            guard coordinator.ready else { return }
            DevelopTools.shared.set(control, value, final: isFinal)
        }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        _ = revision
        context.coordinator.ready = ready
        // Ranges may follow the screen (the HDR headroom slider).
        s.minValue = control.range.lowerBound
        s.maxValue = control.range.upperBound
        s.isEnabled = ready
        s.trackColors = trackColors
        if !s.isDragging { s.doubleValue = ready ? DevelopTools.shared.value(control) : control.defaultValue }
        s.needsDisplay = true
    }
}

private struct DevelopReadyKey: EnvironmentKey { static let defaultValue = false }
private struct DevelopRevisionKey: EnvironmentKey { static let defaultValue = 0 }

extension EnvironmentValues {
    var developReady: Bool {
        get { self[DevelopReadyKey.self] }
        set { self[DevelopReadyKey.self] = newValue }
    }
    var developRevision: Int {
        get { self[DevelopRevisionKey.self] }
        set { self[DevelopRevisionKey.self] = newValue }
    }
}

extension View {
    /// Supplies the develop state the panel controls refresh on.
    func developContext(_ model: AppModel, _ tools: DevelopTools) -> some View {
        environment(\.developReady, model.developStatus == .ready)
            .environment(\.developRevision, model.developRevision &+ tools.revision &* 7919)
    }
}

private struct SubHeader: View {
    let title: String
    init(_ title: String) { self.title = title }
    var body: some View {
        Text(title).font(.system(size: 10, weight: .medium)).foregroundStyle(.tertiary).padding(.top, 4)
    }
}

private struct ToolButton: View {
    let symbol: String
    let help: String
    let on: Bool
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 11, weight: .medium))
                .frame(width: 22, height: 20)
                .foregroundStyle(on ? Color.black.opacity(0.85) : Color.primary)
                .background(RoundedRectangle(cornerRadius: 4).fill(on ? Color(nsColor: Theme.accent) : Color.white.opacity(0.07)))
        }
        .buttonStyle(.plain)
        .help(help)
    }
}

private func swatch(_ hue: Double, l: Double = 0.7, c: Double = 0.13) -> NSColor {
    let rgb = OkLab.srgb(l: l, c: c, hue: hue)
    return NSColor(srgbRed: rgb.r, green: rgb.g, blue: rgb.b, alpha: 1)
}

// MARK: - Tone curve

struct ToneCurvePanel: View {
    let model: AppModel
    @Bindable var tools: DevelopTools

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Picker("", selection: $tools.curveMode) {
                    ForEach(DevelopTools.CurveMode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                Spacer()
                if tools.curveMode == .point {
                    Picker("", selection: $tools.curveChannel) {
                        ForEach(CurveChannel.allCases) { Text($0 == .luminance ? "L" : String($0.title.prefix($0 == .rgb ? 3 : 1))).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
                    .help("Point curve channel: RGB, Red, Green, Blue or Luminance")
                }
            }
            .controlSize(.small)
            CurveEditor(model: model, tools: tools)
                .frame(height: 188)
            if tools.curveMode == .parametric {
                ForEach(ParametricRegion.allCases, id: \.self) { r in
                    ControlSlider(control: r.control).frame(height: 30)
                }
            } else {
                HStack(spacing: 6) {
                    Menu("Curve Presets") {
                        ForEach(PointCurve.presets, id: \.0) { name, curve in
                            Button(name) { tools.setPointCurve(curve, channel: tools.curveChannel, final: true); tools.bump() }
                        }
                    }
                    .fixedSize()
                    Button("Reset \(tools.curveChannel.title)") {
                        tools.setPointCurve(.identity, channel: tools.curveChannel, final: true); tools.bump()
                    }
                    Spacer()
                }
                .controlSize(.small)
                Text("Click to add a point, drag to move, double-click to remove. Arrows nudge the selected point (⇧ ×10).")
                    .font(.system(size: 10)).foregroundStyle(.tertiary).fixedSize(horizontal: false, vertical: true)
            }
        }
        .disabled(!ready)
    }
}

struct CurveEditor: NSViewRepresentable {
    let model: AppModel
    let tools: DevelopTools
    @Environment(\.developReady) private var ready
    @Environment(\.developRevision) private var revision

    @MainActor final class Coordinator: LibraryObserver {
        let view = CurveEditorView(frame: .zero)
        nonisolated(unsafe) var token: NSObjectProtocol?
        init(model: AppModel) {
            model.addObserver(self)
            token = NotificationCenter.default.addObserver(forName: DevelopTools.valuesChanged, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { if self?.view.isInteracting == false { self?.reload() } }
            }
            view.onCurve = { curve, final in
                DevelopTools.shared.setPointCurve(curve, channel: DevelopTools.shared.curveChannel, final: final)
            }
            view.onRegion = { i, v, final in
                let region = ParametricRegion.allCases.reversed()[i]
                DevelopTools.shared.set(region.control, v, final: final)
            }
            view.onSplits = { p, final in DevelopTools.shared.setSplits(p, final: final) }
        }
        deinit { if let token { NotificationCenter.default.removeObserver(token) } }

        func reload() {
            let tools = DevelopTools.shared
            guard tools.ready else { return }
            view.parametric = tools.parametric()
            view.curve = tools.pointCurve(tools.curveChannel)
        }
        func libraryDidReload() {}
        func itemsDidChange(_ positions: IndexSet) {}
        func selectionDidChange(scrollToFocus: Bool) {}
        func developDidChange() { view.histogram = DevelopTools.shared.develop?.histogram; reload() }
        func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {
            if !frame.isOverlay { view.histogram = controller.histogram }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(model: model) }
    func makeNSView(context: Context) -> CurveEditorView { context.coordinator.view }
    func updateNSView(_ v: CurveEditorView, context: Context) {
        _ = revision
        v.isEnabled = ready
        v.mode = tools.curveMode == .parametric ? .parametric : .point
        v.channel = tools.curveChannel
        v.histogram = tools.develop?.histogram
        context.coordinator.reload()
        if !ready { v.curve = .identity; v.parametric = ParametricCurveModel() }
    }
}

// MARK: - HSL

struct HSLPanel: View {
    let model: AppModel
    @Bindable var tools: DevelopTools
    @State private var property: HSLProperty = .hue

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Picker("", selection: $property) {
                    ForEach(HSLProperty.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                Spacer(minLength: 4)
                ToolButton(symbol: "scope", help: "Targeted adjustment: drag up/down on a colour in the loupe to change its \(property.title.lowercased())",
                           on: tools.hslPicker == property) { tools.toggleHSLPicker(property) }
                    .disabled(!ready)
            }
            .controlSize(.small)
            ForEach(HueBand.allCases) { band in
                ControlSlider(control: property.control(band), trackColors: colors(band, property))
                    .frame(height: 30)
            }
        }
        .onChange(of: property) { _, p in if tools.hslPicker != nil { tools.toggleHSLPicker(p) } }
        .disabled(!ready)
    }

    private func colors(_ band: HueBand, _ p: HSLProperty) -> [NSColor] {
        let h = band.center
        switch p {
        case .hue: return [swatch(h - 30), swatch(h), swatch(h + 30)]
        case .saturation: return [swatch(h, c: 0), swatch(h, c: 0.16)]
        case .luminance: return [swatch(h, l: 0.35, c: 0.08), swatch(h, l: 0.9, c: 0.08)]
        }
    }
}

extension HSLProperty {
    func historyTitle(_ band: HueBand) -> String { "\(band.title) \(title)" }
}

// MARK: - Colour grading

struct ColorGradingPanel: View {
    let model: AppModel
    let tools: DevelopTools
    @State private var mode = 0   // 0 = 3-way, 1… = one range

    private static let tabs: [(String, GradeRange?)] = [("3-Way", nil), ("Shadows", .shadows), ("Midtones", .midtones),
                                                          ("Highlights", .highlights), ("Global", .global)]

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: 6) {
            Picker("", selection: $mode) {
                ForEach(Self.tabs.indices, id: \.self) { Text(Self.tabs[$0].0).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .controlSize(.small)
            if let range = Self.tabs[mode].1 {
                GradeWheel(range: range).frame(height: 150)
                ControlSlider(control: range.hue).frame(height: 30)
                ControlSlider(control: range.saturation).frame(height: 30)
                ControlSlider(control: range.luminance).frame(height: 30)
            } else {
                HStack(alignment: .top, spacing: 8) {
                    ForEach([GradeRange.shadows, .midtones, .highlights]) { range in
                        VStack(spacing: 2) {
                            GradeWheel(range: range).frame(height: 76)
                            Text(range.title).font(.system(size: 10)).foregroundStyle(.secondary)
                            ControlSlider(control: range.luminance, title: "Lum").frame(height: 30)
                        }
                    }
                }
                HStack(spacing: 8) {
                    GradeWheel(range: .global).frame(width: 76, height: 76)
                    VStack(alignment: .leading, spacing: 0) {
                        Text("Global").font(.system(size: 10)).foregroundStyle(.secondary)
                        ControlSlider(control: GradeRange.global.luminance).frame(height: 30)
                    }
                }
            }
            ControlSlider(control: GradeRange.blending).frame(height: 30)
            ControlSlider(control: GradeRange.balance).frame(height: 30)
        }
        .disabled(!ready)
    }
}

struct GradeWheel: NSViewRepresentable {
    let range: GradeRange
    @Environment(\.developReady) private var ready
    @Environment(\.developRevision) private var revision

    @MainActor final class Coordinator {
        let view = ColorWheelView(frame: .zero)
        nonisolated(unsafe) var token: NSObjectProtocol?
        let range: GradeRange
        init(range: GradeRange) {
            self.range = range
            view.title = range.title
            token = NotificationCenter.default.addObserver(forName: DevelopTools.valuesChanged, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.reload() }
            }
            view.onChange = { h, s, final in
                DevelopTools.shared.apply(range.wheelPatch(hue: h, saturation: s), final: final,
                                          label: "\(range.title) Grade " + String(format: "%.0f° / %.0f", h, s))
            }
        }
        deinit { if let token { NotificationCenter.default.removeObserver(token) } }
        func reload() {
            let t = DevelopTools.shared
            guard t.ready else { view.hue = 0; view.saturation = 0; return }
            view.hue = t.value(range.hue)
            view.saturation = t.value(range.saturation)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(range: range) }
    func makeNSView(context: Context) -> ColorWheelView { context.coordinator.view }
    func updateNSView(_ v: ColorWheelView, context: Context) {
        _ = revision
        v.isEnabled = ready
        context.coordinator.reload()
    }
}

// MARK: - Detail

struct DetailPanel: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .bottom) {
                SubHeader("Sharpening")
                Spacer()
                ToolButton(symbol: "scope", help: "Choose the 1:1 preview area in the loupe", on: tools.detailPicking) {
                    tools.toggleDetailPicking()
                }
                .disabled(!ready)
            }
            if ready {
                DetailPreview1to1().frame(height: 120)
            }
            ControlSlider(control: DetailControls.amount).frame(height: 30)
            ControlSlider(control: DetailControls.radius).frame(height: 30)
            ControlSlider(control: DetailControls.detail).frame(height: 30)
            ControlSlider(control: DetailControls.masking,
                          onDragBegan: { mods in if mods.contains(.option) { tools.setMaskingPreview(true) } },
                          onDragEnded: { tools.setMaskingPreview(false) })
                .frame(height: 30)
                .help("Hold ⌥ while dragging to see the edge mask (white is sharpened)")
            SubHeader("Noise Reduction")
            ForEach(DetailControls.luminanceNoise) { c in
                ControlSlider(control: c).frame(height: 30)
            }
            ForEach(DetailControls.colorNoise) { c in
                ControlSlider(control: c).frame(height: 30)
            }
        }
        .disabled(!ready)
    }
}

// MARK: - Effects

struct EffectsPanel: View {
    let model: AppModel
    let tools: DevelopTools
    @Environment(\.developRevision) private var revision

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: 6) {
            SubHeader("Post-Crop Vignetting")
            HStack {
                Text("Style").font(.system(size: 11))
                Spacer()
                Picker("", selection: Binding(
                    get: { _ = revision; return VignetteStyle(rawValue: tools.string(VignetteStyle.path) ?? "") ?? .highlightPriority },
                    set: { tools.apply(DevelopController.patch(VignetteStyle.path, $0.rawValue), final: true,
                                       label: "Vignette Style: \($0.title)"); tools.bump() })) {
                    ForEach(VignetteStyle.allCases) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .fixedSize()
            }
            .controlSize(.small)
            ForEach(EffectsControls.vignette) { c in
                ControlSlider(control: c).frame(height: 30)
            }
            SubHeader("Grain")
            ForEach(EffectsControls.grain) { c in
                ControlSlider(control: c).frame(height: 30)
            }
        }
        .disabled(!ready)
    }
}

// MARK: - HDR (M2-22)

/// The recipe's HDR toggle and headroom (0 EV = SDR tone-mapped, up to the screen's EDR
/// potential). On SDR screens the setting is kept in the recipe and the loupe stays SDR.
struct HDRPanel: View {
    let model: AppModel
    let tools: DevelopTools
    @Environment(\.developRevision) private var revision

    var body: some View {
        let ready = model.developStatus == .ready
        let edr = tools.edr
        let on = { _ = revision; return tools.develop?.hdrEnabled ?? false }()
        VStack(alignment: .leading, spacing: 6) {
            Toggle("HDR (extended dynamic range)", isOn: Binding(
                get: { on },
                set: { v in
                    guard let d = tools.develop else { return }
                    tools.apply(d.hdrPatch(v), final: true, label: v ? "HDR On" : "HDR Off")
                    tools.bump()
                }))
                .font(.system(size: 11))
                .accessibilityIdentifier("hdr-toggle")
            ControlSlider(control: HDRControls.headroom(maxStops: max(edr.maxStops, 0.1)))
                .frame(height: 30)
                .disabled(!on || !edr.isEDRCapable)
                .accessibilityIdentifier("hdr-headroom")
            Text(status(edr, on: on)).font(.system(size: 10)).foregroundStyle(.secondary)
                .accessibilityIdentifier("hdr-status")
        }
        .controlSize(.small)
        .disabled(!ready)
    }

    private func status(_ edr: EDRPresentation, on: Bool) -> String {
        guard edr.isEDRCapable else {
            return on ? "SDR display: HDR is kept in the recipe, the loupe shows SDR." : "SDR display"
        }
        guard on else { return edr.readout }
        let h = edr.effectiveHeadroom(stops: tools.develop?.hdrStops ?? 0)
        return edr.readout + String(format: " · showing %.1f× (RGBA16F)", h)
    }
}

// MARK: - Crop

struct CropPanel: View {
    let model: AppModel
    @Bindable var tools: DevelopTools
    @Environment(\.developRevision) private var revision

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                if tools.cropActive {
                    Button("Done") { tools.commitCrop() }.keyboardShortcut(.defaultAction)
                    Button("Cancel") { tools.cancelCrop() }
                } else {
                    Button("Crop & Straighten") { tools.beginCrop() }
                        .help("Open the crop tool in the loupe (R)")
                }
                Spacer()
                ToolButton(symbol: "level", help: "Straighten: draw along a horizon or vertical in the loupe",
                           on: tools.straightening) {
                    if !tools.cropActive { tools.beginCrop() }
                    tools.straightening.toggle()
                    tools.onLoupeToolChange?()
                }
                Button("Reset") { tools.resetCrop() }
            }
            .controlSize(.small)
            HStack(spacing: 6) {
                Text("Aspect").font(.system(size: 11))
                Spacer()
                Picker("", selection: Binding(get: { tools.cropAspect }, set: { tools.setCropAspect($0) })) {
                    ForEach(CropAspect.presets) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .fixedSize()
                ToolButton(symbol: tools.cropPortrait ? "rectangle.portrait" : "rectangle",
                           help: "Swap landscape / portrait (X)", on: false) {
                    if !tools.cropActive { tools.beginCrop() }
                    tools.flipCropOrientation()
                }
            }
            .controlSize(.small)
            CropAngleSlider(tools: tools).frame(height: 30)
            HStack(spacing: 6) {
                Text("Overlay").font(.system(size: 11))
                Spacer()
                Picker("", selection: Binding(get: { tools.cropOverlay },
                                              set: { tools.cropOverlay = $0; tools.onLoupeToolChange?() })) {
                    ForEach(CropOverlay.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .fixedSize()
            }
            .controlSize(.small)
            Toggle("Constrain to image", isOn: $tools.constrainCrop)
                .font(.system(size: 11))
                .controlSize(.small)
            Text(tools.cropActive
                 ? "Drag handles to crop, inside to move, outside to rotate. Return applies, Esc cancels, O cycles overlays."
                 : "R opens the crop tool in the loupe.")
                .font(.system(size: 10)).foregroundStyle(.tertiary).fixedSize(horizontal: false, vertical: true)
        }
        .disabled(!ready)
    }
}

/// The straighten angle (displayed orientation). Dragging it opens the crop tool so the rotation
/// is shown live on the loupe without rendering.
struct CropAngleSlider: NSViewRepresentable {
    let tools: DevelopTools
    @Environment(\.developReady) private var ready
    @Environment(\.developRevision) private var revision

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        s.title = "Angle"
        s.minValue = -45
        s.maxValue = 45
        s.defaultValue = 0
        s.step = 0.05
        s.valueFormat = "%+.2f°"
        s.onChange = { v, final in DevelopTools.shared.setCropAngle(v, settled: final) }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        _ = revision
        s.isEnabled = ready
        guard !s.isDragging else { return }
        s.doubleValue = tools.cropActive ? tools.cropAngle : (tools.storedCrop()?.angle ?? 0)
    }
}

// MARK: - Presets

struct PresetsPanel: View {
    let model: AppModel
    let tools: DevelopTools
    @State private var saving = false
    @State private var name = ""
    @State private var groups = PresetGroup.defaultSelection

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: 4) {
            if tools.presets.isEmpty {
                Text("No presets yet. Save the current look (all or some panels) to reuse it on other photos.")
                    .font(.system(size: 10)).foregroundStyle(.tertiary).fixedSize(horizontal: false, vertical: true)
            }
            ForEach(tools.presets) { p in
                Button { tools.applyPreset(p) } label: {
                    HStack {
                        Text(p.name).font(.system(size: 11)).lineLimit(1)
                        Spacer()
                        Text(p.groups.count == PresetGroup.allCases.count ? "All" : "\(p.groups.count) panels")
                            .font(.system(size: 9)).foregroundStyle(.tertiary)
                    }
                    .padding(.vertical, 3)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(p.groups.map(\.title).joined(separator: ", "))
                .contextMenu { Button("Delete “\(p.name)”") { tools.deletePreset(p) } }
            }
            Button("Save Preset…") {
                name = "Preset \(tools.presets.count + 1)"
                saving = true
            }
            .controlSize(.small)
            .popover(isPresented: $saving, arrowEdge: .leading) {
                VStack(alignment: .leading, spacing: 8) {
                    Text("New Preset").font(.headline)
                    TextField("Name", text: $name).frame(width: 220)
                    Text("Include").font(.system(size: 11)).foregroundStyle(.secondary)
                    ForEach(PresetGroup.allCases) { g in
                        Toggle(g.title, isOn: Binding(get: { groups.contains(g) },
                                                      set: { if $0 { groups.insert(g) } else { groups.remove(g) } }))
                            .font(.system(size: 11))
                    }
                    HStack {
                        Spacer()
                        Button("Cancel") { saving = false }
                        Button("Save") {
                            tools.savePreset(name: name.trimmingCharacters(in: .whitespaces), groups: groups)
                            saving = false
                        }
                        .keyboardShortcut(.defaultAction)
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || groups.isEmpty)
                    }
                }
                .padding(14)
            }
        }
        .disabled(!ready)
    }
}

// MARK: - Snapshots

struct SnapshotsPanel: View {
    let model: AppModel

    var body: some View {
        let ready = model.developStatus == .ready
        let names = model.developHistory?.snapshots ?? []
        VStack(alignment: .leading, spacing: 4) {
            if names.isEmpty {
                Text("Snapshots name a state you can return to.").font(.system(size: 10)).foregroundStyle(.tertiary)
            }
            ForEach(names, id: \.self) { n in
                Button { model.restoreSnapshot(n) } label: {
                    HStack {
                        Image(systemName: "camera.viewfinder").font(.system(size: 10)).foregroundStyle(.secondary)
                        Text(n).font(.system(size: 11)).lineLimit(1)
                        Spacer()
                    }
                    .padding(.vertical, 3)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            Button("New Snapshot…") { model.promptSnapshot() }.controlSize(.small)
        }
        .disabled(!ready)
    }
}

// MARK: - History

struct HistoryPanel: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        let ready = model.developStatus == .ready
        let items = tools.historyItems
        let key = "\(model.developRevision)|\(model.developHistory?.entries ?? 0)|\(model.developHistory?.headLabel ?? "")"
        VStack(alignment: .leading, spacing: 0) {
            if items.isEmpty {
                Text(ready ? "No edits yet" : "Open a RAW in the loupe").font(.system(size: 10)).foregroundStyle(.tertiary)
            }
            ForEach(items.reversed(), id: \.id) { item in HistoryRow(item: item, tools: tools) }
            if !items.isEmpty {
                let atBase = !items.contains { $0.isHead }
                Button { tools.checkout(nil) } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "circle.dashed").font(.system(size: 9)).frame(width: 14)
                        Text("Original").font(.system(size: 11))
                        Spacer()
                    }
                    .padding(.vertical, 3).padding(.horizontal, 4)
                    .background(RoundedRectangle(cornerRadius: 3).fill(atBase ? Color(nsColor: Theme.accent).opacity(0.22) : .clear))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .disabled(!ready)
        .task(id: key) { tools.refreshHistory() }
    }
}

private struct HistoryRow: View {
    let item: HistoryItem
    let tools: DevelopTools

    var body: some View {
        HStack(spacing: 6) {
            if item.toggles == nil && item.applied {
                Toggle("", isOn: Binding(get: { item.enabled }, set: { tools.setStep(item, enabled: $0) }))
                    .toggleStyle(.checkbox)
                    .labelsHidden()
                    .controlSize(.mini)
                    .frame(width: 14)
                    .help(item.enabled ? "Turn this step off (recorded as a new step)" : "Turn this step back on")
            } else {
                Image(systemName: item.toggles != nil ? "arrow.uturn.left" : "circle")
                    .font(.system(size: 8)).foregroundStyle(.tertiary).frame(width: 14)
            }
            Button { tools.checkout(item.id) } label: {
                HStack(spacing: 4) {
                    if item.author.hasPrefix("agent") {
                        Text("AI").font(.system(size: 8, weight: .bold)).padding(.horizontal, 3)
                            .background(RoundedRectangle(cornerRadius: 2).fill(Color.purple.opacity(0.5)))
                    }
                    if let g = item.group { Text(g).font(.system(size: 9)).foregroundStyle(.secondary) }
                    Text(item.label)
                        .font(.system(size: 11))
                        .strikethrough(!item.enabled)
                        .foregroundStyle(item.applied ? .primary : .tertiary)
                        .lineLimit(1)
                    Spacer()
                    Text(Self.time(item.timestampMs)).font(.system(size: 9)).foregroundStyle(.tertiary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.vertical, 3).padding(.horizontal, 4)
        .background(RoundedRectangle(cornerRadius: 3).fill(item.isHead ? Color(nsColor: Theme.accent).opacity(0.22) : .clear))
    }

    private static func time(_ ms: Int64) -> String {
        let d = Date(timeIntervalSince1970: Double(ms) / 1000)
        return Calendar.current.isDateInToday(d) ? d.formatted(date: .omitted, time: .shortened)
            : d.formatted(.dateTime.month(.abbreviated).day())
    }
}

extension DevelopTools {
    /// Refreshes panel controls after a change made from a button (preset, reset, menu).
    func bump() { NotificationCenter.default.post(name: Self.valuesChanged, object: nil) }
}
