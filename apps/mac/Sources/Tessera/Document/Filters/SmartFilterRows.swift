import AppKit
import SwiftUI
import TesseraCore

/// Outline item of one smart filter row (children of a smart object in the Layers panel).
final class SmartFilterItem: NSObject {
    let layer: DocLayerID
    let row: SmartFilterRow
    init(layer: DocLayerID, row: SmartFilterRow) { self.layer = layer; self.row = row }
}

/// Smart filters in the Layers outline (WP B5-05): under each smart object, one row per smart
/// filter, last applied on top (Photoshop's order): enable eye, mask thumbnail, name (double-click
/// re-edits the filter), blending options button. `LayersOutlineController` calls in here from a
/// few clearly marked hooks.
@MainActor
final class SmartFilterOutline {
    /// Rows per smart object, top first, as last shown.
    private var rows: [DocLayerID: [SmartFilterItem]] = [:]
    private var masks = ThumbnailCache()

    func reset() {
        rows.removeAll()
        masks = ThumbnailCache()
    }

    /// Re-reads the rows of every smart object; returns the ones whose rows changed.
    func refresh(_ doc: DocumentController, filters: DocumentFilters) -> [DocLayerID] {
        var changed: [DocLayerID] = []
        var next: [DocLayerID: [SmartFilterItem]] = [:]
        for n in doc.layers where n.kind == .smartObject {
            let list = filters.smartFilters(doc, layer: n.id).reversed()
            let old = rows[n.id] ?? []
            if old.map(\.row) == Array(list) {
                next[n.id] = old
            } else {
                next[n.id] = list.map { SmartFilterItem(layer: n.id, row: $0) }
                changed.append(n.id)
            }
        }
        changed += rows.keys.filter { next[$0] == nil }
        rows = next
        return changed
    }

    func count(_ layer: DocLayerID) -> Int { rows[layer]?.count ?? 0 }
    func item(_ layer: DocLayerID, _ index: Int) -> SmartFilterItem? { rows[layer]?[index] }

    func cell(_ outline: NSOutlineView, _ item: SmartFilterItem, doc: DocumentController, filters: DocumentFilters) -> NSView {
        let cell = (outline.makeView(withIdentifier: SmartFilterRowCell.identifier, owner: nil) as? SmartFilterRowCell)
            ?? SmartFilterRowCell()
        let backend = DocumentFilters.backend(doc)
        let key = "sf\(item.layer):\(item.row.index):\(item.row.hasMask):\(doc.node(item.layer)?.revision ?? 0)"
        let mask = masks.image(key: key) {
            try? backend?.smartFilterMaskThumbnail(layer: item.layer, index: item.row.index, maxPx: 64)
        }
        cell.configure(item, mask: mask,
                       toggle: { [weak doc, weak filters] in
                           guard let doc, let filters else { return }
                           filters.toggleSmartFilter(doc, layer: item.layer, row: item.row)
                       },
                       blending: { [weak doc, weak filters] in
                           guard let doc, let filters else { return }
                           filters.blendingOptions(doc, layer: item.layer, row: item.row)
                       })
        return cell
    }

    func edit(_ item: SmartFilterItem, doc: DocumentController, filters: DocumentFilters) {
        filters.editSmartFilter(doc, layer: item.layer, row: item.row)
    }

    func menu(_ menu: NSMenu, _ item: SmartFilterItem, doc: DocumentController, filters: DocumentFilters) {
        func add(_ title: String, _ action: @escaping @MainActor () -> Void) {
            let m = NSMenuItem(title: title, action: #selector(MenuAction.run), keyEquivalent: "")
            let box = MenuAction(action)
            m.target = box
            m.representedObject = box
            menu.addItem(m)
        }
        add("Edit Smart Filter…") { filters.editSmartFilter(doc, layer: item.layer, row: item.row) }
        add(item.row.enabled ? "Disable Smart Filter" : "Enable Smart Filter") {
            filters.toggleSmartFilter(doc, layer: item.layer, row: item.row)
        }
        add("Blending Options…") { filters.blendingOptions(doc, layer: item.layer, row: item.row) }
        menu.addItem(.separator())
        add("Delete Smart Filter") { filters.deleteSmartFilter(doc, layer: item.layer, row: item.row) }
    }
}

/// One smart filter row.
@MainActor
final class SmartFilterRowCell: NSTableCellView {
    static let identifier = NSUserInterfaceItemIdentifier("SmartFilterRowCell")
    private let eye = NSButton()
    private let mask = CheckerThumbnailView()
    private let name = NSTextField(labelWithString: "")
    private let blending = NSButton()
    private let stack = NSStackView()
    private var onToggle: (() -> Void)?
    private var onBlending: (() -> Void)?

    init() {
        super.init(frame: .zero)
        identifier = Self.identifier
        for b in [eye, blending] {
            b.isBordered = false
            b.bezelStyle = .regularSquare
            b.imagePosition = .imageOnly
            b.contentTintColor = Theme.Palette.textSecondary
            b.target = self
            b.widthAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
            b.heightAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
        }
        eye.action = #selector(eyeClicked)
        blending.action = #selector(blendingClicked)
        blending.image = NSImage(systemSymbolName: "slider.horizontal.3", accessibilityDescription: "Blending options")
        blending.toolTip = "Blending options of this smart filter"
        mask.toolTip = "Smart filter mask (white: filtered)"
        mask.widthAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
        mask.heightAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
        name.font = Theme.NSFonts.caption
        name.lineBreakMode = .byTruncatingTail
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        name.setContentHuggingPriority(.defaultLow, for: .horizontal)
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = Theme.Space.xs
        stack.edgeInsets = NSEdgeInsets(top: 0, left: Theme.Space.l, bottom: 0, right: Theme.Space.s)
        for v in [eye, mask, name, blending] as [NSView] { stack.addArrangedSubview(v) }
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

    func configure(_ item: SmartFilterItem, mask image: NSImage?, toggle: @escaping () -> Void, blending: @escaping () -> Void) {
        let r = item.row
        onToggle = toggle
        onBlending = blending
        name.stringValue = r.opacity < 1 || r.blendMode != "normal"
            ? "\(r.name) · \(DocBlendMode.title(backendName: r.blendMode, groupMode: nil)) \(Int((r.opacity * 100).rounded())) %"
            : r.name
        name.textColor = r.enabled ? Theme.Palette.textPrimary : Theme.Palette.textTertiary
        eye.image = NSImage(systemSymbolName: r.enabled ? "eye" : "eye.slash", accessibilityDescription: r.enabled ? "Disable" : "Enable")
        eye.contentTintColor = r.enabled ? Theme.Palette.textSecondary : Theme.Palette.textTertiary
        eye.toolTip = "Turn this smart filter off or on"
        mask.image = image
        let base = "document.layers.smartFilter.\(item.layer).\(r.index)"
        setAccessibilityIdentifier(base)
        eye.setAccessibilityIdentifier("\(base).visibility")
        mask.setAccessibilityIdentifier("\(base).mask")
        name.setAccessibilityIdentifier("\(base).name")
        self.blending.setAccessibilityIdentifier("\(base).blending")
        setAccessibilityLabel("Smart filter \(r.name)\(r.enabled ? "" : ", off")")
        toolTip = "Double-click to edit \(r.name)"
    }

    @objc private func eyeClicked() { onToggle?() }
    @objc private func blendingClicked() { onBlending?() }
}
