import AppKit
import SwiftUI
import TesseraCore

/// Left sidebar: library sources, folders, albums (with album groups and smart albums, nestable
/// and reorderable by drag), and the culling presets. Plain text rows with counts; no icon
/// column. AppKit `NSOutlineView` for drag-and-drop, inline rename and context menus; the
/// source-list material shows through (no painted background).
struct SidebarView: View {
    let model: AppModel

    var body: some View {
        SidebarOutline(model: model, snapshot: SidebarSnapshot(model: model))
    }
}

/// Everything the sidebar shows, read in `body` so SwiftUI observation re-renders on change.
struct SidebarSnapshot: Equatable {
    var engineBacked: Bool
    var allCount: Int
    var unfiledCount: Int
    var folder: URL?
    var title: String
    var hasItems: Bool
    var subfolders: [URL]
    var recents: [URL]
    var nodes: [CollectionNode]
    /// Current member counts by album handle (from the cull session, live with the basket).
    var albumCounts: [String: Int]
    var basketTarget: String
    var keep: Int
    var undecided: Int
    var reject: Int
    var source: LibrarySource

    @MainActor init(model: AppModel) {
        // The library object updates in place (M2-28): observe its revision for the counts.
        _ = model.libraryRevision
        engineBacked = model.isEngineBacked
        allCount = model.library.items.count
        let filed = Set(model.albums.flatMap(\.members))
        unfiledCount = model.library.items.count - filed.count
        folder = model.library.folder
        title = model.library.title
        hasItems = !model.library.items.isEmpty
        subfolders = model.library.subfolders
        recents = Array(model.recentFolders.filter { $0 != model.library.folder }.prefix(5))
        nodes = model.collections.nodes
        albumCounts = Dictionary(model.albums.map { ($0.name, $0.members.count) }, uniquingKeysWith: { a, _ in a })
        basketTarget = model.basketTarget
        keep = model.counts.keep
        undecided = model.counts.undecided
        reject = model.counts.reject
        source = model.source
    }
}

extension NSPasteboard.PasteboardType {
    static let sidebarNode = NSPasteboard.PasteboardType("dev.tessera.sidebar-node")
}

/// Outline rows. Reference type so the outline can keep expansion state per row.
final class SidebarRow: NSObject {
    enum Section: String { case library = "Library", folders = "Folders", albums = "Albums", cull = "Culling" }
    enum Kind {
        case header(Section)
        case source(LibrarySource)
        case currentFolder(URL?)
        case folder(URL)
        case node(CollectionNode)
        /// Basket target album that library.json does not contain yet (created on the first B).
        case pendingBasket(String)
    }
    let kind: Kind
    let key: String
    var title: String
    var count: Int?
    var swatch: NSColor?
    var outlined = false
    /// Nesting inside a section that is not an outline level (subfolders of the open folder).
    var indent = 0
    /// Disambiguation after the title in tertiary text (a recent folder's parent).
    var detail: String?
    var badge: String?
    var tooltip: String?
    var children: [SidebarRow] = []

    init(_ kind: Kind, key: String, title: String) {
        self.kind = kind; self.key = key; self.title = title
    }

    var node: CollectionNode? { if case .node(let n) = kind { n } else { nil } }
    var isHeader: Bool { if case .header = kind { true } else { false } }
}

struct SidebarOutline: NSViewRepresentable {
    let model: AppModel
    let snapshot: SidebarSnapshot

    func makeCoordinator() -> SidebarController { SidebarController(model: model) }

    func makeNSView(context: Context) -> NSScrollView {
        let c = context.coordinator
        c.apply(snapshot)
        return c.scrollView
    }

    func updateNSView(_ view: NSScrollView, context: Context) {
        context.coordinator.apply(snapshot)
    }
}

