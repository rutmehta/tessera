import AppKit
import SwiftUI
import TesseraCore

/// The Layer Style inspector (WP B5-07): a floating panel beside the document. Left, Blending
/// Options and the effect kinds in the engine's stacking order (top first), each present effect
/// with its enable checkbox, repeatable kinds with "+"; right, the chosen effect's editor generated
/// from the engine's schema. Edits apply live; a drag or a click is one history node. It follows
/// the document's primary layer.
struct LayerStyleInspector: View {
    let styles: DocumentStyles

    var body: some View {
        if let doc = styles.document, let layer = doc.primary {
            content(doc, layer)
        } else {
            VStack(spacing: Theme.Space.s) {
                Hint("Select a layer to edit its layer style.")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.panel)
        }
    }

    @ViewBuilder private func content(_ doc: DocumentController, _ layer: LayerRecord) -> some View {
        let model = styles.model(doc, layer.id) ?? LayerStyleModel()
        let styleable = DocumentStyles.canStyle(layer)
        SheetScaffold(title: "Layer Style", subtitle: layer.name) {
            if layer.locks.all {
                Chip(text: "Locked", color: Theme.warning, style: .outlined)
                    .help("Lock All is on: unlock the layer to change its layer style")
            }
        } content: {
            HStack(spacing: 0) {
                EffectList(styles: styles, doc: doc, layer: layer, model: model, styleable: styleable)
                    .frame(width: Theme.Width.inspectorMin * 0.75)
                    .accessibilityIdentifier("document.layerStyle.list")
                Rectangle().fill(Theme.hairline).frame(width: Theme.Space.hairline)
                ScrollView {
                    VStack(alignment: .leading, spacing: Theme.Space.xs) {
                        detail(doc, layer, model, styleable)
                    }
                    .padding(Theme.Space.l)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .accessibilityIdentifier("document.layerStyle.detail")
            }
        } leading: {
            Text(summary(model)).monospacedDigit()
        } actions: {
            Button("Done") { styles.closeInspector() }
                .sheetButton(primary: true)
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("document.layerStyle.done")
        }
        .frame(minWidth: 600, minHeight: 520)   // lint:allow (panel content size)
    }

    private func summary(_ m: LayerStyleModel) -> String {
        let on = m.effects.filter(\.enabled).count
        return m.isEmpty ? "No effects" : "\(m.effects.count) effect\(m.effects.count == 1 ? "" : "s"), \(on) on"
    }

    @ViewBuilder private func detail(_ doc: DocumentController, _ layer: LayerRecord, _ model: LayerStyleModel,
                                     _ styleable: Bool) -> some View {
        switch styles.pane {
        case .effect(let i) where model.effects.indices.contains(i):
            if let schema = styles.schema?.effect(model.effects[i].kind) {
                EffectEditor(styles: styles, doc: doc, layer: layer.id, index: i, model: model, schema: schema)
                    .disabled(layer.locks.all)
            }
        default:
            BlendingOptions(styles: styles, doc: doc, layer: layer, model: model, styleable: styleable)
        }
    }
}

/// The inspector's left column.
private struct EffectList: View {
    let styles: DocumentStyles
    let doc: DocumentController
    let layer: LayerRecord
    let model: LayerStyleModel
    let styleable: Bool

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    row(title: "Blending Options", selected: styles.pane == .blending, id: "blending") {
                        styles.pane = .blending
                    }
                    Hairline().padding(.vertical, Theme.Space.xs)
                    ForEach(kinds, id: \.self) { kind in
                        let ids = model.indices(of: kind)
                        if ids.isEmpty {
                            effectRow(kind: kind, index: nil)
                        } else {
                            ForEach(ids, id: \.self) { i in effectRow(kind: kind, index: i) }
                        }
                    }
                }
                .padding(.vertical, Theme.Space.s)
            }
            Hairline()
            footer
        }
        .background(Theme.panel)
        .disabled(layer.locks.all)
    }

    /// Menu kinds, plus the generic overlay when the layer has one (PSD imports, agents).
    private var kinds: [StyleEffectKind] {
        StyleEffectKind.allCases.filter { $0 != .overlay || model.count(of: .overlay) > 0 }
    }

    private func row(title: String, selected: Bool, id: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(Theme.Fonts.label)
                .fontWeight(selected ? .medium : .regular)
                .foregroundStyle(Theme.textPrimary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Theme.Space.s)
                .frame(height: Theme.Height.regular)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(selected ? Theme.accentSubtle : Theme.clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, Theme.Space.xs)
        .accessibilityIdentifier("document.layerStyle.row.\(id)")
    }

    @ViewBuilder private func effectRow(kind: StyleEffectKind, index: Int?) -> some View {
        let selected = index.map { styles.pane == .effect($0) } ?? false
        let enabled = index.map { model.effects[$0].enabled } ?? false
        let id = index.map { "\(kind.rawValue).\($0)" } ?? kind.rawValue
        HStack(spacing: Theme.Space.xs) {
            Toggle("", isOn: Binding(get: { enabled }, set: { on in
                if let index { styles.setEnabled(doc, layer.id, index, on) } else { styles.addEffect(doc, layer.id, kind) }
            }))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .disabled(!styleable)
            .help(index == nil ? "Add \(kind.title)" : enabled ? "Hide this effect" : "Show this effect")
            .accessibilityIdentifier("document.layerStyle.enable.\(id)")
            Text(kind.title)
                .font(Theme.Fonts.label)
                .fontWeight(selected ? .medium : .regular)
                .foregroundStyle(index == nil ? Theme.textSecondary : enabled ? Theme.textPrimary : Theme.textTertiary)
                .lineLimit(1)
            Spacer(minLength: 0)
            if kind.isRepeatable, let index {
                IconButton(symbol: "plus", help: "Add another \(kind.title) above this one", size: Theme.Height.small) {
                    styles.addEffect(doc, layer.id, kind, above: index)
                }
                .disabled(!model.canAdd(kind))
                .accessibilityIdentifier("document.layerStyle.add.\(id)")
            }
        }
        .padding(.leading, Theme.Space.s)
        .padding(.trailing, Theme.Space.xs)
        .frame(height: Theme.Height.regular)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(selected ? Theme.accentSubtle : Theme.clear))
        .contentShape(Rectangle())
        .onTapGesture {
            if let index { styles.pane = .effect(index) } else if styleable { styles.addEffect(doc, layer.id, kind) }
        }
        .padding(.horizontal, Theme.Space.xs)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("document.layerStyle.row.\(id)")
    }

    private var footer: some View {
        HStack(spacing: Theme.Space.xxs) {
            Menu {
                ForEach(StyleEffectKind.menuKinds) { k in
                    Button(k.title) { styles.addEffect(doc, layer.id, k) }.disabled(!model.canAdd(k))
                }
            } label: { Image(systemName: "fx") }
                .menuStyle(IconMenuStyle())
                .disabled(!styleable)
                .help("Add an effect")
                .accessibilityIdentifier("document.layerStyle.addMenu")
            Spacer(minLength: 0)
            IconButton(symbol: "trash", help: "Delete the selected effect", size: Theme.Height.small) {
                if case .effect(let i) = styles.pane { styles.removeEffect(doc, layer.id, i) }
            }
            .disabled({ if case .effect = styles.pane { false } else { true } }())
            .accessibilityIdentifier("document.layerStyle.delete")
        }
        .padding(.horizontal, Theme.Space.s)
        .frame(height: Theme.Height.large)
    }
}

