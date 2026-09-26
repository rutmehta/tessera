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

private func swatch(_ hue: Double, l: Double = 0.7, c: Double = 0.13) -> NSColor {
    let rgb = OkLab.srgb(l: l, c: c, hue: hue)
    return NSColor(srgbRed: rgb.r, green: rgb.g, blue: rgb.b, alpha: 1)   // lint:allow (OkLab hue swatch, data not chrome)
}

// MARK: - Tone curve

struct ToneCurvePanel: View {
    let model: AppModel
    @Bindable var tools: DevelopTools

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(spacing: Theme.Space.s) {
                SegmentedPicker(selection: $tools.curveMode, segments: DevelopTools.CurveMode.allCases.map {
                    .init(value: $0, title: $0.rawValue)
                }, height: Theme.Height.small, fill: false)
                .fixedSize()
                Spacer(minLength: 0)
                if tools.curveMode == .point {
                    SegmentedPicker(selection: $tools.curveChannel, segments: CurveChannel.allCases.map {
                        .init(value: $0, title: $0 == .luminance ? "L" : String($0.title.prefix($0 == .rgb ? 3 : 1)), help: $0.title)
                    }, height: Theme.Height.small, fill: false)
                    .fixedSize()
                    .help("Point curve channel: RGB, Red, Green, Blue or Luminance")
                }
            }
            CurveEditor(model: model, tools: tools)
                .aspectRatio(1, contentMode: .fit)
            if tools.curveMode == .parametric {
                ForEach(ParametricRegion.allCases, id: \.self) { r in
                    ControlSlider(control: r.control).frame(height: Theme.Height.slider)
                }
            } else {
                HStack(spacing: Theme.Space.xs) {
                    Menu("Curve Presets") {
                        ForEach(PointCurve.presets, id: \.0) { name, curve in
                            Button(name) { tools.setPointCurve(curve, channel: tools.curveChannel, final: true); tools.bump() }
                        }
                    }
                    .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
                    Button("Reset \(tools.curveChannel.title)") {
                        tools.setPointCurve(.identity, channel: tools.curveChannel, final: true); tools.bump()
                    }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    Spacer()
                }
                Hint("Click to add a point, drag to move, double-click to remove. Arrows nudge the selected point (⇧ ×10).")
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
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: Theme.Space.s) {
                SegmentedPicker(selection: $property, segments: HSLProperty.allCases.map { .init(value: $0, title: $0.title) },
                                height: Theme.Height.small)
                IconButton(symbol: "scope", help: "Targeted adjustment: drag up/down on a colour in the loupe to change its \(property.title.lowercased())",
                           on: tools.hslPicker == property, size: Theme.Height.small) { tools.toggleHSLPicker(property) }
                    .disabled(!ready)
            }
            .padding(.bottom, Theme.Space.xs)
            ForEach(HueBand.allCases) { band in
                ControlSlider(control: property.control(band), trackColors: colors(band, property))
                    .frame(height: Theme.Height.slider)
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
    private static let symbols = ["", "circle.bottomhalf.filled", "circle.lefthalf.filled", "circle.tophalf.filled", "globe"]

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            // Icons keep five modes inside the inspector's minimum width (no layout jump).
            SegmentedPicker(selection: $mode, segments: Self.tabs.indices.map { i in
                .init(value: i, title: i == 0 ? "3-Way" : "", symbol: i == 0 ? nil : Self.symbols[i], help: Self.tabs[i].0)
            }, height: Theme.Height.small)
            if let range = Self.tabs[mode].1 {
                SubHeader(range.title)
                GradeWheel(range: range).frame(height: 144)
                ControlSlider(control: range.hue).frame(height: Theme.Height.slider)
                ControlSlider(control: range.saturation).frame(height: Theme.Height.slider)
                ControlSlider(control: range.luminance).frame(height: Theme.Height.slider)
            } else {
                HStack(alignment: .top, spacing: Theme.Space.m) {
                    ForEach([GradeRange.shadows, .midtones, .highlights]) { range in
                        VStack(spacing: Theme.Space.xs) {
                            GradeWheel(range: range).frame(height: 72)
                            ControlSlider(control: range.luminance, title: range.title).frame(height: Theme.Height.slider)
                        }
                    }
                }
                HStack(alignment: .center, spacing: Theme.Space.m) {
                    GradeWheel(range: .global).frame(width: 72, height: 72)
                    ControlSlider(control: GradeRange.global.luminance, title: "Global").frame(height: Theme.Height.slider)
                }
            }
            ControlSlider(control: GradeRange.blending).frame(height: Theme.Height.slider)
            ControlSlider(control: GradeRange.balance).frame(height: Theme.Height.slider)
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
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .center) {
                SubHeader("Sharpening")
                Spacer()
                IconButton(symbol: "scope", help: "Choose the 1:1 preview area in the loupe", on: tools.detailPicking,
                           size: Theme.Height.small) {
                    tools.toggleDetailPicking()
                }
                .disabled(!ready)
                .padding(.top, Theme.Space.s)
            }
            if ready {
                DetailPreview1to1().frame(height: 120).padding(.vertical, Theme.Space.xs)
            }
            ControlSlider(control: DetailControls.amount).frame(height: Theme.Height.slider)
            ControlSlider(control: DetailControls.radius).frame(height: Theme.Height.slider)
            ControlSlider(control: DetailControls.detail).frame(height: Theme.Height.slider)
            ControlSlider(control: DetailControls.masking,
                          onDragBegan: { mods in if mods.contains(.option) { tools.setMaskingPreview(true) } },
                          onDragEnded: { tools.setMaskingPreview(false) })
                .frame(height: Theme.Height.slider)
                .help("Hold ⌥ while dragging to see the edge mask (white is sharpened)")
            SubHeader("Noise Reduction")
            ForEach(DetailControls.luminanceNoise) { c in
                ControlSlider(control: c).frame(height: Theme.Height.slider)
            }
            ForEach(DetailControls.colorNoise) { c in
                ControlSlider(control: c).frame(height: Theme.Height.slider)
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
        VStack(alignment: .leading, spacing: 0) {
            SubHeader("Post-Crop Vignetting")
            HStack {
                Text("Style").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                Spacer()
                MenuPicker(selection: Binding(
                    get: { _ = revision; return VignetteStyle(rawValue: tools.string(VignetteStyle.path) ?? "") ?? .highlightPriority },
                    set: { tools.apply(DevelopController.patch(VignetteStyle.path, $0.rawValue), final: true,
                                       label: "Vignette Style: \($0.title)"); tools.bump() }),
                           options: VignetteStyle.allCases.map { ($0, $0.title) })
            }
            .frame(height: Theme.Height.large)
            ForEach(EffectsControls.vignette) { c in
                ControlSlider(control: c).frame(height: Theme.Height.slider)
            }
            SubHeader("Grain")
            ForEach(EffectsControls.grain) { c in
                ControlSlider(control: c).frame(height: Theme.Height.slider)
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
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Toggle("HDR (extended dynamic range)", isOn: Binding(
                get: { on },
                set: { v in
                    guard let d = tools.develop else { return }
                    tools.apply(d.hdrPatch(v), final: true, label: v ? "HDR On" : "HDR Off")
                    tools.bump()
                }))
                .font(Theme.Fonts.caption)
                .accessibilityIdentifier("hdr-toggle")
            ControlSlider(control: HDRControls.headroom(maxStops: max(edr.maxStops, 0.1)))
                .frame(height: Theme.Height.slider)
                .disabled(!on || !edr.isEDRCapable)
                .accessibilityIdentifier("hdr-headroom")
            Text(status(edr, on: on)).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
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
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.xs) {
                if tools.cropActive {
                    Button("Done") { tools.commitCrop() }.keyboardShortcut(.defaultAction)
                        .buttonStyle(.theme(.primary, height: Theme.Height.small))
                    Button("Cancel") { tools.cancelCrop() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                } else {
                    Button("Crop & Straighten") { tools.beginCrop() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .help("Open the crop tool in the loupe (R)")
                }
                Spacer()
                IconButton(symbol: "level", help: "Straighten: draw along a horizon or vertical in the loupe",
                           on: tools.straightening, size: Theme.Height.small) {
                    if !tools.cropActive { tools.beginCrop() }
                    tools.straightening.toggle()
                    tools.onLoupeToolChange?()
                }
                Button("Reset") { tools.resetCrop() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            }
            HStack(spacing: Theme.Space.xs) {
                Text("Aspect").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                Spacer()
                MenuPicker(selection: Binding(get: { tools.cropAspect }, set: { tools.setCropAspect($0) }),
                           options: CropAspect.presets.map { ($0, $0.title) })
                IconButton(symbol: tools.cropPortrait ? "rectangle.portrait" : "rectangle",
                           help: "Swap landscape / portrait (X)", on: false, size: Theme.Height.small) {
                    if !tools.cropActive { tools.beginCrop() }
                    tools.flipCropOrientation()
                }
            }
            .controlSize(.small)
            .frame(height: Theme.Height.regular)
            CropAngleSlider(tools: tools).frame(height: Theme.Height.slider)
            HStack(spacing: Theme.Space.xs) {
                Text("Overlay").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                Spacer()
                MenuPicker(selection: Binding(get: { tools.cropOverlay },
                                              set: { tools.cropOverlay = $0; tools.onLoupeToolChange?() }),
                           options: CropOverlay.allCases.map { ($0, $0.title) })
            }
            .frame(height: Theme.Height.regular)
            Toggle("Constrain to image", isOn: $tools.constrainCrop)
                .font(Theme.Fonts.caption)
                .controlSize(.small)
            Hint(tools.cropActive
                 ? "Drag handles to crop, inside to move, outside to rotate. Return applies, Esc cancels, O cycles overlays."
                 : "R opens the crop tool in the loupe.")
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
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if tools.presets.isEmpty {
                Hint("No presets yet. Save the current look (all or some panels) to reuse it on other photos.")
            }
            ForEach(tools.presets) { p in
                Button { tools.applyPreset(p) } label: {
                    HStack {
                        Text(p.name).font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary).lineLimit(1)
                        Spacer()
                        Text(p.groups.count == PresetGroup.allCases.count ? "All" : "\(p.groups.count) panels")
                            .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                .help(p.groups.map(\.title).joined(separator: ", "))
                .contextMenu { Button("Delete “\(p.name)”") { tools.deletePreset(p) } }
            }
            Button("Save Preset…") {
                name = "Preset \(tools.presets.count + 1)"
                saving = true
            }
            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            .popover(isPresented: $saving, arrowEdge: .leading) {
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    Text("New Preset").font(Theme.Fonts.title)
                    TextField("Name", text: $name).textFieldStyle(.roundedBorder).frame(width: 220)
                    SubHeader("Include")
                    ForEach(PresetGroup.allCases) { g in
                        Toggle(g.title, isOn: Binding(get: { groups.contains(g) },
                                                      set: { if $0 { groups.insert(g) } else { groups.remove(g) } }))
                            .font(Theme.Fonts.caption)
                    }
                    HStack(spacing: Theme.Space.s) {
                        Spacer()
                        Button("Cancel") { saving = false }.buttonStyle(.themeBordered)
                        Button("Save") {
                            tools.savePreset(name: name.trimmingCharacters(in: .whitespaces), groups: groups)
                            saving = false
                        }
                        .keyboardShortcut(.defaultAction)
                        .buttonStyle(.themePrimary)
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || groups.isEmpty)
                    }
                }
                .padding(Theme.Space.l)
                .tint(Theme.accent)
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
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if names.isEmpty {
                Hint("Snapshots name a state you can return to.")
            }
            ForEach(names, id: \.self) { n in
                Button { model.restoreSnapshot(n) } label: {
                    HStack(spacing: Theme.Space.s) {
                        Image(systemName: "camera.viewfinder").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textSecondary)
                        Text(n).font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary).lineLimit(1)
                        Spacer()
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
            }
            Button("New Snapshot…") { model.promptSnapshot() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
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
        let groups = tools.historyGroups
        let grouped = Set(groups.flatMap(\.steps))
        VStack(alignment: .leading, spacing: 0) {
            if items.isEmpty {
                Hint(ready ? "No edits yet" : "Open a photo in the loupe (E)")
            }
            // Agent groups first (docs/10 §2): amount, per-step toggles, rationale, redo.
            ForEach(groups.reversed(), id: \.groupId) { group in
                AgentGroupSection(model: model, tools: tools, group: group)
                Hairline().padding(.bottom, Theme.Space.xs)
            }
            ForEach(items.reversed().filter { !grouped.contains($0.id) }, id: \.id) { item in HistoryRow(item: item, tools: tools) }
            if !items.isEmpty {
                let atBase = !items.contains { $0.isHead }
                Button { tools.checkout(nil) } label: {
                    HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
                        Image(systemName: "circle.dashed").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                            .frame(width: Theme.Space.l)
                        Text("Original").font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary)
                        Spacer()
                    }
                    .padding(.horizontal, Theme.Space.xs)
                    .frame(height: Theme.Height.regular)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(atBase ? Theme.accentSubtle : Theme.clear))
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
        HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
            if item.toggles == nil && item.groupAmount == nil && item.applied {
                Toggle("", isOn: Binding(get: { item.enabled }, set: { tools.setStep(item, enabled: $0) }))
                    .toggleStyle(.checkbox)
                    .labelsHidden()
                    .controlSize(.mini)
                    .frame(width: Theme.Space.l)
                    .help(item.enabled ? "Turn this step off (recorded as a new step)" : "Turn this step back on")
            } else {
                Image(systemName: item.toggles != nil ? "arrow.uturn.left" : item.groupAmount != nil ? "slider.horizontal.below.rectangle" : "circle")
                    .font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary).frame(width: Theme.Space.l)
            }
            Button { tools.checkout(item.id) } label: {
                HStack(spacing: Theme.Space.xs) {
                    if item.author.hasPrefix("agent") {
                        Chip(text: "AI", color: Theme.accent, style: .outlined, height: Theme.Height.chip)
                    }
                    if let g = item.group { Text(g).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary) }
                    Text(item.label)
                        .font(Theme.Fonts.caption)
                        .strikethrough(!item.enabled)
                        .foregroundStyle(item.applied ? Theme.textPrimary : Theme.textTertiary)
                        .lineLimit(1)
                    Spacer()
                    Text(Self.time(item.timestampMs)).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, Theme.Space.xs)
        .frame(height: Theme.Height.regular)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(item.isHead ? Theme.accentSubtle : Theme.clear))
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