@MainActor
final class SidebarController: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate, NSMenuDelegate, NSTextFieldDelegate {
    let model: AppModel
    let scrollView = NSScrollView()
    let outline = NSOutlineView()
    private var roots: [SidebarRow] = []
    private var snapshot: SidebarSnapshot?
    private var collapsed: Set<String> = []
    private var applying = false
    private var editingID: Int64?

    init(model: AppModel) {
        self.model = model
        super.init()
        let column = NSTableColumn(identifier: .init("main"))
        column.resizingMask = .autoresizingMask
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        outline.autoresizingMask = [.width]
        outline.headerView = nil
        outline.style = .sourceList
        outline.backgroundColor = .clear
        outline.floatsGroupRows = false
        outline.indentationPerLevel = Theme.Space.m
        outline.rowSizeStyle = .custom
        outline.intercellSpacing = NSSize(width: 0, height: 0)
        outline.dataSource = self
        outline.delegate = self
        outline.registerForDraggedTypes([.sidebarNode])
        outline.setDraggingSourceOperationMask(.move, forLocal: true)
        outline.draggingDestinationFeedbackStyle = .sourceList
        outline.target = self
        outline.doubleAction = #selector(doubleClicked)
        let menu = NSMenu()
        menu.delegate = self
        outline.menu = menu
        outline.setAccessibilityIdentifier("sidebarOutline")
        scrollView.documentView = outline
        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
    }

    // MARK: Model → rows

    func apply(_ snap: SidebarSnapshot) {
        guard snap != snapshot else { return }
        snapshot = snap
        guard editingID == nil else { return }   // rebuild after the inline rename ends
        roots = rows(for: snap)
        applying = true
        outline.reloadData()
        for header in roots { outline.expandItem(header) }
        expandGroups(roots)
        selectSource(snap.source)
        applying = false
    }

    private func expandGroups(_ rows: [SidebarRow]) {
        for row in rows {
            if row.node?.kind == .group, !collapsed.contains(row.key) { outline.expandItem(row) }
            expandGroups(row.children)
        }
    }

    private func sourceKey(_ s: LibrarySource) -> String {
        switch s {
        case .all: "src:all"
        case .notInAlbum: "src:unfiled"
        case .album(let h):
            CollectionNode.flatten(snapshot?.nodes ?? []).first { $0.kind == .album && $0.handle == h }
                .map { "node:\($0.id)" } ?? "basket:\(h)"
        case .smartAlbum(let id, _), .group(let id, _): "node:\(id)"
        case .decision(let d): "src:decision:\(d)"
        case .mark(let m): "src:mark:\(m)"
        }
    }

    private func find(_ key: String, in rows: [SidebarRow]) -> SidebarRow? {
        for row in rows {
            if row.key == key { return row }
            if let hit = find(key, in: row.children) { return hit }
        }
        return nil
    }

    private func selectSource(_ source: LibrarySource) {
        guard let row = find(sourceKey(source), in: roots) else { outline.deselectAll(nil); return }
        let index = outline.row(forItem: row)
        if index >= 0 { outline.selectRowIndexes([index], byExtendingSelection: false) }
    }

