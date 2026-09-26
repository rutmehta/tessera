import AppKit
import IOSurface
import SwiftUI
import TesseraCore

/// The Layers panel's tree: an `NSOutlineView` (docs/11 §1.5: row recycling for documents of
/// hundreds of layers, which SwiftUI `List` does not do). Rows: eye, clipping indent, thumbnail,
/// mask thumbnail with link chain, name (double-click renames), kind glyph, lock glyph. Drag to
/// reorder and into / out of groups; the context menu mirrors the Layer menu. Changes from the
/// backend are applied as the minimal outline edits of `DocumentOutline.diff`.
struct LayersOutline: NSViewRepresentable {
    let document: DocumentController

    func makeCoordinator() -> LayersOutlineController { LayersOutlineController() }

    func makeNSView(context: Context) -> NSScrollView {
        let c = context.coordinator
        let scroll = NSScrollView()
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder
        scroll.documentView = c.outline
        c.attach(document)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.attach(document)
    }
}

/// Reference wrapper for outline items (NSOutlineView compares items by identity).
final class LayerItem: NSObject {
    let id: DocLayerID
    init(_ id: DocLayerID) { self.id = id }
}

@MainActor
final class LayersOutlineController: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate, NSMenuDelegate,
    NSTextFieldDelegate {
    let outline = LayersOutlineView()
    private(set) weak var document: DocumentController?
    private var tree = DocumentOutline()
    private var items: [DocLayerID: LayerItem] = [:]
    private var syncingSelection = false
    private var thumbnailCache = ThumbnailCache()
    private let thumbnailLoader = LayerThumbnailLoader()
    static let dragType = NSPasteboard.PasteboardType("dev.tessera.layer-ids")
    // Smart filter rows under smart objects (WP M5-12).
    private let smart = SmartFilterOutline()
    private var filters: DocumentFilters? { DocumentFilters.active }
    static let thumbnailPx: UInt32 = 64

    override init() {
        super.init()
        let column = NSTableColumn(identifier: .init("layer"))
        column.resizingMask = .autoresizingMask
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.headerView = nil
        outline.style = .plain
        outline.backgroundColor = .clear
        outline.rowHeight = Theme.Height.sectionHeader
        outline.intercellSpacing = .zero
        outline.indentationPerLevel = Theme.Space.m
        outline.allowsMultipleSelection = true
        outline.allowsEmptySelection = true
        outline.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        // The outline column keeps the view's width (by default it shrinks to fit on expand / move).
        outline.autoresizesOutlineColumn = false
        outline.selectionHighlightStyle = .regular
        outline.focusRingType = .none
        outline.dataSource = self
        outline.delegate = self
        outline.registerForDraggedTypes([Self.dragType])
        outline.setDraggingSourceOperationMask(.move, forLocal: true)
        outline.draggingDestinationFeedbackStyle = .regular
        outline.target = self
        outline.doubleAction = #selector(doubleClicked(_:))
        let menu = NSMenu()
        menu.delegate = self
        outline.menu = menu
        outline.setAccessibilityIdentifier("document.layers.outline")
        outline.setAccessibilityLabel("Layers")
    }

    func attach(_ doc: DocumentController) {
        guard doc !== document else { return }
        document?.onLayersReload = nil
        document?.onSelectionChange = nil
        document = doc
        tree = doc.outline
        items.removeAll()
        thumbnailCache = ThumbnailCache()
        thumbnailLoader.reset()
        smart.reset()
        if let f = filters { _ = smart.refresh(doc, filters: f) }   // WP M5-12
        doc.onLayersReload = { [weak self] old, new in self?.apply(old: old, new: new) }
        doc.onSelectionChange = { [weak self] in self?.syncSelectionFromModel() }
        outline.reloadData()
        outline.expandItem(nil, expandChildren: true)
        outline.sizeLastColumnToFit()
        syncSelectionFromModel()
    }

    private func item(_ id: DocLayerID) -> LayerItem {
        if let i = items[id] { return i }
        let i = LayerItem(id)
        items[id] = i
        return i
    }

    private func parentItem(_ id: DocLayerID) -> LayerItem? { id == DocumentOutline.root ? nil : item(id) }

    // MARK: Updates

    private func apply(old: DocumentOutline, new: DocumentOutline) {
        guard let changes = DocumentOutline.diff(from: tree, to: new) else {
            tree = new
            if let d = document, let f = filters { _ = smart.refresh(d, filters: f) }   // WP M5-12
            outline.reloadData()
            outline.expandItem(nil, expandChildren: true)
            syncSelectionFromModel()
            return
        }
        let before = tree
        tree = new
        if !changes.isEmpty {
            outline.beginUpdates()
            for c in changes {
                switch c {
                case .insert(let id, let p, let i):
                    outline.insertItems(at: IndexSet(integer: i), inParent: parentItem(p), withAnimation: .effectFade)
                    _ = id
                case .remove(let id, let p, let i):
                    outline.removeItems(at: IndexSet(integer: i), inParent: parentItem(p), withAnimation: .effectFade)
                    items[id] = nil
                case .move(_, let fp, let fi, let tp, let ti):
                    outline.moveItem(at: fi, inParent: parentItem(fp), to: ti, inParent: parentItem(tp))
                }
            }
            outline.endUpdates()
            outline.sizeLastColumnToFit()
        }
        // Smart filter rows (WP M5-12): reload the smart objects whose list changed.
        if let doc = document, let f = filters {
            for id in smart.refresh(doc, filters: f) {
                guard new.node(id) != nil else { continue }
                outline.reloadItem(item(id), reloadChildren: true)
                outline.expandItem(item(id))
            }
        }
        // New groups open; rows whose record changed are rebuilt (thumbnails follow `revision`).
        for id in new.flattened {
            let n = new.node(id)
            if n?.kind == .group, before.node(id) == nil { outline.expandItem(item(id)) }
            if before.node(id) != n, before.node(id) != nil {
                let row = outline.row(forItem: item(id))
                if row >= 0, let cell = outline.view(atColumn: 0, row: row, makeIfNecessary: false) as? LayerRowCell, let n {
                    configure(cell, node: n, row: row)
                }
            }
        }
        syncSelectionFromModel()
    }

    func syncSelectionFromModel() {
        guard let doc = document else { return }
        let rows = IndexSet(doc.selection.compactMap { id -> Int? in
            guard let i = items[id] else { return nil }
            let r = outline.row(forItem: i)
            return r >= 0 ? r : nil
        })
        guard rows != outline.selectedRowIndexes else { return }
        syncingSelection = true
        outline.selectRowIndexes(rows, byExtendingSelection: false)
        if let last = rows.last { outline.scrollRowToVisible(last) }
        syncingSelection = false
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !syncingSelection, let doc = document else { return }
        let ids = outline.selectedRowIndexes.compactMap { (outline.item(atRow: $0) as? LayerItem)?.id }
        // Keep the clicked row primary (last).
        let clicked = outline.clickedRow >= 0 ? (outline.item(atRow: outline.clickedRow) as? LayerItem)?.id : nil
        var ordered = ids.filter { $0 != clicked }
        if let clicked, ids.contains(clicked) { ordered.append(clicked) }
        if ordered != doc.selection { doc.selection = ordered }
    }

    // MARK: Data source

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        if let i = item as? LayerItem, tree.node(i.id)?.kind == .smartObject { return smart.count(i.id) }   // WP M5-12
        if item is SmartFilterItem { return 0 }
        return tree.children(of: (item as? LayerItem)?.id ?? DocumentOutline.root).count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        if let i = item as? LayerItem, let f = smart.item(i.id, index) { return f }   // WP M5-12
        return self.item(tree.children(of: (item as? LayerItem)?.id ?? DocumentOutline.root)[index])
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        guard let i = item as? LayerItem else { return false }
        return tree.node(i.id)?.kind == .group || smart.count(i.id) > 0   // WP M5-12: smart filters
    }

    /// Smart filter rows are not layers: they act on click, they are not selected (WP M5-12).
    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool { !(item is SmartFilterItem) }

    func outlineView(_ outlineView: NSOutlineView, rowViewForItem item: Any) -> NSTableRowView? { LayerRowView() }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        if let f = item as? SmartFilterItem, let doc = document, let filters {   // WP M5-12
            return smart.cell(outlineView, f, doc: doc, filters: filters)
        }
        guard let i = item as? LayerItem, let n = tree.node(i.id) else { return nil }
        let cell = (outlineView.makeView(withIdentifier: LayerRowCell.identifier, owner: self) as? LayerRowCell) ?? LayerRowCell()
        cell.owner = self
        configure(cell, node: n, row: outlineView.row(forItem: item))
        return cell
    }

    func outlineView(_ outlineView: NSOutlineView, didAdd rowView: NSTableRowView, forRow row: Int) {
        rowView.setAccessibilityIdentifier("document.layers.row.\(row)")
        (outlineView.view(atColumn: 0, row: row, makeIfNecessary: false) as? LayerRowCell)?.setRow(row)
    }

    private func configure(_ cell: LayerRowCell, node n: LayerRecord, row: Int) {
        guard let doc = document else { return }
        var thumb: NSImage?
        let backend = doc.backend, id = n.id, px = Self.thumbnailPx
        if n.kind != .adjustment {
            thumb = thumbnail(key: "l\(n.id):\(n.revision)", slot: "l\(n.id)") { try? backend.layerThumbnail(id: id, maxPx: px) }
        }
        let mask = n.hasMask ? thumbnail(key: "m\(n.id):\(n.revision):\(n.maskEnabled)", slot: "m\(n.id)") {
            try? backend.maskThumbnail(id: id, maxPx: px)
        } : nil
        cell.configure(n, thumbnail: thumb, mask: mask)
        cell.setRow(row)
    }

    /// A cached thumbnail, or (on a miss) the slot's previous image while the new one renders off the
    /// main thread (thumbnails of large layers take tens of milliseconds); the row refreshes when it lands.
    private func thumbnail(key: String, slot: String, fetch: @escaping @Sendable () -> UInt32?) -> NSImage? {
        if let image = thumbnailCache.cached(key) {
            thumbnailLoader.shown[slot] = image
            return image
        }
        thumbnailLoader.load(key: key, slot: slot, fetch: fetch) { [weak self] image in
            guard let self, let image else { return }
            self.thumbnailCache.store(key, image)
            self.refreshRows(showing: key)
        }
        return thumbnailLoader.shown[slot]
    }

    /// Re-configures the row whose current record maps to `key`.
    private func refreshRows(showing key: String) {
        guard let doc = document else { return }
        for n in doc.layers {
            let keys = ["l\(n.id):\(n.revision)", "m\(n.id):\(n.revision):\(n.maskEnabled)"]
            guard keys.contains(key), let i = items[n.id] else { continue }
            let row = outline.row(forItem: i)
            if row >= 0, let cell = outline.view(atColumn: 0, row: row, makeIfNecessary: false) as? LayerRowCell {
                configure(cell, node: n, row: row)
            }
        }
    }

    // MARK: Actions from rows

    func toggleVisibility(_ id: DocLayerID, solo: Bool) {
        guard let doc = document, let n = doc.node(id) else { return }
        if solo { doc.soloVisibility(id) } else { doc.setVisible(id, !n.visible) }
    }

    func maskClicked(_ id: DocLayerID, shift: Bool) {
        guard let doc = document else { return }
        if shift { doc.toggleMaskEnabled(id) } else { doc.select(id) }
    }

    func toggleLink(_ id: DocLayerID) { document?.toggleMaskLinked(id) }

    func rename(_ id: DocLayerID, _ name: String) { document?.rename(id, to: name.trimmingCharacters(in: .whitespaces)) }

    @objc private func doubleClicked(_ sender: Any?) {
        let row = outline.clickedRow
        if row >= 0, let f = outline.item(atRow: row) as? SmartFilterItem, let doc = document, let filters {   // WP M5-12
            smart.edit(f, doc: doc, filters: filters)
            return
        }
        guard row >= 0, let cell = outline.view(atColumn: 0, row: row, makeIfNecessary: false) as? LayerRowCell else { return }
        let p = cell.convert(outline.window?.mouseLocationOutsideOfEventStream ?? .zero, from: nil)
        if cell.nameHit(p) || !(tree.node(cell.layerID ?? 0)?.kind == .group) {
            cell.beginRename()
        } else if let i = outline.item(atRow: row) {
            outline.isItemExpanded(i) ? outline.collapseItem(i) : outline.expandItem(i)
        }
    }

    // MARK: Drag and drop

    func outlineView(_ outlineView: NSOutlineView, pasteboardWriterForItem item: Any) -> (any NSPasteboardWriting)? {
        guard let i = item as? LayerItem else { return nil }
        let p = NSPasteboardItem()
        p.setString(String(i.id), forType: Self.dragType)
        return p
    }

    private func draggedIDs(_ info: any NSDraggingInfo) -> [DocLayerID] {
        (info.draggingPasteboard.pasteboardItems ?? []).compactMap { $0.string(forType: Self.dragType).flatMap(DocLayerID.init) }
    }

    func outlineView(_ outlineView: NSOutlineView, validateDrop info: any NSDraggingInfo, proposedItem item: Any?,
                     proposedChildIndex index: Int) -> NSDragOperation {
        if item is SmartFilterItem { return [] }   // WP M5-12
        if let i = item as? LayerItem, tree.node(i.id)?.kind == .smartObject, index != NSOutlineViewDropOnItemIndex { return [] }
        guard let target = dropTarget(item: item, index: index) else { return [] }
        let ids = draggedIDs(info)
        guard !ids.isEmpty, tree.moving(ids, into: target.parent, at: target.index) != nil else { return [] }
        if target.retargeted {
            outlineView.setDropItem(target.parent == DocumentOutline.root ? nil : self.item(target.parent), dropChildIndex: target.index)
        }
        return .move
    }

    func outlineView(_ outlineView: NSOutlineView, acceptDrop info: any NSDraggingInfo, item: Any?, childIndex index: Int) -> Bool {
        guard let doc = document, let target = dropTarget(item: item, index: index) else { return false }
        let ids = draggedIDs(info)
        let ok = doc.moveLayers(ids, into: target.parent, at: target.index)
        if ok { doc.selection = ids }
        return ok
    }

    /// Drops on a group go into it (top); drops on a layer go above it.
    private func dropTarget(item: Any?, index: Int) -> (parent: DocLayerID, index: Int, retargeted: Bool)? {
        let id = (item as? LayerItem)?.id ?? DocumentOutline.root
        if index != NSOutlineViewDropOnItemIndex { return (id, index, false) }
        if id == DocumentOutline.root { return (id, 0, true) }
        if tree.node(id)?.kind == .group { return (id, 0, true) }
        guard let pos = tree.position(of: id) else { return nil }
        return (pos.parent, pos.index, true)
    }

    // MARK: Context menu (mirrors the Layer menu)

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard let doc = document else { return }
        let row = outline.clickedRow
        if row >= 0, let f = outline.item(atRow: row) as? SmartFilterItem, let filters {   // WP M5-12
            smart.menu(menu, f, doc: doc, filters: filters)
            return
        }
        if row >= 0, let i = outline.item(atRow: row) as? LayerItem, !doc.selection.contains(i.id) {
            doc.selection = [i.id]
        }
        let primary = doc.primary
        func add(_ title: String, _ enabled: Bool = true, _ action: @escaping @MainActor () -> Void) {
            let item = NSMenuItem(title: title, action: #selector(MenuAction.run), keyEquivalent: "")
            let box = MenuAction(action)
            item.target = box
            item.representedObject = box
            item.isEnabled = enabled
            menu.addItem(item)
        }
        add("Rename Layer…", primary != nil) { [weak self] in
            guard let self, let id = primary?.id, let item = self.items[id] else { return }
            let r = self.outline.row(forItem: item)
            (self.outline.view(atColumn: 0, row: r, makeIfNecessary: false) as? LayerRowCell)?.beginRename()
        }
        add("Duplicate Layer", primary != nil) { doc.duplicateSelection() }
        add("Delete Layer", primary != nil) { doc.deleteSelection() }
        menu.addItem(.separator())
        add("Group Layers", true) { doc.groupSelection() }
        add("Ungroup Layers", primary?.kind == .group) { doc.ungroupSelection() }
        add("Merge Down", primary != nil) { doc.mergeDown() }
        add("Flatten Image") { doc.flatten() }
        menu.addItem(.separator())
        add(primary?.clipped == true ? "Release Clipping Mask" : "Create Clipping Mask", primary != nil) { doc.toggleClipping() }
        if primary?.hasMask == true {
            add(primary?.maskEnabled == true ? "Disable Layer Mask" : "Enable Layer Mask") { doc.toggleMaskEnabled() }
            add("Delete Layer Mask") { doc.deleteMask() }
        } else {
            add("Add Layer Mask: Reveal All", primary != nil) { doc.addMask(.revealAll) }
            add("Add Layer Mask: Hide All", primary != nil) { doc.addMask(.hideAll) }
            add("Add Layer Mask: From Selection", primary != nil && doc.marquee != nil) { doc.addMask(.fromSelection) }
        }
    }
}

