import AppKit
import SwiftUI
import TesseraCore

/// Properties panel (spec 02 §16, context-aware): the selected layer's name, kind and bounds, then
/// its editor — every `Adjustment` of crates/compositor/src/adjust.rs (live drag on the
/// interactive path, one history entry on release), fill editors, the group mode, smart-object
/// and text placeholders.
struct PropertiesPanel: View {
    let document: DocumentController
    @State private var name = ""

    var body: some View {
        if let n = document.primary {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                HStack(spacing: Theme.Space.s) {
                    Text("Name").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        .frame(width: Theme.Width.label, alignment: .leading)
                    TextField("Name", text: $name)
                        .textFieldStyle(.roundedBorder)
                        .controlSize(.small)
                        .font(Theme.Fonts.caption)
                        .onSubmit { document.rename(n.id, to: name) }
                        .accessibilityIdentifier("document.properties.name")
                }
                InfoRow(label: "Kind", value: kindText(n))
                    .accessibilityIdentifier("document.properties.kind")
                InfoRow(label: "Bounds", value: n.bounds.map { "\($0.x), \($0.y) · \($0.width) × \($0.height) px" } ?? "Whole canvas")
                    .accessibilityIdentifier("document.properties.bounds")
                if n.hasMask {
                    InfoRow(label: "Mask", value: (n.maskEnabled ? "On" : "Off") + (n.maskLinked ? " · linked" : " · unlinked"))
                }
                editor(n)
            }
            .onAppear { name = n.name }
            .onChange(of: n.id) { _, _ in name = n.name }
            .onChange(of: n.name) { _, new in name = new }
        } else {
            Hint("Select a layer to see its properties.")
        }
    }

    private func kindText(_ n: LayerRecord) -> String {
        switch n.kind {
        case .adjustment: "Adjustment · " + (AdjustmentModel(json: n.adjustmentJson)?.kind.title ?? "Unknown")
        case .fill: "Fill · " + (FillModel(json: n.fillJson)?.kind.title ?? "Unknown")
        case .group: "Group · " + (n.groupMode == .isolated ? "Isolated" : "Pass Through")
        default: n.kind.title
        }
    }

    @ViewBuilder private func editor(_ n: LayerRecord) -> some View {
        switch n.kind {
        case .adjustment:
            if let model = document.adjustment(of: n.id) {
                AdjustmentEditor(document: document, id: n.id, model: model)
            }
        case .fill:
            if let fill = document.fill(of: n.id) { FillEditor(document: document, id: n.id, fill: fill) }
        case .group:
            SubHeader("Group")
            SegmentedPicker(selection: Binding(get: { n.groupMode ?? .passThrough }, set: { document.setGroupMode($0) }),
                            segments: [.init(value: LayerGroupMode.passThrough, title: "Pass Through",
                                             help: "Children blend straight into the layers below"),
                                       .init(value: LayerGroupMode.isolated, title: "Isolated",
                                             help: "Children composite on their own, then blend with the group's mode")],
                            height: Theme.Height.small)
                .accessibilityIdentifier("document.properties.groupMode")
        case .smartObject:
            SubHeader("Transform")
            InfoRow(label: "Position", value: "0, 0")
            InfoRow(label: "Scale", value: "100 %")
            InfoRow(label: "Rotation", value: "0°")
            Button("Edit Contents") {}
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(true)
                .help("Opens the smart object's document (arrives with smart-object editing)")
                .accessibilityIdentifier("document.properties.editContents")
        case .text:
            Hint("Text layers show their rasterized proxy. Editing type arrives with the type work package.")
        case .pixel:
            EmptyView()
        }
    }
}

/// Editors for each `Adjustment` variant.
struct AdjustmentEditor: View {
    let document: DocumentController
    let id: DocLayerID
    let model: AdjustmentModel
    /// Edits go here instead of to the adjustment layer (Image ▸ Adjustments sheets, WP B5-05).
    var onEdit: ((AdjustmentModel, Bool) -> Void)? = nil
    @State private var channel = 0      // 0 = composite (RGB), 1…3 = red, green, blue
    @State private var mixerRow = 0