/// Blending Options: the layer's blend mode, opacity and fill (the Layers panel's controls), and
/// Scale Effects.
private struct BlendingOptions: View {
    let styles: DocumentStyles
    let doc: DocumentController
    let layer: LayerRecord
    let model: LayerStyleModel
    let styleable: Bool

    var body: some View {
        let revision = doc.revision &+ styles.revision
        SubHeader("General Blending")
        HStack(spacing: Theme.Space.s) {
            Text("Blend Mode").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.Width.labelWide, alignment: .leading)
            BlendModeMenu(document: doc, node: layer)
            Spacer(minLength: 0)
        }
        DocSlider(title: "Opacity", value: Double(layer.opacity) * 100, range: 0...100, defaultValue: 100, format: "%.0f %%",
                  identifier: "document.layerStyle.opacity", revision: revision) { v, final in doc.setOpacity(v, final: final) }
            .frame(height: Theme.Height.slider)
        SubHeader("Advanced Blending")
        DocSlider(title: "Fill Opacity", value: Double(layer.fillOpacity) * 100, range: 0...100, defaultValue: 100,
                  format: "%.0f %%", enabled: layer.kind != .group, identifier: "document.layerStyle.fill",
                  revision: revision) { v, final in doc.setFillOpacity(v, final: final) }
            .frame(height: Theme.Height.slider)
        Hint("Fill Opacity fades the layer's own pixels only; its effects stay at full strength.")
        SubHeader("Effects")
        if let f = styles.schema?.scale {
            DocSlider(title: f.title, value: model.scale * f.displayScale, range: f.min * f.displayScale...f.max * f.displayScale,
                      defaultValue: 100, format: "%.0f %%", enabled: styleable && !model.isEmpty,
                      identifier: "document.layerStyle.scale", revision: revision) { v, final in
                guard var m = styles.model(doc, layer.id) else { return }
                m.scale = max(v / f.displayScale, f.min)
                styles.apply(doc, layer.id, m, label: "Scale Effects", final: final)
            }
            .frame(height: Theme.Height.slider)
        }
        HStack(spacing: Theme.Space.s) {
            Button("Global Light…") { styles.openGlobalLight(doc) }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .accessibilityIdentifier("document.layerStyle.globalLight")
            Text(lightText).font(Theme.Fonts.caption).monospacedDigit().foregroundStyle(Theme.textSecondary)
        }
        .padding(.top, Theme.Space.xs)
        if !styleable {
            StatusLine(text: layer.kind == .adjustment
                ? "Adjustment layers cannot have effects."
                : "Pass-through groups cannot have effects: set the group's blend mode to Normal (isolated) first.",
                kind: .warning)
                .padding(.top, Theme.Space.s)
        } else if layer.locks.all {
            StatusLine(text: "Lock All is on: unlock the layer to change its effects.", kind: .warning)
                .padding(.top, Theme.Space.s)
        }
    }

    private var lightText: String {
        let g = styles.globalLight(doc)
        return "\(Int(g.angle.rounded()))°, altitude \(Int(g.altitude.rounded()))°"
    }
}

