import AppKit
import Observation
import TesseraCore
import TesseraFFI

/// Smart album sheet state (new or edit). The rule is kept both as a tree and as text; the
/// engine renders, parses and validates both directions.
struct SmartAlbumDraft: Identifiable {
    let id = UUID()
    /// nil: create a new smart album.
    var editing: Int64?
    var name: String
    var parent: Int64?
    var scoped: Bool
    var tree: RuleNode
    var text: String
    var diagnostic: RuleDiagnostic?
    /// Photos in the open folder matching the rule (nil while invalid).
    var matchCount: Int?
}

/// Albums, album groups and smart albums (library.json), the filter bar, keywords and the
/// metadata panel. Owned by `AppModel`; engine calls are synchronous (library.json + SQLite at
/// folder scale), while filter-bar typing is debounced.
@MainActor @Observable
final class LibraryModel {
    @ObservationIgnored weak var app: AppModel?
    @ObservationIgnored private(set) var catalog: LibraryCatalog?

    private(set) var isAvailable = false
    /// Sidebar tree (roots).
    private(set) var nodes: [CollectionNode] = []
    var filter = LibraryFilter() {
        didSet { if filter != oldValue { scheduleSearch() } }
    }
    private(set) var facets: SearchFacets?
    private(set) var diagnostic: RuleDiagnostic?
    private(set) var matchCount: Int?
    /// Grammar text of source scope + filter, for "Save as Smart Album".
    private(set) var composedRule = ""
    private(set) var scopeGroup: Int64?
    private(set) var keywords: [KeywordInfo] = []
    /// Metadata of the focused photo.
    private(set) var metadata: ImageMetadata?
    /// IPTC fields whose values differ across the selection.
    private(set) var mixed: Set<String> = []
    /// Bumped when the metadata panel should drop edits in progress (focus or file changed).
    private(set) var metadataRevision = 0
    var editor: SmartAlbumDraft?

    /// Items matching the current source scope and filter bar; nil when the source needs no
    /// engine search (All Photos, an album, a cull preset) and the filter bar is empty.
    @ObservationIgnored private(set) var matches: [Int]?
    @ObservationIgnored private var searchTask: Task<Void, Never>?
    @ObservationIgnored private var facetTask: Task<Void, Never>?

    // MARK: Lifecycle

    func install(_ library: any PhotoLibrary) {
        searchTask?.cancel()
        matches = nil
        filter = LibraryFilter()
        searchTask?.cancel()
        facets = nil
        diagnostic = nil
        matchCount = nil
        composedRule = ""
        metadata = nil
        catalog = nil
        if let engine = library as? EngineLibrary {
            do { catalog = try LibraryCatalog(library: engine) } catch {
                app?.statusMessage = "Library unavailable: \(error.localizedDescription)"
            }
        }
        isAvailable = catalog != nil
        reloadNodes()
        reloadKeywords()
        refreshMatches(applyMatches: false)
    }

    private func report(_ verb: String, _ body: () throws -> Void) -> Bool {
        do {
            try body()
            return true
        } catch {
            app?.statusMessage = "\(verb) failed: \(error.localizedDescription)"
            return false
        }
    }

    // MARK: Search and the filter bar

    /// Engine scope for the current source; nil means "all photos in the folder".
    private func scope(for source: LibrarySource) -> (SearchScope, needed: Bool) {
        switch source {
        case .all: return (.all, false)
        case .decision, .mark: return (.all, false)
        case .notInAlbum: return (.all, true)
        case .album(let name):
            if let id = node(handle: name)?.id { return (.album(id: id), false) }
            return (.all, false)
        case .smartAlbum(let id, _): return (.smartAlbum(id: id), true)
        case .group(let id, _): return (.group(id: id), true)
        }
    }