    private func rows(for s: SidebarSnapshot) -> [SidebarRow] {
        let library = SidebarRow(.header(.library), key: "hdr:library", title: "Library")
        let all = SidebarRow(.source(.all), key: "src:all", title: "All Photos")
        all.count = s.allCount
        library.children = [all]
        if s.engineBacked {
            let unfiled = SidebarRow(.source(.notInAlbum), key: "src:unfiled", title: "Not in Any Album")
            unfiled.count = s.unfiledCount
            unfiled.tooltip = "Derived status: photos in this folder that no album contains"
            library.children.append(unfiled)
        }

        let folders = SidebarRow(.header(.folders), key: "hdr:folders", title: "Folders")
        let current = SidebarRow(.currentFolder(s.folder), key: "folder:current",
                                 title: s.folder?.lastPathComponent ?? (s.hasItems ? s.title : "No folder open"))
        current.tooltip = s.folder?.path
        folders.children = [current]
        for sub in s.subfolders {
            let r = SidebarRow(.folder(sub), key: "folder:\(sub.path)", title: sub.lastPathComponent)
            r.indent = 1
            r.tooltip = sub.path
            folders.children.append(r)
        }
        for url in s.recents {
            let r = SidebarRow(.folder(url), key: "recent:\(url.path)", title: url.lastPathComponent)
            r.detail = url.deletingLastPathComponent().lastPathComponent
            r.tooltip = url.path
            folders.children.append(r)
        }

        let albums = SidebarRow(.header(.albums), key: "hdr:albums", title: "Albums")
        func build(_ node: CollectionNode) -> SidebarRow {
            let r = SidebarRow(.node(node), key: "node:\(node.id)", title: node.name)
            switch node.kind {
            case .album:
                let handle = node.handle ?? node.name
                r.count = s.albumCounts[handle] ?? node.imageCount
                if handle == s.basketTarget {
                    r.swatch = Theme.Palette.basket
                    r.badge = "B"
                    r.tooltip = "Basket target: B adds here. ⌫ in this album removes from the album only."
                } else {
                    r.tooltip = "Album. Right-click for Set as Basket Target, Add Selected Photos, Rename, Delete."
                }
            case .smartAlbum:
                r.outlined = true
                r.tooltip = "Smart album: \(node.rule ?? "")" + (node.scoped ? "\nSearches only this group's albums" : "")
                if node.scoped { r.badge = "⌂" }
            case .group:
                r.tooltip = "Album group. Drag albums in or out; right-click to add inside."
                r.children = node.children.map(build)
            }
            return r
        }
        albums.children = s.nodes.map(build)
        let hasTarget = CollectionNode.flatten(s.nodes).contains { $0.kind == .album && $0.handle == s.basketTarget }
        if !hasTarget {
            let r = SidebarRow(.pendingBasket(s.basketTarget), key: "basket:\(s.basketTarget)", title: s.basketTarget)
            r.count = s.albumCounts[s.basketTarget] ?? 0
            r.swatch = Theme.Palette.basket
            r.badge = "B"
            r.tooltip = "Basket target: created on the first B"
            albums.children.insert(r, at: 0)
        }

        let cull = SidebarRow(.header(.cull), key: "hdr:cull", title: "Culling")
        let presets: [(LibrarySource, String, Int?, NSColor?)] = [
            (.decision(.keep), "src:decision:\(Decision.keep)", s.keep, Theme.Palette.keep),
            (.decision(.undecided), "src:decision:\(Decision.undecided)", s.undecided, nil),
            (.decision(.reject), "src:decision:\(Decision.reject)", s.reject, Theme.Palette.reject),
        ] + [UInt8(6), 7, 8, 9].map { m in (LibrarySource.mark(m), "src:mark:\(m)", nil, MarkStyle.color(m)) }
        cull.children = presets.map { source, key, count, swatch in
            let r = SidebarRow(.source(source), key: key, title: source.title)
            r.count = count
            r.swatch = swatch
            return r
        }
        return [library, folders, albums, cull]
    }

    // MARK: Data source

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        (item as? SidebarRow)?.children.count ?? roots.count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        (item as? SidebarRow)?.children[index] ?? roots[index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        guard let row = item as? SidebarRow else { return false }
        return row.isHeader || row.node?.kind == .group
    }

    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool {
        (item as? SidebarRow)?.isHeader == true
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        guard let row = item as? SidebarRow else { return false }
        switch row.kind {
        case .header, .currentFolder: return false
        default: return true
        }
    }

    func outlineView(_ outlineView: NSOutlineView, shouldShowOutlineCellForItem item: Any) -> Bool {
        !((item as? SidebarRow)?.isHeader ?? false)
    }

    func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat {
        (item as? SidebarRow)?.isHeader == true ? Theme.Height.large : Theme.Height.row
    }

    func outlineView(_ outlineView: NSOutlineView, rowViewForItem item: Any) -> NSTableRowView? {
        SidebarRowView()
    }