/// Target for context-menu items built from closures.
@MainActor
final class MenuAction: NSObject {
    let action: @MainActor () -> Void
    init(_ action: @escaping @MainActor () -> Void) { self.action = action }
    @objc func run() { action() }
}

/// Layer thumbnails decoded from backend IOSurfaces, keyed by layer revision.
@MainActor
struct ThumbnailCache {
    private var images: [String: NSImage] = [:]

    mutating func image(key: String, surfaceID: () -> UInt32?) -> NSImage? {
        if let i = images[key] { return i }
        guard let id = surfaceID(), let s = IOSurfaceLookup(id), let image = Self.image(from: s) else { return nil }
        store(key, image)
        return image
    }

    func cached(_ key: String) -> NSImage? { images[key] }

    mutating func store(_ key: String, _ image: NSImage) {
        if images.count > 600 { images.removeAll() }
        images[key] = image
    }

    /// RGBA8 straight alpha → NSImage.
    nonisolated static func image(from s: IOSurfaceRef) -> NSImage? {
        let w = IOSurfaceGetWidth(s), h = IOSurfaceGetHeight(s), stride = IOSurfaceGetBytesPerRow(s)
        IOSurfaceLock(s, .readOnly, nil)
        let data = Data(bytes: IOSurfaceGetBaseAddress(s), count: stride * h)
        IOSurfaceUnlock(s, .readOnly, nil)
        guard let provider = CGDataProvider(data: data as CFData),
              let cg = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: stride,
                               space: CGColorSpace(name: CGColorSpace.sRGB)!,
                               bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                               provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        return NSImage(cgImage: cg, size: NSSize(width: w, height: h))
    }
}

