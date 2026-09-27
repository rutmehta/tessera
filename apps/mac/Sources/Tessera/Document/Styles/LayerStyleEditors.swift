import AppKit
import SwiftUI
import TesseraCore

/// One effect's editor, generated from the engine's schema (`style_effects_schema_json`): sliders
/// for numbers (live drag, one history node on release), an angle dial beside angles (Use Global
/// Light moves the document's light, as in Photoshop), colour wells, blend mode pop-ups, choices,
/// fills. Contour, jitter and texture are listed as kept-but-not-rendered metadata, not controls.
struct EffectEditor: View {
    let styles: DocumentStyles
    let doc: DocumentController
    let layer: DocLayerID
    let index: Int
    let model: LayerStyleModel
    let schema: StyleSchema.Effect

    private var effect: StyleEffect { model.effects[index] }
    private var revision: Int { doc.revision &+ styles.revision }
    private var global: DocGlobalLight { styles.globalLight(doc) }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.s) {
                Text(schema.title).font(Theme.Fonts.title).foregroundStyle(Theme.textPrimary)
                Spacer(minLength: 0)
                if model.count(of: effect.kind) > 1 {
                    let n = (model.indices(of: effect.kind).firstIndex(of: index) ?? 0) + 1
                    Chip(text: "\(n) of \(model.count(of: effect.kind))")
                }
                if !schema.psd { Chip(text: "Not kept in PSD").help("PSD files store shadows, glows, solid overlays and solid strokes; save as .tessera-doc to keep this effect") }
            }
            .padding(.bottom, Theme.Space.xs)
            ForEach(schema.fields) { f in field(f) }
            metadata
        }
        .accessibilityIdentifier("document.layerStyle.editor.\(effect.kind.rawValue)")
    }

    // MARK: Edits

    /// Re-reads the live model (so fields of one drag compose), changes this effect and writes it.
    private func update(final: Bool, _ change: (inout StyleEffect) -> Void) {
        guard var m = styles.model(doc, layer), m.effects.indices.contains(index) else { return }
        change(&m.effects[index])
        styles.apply(doc, layer, m, label: schema.title, final: final)
    }

    private func updateColor(_ change: (inout StyleEffect) -> Void) {
        guard var m = styles.model(doc, layer), m.effects.indices.contains(index) else { return }
        change(&m.effects[index])
        styles.applyColor(doc, layer, m, label: schema.title)
    }

    /// The light of this effect: the document's Global Light when the effect uses it.
    private func setLight(angle: Double? = nil, altitude: Double? = nil, final: Bool) {
        guard var m = styles.model(doc, layer), m.effects.indices.contains(index) else { return }
        let g = global
        let a = angle ?? m.angle(of: index, global: g)
        if m.effects[index].usesGlobalLight {
            if let changed = m.setLight(of: index, angle: a, altitude: altitude, global: g) {
                styles.setGlobalLight(doc, changed, final: final)
            } else if final {
                styles.commit(doc, "Global Light")
            }
        } else {
            _ = m.setLight(of: index, angle: a, altitude: altitude, global: g)
            styles.apply(doc, layer, m, label: schema.title, final: final)
        }
    }

    // MARK: Fields

    private func identifier(_ f: StyleSchema.Field) -> String { "document.layerStyle.\(effect.kind.rawValue).\(f.key)" }

    private func format(_ f: StyleSchema.Field) -> String {
        switch f.unit {
        case "%": "%.0f %%"
        case "°": "%.0f°"
        case "": "%.0f"
        default: "%.0f " + f.unit
        }
    }

    private func defaultNumber(_ f: StyleSchema.Field) -> Double { schema.defaults[f.key]?.double ?? f.min }

    @ViewBuilder private func field(_ f: StyleSchema.Field) -> some View {
        switch f.type {
        case .number: number(f)
        case .angle: angle(f)
        case .bool: boolean(f)
        case .color: color(f)
        case .blendMode: blendMode(f)
        case .choice: choice(f)
        case .fill: fill(f)
        }
    }

    @ViewBuilder private func number(_ f: StyleSchema.Field) -> some View {
        let isAltitude = effect.kind == .bevel && f.key == "elevation"
        let stored = isAltitude ? model.altitude(of: index, global: global) : (effect.number(f.key) ?? defaultNumber(f))
        let k = f.displayScale
        DocSlider(title: f.title + (isAltitude && effect.usesGlobalLight ? " (global)" : ""), value: stored * k,
                  range: f.min * k...max(f.max, stored) * k, defaultValue: defaultNumber(f) * k, format: format(f),
                  identifier: identifier(f), revision: revision) { v, final in
            if isAltitude {
                setLight(altitude: v / k, final: final)
            } else {
                update(final: final) { $0.setNumber(f.key, v / k) }
            }
        }
        .frame(height: Theme.Height.slider)
    }

    @ViewBuilder private func angle(_ f: StyleSchema.Field) -> some View {
        // Satin's angle is its own; shadows and bevels may follow the Global Light.
        let a = effect.kind.usesGlobalLight ? model.angle(of: index, global: global) : (effect.number(f.key) ?? defaultNumber(f))
        HStack(spacing: Theme.Space.s) {
            AngleDial(degrees: a) { v, final in
                if effect.kind.usesGlobalLight { setLight(angle: v, final: final) } else { update(final: final) { $0.setNumber(f.key, v) } }
            }
            .frame(width: Theme.Height.large, height: Theme.Height.large)
            .accessibilityIdentifier(identifier(f) + ".dial")
            DocSlider(title: f.title + (effect.usesGlobalLight ? " (global)" : ""), value: a, range: f.min...f.max,
                      defaultValue: defaultNumber(f), format: "%.0f°", identifier: identifier(f), revision: revision) { v, final in
                if effect.kind.usesGlobalLight { setLight(angle: v, final: final) } else { update(final: final) { $0.setNumber(f.key, v) } }
            }
            .frame(height: Theme.Height.slider)
        }
    }

    @ViewBuilder private func boolean(_ f: StyleSchema.Field) -> some View {
        let on = effect.flag(f.key) ?? (schema.defaults[f.key]?.bool ?? false)
        Toggle(isOn: Binding(get: { on }, set: { v in
            if f.key == "use_global_light" {
                guard var m = styles.model(doc, layer) else { return }
                m.setUsesGlobalLight(v, of: index, global: global)
                styles.apply(doc, layer, m, label: schema.title, final: true)
            } else {
                update(final: true) { $0.setFlag(f.key, v) }
            }
        })) {
            Text(f.title).font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary)
        }
        .toggleStyle(.checkbox)
        .frame(height: Theme.Height.regular)
        .help(f.key == "use_global_light" ? "Follow the document's light (Layer ▸ Layer Style ▸ Global Light); every layer that uses it moves together" : f.title)
        .accessibilityIdentifier(identifier(f))
    }

    private func labelled<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: Theme.Space.s) {
            Text(title).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.Width.labelWide, alignment: .leading)
            content()
            Spacer(minLength: 0)
        }
        .frame(height: Theme.Height.regular)
    }

    @ViewBuilder private func color(_ f: StyleSchema.Field) -> some View {
        let rgba = effect.color(f.key) ?? schema.defaults[f.key]?.doubles ?? [0, 0, 0, 1]
        labelled(f.title) {
            DocColorWell(rgb: Array(rgba.prefix(3)), identifier: identifier(f)) { rgb in
                updateColor { $0.setColor(f.key, rgb + [rgba.count > 3 ? rgba[3] : 1]) }
            }
            .frame(width: Theme.Height.large * 2, height: Theme.Height.regular)
        }
    }

    @ViewBuilder private func blendMode(_ f: StyleSchema.Field) -> some View {
        let current = effect.text(f.key) ?? schema.defaults[f.key]?.string ?? "normal"
        labelled(f.title) {
            MenuPicker(selection: Binding(get: { current }, set: { v in update(final: true) { $0.setText(f.key, v) } }),
                       options: DocBlendMode.allCases.map { ($0.backendName, $0.title) })
                .accessibilityIdentifier(identifier(f))
        }
    }

    @ViewBuilder private func choice(_ f: StyleSchema.Field) -> some View {
        let current = effect.settings[f.key] ?? schema.defaults[f.key] ?? .null
        let selected = f.options.firstIndex { $0.value == current } ?? 0
        let binding = Binding<Int>(get: { selected }, set: { i in
            guard f.options.indices.contains(i) else { return }
            update(final: true) { $0.settings[f.key] = f.options[i].value }
        })
        labelled(f.title) {
            if f.options.count <= 3 {
                SegmentedPicker(selection: binding, segments: f.options.indices.map { .init(value: $0, title: f.options[$0].title) },
                                height: Theme.Height.small)
                    .accessibilityIdentifier(identifier(f))
            } else {
                MenuPicker(selection: binding, options: f.options.indices.map { ($0, f.options[$0].title) })
                    .accessibilityIdentifier(identifier(f))
            }
        }
    }

    @ViewBuilder private func fill(_ f: StyleSchema.Field) -> some View {
        let fill = effect.fill ?? .solid(color: [0, 0, 0])
        if effect.kind == .stroke {
            labelled("Fill Type") {
                SegmentedPicker(selection: Binding(get: { fill.kind }, set: { k in
                    guard k != fill.kind else { return }
                    update(final: true) { $0.fill = FillModel.neutral(k, width: Double(doc.info.width), height: Double(doc.info.height)) }
                }), segments: FillModel.Kind.allCases.map { .init(value: $0, title: $0 == .solid ? "Color" : $0.title) },
                   height: Theme.Height.small)
                .accessibilityIdentifier(identifier(f) + ".kind")
            }
        }
        switch fill {
        case .solid(let c):
            labelled("Color") {
                DocColorWell(rgb: c, identifier: identifier(f) + ".color") { rgb in updateColor { $0.fill = .solid(color: rgb) } }
                    .frame(width: Theme.Height.large * 2, height: Theme.Height.regular)
            }
        case .gradient(let radial, let start, let end, let stops):
            labelled("Gradient") {
                RoundedRectangle(cornerRadius: Theme.Radius.chip)
                    .fill(LinearGradient(stops: stops.sorted { $0.position < $1.position }
                        .map { .init(color: documentColor($0.color), location: $0.position) }, startPoint: .leading, endPoint: .trailing))
                    .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
                    .frame(width: Theme.Width.labelWide * 1.5, height: Theme.Height.small)
                Button("Reverse") {
                    let reversed = stops.map { FillModel.Stop(position: 1 - $0.position, color: $0.color) }
                    update(final: true) { $0.fill = .gradient(radial: radial, start: start, end: end, stops: reversed) }
                }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .accessibilityIdentifier(identifier(f) + ".reverse")
            }
            labelled("Style") {
                SegmentedPicker(selection: Binding(get: { radial }, set: { r in
                    update(final: true) { $0.fill = .gradient(radial: r, start: start, end: end, stops: stops) }
                }), segments: [.init(value: false, title: "Linear"), .init(value: true, title: "Radial")], height: Theme.Height.small)
                .accessibilityIdentifier(identifier(f) + ".style")
            }
            Hint("The gradient spans \(Int(start[0])), \(Int(start[1])) to \(Int(end[0])), \(Int(end[1])) px on the canvas.")
        case .pattern(let w, let h, _):
            InfoRow(label: "Pattern", value: "\(w) × \(h) px")
            Hint("Choosing and scaling patterns arrives with the pattern library.")
        }
    }

    /// Stored-only settings: listed, never offered as working controls.
    @ViewBuilder private var metadata: some View {
        if !schema.metadata.isEmpty {
            SubHeader("Kept, not rendered")
            ForEach(schema.metadata, id: \.key) { m in
                InfoRow(label: m.title, value: metadataValue(m.key))
                    .help(m.note)
                    .accessibilityIdentifier("document.layerStyle.\(effect.kind.rawValue).metadata.\(m.key)")
            }
            Hint(schema.metadata.first?.note ?? "")
        }
    }

    private func metadataValue(_ key: String) -> String {
        switch key {
        case "shape":
            let points = effect.settings["shape"]?["contour"]?.array?.count ?? 0
            let jitter = effect.settings["shape"]?["jitter"]?.double ?? 0
            let contour = points == 0 ? "Linear" : "Custom, \(points) points"
            return jitter > 0 ? "\(contour) · jitter \(Int((jitter * 100).rounded())) %" : contour
        case "texture": return "From the PSD, if any"
        default: return "Stored"
        }
    }
}