    func outlineViewItemDidCollapse(_ notification: Notification) {
        if !applying, let row = notification.userInfo?["NSObject"] as? SidebarRow { collapsed.insert(row.key) }
    }

    func outlineViewItemDidExpand(_ notification: Notification) {
        if !applying, let row = notification.userInfo?["NSObject"] as? SidebarRow { collapsed.remove(row.key) }
    }

    // MARK: Views

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let row = item as? SidebarRow else { return nil }
        if case .header(let section) = row.kind {
            let v = outlineView.makeView(withIdentifier: SidebarHeaderCell.identifier, owner: nil) as? SidebarHeaderCell
                ?? SidebarHeaderCell()
            v.configure(title: row.title, addMenu: section == .albums && snapshot?.engineBacked == true ? addMenu(parent: nil) : nil)
            return v
        }
        let v = outlineView.makeView(withIdentifier: SidebarCell.identifier, owner: nil) as? SidebarCell ?? SidebarCell()
        v.configure(row)
        v.textField?.delegate = self
        return v
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !applying, let row = outline.item(atRow: outline.selectedRow) as? SidebarRow else { return }
        switch row.kind {
        case .source(let s): model.setSource(s)
        case .folder(let url): model.openFolder(url)
        case .node(let n): model.setSource(model.collections.source(for: n))
        case .pendingBasket(let name): model.setSource(.album(name))
        case .header, .currentFolder: break
        }
    }

    @objc private func doubleClicked() {
        let r = outline.clickedRow
        guard r >= 0, let row = outline.item(atRow: r) as? SidebarRow, let node = row.node else { return }
        if node.kind == .smartAlbum { model.collections.editSmartAlbum(node.id) } else { beginRename(row: r) }
    }

    // MARK: Inline rename

    private func beginRename(row: Int) {
        guard let cell = outline.view(atColumn: 0, row: row, makeIfNecessary: false) as? SidebarCell,
              let item = outline.item(atRow: row) as? SidebarRow, let node = item.node,
              let field = cell.textField else { return }
        editingID = node.id
        field.isEditable = true
        field.stringValue = node.name
        outline.window?.makeFirstResponder(field)
        field.currentEditor()?.selectAll(nil)
    }

    func controlTextDidEndEditing(_ note: Notification) {
        guard let field = note.object as? NSTextField, let id = editingID else { return }
        field.isEditable = false
        editingID = nil
        let name = field.stringValue
        let cancelled = (note.userInfo?["NSTextMovement"] as? Int) == NSTextMovement.cancel.rawValue
        if !cancelled { model.collections.rename(id, to: name) }
        if let snap = snapshot { snapshot = nil; apply(snap) }
    }

    // MARK: Context menus

    private func item(_ title: String, _ action: @escaping () -> Void) -> NSMenuItem {
        let i = ClosureMenuItem(title: title, action: action)
        return i
    }

    private func addMenu(parent: Int64?) -> NSMenu {
        let menu = NSMenu()
        let c = model.collections
        menu.addItem(item("New Album…") { c.newAlbum(in: parent) })
        menu.addItem(item("New Album Group…") { c.newGroup(in: parent) })
        menu.addItem(item("New Smart Album…") { c.newSmartAlbum(in: parent) })
        return menu
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let r = outline.clickedRow
        let row = r >= 0 ? outline.item(atRow: r) as? SidebarRow : nil
        let c = model.collections
        guard snapshot?.engineBacked == true else { return }
        switch row?.kind {
        case .node(let n):
            let index = r
            switch n.kind {
            case .album:
                let handle = n.handle ?? n.name
                let target = handle == model.basketTarget
                let t = item("Set as Basket Target") { [weak self] in self?.model.setBasketTarget(handle) }
                t.isEnabled = !target
                menu.addItem(t)
                menu.addItem(item("Add Selected Photos") { c.addSelection(toAlbum: n.id) })
                menu.addItem(.separator())
                menu.addItem(item("Rename") { [weak self] in self?.beginRename(row: index) })
                menu.addItem(item("Delete Album…") { c.confirmDelete(n.id) })
            case .group:
                for i in addMenu(parent: n.id).items { i.title = i.title.replacingOccurrences(of: "…", with: " Inside…"); menu.addItem(i.copy() as! NSMenuItem) }
                menu.addItem(.separator())
                menu.addItem(item("Rename") { [weak self] in self?.beginRename(row: index) })
                menu.addItem(item("Delete Group…") { c.confirmDelete(n.id) })
            case .smartAlbum:
                menu.addItem(item("Edit Smart Album…") { c.editSmartAlbum(n.id) })
                if n.parent != nil {
                    let s = item("Search Only This Group") { c.setScoped(n.id, !n.scoped) }
                    s.state = n.scoped ? .on : .off
                    menu.addItem(s)
                }
                menu.addItem(.separator())
                menu.addItem(item("Rename") { [weak self] in self?.beginRename(row: index) })
                menu.addItem(item("Delete Smart Album…") { c.confirmDelete(n.id) })
            }
        case .pendingBasket, .header(.albums), .none:
            for i in addMenu(parent: nil).items { menu.addItem(i.copy() as! NSMenuItem) }
        default:
            break
        }
    }

    // MARK: Drag and drop (reorder and nest)

    func outlineView(_ outlineView: NSOutlineView, pasteboardWriterForItem item: Any) -> NSPasteboardWriting? {
        guard let node = (item as? SidebarRow)?.node else { return nil }
        let p = NSPasteboardItem()
        p.setString(String(node.id), forType: .sidebarNode)
        return p
    }

    private func draggedID(_ info: NSDraggingInfo) -> Int64? {
        info.draggingPasteboard.string(forType: .sidebarNode).flatMap(Int64.init)
    }

    private func parentRow(of row: SidebarRow) -> SidebarRow? { outline.parent(forItem: row) as? SidebarRow }

    /// The group (or root) and child index a drop lands in; albums and smart albums are not containers.
    private func dropTarget(_ item: Any?, _ index: Int) -> (row: SidebarRow, index: Int)? {
        guard let row = item as? SidebarRow else { return nil }
        if case .header(.albums) = row.kind { return (row, index) }
        guard let node = row.node else { return nil }
        if node.kind == .group { return (row, index) }
        // Dropped on an album: place after it in its parent.
        guard let parent = parentRow(of: row), let i = parent.children.firstIndex(of: row) else { return nil }
        return (parent, i + 1)
    }

    func outlineView(_ outlineView: NSOutlineView, validateDrop info: NSDraggingInfo, proposedItem item: Any?,
                     proposedChildIndex index: Int) -> NSDragOperation {
        guard let id = draggedID(info), var target = dropTarget(item, index) else { return [] }
        // A group cannot go into itself or its descendants.
        var walk: SidebarRow? = target.row
        while let w = walk {
            if w.node?.id == id { return [] }
            walk = parentRow(of: w)
        }
        if target.index < 0 { target.index = target.row.children.count }
        if (item as? SidebarRow) !== target.row || index != target.index {
            outlineView.setDropItem(target.row, dropChildIndex: target.index)
        }
        return .move
    }

    func outlineView(_ outlineView: NSOutlineView, acceptDrop info: NSDraggingInfo, item: Any?, childIndex index: Int) -> Bool {
        guard let id = draggedID(info), let target = dropTarget(item, index) else { return false }
        let parent = target.row.node?.id
        var at = target.index < 0 ? target.row.children.count : target.index
        // Indices count the dragged row itself when it stays in the same parent.
        let siblings = target.row.children.compactMap(\.node?.id)
        if let old = siblings.firstIndex(of: id), old < at { at -= 1 }
        // The pending basket placeholder is not in library.json.
        let placeholders = target.row.children.prefix(at).filter { if case .pendingBasket = $0.kind { true } else { false } }.count
        model.collections.move(id, to: parent, index: at - placeholders)
        return true
    }
}