/// Layer ▸ Layer Style ▸ Global Light…: the document's light for every effect that uses it.
struct GlobalLightPanel: View {
    let styles: DocumentStyles

    var body: some View {
        if let doc = styles.document {
            let g = styles.globalLight(doc)
            let revision = doc.revision &+ styles.revision
            let followers = styles.rows(doc).keys.filter { id in styles.model(doc, id)?.followsGlobalLight == true }.count
            SheetScaffold(title: "Global Light", subtitle: "\(followers) layer\(followers == 1 ? "" : "s") use it") {
                EmptyView()
            } content: {
                HStack(alignment: .center, spacing: Theme.Space.m) {
                    AngleDial(degrees: g.angle) { v, final in
                        styles.setGlobalLight(doc, DocGlobalLight(angle: v, altitude: g.altitude), final: final)
                    }
                    .frame(width: Theme.Height.large * 2, height: Theme.Height.large * 2)
                    .accessibilityIdentifier("document.globalLight.dial")
                    VStack(spacing: 0) {
                        DocSlider(title: "Angle", value: g.angle, range: -180...180, defaultValue: 120, format: "%.0f°",
                                  identifier: "document.globalLight.angle", revision: revision) { v, final in
                            styles.setGlobalLight(doc, DocGlobalLight(angle: v, altitude: g.altitude), final: final)
                        }
                        .frame(height: Theme.Height.slider)
                        DocSlider(title: "Altitude", value: g.altitude, range: 0...90, defaultValue: 30, format: "%.0f°",
                                  identifier: "document.globalLight.altitude", revision: revision) { v, final in
                            styles.setGlobalLight(doc, DocGlobalLight(angle: g.angle, altitude: v), final: final)
                        }
                        .frame(height: Theme.Height.slider)
                    }
                }
                .padding(Theme.Space.l)
            } leading: {
                EmptyView()
            } actions: {
                Button("Done") { styles.closeGlobalLight() }
                    .sheetButton(primary: true)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("document.globalLight.done")
            }
        } else {
            Hint("Open a document to set its global light.")
        }
    }
}