/// Renders layer and mask thumbnails on a background queue, newest request per slot only: while a
/// value drags, stale requests are skipped instead of queueing one render per step.
@MainActor
final class LayerThumbnailLoader {
    /// The image each slot (`l<id>` / `m<id>`) last showed.
    var shown: [String: NSImage] = [:]
    private var inFlight = Set<String>()
    private let latest = LatestKeys()
    private let queue = DispatchQueue(label: "dev.tessera.layer-thumbnails", qos: .userInitiated)
    private var generation = 0

    /// Thread-safe newest key per slot.
    private final class LatestKeys: @unchecked Sendable {
        private let lock = NSLock()
        private var keys: [String: String] = [:]
        func set(_ slot: String, _ key: String) { lock.lock(); keys[slot] = key; lock.unlock() }
        func isLatest(_ slot: String, _ key: String) -> Bool { lock.lock(); defer { lock.unlock() }; return keys[slot] == key }
        func removeAll() { lock.lock(); keys.removeAll(); lock.unlock() }
    }

    func reset() {
        shown.removeAll()
        inFlight.removeAll()
        latest.removeAll()
        generation += 1
    }

    func load(key: String, slot: String, fetch: @escaping @Sendable () -> UInt32?,
              done: @escaping @MainActor (NSImage?) -> Void) {
        latest.set(slot, key)
        guard inFlight.insert(key).inserted else { return }
        let latest = self.latest, gen = generation
        queue.async { [weak self] in
            // Skipped when a newer revision of the slot was asked for meanwhile.
            let image = latest.isLatest(slot, key)
                ? fetch().flatMap { IOSurfaceLookup($0) }.flatMap { ThumbnailCache.image(from: $0) } : nil
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, self.generation == gen else { return }
                    self.inFlight.remove(key)
                    if let image { self.shown[slot] = image }
                    done(image)
                }
            }
        }
    }
}