/// NSMenuItem with a closure action.
final class ClosureMenuItem: NSMenuItem {
    private var handler: () -> Void = {}

    convenience init(title: String, action: @escaping () -> Void) {
        self.init(title: title, action: #selector(run), keyEquivalent: "")
        handler = action
        target = self
    }

    @objc private func run() { handler() }

    override func copy(with zone: NSZone? = nil) -> Any {
        let c = ClosureMenuItem(title: title, action: handler)
        c.state = state
        c.isEnabled = isEnabled
        return c
    }
}

/// Selection: the subtle accent fill with the control radius (not the system accent slab), so
/// text keeps its own colour and the sidebar has one accent.
final class SidebarRowView: NSTableRowView {
    override var isEmphasized: Bool { get { false } set {} }

    override func drawSelection(in dirtyRect: NSRect) {
        guard selectionHighlightStyle != .none else { return }
        let r = bounds.insetBy(dx: Theme.Space.s, dy: 0)
        Theme.Palette.accentSubtle.setFill()
        NSBezierPath(roundedRect: r, xRadius: Theme.Radius.control, yRadius: Theme.Radius.control).fill()
    }
}

final class SidebarHeaderCell: NSTableCellView {
    static let identifier = NSUserInterfaceItemIdentifier("SidebarHeaderCell")
    private let label = NSTextField(labelWithString: "")
    private let add = NSPopUpButton(frame: .zero, pullsDown: true)

