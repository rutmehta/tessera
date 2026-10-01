import AppKit
import SwiftUI
import TesseraCore

/// Layer ▸ Layer Style (WP B5-07): Blending Options…, one item per effect, Copy / Paste / Clear
/// Layer Style and Global Light…. The Layers panel's fx button shows the same menu.
struct LayerStyleMenu: View {
    let doc: DocumentController?
    var styles = DocumentStyles.shared

    var body: some View {
        Menu("Layer Style") { LayerStyleMenuItems(doc: doc, styles: styles) }
            .disabled(doc?.primary == nil)
    }
}

struct LayerStyleMenuItems: View {
    let doc: DocumentController?
    let styles: DocumentStyles

    var body: some View {
        let primary = doc?.primary
        let styleable = DocumentStyles.canStyle(primary) && primary?.locks.all == false
        Button("Blending Options…") { if let doc { styles.open(doc) } }
            .disabled(primary == nil)
        Divider()
        ForEach(StyleEffectKind.menuKinds) { k in
            Button(k.title + "…") { if let doc { styles.open(doc, kind: k) } }
                .disabled(!styleable)
        }
        Divider()
        Button("Copy Layer Style") { if let doc { styles.copy(doc) } }
            .disabled(primary == nil)
        Button("Paste Layer Style") { if let doc { styles.paste(doc) } }
            .disabled(primary == nil || !styles.canPaste(doc))
        Button("Clear Layer Style") { if let doc { styles.clear(doc) } }
            .disabled(primary == nil || primary?.locks.all == true)
        Divider()
        Button("Global Light…") { if let doc { styles.openGlobalLight(doc) } }
            .disabled(doc == nil)
    }
}

/// The Layers panel footer's fx button (Photoshop's "Add a layer style").
struct LayerStyleFooterButton: View {
    let document: DocumentController

    var body: some View {
        Menu {
            LayerStyleMenuItems(doc: document, styles: .shared)
        } label: { Image(systemName: "fx") }
            .menuStyle(IconMenuStyle())
            .disabled(document.primary == nil)
            .help("Add a layer style")
            .accessibilityIdentifier("document.layers.addStyle")
            .accessibilityLabel("Add a layer style")
    }
}

/// Properties panel: a compact summary of the selected layer's effects (WP B5-07). Each effect is
/// a line (off ones in tertiary ink); clicking one opens the Layer Style inspector on it.
struct LayerStylesSummary: View {
    let document: DocumentController
    let layer: LayerRecord
    var styles = DocumentStyles.shared

    var body: some View {
        let model = styles.model(document, layer.id)
        if let model, !model.isEmpty {
            SubHeader("Layer Style")
            ForEach(model.displayOrder, id: \.self) { i in
                let e = model.effects[i]
                Button { styles.open(document, layer: layer.id, effect: i) } label: {
                    HStack(spacing: Theme.Space.xs) {
                        Image(systemName: e.enabled ? "eye" : "eye.slash")
                            .font(Theme.Fonts.iconSmall)
                            .foregroundStyle(e.enabled ? Theme.textSecondary : Theme.textTertiary)
                        Text(e.kind.title).font(Theme.Fonts.caption)
                            .foregroundStyle(e.enabled ? Theme.textPrimary : Theme.textTertiary)
                        Spacer(minLength: 0)
                        Text(detail(e, model, i)).font(Theme.Fonts.caption).monospacedDigit().foregroundStyle(Theme.textTertiary)
                    }
                    .frame(height: Theme.Height.small)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("Edit \(e.kind.title) in the Layer Style inspector")
                .accessibilityIdentifier("document.properties.style.\(i)")
            }
            HStack(spacing: Theme.Space.s) {
                if model.scale != 1 {
                    Text("Scale \(Int((model.scale * 100).rounded())) %").font(Theme.Fonts.caption).monospacedDigit()
                        .foregroundStyle(Theme.textSecondary)
                }
                Spacer(minLength: 0)
                Button("Edit…") { styles.open(document, layer: layer.id) }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("document.properties.style.edit")
            }
            .padding(.top, Theme.Space.xs)
        }
    }

    private func detail(_ e: StyleEffect, _ m: LayerStyleModel, _ i: Int) -> String {
        var parts: [String] = []
        if let size = e.number("size") { parts.append("\(Int(size.rounded())) px") }
        if e.kind.usesGlobalLight {
            parts.append("\(Int(m.angle(of: i, global: styles.globalLight(document)).rounded()))°")
        }
        return parts.joined(separator: " · ")
    }
}