/// The outline view: Delete removes the selected layers; everything else goes to the menus.
@MainActor
final class LayersOutlineView: NSOutlineView, KeyOwningControl {
    override func keyDown(with event: NSEvent) {
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if (event.keyCode == 51 || event.keyCode == 117), mods.isEmpty,
           let c = delegate as? LayersOutlineController, let doc = c.document {
            doc.deleteSelection()
            return
        }
        super.keyDown(with: event)
    }
}

/// Selection fill: accent-subtle at radius 6 (DESIGN.md §5, list rows).
final class LayerRowView: NSTableRowView {
    override func drawSelection(in dirtyRect: NSRect) {
        guard selectionHighlightStyle != .none else { return }
        Theme.Palette.accentSubtle.setFill()
        NSBezierPath(roundedRect: bounds.insetBy(dx: Theme.Space.xs, dy: 1), xRadius: Theme.Radius.control,
                     yRadius: Theme.Radius.control).fill()
    }
    override var isEmphasized: Bool { get { false } set {} }
}

/// Checkerboard under a thumbnail (transparency), radius 4 with a hairline.
final class CheckerThumbnailView: NSView {
    var image: NSImage? { didSet { needsDisplay = true } }
    /// Draws a reject-coloured cross (a disabled mask).
    var crossed = false { didSet { needsDisplay = true } }
    var onClick: ((NSEvent) -> Void)?