    init() {
        super.init(frame: .zero)
        identifier = Self.identifier
        label.font = Theme.NSFonts.captionSemibold
        label.textColor = Theme.Palette.textTertiary
        label.translatesAutoresizingMaskIntoConstraints = false
        add.translatesAutoresizingMaskIntoConstraints = false
        add.isBordered = false
        add.controlSize = .small
        (add.cell as? NSPopUpButtonCell)?.arrowPosition = .noArrow
        add.setAccessibilityIdentifier("sidebarAddMenu")
        add.toolTip = "New album, album group or smart album"
        addSubview(label)
        addSubview(add)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Space.xxs),
            label.firstBaselineAnchor.constraint(equalTo: bottomAnchor, constant: -Theme.Space.s),
            add.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Space.xs),
            add.centerYAnchor.constraint(equalTo: label.centerYAnchor),
            add.widthAnchor.constraint(equalToConstant: Theme.Height.small),
            add.heightAnchor.constraint(equalToConstant: Theme.Height.small),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func configure(title: String, addMenu: NSMenu?) {
        label.stringValue = title
        add.isHidden = addMenu == nil
        guard let addMenu else { return }
        let menu = NSMenu()
        let plus = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        plus.image = NSImage(systemSymbolName: "plus", accessibilityDescription: "Add")?
            .withSymbolConfiguration(.init(pointSize: 11, weight: .medium))
        menu.addItem(plus)
        for i in addMenu.items { menu.addItem(i.copy() as! NSMenuItem) }
        add.menu = menu
        add.contentTintColor = Theme.Palette.textTertiary
    }
}

final class SidebarCell: NSTableCellView {
    static let identifier = NSUserInterfaceItemIdentifier("SidebarCell")
    private let swatch = NSView()
    private let badge = NSTextField(labelWithString: "")
    private let detail = NSTextField(labelWithString: "")
    private let count = NSTextField(labelWithString: "")
    private var leading: NSLayoutConstraint?

