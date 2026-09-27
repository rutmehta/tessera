import AppKit
import SwiftUI
import TesseraCore

/// Outline item of one effect row (children of a styled layer in the Layers panel).
final class StyleEffectItem: NSObject {
    let layer: DocLayerID
    let effect: LayerStyleRow.Effect
    init(layer: DocLayerID, effect: LayerStyleRow.Effect) { self.layer = layer; self.effect = effect }
}

/// Layer styles in the Layers outline (WP B5-07): an fx glyph on styled layer rows and, under each
/// styled layer that is not a group, one row per effect top first (after any smart filter rows):
/// enable eye and name; double-click opens the Layer Style inspector on it. Groups show the glyph
/// only (their children are the group's rows). `LayersOutlineController` calls in here from a few
/// `// B5-07` hooks.
@MainActor
final class LayerStyleOutline {
    private var rows: [DocLayerID: [StyleEffectItem]] = [:]
    private var styled = Set<DocLayerID>()
    private var styles: DocumentStyles { .shared }

    func reset() {
        rows.removeAll()
        styled.removeAll()
    }

    /// Re-reads the styled layers; returns the layers whose rows or glyph changed.
    func refresh(_ doc: DocumentController) -> [DocLayerID] {
        let all = styles.rows(doc)
        var changed: [DocLayerID] = []
        var next: [DocLayerID: [StyleEffectItem]] = [:]
        for (id, row) in all where doc.node(id)?.kind != .group {
            let old = rows[id] ?? []
            if old.map(\.effect) == row.effects {
                next[id] = old
            } else {
                next[id] = row.effects.map { StyleEffectItem(layer: id, effect: $0) }
                changed.append(id)
            }
        }
        changed += rows.keys.filter { next[$0] == nil }
        let nowStyled = Set(all.keys)
        changed += nowStyled.symmetricDifference(styled)
        styled = nowStyled
        rows = next
        return Array(Set(changed))
    }

    func isStyled(_ layer: DocLayerID) -> Bool { styled.contains(layer) }
    func count(_ layer: DocLayerID) -> Int { rows[layer]?.count ?? 0 }
    func item(_ layer: DocLayerID, _ index: Int) -> StyleEffectItem? {
        guard let r = rows[layer], r.indices.contains(index) else { return nil }
        return r[index]
    }

    func cell(_ outline: NSOutlineView, _ item: StyleEffectItem, doc: DocumentController) -> NSView {
        let cell = (outline.makeView(withIdentifier: StyleEffectRowCell.identifier, owner: nil) as? StyleEffectRowCell)
            ?? StyleEffectRowCell()
        cell.configure(item) { [weak doc] in
            guard let doc else { return }
            DocumentStyles.shared.setEnabled(doc, item.layer, item.effect.index, !item.effect.enabled)
        }
        return cell
    }

    func edit(_ item: StyleEffectItem, doc: DocumentController) {
        styles.open(doc, layer: item.layer, effect: item.effect.index)
    }

    /// Double-click on a layer row away from its name: Blending Options.
    func editLayer(_ layer: DocLayerID, doc: DocumentController) { styles.open(doc, layer: layer) }

    func menu(_ menu: NSMenu, _ item: StyleEffectItem, doc: DocumentController) {
        add(menu, "Edit \(item.effect.kind.title)…") { self.edit(item, doc: doc) }
        add(menu, item.effect.enabled ? "Hide Effect" : "Show Effect") {
            DocumentStyles.shared.setEnabled(doc, item.layer, item.effect.index, !item.effect.enabled)
        }
        menu.addItem(.separator())
        add(menu, "Delete Effect") { DocumentStyles.shared.removeEffect(doc, item.layer, item.effect.index) }
    }

    /// The layer context menu's Layer Style items.
    func layerMenu(_ menu: NSMenu, doc: DocumentController) {
        let primary = doc.primary
        menu.addItem(.separator())
        add(menu, "Blending Options…", primary != nil) { self.styles.open(doc) }
        add(menu, "Copy Layer Style", primary != nil) { self.styles.copy(doc) }
        add(menu, "Paste Layer Style", primary != nil && styles.canPaste(doc)) { self.styles.paste(doc) }
        add(menu, "Clear Layer Style", primary.map { isStyled($0.id) } ?? false) { self.styles.clear(doc) }
    }

    private func add(_ menu: NSMenu, _ title: String, _ enabled: Bool = true, _ action: @escaping @MainActor () -> Void) {
        let m = NSMenuItem(title: title, action: #selector(MenuAction.run), keyEquivalent: "")
        let box = MenuAction(action)
        m.target = box
        m.representedObject = box
        m.isEnabled = enabled
        menu.addItem(m)
    }

    /// The fx glyph of layer rows (tertiary, like the kind and lock glyphs).
    static func badge() -> NSImageView {
        let v = NSImageView()
        v.image = NSImage(systemSymbolName: "fx", accessibilityDescription: "Layer style")
        v.contentTintColor = Theme.Palette.textSecondary
        v.symbolConfiguration = .init(pointSize: 11, weight: .medium)
        v.toolTip = "Layer style. Double-click the row to edit it."
        v.isHidden = true
        return v
    }
}

/// One effect row: enable eye and the effect's name, indented under its layer.
@MainActor
final class StyleEffectRowCell: NSTableCellView {
    static let identifier = NSUserInterfaceItemIdentifier("StyleEffectRowCell")
    private let eye = NSButton()
    private let name = NSTextField(labelWithString: "")
    private let stack = NSStackView()
    private var onToggle: (() -> Void)?

    init() {
        super.init(frame: .zero)
        identifier = Self.identifier
        eye.isBordered = false
        eye.bezelStyle = .regularSquare
        eye.imagePosition = .imageOnly
        eye.target = self
        eye.action = #selector(eyeClicked)
        eye.widthAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
        eye.heightAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
        name.font = Theme.NSFonts.caption
        name.lineBreakMode = .byTruncatingTail
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = Theme.Space.xs
        stack.edgeInsets = NSEdgeInsets(top: 0, left: Theme.Space.l, bottom: 0, right: Theme.Space.s)
        for v in [eye, name] as [NSView] { stack.addArrangedSubview(v) }
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        textField = name
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func configure(_ item: StyleEffectItem, toggle: @escaping () -> Void) {
        let e = item.effect
        onToggle = toggle
        name.stringValue = e.kind.title
        name.textColor = e.enabled ? Theme.Palette.textPrimary : Theme.Palette.textTertiary
        eye.image = NSImage(systemSymbolName: e.enabled ? "eye" : "eye.slash", accessibilityDescription: e.enabled ? "Hide" : "Show")
        eye.contentTintColor = e.enabled ? Theme.Palette.textSecondary : Theme.Palette.textTertiary
        eye.toolTip = "Show or hide this effect"
        let base = "document.layers.effect.\(item.layer).\(e.index)"
        setAccessibilityIdentifier(base)
        eye.setAccessibilityIdentifier("\(base).visibility")
        name.setAccessibilityIdentifier("\(base).name")
        setAccessibilityLabel("Effect \(e.kind.title)\(e.enabled ? "" : ", hidden")")
        toolTip = "Double-click to edit \(e.kind.title)"
    }

    @objc private func eyeClicked() { onToggle?() }
}
