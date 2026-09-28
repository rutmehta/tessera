import AppKit
import SwiftUI
import TesseraCore

/// Layers panel: blend mode, Opacity and Fill (ValueSlider: live drag, one history entry on
/// release), lock buttons and a filter row placeholder over the AppKit outline, and a footer with
/// add layer, add mask, add adjustment, group and delete. B5-16: the inspector's Stack tab; Opacity
/// and Fill share a row only when the interior is at least 300 pt wide (H4), and the filter field
/// shrinks to 48 pt, then moves into an overflow menu (H5).
struct LayersPanel: View {
    let document: DocumentController
    @State private var interior: CGFloat = 0

    /// Outline rows always visible (the outline scrolls the rest).
    static let outlineMinimum: CGFloat = Theme.Height.sectionHeader * 4
    private static let filterMinimum: CGFloat = Theme.Width.labelWide / 2
    private static let filterHelp = "Filter by kind, name, mode or attribute (arrives with the next document work package)"

    /// The panel's minimum height (sliders stacked, the narrow case): the Stack tab's minimum in the
    /// inspector budget. Top padding, blend row, two slider rows, lock row, bottom padding, hairline,
    /// outline minimum, hairline, footer.
    static var minimumHeight: CGFloat {
        let controls = Theme.Space.s + Theme.Height.small + Theme.Height.slider * 2 + Theme.Height.small + Theme.Space.xs * 3
        return controls + Theme.Space.s + Theme.Space.hairline + outlineMinimum + Theme.Space.hairline + Theme.Height.large
    }

    var body: some View {
        let primary = document.primary
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                HStack(spacing: Theme.Space.s) {
                    BlendModeMenu(document: document, node: primary)
                    Spacer(minLength: 0)
                }
                if DocumentLayersRow.slidersSideBySide(interiorWidth: interior) {
                    HStack(spacing: Theme.Space.m) {
                        opacitySlider(primary)
                        fillSlider(primary)
                    }
                } else {
                    opacitySlider(primary)
                    fillSlider(primary)
                }
                ViewThatFits(in: .horizontal) {
                    lockRow(filterField: true)
                    lockRow(filterField: false)
                }
                .disabled(primary == nil)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { interior = $0 }
            .padding(.horizontal, Theme.Space.gutter)
            .padding(.top, Theme.Space.s)
            .padding(.bottom, Theme.Space.s)
            Hairline()
            LayersOutline(document: document)
                .frame(minHeight: Self.outlineMinimum, maxHeight: .infinity)
                .accessibilityIdentifier("document.layers")
            Hairline()
            footer
                .inspectorProbe("layersFooter")
        }
    }

    private func opacitySlider(_ primary: LayerRecord?) -> some View {
        DocSlider(title: "Opacity", value: Double(primary?.opacity ?? 1) * 100, range: 0...100, defaultValue: 100,
                  format: "%.0f %%", enabled: primary != nil, identifier: "document.layers.opacity",
                  revision: document.revision) { v, final in document.setOpacity(v, final: final) }
            .frame(height: Theme.Height.slider)
    }

    private func fillSlider(_ primary: LayerRecord?) -> some View {
        DocSlider(title: "Fill", value: Double(primary?.fillOpacity ?? 1) * 100, range: 0...100, defaultValue: 100,
                  format: "%.0f %%", enabled: primary != nil && primary?.kind != .group,
                  identifier: "document.layers.fill", revision: document.revision) { v, final in
            document.setFillOpacity(v, final: final)
        }
        .frame(height: Theme.Height.slider)
    }

    private func lockRow(filterField: Bool) -> some View {
        HStack(spacing: Theme.Space.xxs) {
            Text("Lock").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .padding(.trailing, Theme.Space.xs)
                .fixedSize()
            lockButton(.transparency, "checkerboard.rectangle", "Lock transparent pixels")
            lockButton(.pixels, "paintbrush.pointed", "Lock image pixels")
            lockButton(.position, "arrow.up.and.down.and.arrow.left.and.right", "Lock position")
            lockButton(.all, "lock", "Lock all")
            Spacer(minLength: Theme.Space.s)
            if filterField {
                FieldContainer(symbol: "line.3.horizontal.decrease") {
                    Text("Filter").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).lineLimit(1)
                    Spacer(minLength: 0)
                }
                .frame(minWidth: Self.filterMinimum, idealWidth: Theme.Width.labelWide, maxWidth: Theme.Width.labelWide)
                .opacity(Theme.Opacity.disabled)
                .help(Self.filterHelp)
                .accessibilityIdentifier("document.layers.filter")
            } else {
                Menu {
                    Button("Filter Layers…") {}.disabled(true)
                    Text(Self.filterHelp)
                } label: { Image(systemName: "line.3.horizontal.decrease") }
                    .menuStyle(IconMenuStyle())
                    .help(Self.filterHelp)
                    .accessibilityIdentifier("document.layers.filter")
            }
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
                ForEach(Array(AdjustmentModel.Kind.layerMenuSections.enumerated()), id: \.offset) { i, section in
                    if i > 0 { Divider() }
                    ForEach(section) { k in
                        Button { document.addAdjustment(k) } label: { Label(k.title, systemImage: k.symbol) }
                    }
                }
            } label: { Image(systemName: "circle.lefthalf.filled") }
                .menuStyle(IconMenuStyle())
                .help("New adjustment layer")
                .accessibilityIdentifier("document.layers.addAdjustment")
            LayerStyleFooterButton(document: document)   // B5-07
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