    private var revision: Int { document.revision }
    private func set(_ m: AdjustmentModel, _ final: Bool) {
        if let onEdit { onEdit(m, final) } else { document.setAdjustment(id, m, final: final) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            SubHeader(model.kind.title)
            switch model {
            case .levels(let master, let rgb): levels(master, rgb)
            case .curves(let master, let rgb): curves(master, rgb)
            case .hueSaturation(let h, let s, let l, let colorize): hueSat(h, s, l, colorize)
            case .exposure(let e, let o, let g): exposure(e, o, g)
            case .invert: Hint("Invert has no settings.")
            case .posterize(let n):
                slider("Levels", Double(n), 2...255, 4, "%.0f", 1, "posterize.levels") { v, f in set(.posterize(levels: Int(v)), f) }
            case .threshold(let level):
                slider("Threshold level", level * 255, 1...255, 128, "%.0f", 1, "threshold.level") { v, f in
                    set(.threshold(level: v / 255), f)
                }
            case .channelMixer(let m, let k, let mono): mixer(m, k, mono)
            }
        }
    }

    private func slider(_ title: String, _ value: Double, _ range: ClosedRange<Double>, _ def: Double, _ format: String,
                        _ step: Double, _ key: String, _ change: @escaping (Double, Bool) -> Void) -> some View {
        DocSlider(title: title, value: value, range: range, defaultValue: def, format: format, step: step,
                  identifier: "document.properties.\(key)", revision: revision, onChange: change)
            .frame(height: Theme.Height.slider)
    }

    private var channelPicker: some View {
        SegmentedPicker(selection: $channel, segments: [
            .init(value: 0, title: "RGB"), .init(value: 1, title: "Red"), .init(value: 2, title: "Green"), .init(value: 3, title: "Blue"),
        ], height: Theme.Height.small)
        .padding(.bottom, Theme.Space.xs)
        .accessibilityIdentifier("document.properties.channel")
    }

    // MARK: Levels

    @ViewBuilder private func levels(_ master: LevelsChannelModel, _ rgb: [LevelsChannelModel]) -> some View {
        channelPicker
        let c = channel == 0 ? master : rgb[channel - 1]
        let update = { (edit: (inout LevelsChannelModel) -> Void, final: Bool) in
            var m = master, r = rgb
            if channel == 0 { edit(&m) } else { edit(&r[channel - 1]) }
            set(.levels(master: m, rgb: r), final)
        }
        slider("Input black", c.inBlack * 255, 0...253, 0, "%.0f", 1, "levels.inBlack") { v, f in update({ $0.inBlack = v / 255 }, f) }
        slider("Gamma", c.gamma, 0.1...9.99, 1, "%.2f", 0.01, "levels.gamma") { v, f in update({ $0.gamma = v }, f) }
        slider("Input white", c.inWhite * 255, 2...255, 255, "%.0f", 1, "levels.inWhite") { v, f in update({ $0.inWhite = v / 255 }, f) }
        slider("Output black", c.outBlack * 255, 0...255, 0, "%.0f", 1, "levels.outBlack") { v, f in update({ $0.outBlack = v / 255 }, f) }
        slider("Output white", c.outWhite * 255, 0...255, 255, "%.0f", 1, "levels.outWhite") { v, f in update({ $0.outWhite = v / 255 }, f) }
    }

    // MARK: Curves

    @ViewBuilder private func curves(_ master: [[Double]], _ rgb: [[[Double]]]) -> some View {
        channelPicker
        let points = channel == 0 ? master : rgb[channel - 1]
        let curveChannel: CurveChannel = [.rgb, .red, .green, .blue][channel]
        DocCurveEditor(points: points, channel: curveChannel, revision: revision &+ channel) { pts, final in
            var m = master, r = rgb
            if channel == 0 { m = pts } else { r[channel - 1] = pts }
            set(.curves(master: m, rgb: r), final)
        }
        .aspectRatio(1, contentMode: .fit)
        HStack(spacing: Theme.Space.xs) {
            Button("Reset \(["RGB", "Red", "Green", "Blue"][channel])") {
                var m = master, r = rgb
                if channel == 0 { m = [] } else { r[channel - 1] = [] }
                set(.curves(master: m, rgb: r), true)
            }
            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            .accessibilityIdentifier("document.properties.curves.reset")
            Spacer()
        }
        .padding(.top, Theme.Space.s)
        Hint("Click to add a point, drag to move, double-click to remove.")
    }