    override var intrinsicContentSize: NSSize { NSSize(width: Theme.Height.regular, height: Theme.Height.regular) }
    override func viewDidChangeEffectiveAppearance() { needsDisplay = true }

    override func draw(_ dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 0.5, dy: 0.5)
        let clip = NSBezierPath(roundedRect: r, xRadius: Theme.Radius.chip, yRadius: Theme.Radius.chip)
        NSGraphicsContext.saveGraphicsState()
        clip.addClip()
        Theme.Palette.checkerLight.setFill()
        r.fill()
        Theme.Palette.checkerDark.setFill()
        let cell = Theme.Space.xs
        var y = r.minY
        var row = 0
        while y < r.maxY {
            var x = r.minX + (row % 2 == 0 ? 0 : cell)
            while x < r.maxX { NSRect(x: x, y: y, width: cell, height: cell).fill(); x += 2 * cell }
            y += cell
            row += 1
        }
        if let image {
            let s = image.size
            let k = min(r.width / max(s.width, 1), r.height / max(s.height, 1))
            let d = NSRect(x: r.midX - s.width * k / 2, y: r.midY - s.height * k / 2, width: s.width * k, height: s.height * k)
            image.draw(in: d)
        }
        if crossed {
            let p = NSBezierPath()
            p.move(to: NSPoint(x: r.minX + 2, y: r.minY + 2)); p.line(to: NSPoint(x: r.maxX - 2, y: r.maxY - 2))
            p.move(to: NSPoint(x: r.minX + 2, y: r.maxY - 2)); p.line(to: NSPoint(x: r.maxX - 2, y: r.minY + 2))
            p.lineWidth = Theme.Space.xxs
            Theme.Palette.reject.setStroke()
            p.stroke()
        }
        NSGraphicsContext.restoreGraphicsState()
        Theme.Palette.hairlineStrong.setStroke()
        clip.lineWidth = Theme.Space.hairline
        clip.stroke()
    }

    override func mouseDown(with event: NSEvent) {
        if let onClick { onClick(event) } else { super.mouseDown(with: event) }
    }
}