    init() {
        super.init(frame: .zero)
        identifier = Self.identifier
        let title = NSTextField(labelWithString: "")
        title.lineBreakMode = .byTruncatingTail
        title.isEditable = false
        title.focusRingType = .none
        title.drawsBackground = false
        textField = title
        swatch.wantsLayer = true
        swatch.layer?.cornerRadius = Theme.Space.xxs
        badge.font = Theme.NSFonts.captionMedium
        badge.textColor = Theme.Palette.textTertiary
        badge.alignment = .center
        badge.wantsLayer = true
        badge.layer?.cornerRadius = Theme.Radius.chip
        badge.layer?.borderWidth = Theme.Space.hairline
        detail.font = Theme.NSFonts.caption
        detail.textColor = Theme.Palette.textTertiary
        detail.lineBreakMode = .byTruncatingTail
        count.font = Theme.NSFonts.captionNumeric
        count.textColor = Theme.Palette.textTertiary
        count.alignment = .right
        for v in [swatch, title, badge, detail, count] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        title.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        detail.setContentCompressionResistancePriority(.defaultLow - 1, for: .horizontal)
        count.setContentCompressionResistancePriority(.required, for: .horizontal)
        badge.setContentCompressionResistancePriority(.required, for: .horizontal)
        let leading = swatch.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Space.xxs)
        self.leading = leading
        NSLayoutConstraint.activate([
            leading,
            swatch.centerYAnchor.constraint(equalTo: centerYAnchor),
            swatch.widthAnchor.constraint(equalToConstant: Theme.Space.s),
            swatch.heightAnchor.constraint(equalToConstant: Theme.Space.s),
            title.leadingAnchor.constraint(equalTo: swatch.trailingAnchor, constant: Theme.Space.s),
            title.centerYAnchor.constraint(equalTo: centerYAnchor),
            detail.leadingAnchor.constraint(equalTo: title.trailingAnchor, constant: Theme.Space.xs),
            detail.firstBaselineAnchor.constraint(equalTo: title.firstBaselineAnchor),
            badge.leadingAnchor.constraint(equalTo: detail.trailingAnchor, constant: Theme.Space.xs),
            badge.centerYAnchor.constraint(equalTo: centerYAnchor),
            badge.widthAnchor.constraint(equalToConstant: Theme.Height.chip),
            badge.heightAnchor.constraint(equalToConstant: Theme.Height.chip),
            count.leadingAnchor.constraint(greaterThanOrEqualTo: badge.trailingAnchor, constant: Theme.Space.xs),
            count.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Space.s),
            count.firstBaselineAnchor.constraint(equalTo: title.firstBaselineAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        badge.layer?.borderColor = Theme.Palette.hairlineStrong.cgColor(for: self)
        swatch.layer?.borderColor = Theme.Palette.textTertiary.cgColor(for: self)
    }

    func configure(_ row: SidebarRow) {
        guard let title = textField else { return }
        title.stringValue = row.title
        title.isEditable = false
        let group = row.node?.kind == .group
        let secondary: Bool = switch row.kind {
        case .folder, .currentFolder: row.key.hasPrefix("recent:") || row.indent > 0
        default: false
        }
        title.font = group || row.key == "folder:current" ? Theme.NSFonts.labelMedium : Theme.NSFonts.label
        title.textColor = secondary ? Theme.Palette.textSecondary : Theme.Palette.textPrimary
        leading?.constant = Theme.Space.xxs + CGFloat(row.indent) * Theme.Space.m
        swatch.layer?.backgroundColor = row.swatch?.cgColor ?? NSColor.clear.cgColor
        swatch.layer?.borderWidth = row.outlined ? Theme.Space.hairline : 0
        swatch.layer?.borderColor = Theme.Palette.textTertiary.cgColor(for: self)
        detail.stringValue = row.detail ?? ""
        detail.isHidden = row.detail == nil
        badge.stringValue = row.badge ?? ""
        badge.isHidden = row.badge == nil
        badge.layer?.borderWidth = row.badge == "⌂" ? 0 : Theme.Space.hairline
        badge.layer?.borderColor = Theme.Palette.hairlineStrong.cgColor(for: self)
        count.stringValue = row.count.map { $0.formatted() } ?? ""
        toolTip = row.tooltip
        setAccessibilityLabel(row.title)
    }
}