    /// Runs the search for the app's source + filter. `applyMatches` false refreshes only the
    /// facet counts (after culling edits: filters apply when chosen, not live).
    func refreshMatches(applyMatches: Bool = true) {
        guard let catalog, let app else {
            if applyMatches { matches = nil }
            return
        }
        let (scope, needed) = scope(for: app.source)
        var f = filter
        if app.source == .notInAlbum { f.albumStatus = "none" }
        do {
            let r = try catalog.search(f, scope: scope)
            if applyMatches {
                let albumMissing: Bool = if case .album(let name) = app.source { node(handle: name) == nil } else { false }
                matches = (needed || !filter.isEmpty) && !albumMissing ? r.ids : nil
            }
            if facets != r.facets { facets = r.facets }
            if diagnostic != r.diagnostic { diagnostic = r.diagnostic }
            matchCount = r.ids.count
            composedRule = r.rule
            scopeGroup = r.group
        } catch {
            if applyMatches { matches = needed ? [] : nil }
            app.statusMessage = "Search failed: \(error.localizedDescription)"
        }
    }

    private func scheduleSearch() {
        searchTask?.cancel()
        searchTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(120))
            guard let self, !Task.isCancelled, let app = self.app else { return }
            app.refreshVisible { self.refreshMatches() }
        }
    }

    /// Decisions, grades, marks or basket changed: facet counts follow (the grid does not).
    func cullDidChange(albums: Bool) {
        if albums { reloadNodes() }
        facetTask?.cancel()
        facetTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(250))
            guard let self, !Task.isCancelled else { return }
            self.refreshMatches(applyMatches: false)
        }
    }

    func clearFilter() { filter = LibraryFilter() }

    func toggle(_ keyPath: WritableKeyPath<LibraryFilter, Set<String>>, _ value: String) {
        if filter[keyPath: keyPath].contains(value) { filter[keyPath: keyPath].remove(value) }
        else { filter[keyPath: keyPath].insert(value) }
    }

    // MARK: Sidebar nodes

    func reloadNodes() {
        guard let catalog else { if !nodes.isEmpty { nodes = [] }; return }
        do {
            let fresh = try catalog.nodes()
            if fresh != nodes { nodes = fresh }
        } catch {
            app?.statusMessage = "Could not read library.json: \(error.localizedDescription)"
        }
    }

    var flatNodes: [CollectionNode] { CollectionNode.flatten(nodes) }
    func node(id: Int64) -> CollectionNode? { flatNodes.first { $0.id == id } }
    func node(handle: String) -> CollectionNode? { flatNodes.first { $0.kind == .album && $0.handle == handle } }
    /// Groups with their depth, for location pickers.
    var groups: [CollectionNode] { flatNodes.filter { $0.kind == .group } }

    func source(for node: CollectionNode) -> LibrarySource {
        switch node.kind {
        case .album: .album(node.handle ?? node.name)
        case .smartAlbum: .smartAlbum(id: node.id, name: node.name)
        case .group: .group(id: node.id, name: node.name)
        }
    }

    private func libraryChanged(_ message: String? = nil) {
        reloadNodes()
        app?.libraryDidChange(message: message)
    }

    func promptName(title: String, message: String, initial: String = "", confirm: String = "Create",
                    _ action: @escaping (String) -> Void) {
        guard let window = app?.mainWindow else { return }
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24))
        field.stringValue = initial
        field.placeholderString = "Name"
        alert.accessoryView = field
        alert.addButton(withTitle: confirm)
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { response in
            let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
            MainActor.assumeIsolated {
                if response == .alertFirstButtonReturn, !name.isEmpty { action(name) }
            }
        }
    }

    func newAlbum(in parent: Int64? = nil) {
        promptName(title: "New Album", message: "A manual list of photos with its own order. Photos can be in many albums.") { [weak self] name in
            guard let self, let catalog = self.catalog else { return }
            var created: Int64?
            guard self.report("New album", { created = try catalog.store.createAlbum(name: name, parent: parent) }) else { return }
            self.libraryChanged("Created album “\(name)”")
            if created != nil { self.app?.setSource(.album(name)) }
        }
    }

    func newGroup(in parent: Int64? = nil) {
        promptName(title: "New Album Group", message: "Groups hold albums, smart albums and other groups. A smart album inside can search only the group.") { [weak self] name in
            guard let self, let catalog = self.catalog else { return }
            guard self.report("New group", { _ = try catalog.store.createGroup(name: name, parent: parent) }) else { return }
            self.libraryChanged("Created group “\(name)”")
        }
    }

    func rename(_ id: Int64, to name: String) {
        guard let catalog, let old = node(id: id) else { return }
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != old.name else { return }
        guard report("Rename", { try catalog.store.rename(id: id, name: name) }) else { reloadNodes(); return }
        // The basket target and the sidebar source follow an album's new handle.
        if old.kind == .album, let handle = old.handle, handle == app?.basketTarget { app?.setBasketTarget(name) }
        if let app {
            switch app.source {
            case .album(let h) where h == old.handle: app.setSource(.album(name))
            case .smartAlbum(let sid, _) where sid == id: app.setSource(.smartAlbum(id: id, name: name))
            case .group(let gid, _) where gid == id: app.setSource(.group(id: id, name: name))
            default: break
            }
        }
        libraryChanged("Renamed “\(old.name)” to “\(name)”")
    }

    func move(_ id: Int64, to parent: Int64?, index: Int) {
        guard let catalog else { return }
        guard report("Move", { try catalog.store.moveNode(id: id, parent: parent, index: UInt32(max(0, index))) }) else { return }
        libraryChanged()
    }

    /// Safe delete: confirms, then removes only the sidebar entry. Photos and files stay.
    func confirmDelete(_ id: Int64) {
        guard let catalog, let node = node(id: id), let window = app?.mainWindow else { return }
        let alert = NSAlert()
        switch node.kind {
        case .album:
            alert.messageText = "Delete the album “\(node.name)”?"
            alert.informativeText = "Only the album is removed. Its \(node.imageCount) photo\(node.imageCount == 1 ? "" : "s") stay in the folder and in other albums; no files are deleted."
        case .group:
            let n = node.children.count
            alert.messageText = "Delete the group “\(node.name)”?"
            alert.informativeText = n == 0 ? "The group is empty. No photos or files are affected."
                : "Its \(n) item\(n == 1 ? "" : "s") (albums, smart albums, groups) move up one level. No photos or files are deleted."
        case .smartAlbum:
            alert.messageText = "Delete the smart album “\(node.name)”?"
            alert.informativeText = "Only the saved search is removed. No photos or files are affected."
        }
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        // Return cancels, as with every delete in the app.
        alert.buttons[0].keyEquivalent = ""
        alert.buttons[1].keyEquivalent = "\r"
        alert.beginSheetModal(for: window) { [weak self] response in
            MainActor.assumeIsolated {
                guard response == .alertFirstButtonReturn, let self else { return }
                guard self.report("Delete", { try catalog.store.deleteNode(id: id) }) else { return }
                if let app = self.app, app.source == self.source(for: node) { app.setSource(.all) }
                self.libraryChanged("Deleted “\(node.name)”. No photos were deleted.")
            }
        }
    }

    func addSelection(toAlbum id: Int64) {
        guard let catalog, let app, let node = node(id: id) else { return }
        let items = app.targetIDs
        guard !items.isEmpty else { app.statusMessage = "Select photos to add"; return }
        guard report("Add to album", { try catalog.addToAlbum(id, items: items) }) else { return }
        libraryChanged("Added \(items.count) photo\(items.count == 1 ? "" : "s") to “\(node.name)”")
    }

    func setScoped(_ id: Int64, _ scoped: Bool) {
        guard let catalog else { return }
        guard report("Scope", { try catalog.store.updateSmartAlbum(id: id, rule: nil, scoped: scoped) }) else { return }
        libraryChanged(scoped ? "Searches only this group's albums" : "Searches all photos")
    }

    // MARK: Smart album editor

    func newSmartAlbum(in parent: Int64? = nil, rule: String? = nil) {
        guard catalog != nil else { app?.statusMessage = "Smart albums need a folder opened with the engine"; return }
        let text = rule ?? "decision:keep"
        var draft = SmartAlbumDraft(editing: nil, name: "Smart Album", parent: parent, scoped: parent != nil,
                                    tree: RuleNode(kind: .all, children: [.condition("decision", ":", "keep")]),
                                    text: text)
        setText(text, in: &draft)
        editor = draft
    }

    func editSmartAlbum(_ id: Int64) {
        guard let node = node(id: id), node.kind == .smartAlbum else { return }
        var draft = SmartAlbumDraft(editing: id, name: node.name, parent: node.parent, scoped: node.scoped,
                                    tree: RuleNode(kind: .all), text: node.rule ?? "")
        setText(draft.text, in: &draft)
        editor = draft
    }

    /// Free-text edit: validate; when it parses, the tree follows.
    func setText(_ text: String, in draft: inout SmartAlbumDraft) {
        draft.text = text
        guard let catalog else { return }
        do {
            let check = try catalog.store.checkRule(text: text)
            draft.diagnostic = check.diagnostic
            if check.diagnostic == nil, let tree = RuleNode.from(check.items) { draft.tree = tree }
        } catch {
            draft.diagnostic = RuleDiagnostic(start: 0, end: UInt32(text.utf8.count), message: error.localizedDescription)
        }
        updateCount(&draft)
    }

    /// Tree edit: the engine renders the text and locates any problem in it.
    func setTree(_ tree: RuleNode, in draft: inout SmartAlbumDraft) {
        draft.tree = tree
        guard let catalog else { return }
        do {
            let check = try catalog.store.formatRule(items: tree.items)
            draft.text = check.text
            draft.diagnostic = check.diagnostic
        } catch {
            draft.diagnostic = RuleDiagnostic(start: 0, end: 0, message: error.localizedDescription)
        }
        updateCount(&draft)
    }

    private func updateCount(_ draft: inout SmartAlbumDraft) {
        guard let catalog, draft.diagnostic == nil else { draft.matchCount = nil; return }
        var f = LibraryFilter()
        f.text = draft.text
        let scope: SearchScope = draft.scoped ? draft.parent.map { .group(id: $0) } ?? .all : .all
        draft.matchCount = (try? catalog.search(f, scope: scope)).flatMap { $0.diagnostic == nil ? $0.ids.count : nil }
    }

    func refreshEditorCount() {
        guard var draft = editor else { return }
        updateCount(&draft)
        editor = draft
    }

    func saveEditor() {
        guard let draft = editor, let catalog, draft.diagnostic == nil else { return }
        let name = draft.name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { app?.statusMessage = "Name the smart album"; return }
        var id = draft.editing
        let ok = report("Save smart album") {
            if let existing = draft.editing {
                try catalog.store.updateSmartAlbum(id: existing, rule: draft.text, scoped: draft.scoped)
                if node(id: existing)?.name != name { try catalog.store.rename(id: existing, name: name) }
                if node(id: existing)?.parent != draft.parent {
                    try catalog.store.moveNode(id: existing, parent: draft.parent, index: UInt32.max)
                }
            } else {
                id = try catalog.store.createSmartAlbum(name: name, rule: draft.text, parent: draft.parent, scoped: draft.scoped)
            }
        }
        guard ok, let id else { return }
        editor = nil
        reloadNodes()
        app?.setSource(.smartAlbum(id: id, name: name))
        if draft.editing == nil { filter = LibraryFilter() }
        libraryChanged("Saved smart album “\(name)”")
    }

    /// Filter bar ▸ Save as Smart Album: the composed rule, placed (and scoped) in the group
    /// being browsed.
    func saveFilterAsSmartAlbum() {
        guard !composedRule.isEmpty else { app?.statusMessage = "Set a filter first"; return }
        newSmartAlbum(in: scopeGroup, rule: composedRule)
        if var draft = editor {
            draft.scoped = scopeGroup != nil
            updateCount(&draft)
            editor = draft
        }
    }

    // MARK: Keywords

    func reloadKeywords() {
        guard let catalog else { if !keywords.isEmpty { keywords = [] }; return }
        if let fresh = try? catalog.keywords(), fresh != keywords { keywords = fresh }
    }

    /// Bulk apply (or remove) to the selection; writes each photo's XMP sidecar.
    func applyKeywords(_ names: [String], add: Bool) {
        guard let catalog, let app else { return }
        let names = names.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        let items = app.targetIDs
        guard !names.isEmpty, !items.isEmpty else { return }
        guard report(add ? "Add keyword" : "Remove keyword", { try catalog.applyKeywords(names, to: items, add: add) }) else { return }
        reloadKeywords()
        reloadMetadata()
        cullDidChange(albums: false)
        let what = names.count == 1 ? "“\(names[0])”" : "\(names.count) keywords"
        app.statusMessage = add ? "Added \(what) to \(items.count) photo\(items.count == 1 ? "" : "s")"
            : "Removed \(what) from \(items.count) photo\(items.count == 1 ? "" : "s")"
    }

    func newKeyword(parent: String?) {
        promptName(title: parent.map { "New Keyword in “\($0)”" } ?? "New Keyword",
                   message: "Keywords are written to each photo's XMP sidecar when applied. Child keywords match searches for their parent.") { [weak self] name in
            guard let self, let catalog = self.catalog else { return }
            guard self.report("New keyword", { try catalog.store.addKeyword(name: name, parent: parent) }) else { return }
            self.reloadKeywords()
        }
    }

    func moveKeyword(_ name: String, to parent: String?) {
        guard let catalog else { return }
        guard report("Move keyword", { try catalog.store.moveKeyword(name: name, parent: parent) }) else { return }
        reloadKeywords()
    }

    /// Safe: removes the keyword from the list only; photos keep the tag in their sidecars.
    func deleteKeyword(_ name: String) {
        guard let catalog else { return }
        guard report("Delete keyword", { try catalog.store.deleteKeyword(name: name) }) else { return }
        reloadKeywords()
        app?.statusMessage = "Removed “\(name)” from the keyword list. Photos keep the tag; remove it from photos with −."
    }

    // MARK: Metadata

    func focusDidChange() { reloadMetadata() }

    func reloadMetadata() {
        metadataRevision += 1
        guard let catalog, let app, let item = app.focusedItem else { metadata = nil; mixed = []; return }
        metadata = try? catalog.metadata(of: item.id)
        // Fields that differ across the selection (up to 500 photos compared) show "Mixed".
        var differing = Set<String>()
        let others = app.targetIDs.filter { $0 != item.id }.prefix(499)
        if let base = metadata, !others.isEmpty {
            for id in others {
                guard let m = try? catalog.metadata(of: id) else { continue }
                if m.title != base.title { differing.insert("title") }
                if m.caption != base.caption { differing.insert("caption") }
                if m.creator != base.creator { differing.insert("creator") }
                if m.copyright != base.copyright { differing.insert("copyright") }
                if Set(m.keywords) != Set(base.keywords) { differing.insert("keywords") }
            }
        }
        mixed = differing
    }

    func saveIPTC(_ edit: IptcEdit, items: [Int]) {
        guard let catalog, let app else { return }
        guard !items.isEmpty else { return }
        guard report("Save metadata", { try catalog.setIPTC(edit, items: items) }) else { return }
        reloadMetadata()
        reloadKeywords()
        app.statusMessage = "Saved metadata to \(items.count) XMP sidecar\(items.count == 1 ? "" : "s")"
    }
}