/// One layer row.
@MainActor
final class LayerRowCell: NSTableCellView, NSTextFieldDelegate {
    static let identifier = NSUserInterfaceItemIdentifier("LayerRowCell")
    weak var owner: LayersOutlineController?
    private(set) var layerID: DocLayerID?
    private let eye = NSButton()
    private let clip = NSImageView()
    private let thumb = CheckerThumbnailView()
    private let glyph = NSImageView()
    private let chain = NSButton()
    private let mask = CheckerThumbnailView()
    private let name = NSTextField(labelWithString: "")
    private let kind = NSImageView()
    private let lock = NSImageView()
    private let stack = NSStackView()
    private var editing = false

    init() {
        super.init(frame: .zero)
        identifier = Self.identifier
        eye.isBordered = false
        eye.bezelStyle = .regularSquare
        eye.imagePosition = .imageOnly
        eye.contentTintColor = Theme.Palette.textSecondary
        eye.target = self
        eye.action = #selector(eyeClicked(_:))
        for v in [clip, glyph, kind, lock] {
            v.contentTintColor = Theme.Palette.textTertiary
            v.symbolConfiguration = .init(pointSize: 11, weight: .medium)
        }
        glyph.contentTintColor = Theme.Palette.textSecondary
        glyph.symbolConfiguration = .init(pointSize: 13, weight: .regular)
        chain.isBordered = false
        chain.imagePosition = .imageOnly
        chain.contentTintColor = Theme.Palette.textTertiary
        chain.target = self
        chain.action = #selector(chainClicked(_:))
        chain.toolTip = "Link or unlink the mask and the layer"
        name.font = Theme.NSFonts.label
        name.textColor = Theme.Palette.textPrimary
        name.lineBreakMode = .byTruncatingTail
        name.delegate = self
        name.focusRingType = .none
        name.cell?.sendsActionOnEndEditing = true
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        name.setContentHuggingPriority(.defaultLow, for: .horizontal)
        thumb.onClick = { [weak self] e in self?.forwardClick(e) }
        mask.onClick = { [weak self] e in
            guard let self, let id = self.layerID else { return }
            self.owner?.maskClicked(id, shift: e.modifierFlags.contains(.shift))
        }
        mask.toolTip = "Layer mask. ⇧-click turns it off or on."
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = Theme.Space.xs
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 0, bottom: 0, right: Theme.Space.s)
        for v in [eye, clip, thumb, glyph, chain, mask, name, kind, lock] as [NSView] { stack.addArrangedSubview(v) }
        stack.setCustomSpacing(Theme.Space.s, after: mask)
        stack.setCustomSpacing(Theme.Space.s, after: thumb)
        for v in [eye, chain] as [NSView] {
            v.widthAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
            v.heightAnchor.constraint(equalToConstant: Theme.Height.small).isActive = true
        }
        for v in [thumb, mask, glyph] as [NSView] {
            v.widthAnchor.constraint(equalToConstant: Theme.Height.regular).isActive = true
            v.heightAnchor.constraint(equalToConstant: Theme.Height.regular).isActive = true
        }
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