    // MARK: Hue / Saturation

    @ViewBuilder private func hueSat(_ h: Double, _ s: Double, _ l: Double, _ colorize: Bool) -> some View {
        slider("Hue", h, colorize ? 0...360 : -180...180, 0, colorize ? "%.0f°" : "%+.0f°", 1, "hueSaturation.hue") { v, f in
            set(.hueSaturation(hue: v, saturation: s, lightness: l, colorize: colorize), f)
        }
        slider("Saturation", s, colorize ? 0...100 : -100...100, colorize ? 25 : 0, "%+.0f", 1, "hueSaturation.saturation") { v, f in
            set(.hueSaturation(hue: h, saturation: v, lightness: l, colorize: colorize), f)
        }
        slider("Lightness", l, -100...100, 0, "%+.0f", 1, "hueSaturation.lightness") { v, f in
            set(.hueSaturation(hue: h, saturation: s, lightness: v, colorize: colorize), f)
        }
        Toggle("Colorize", isOn: Binding(get: { colorize }, set: { on in
            set(.hueSaturation(hue: on ? max(h, 0) : h, saturation: on ? max(s, 25) : s, lightness: l, colorize: on), true)
        }))
        .toggleStyle(.checkbox)
        .font(Theme.Fonts.caption)
        .accessibilityIdentifier("document.properties.hueSaturation.colorize")
    }

    // MARK: Exposure

    @ViewBuilder private func exposure(_ e: Double, _ o: Double, _ g: Double) -> some View {
        slider("Exposure", e, -20...20, 0, "%+.2f", 0.01, "exposure.exposure") { v, f in set(.exposure(exposure: v, offset: o, gamma: g), f) }
        slider("Offset", o, -0.5...0.5, 0, "%+.4f", 0.0001, "exposure.offset") { v, f in set(.exposure(exposure: e, offset: v, gamma: g), f) }
        slider("Gamma correction", g, 0.01...9.99, 1, "%.2f", 0.01, "exposure.gamma") { v, f in
            set(.exposure(exposure: e, offset: o, gamma: v), f)
        }
    }

    // MARK: Channel Mixer

    @ViewBuilder private func mixer(_ m: [[Double]], _ k: [Double], _ mono: Bool) -> some View {
        SegmentedPicker(selection: $mixerRow, segments: [
            .init(value: 0, title: mono ? "Gray" : "Red"), .init(value: 1, title: "Green"), .init(value: 2, title: "Blue"),
        ], height: Theme.Height.small)
        .disabled(mono)
        .padding(.bottom, Theme.Space.xs)
        .accessibilityIdentifier("document.properties.channelMixer.output")
        let row = mono ? 0 : mixerRow
        let update = { (col: Int?, value: Double, final: Bool) in
            var mm = m, kk = k
            if let col { mm[row][col] = value / 100 } else { kk[row] = value / 100 }
            set(.channelMixer(matrix: mm, constant: kk, monochrome: mono), final)
        }
        ForEach(0..<3, id: \.self) { c in
            slider(["Red", "Green", "Blue"][c], m[row][c] * 100, -200...200, row == c ? 100 : 0, "%+.0f %%", 1,
                   "channelMixer.\(["red", "green", "blue"][c])") { v, f in update(c, v, f) }
        }
        slider("Constant", k[row] * 100, -200...200, 0, "%+.0f %%", 1, "channelMixer.constant") { v, f in update(nil, v, f) }
        Toggle("Monochrome", isOn: Binding(get: { mono }, set: { set(.channelMixer(matrix: m, constant: k, monochrome: $0), true) }))
            .toggleStyle(.checkbox)
            .font(Theme.Fonts.caption)
            .accessibilityIdentifier("document.properties.channelMixer.monochrome")
    }
}

