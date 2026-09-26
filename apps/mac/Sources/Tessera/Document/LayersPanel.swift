import AppKit
import SwiftUI
import TesseraCore

/// Layers panel: blend mode, Opacity and Fill (ValueSlider: live drag, one history entry on
/// release), lock buttons and a filter row placeholder over the AppKit outline, and a footer with
/// add layer, add mask, add adjustment, group and delete.
struct LayersPanel: View {
    let document: DocumentController

    var body: some View {
        let primary = document.primary
        let revision = document.revision
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                HStack(spacing: Theme.Space.s) {
                    BlendModeMenu(document: document, node: primary)
                    Spacer(minLength: 0)
                }
                HStack(spacing: Theme.Space.m) {
                    DocSlider(title: "Opacity", value: Double(primary?.opacity ?? 1) * 100, range: 0...100, defaultValue: 100,
                              format: "%.0f %%", enabled: primary != nil, identifier: "document.layers.opacity",
                              revision: revision) { v, final in document.setOpacity(v, final: final) }
                        .frame(height: Theme.Height.slider)
                    DocSlider(title: "Fill", value: Double(primary?.fillOpacity ?? 1) * 100, range: 0...100, defaultValue: 100,
                              format: "%.0f %%", enabled: primary != nil && primary?.kind != .group,
                              identifier: "document.layers.fill", revision: revision) { v, final in
                        document.setFillOpacity(v, final: final)
                    }
                    .frame(height: Theme.Height.slider)
                }
                HStack(spacing: Theme.Space.xxs) {
                    Text("Lock").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        .padding(.trailing, Theme.Space.xs)
                    lockButton(.transparency, "checkerboard.rectangle", "Lock transparent pixels")
                    lockButton(.pixels, "paintbrush.pointed", "Lock image pixels")
                    lockButton(.position, "arrow.up.and.down.and.arrow.left.and.right", "Lock position")
                    lockButton(.all, "lock", "Lock all")
                    Spacer(minLength: Theme.Space.s)
                    FieldContainer(symbol: "line.3.horizontal.decrease") {
                        Text("Filter").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                        Spacer(minLength: 0)
                    }
                    .frame(width: Theme.Width.labelWide)
                    .opacity(Theme.Opacity.disabled)
                    .help("Filter by kind, name, mode or attribute (arrives with the next document work package)")
                    .accessibilityIdentifier("document.layers.filter")
                }
                .disabled(primary == nil)
            }
            .padding(.horizontal, Theme.Space.gutter)
            .padding(.bottom, Theme.Space.s)
            Hairline()
            LayersOutline(document: document)
                .frame(minHeight: Theme.Height.sectionHeader * 4)
                .accessibilityIdentifier("document.layers")
            Hairline()
            footer
        }
    }

    private func lockButton(_ lock: DocumentController.Lock, _ symbol: String, _ help: String) -> some View {
        IconButton(symbol: symbol, help: help, on: document.isLocked(lock), size: Theme.Height.small) {
            document.toggleLock(lock)
        }
        .accessibilityIdentifier("document.layers.lock.\(lock.rawValue)")
    }

    private var footer: some View {
        HStack(spacing: Theme.Space.xxs) {
            Menu {
                Button("New Layer") { document.addLayer(.pixel) }
                Button("New Group") { document.addLayer(.group(mode: .passThrough)) }
                Menu("New Fill Layer") {
                    ForEach(FillModel.Kind.allCases) { k in Button(k.title) { document.addFill(k) } }
                }
            } label: { Image(systemName: "plus.square") }
                .menuStyle(IconMenuStyle())
                .help("New layer, group or fill layer")
                .accessibilityIdentifier("document.layers.add")
            IconButton(symbol: "circle.rectangle.filled.pattern.diagonalline", help: "Add a layer mask (reveal all; from the selection when there is one)") {
                document.addMask(document.marquee != nil ? .fromSelection : .revealAll)
            }
            .disabled(document.primary == nil || document.primary?.hasMask == true)
            .accessibilityIdentifier("document.layers.addMask")
            Menu {
                ForEach(AdjustmentModel.Kind.allCases) { k in
                    Button { document.addAdjustment(k) } label: { Label(k.title, systemImage: k.symbol) }
                }
            } label: { Image(systemName: "circle.lefthalf.filled") }
                .menuStyle(IconMenuStyle())
                .help("New adjustment layer")
                .accessibilityIdentifier("document.layers.addAdjustment")
            Spacer(minLength: 0)
            IconButton(symbol: "folder.badge.plus", help: "Group the selected layers (⌘G)") { document.groupSelection() }
                .accessibilityIdentifier("document.layers.group")
            IconButton(symbol: "trash", help: "Delete the selected layers (⌫)") { document.deleteSelection() }
                .disabled(document.selection.isEmpty)
                .accessibilityIdentifier("document.layers.delete")
        }
        .padding(.horizontal, Theme.Space.s)
        .frame(height: Theme.Height.large)
    }
}

/// Blend mode pop-up: the 27 modes in Photoshop's groups (dividers between them), plus Pass
/// Through for groups.
struct BlendModeMenu: View {
    let document: DocumentController
    let node: LayerRecord?

    var body: some View {
        let title = node.map { DocBlendMode.title(backendName: $0.blendMode, groupMode: $0.groupMode) } ?? "Normal"
        Menu {
            if node?.kind == .group {
                Toggle("Pass Through", isOn: Binding(get: { node?.groupMode == .passThrough },
                                                    set: { if $0 { document.setBlendMode(nil) } }))
                Divider()
            }
            ForEach(Array(DocBlendMode.grouped.enumerated()), id: \.offset) { i, section in
                if i > 0 { Divider() }
                ForEach(section.1) { mode in
                    Toggle(mode.title, isOn: Binding(get: { node?.groupMode != .passThrough && node?.blendMode == mode.backendName },
                                                     set: { if $0 { document.setBlendMode(mode) } }))
                }
            }
        } label: {
            Text(title).frame(minWidth: Theme.Width.labelWide, alignment: .leading)
        }
        .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
        .disabled(node == nil)
        .help("Blend mode")
        .accessibilityIdentifier("document.layers.blendMode")
    }
}