    private static func symbol(_ name: String, _ label: String) -> NSImage? {
        NSImage(systemSymbolName: name, accessibilityDescription: label)
    }

    static func kindSymbol(_ n: LayerRecord) -> String? {
        switch n.kind {
        case .pixel: nil
        case .adjustment: AdjustmentModel(json: n.adjustmentJson)?.kind.symbol ?? "circle.lefthalf.filled"
        case .fill: "drop.halffull"
        case .group: "folder"
        case .smartObject: "square.on.square.dashed"
        case .text: "textformat"
        }
    }

    func configure(_ n: LayerRecord, thumbnail: NSImage?, mask maskImage: NSImage?) {
        layerID = n.id
        if !editing { name.stringValue = n.name }
        name.textColor = n.visible ? Theme.Palette.textPrimary : Theme.Palette.textTertiary
        eye.image = Self.symbol(n.visible ? "eye" : "eye.slash", n.visible ? "Hide" : "Show")
        eye.contentTintColor = n.visible ? Theme.Palette.textSecondary : Theme.Palette.textTertiary
        eye.toolTip = "Show or hide the layer. ⌥-click shows only this layer."
        clip.isHidden = !n.clipped
        clip.image = Self.symbol("arrow.turn.left.down", "Clipped to the layer below")
        // Adjustments show their glyph where a thumbnail would be; everything else a thumbnail.
        let isAdjustment = n.kind == .adjustment
        thumb.isHidden = isAdjustment
        thumb.image = thumbnail
        glyph.isHidden = !isAdjustment
        glyph.image = Self.symbol(Self.kindSymbol(n) ?? "circle.lefthalf.filled", "Adjustment")
        mask.isHidden = !n.hasMask
        mask.image = maskImage
        mask.crossed = n.hasMask && !n.maskEnabled
        chain.isHidden = !n.hasMask
        chain.image = Self.symbol("link", n.maskLinked ? "Linked" : "Unlinked")
        chain.alphaValue = n.maskLinked ? 1 : CGFloat(Theme.Opacity.hidden)
        let k = isAdjustment ? nil : Self.kindSymbol(n)
        kind.isHidden = k == nil
        kind.image = k.flatMap { Self.symbol($0, n.kind.title) }
        lock.isHidden = !n.locks.any
        lock.image = Self.symbol(n.locks.all ? "lock.fill" : "lock", "Locked")
        lock.toolTip = lockText(n.locks)
        setAccessibilityLabel("\(n.name), \(n.kind.title)\(n.visible ? "" : ", hidden")\(n.hasMask ? ", masked" : "")")
        toolTip = n.kind == .adjustment ? AdjustmentModel(json: n.adjustmentJson)?.kind.title : nil
    }