/// Fill layer editors: solid colour (colour well), gradient stops, pattern placeholder.
struct FillEditor: View {
    let document: DocumentController
    let id: DocLayerID
    let fill: FillModel

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            SubHeader(fill.kind.title)
            switch fill {
            case .solid(let c):
                HStack(spacing: Theme.Space.s) {
                    Text("Color").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        .frame(width: Theme.Width.label, alignment: .leading)
                    DocColorWell(rgb: c, identifier: "document.properties.fill.color") { rgb in
                        document.setFill(id, .solid(color: rgb), debounce: true)
                    }
                    .frame(width: Theme.Height.large * 2, height: Theme.Height.regular)
                    Spacer()
                }
            case .gradient(let radial, let start, let end, let stops):
                gradient(radial, start, end, stops)
            case .pattern(let w, let h, _):
                InfoRow(label: "Pattern", value: "\(w) × \(h) px")
                Hint("Pattern choice and scale arrive with the pattern library.")
            }
        }
    }

    @ViewBuilder private func gradient(_ radial: Bool, _ start: [Double], _ end: [Double], _ stops: [FillModel.Stop]) -> some View {
        let sorted = stops.sorted { $0.position < $1.position }
        let apply = { (s: [FillModel.Stop], r: Bool, final: Bool) in
            if final { document.setFill(id, .gradient(radial: r, start: start, end: end, stops: s), debounce: true) }
        }
        SegmentedPicker(selection: Binding(get: { radial }, set: { apply(sorted, $0, true) }), segments: [
            .init(value: false, title: "Linear"), .init(value: true, title: "Radial"),
        ], height: Theme.Height.small)
        .accessibilityIdentifier("document.properties.fill.gradientKind")
        LinearGradient(stops: sorted.map { .init(color: documentColor($0.color), location: $0.position) },
                       startPoint: .leading, endPoint: .trailing)
            .frame(height: Theme.Height.small)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
            .padding(.vertical, Theme.Space.xs)
            .accessibilityIdentifier("document.properties.fill.gradientPreview")
        ForEach(Array(sorted.enumerated()), id: \.offset) { i, stop in
            HStack(spacing: Theme.Space.s) {
                DocColorWell(rgb: stop.color, identifier: "document.properties.fill.stop.\(i).color") { rgb in
                    var s = sorted
                    s[i].color = rgb + [s[i].color.count > 3 ? s[i].color[3] : 1]
                    apply(s, radial, true)
                }
                .frame(width: Theme.Height.large, height: Theme.Height.small)
                DocSlider(title: "Stop \(i + 1)", value: stop.position * 100, range: 0...100, defaultValue: i == 0 ? 0 : 100,
                          format: "%.0f %%", identifier: "document.properties.fill.stop.\(i).position",
                          revision: document.revision) { v, final in
                    var s = sorted
                    s[i].position = v / 100
                    apply(s, radial, final)
                }
                .frame(height: Theme.Height.slider)
                IconButton(symbol: "minus", help: "Remove this stop", size: Theme.Height.small) {
                    var s = sorted
                    s.remove(at: i)
                    apply(s, radial, true)
                }
                .disabled(sorted.count <= 2)
                .accessibilityIdentifier("document.properties.fill.stop.\(i).remove")
            }
        }
        HStack(spacing: Theme.Space.xs) {
            Button("Add Stop") {
                var s = sorted
                let mid = s.count >= 2 ? (s[0].position + s[1].position) / 2 : 0.5
                s.append(.init(position: mid, color: s.first?.color ?? [0.5, 0.5, 0.5, 1]))
                apply(s, radial, true)
            }
            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            .accessibilityIdentifier("document.properties.fill.addStop")
            Button("Reverse") {
                apply(sorted.map { .init(position: 1 - $0.position, color: $0.color) }, radial, true)
            }
            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            .accessibilityIdentifier("document.properties.fill.reverse")
            Spacer()
        }
    }
}