    private func lockText(_ l: LayerLockFlags) -> String {
        var parts: [String] = []
        if l.all { parts.append("all") }
        if l.transparency { parts.append("transparent pixels") }
        if l.pixels { parts.append("image pixels") }
        if l.position { parts.append("position") }
        return "Locked: " + parts.joined(separator: ", ")
    }

    func setRow(_ row: Int) {
        setAccessibilityIdentifier("document.layers.row.\(row).cell")
        eye.setAccessibilityIdentifier("document.layers.row.\(row).visibility")
        name.setAccessibilityIdentifier("document.layers.row.\(row).name")
        mask.setAccessibilityIdentifier("document.layers.row.\(row).mask")
        chain.setAccessibilityIdentifier("document.layers.row.\(row).maskLink")
        thumb.setAccessibilityIdentifier("document.layers.row.\(row).thumbnail")
    }

    @objc private func eyeClicked(_ sender: NSButton) {
        guard let id = layerID else { return }
        owner?.toggleVisibility(id, solo: NSApp.currentEvent?.modifierFlags.contains(.option) == true)
    }

    @objc private func chainClicked(_ sender: NSButton) {
        guard let id = layerID else { return }
        owner?.toggleLink(id)
    }

    private func forwardClick(_ e: NSEvent) {
        guard let row = owner?.outline.row(for: self), row >= 0 else { return }
        let extend = e.modifierFlags.contains(.command) || e.modifierFlags.contains(.shift)
        owner?.outline.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: extend)
    }

    func nameHit(_ p: NSPoint) -> Bool { name.frame.insetBy(dx: -Theme.Space.xs, dy: -Theme.Space.xs).contains(convert(p, to: stack)) || name.frame.contains(p) }

    func beginRename() {
        editing = true
        name.isEditable = true
        name.isBordered = false
        name.drawsBackground = true
        name.backgroundColor = Theme.Palette.raised
        window?.makeFirstResponder(name)
        name.currentEditor()?.selectAll(nil)
    }

    func controlTextDidEndEditing(_ obj: Notification) {
        guard editing else { return }
        editing = false
        name.isEditable = false
        name.drawsBackground = false
        if let id = layerID { owner?.rename(id, name.stringValue) }
    }
}
