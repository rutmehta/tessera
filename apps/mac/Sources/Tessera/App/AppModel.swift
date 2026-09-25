import AppKit
import Observation
import TesseraCore

enum ViewMode: String, CaseIterable, Identifiable {
    case grid = "Grid"
    case loupe = "Loupe"
    case compare = "Compare"
    var id: String { rawValue }
}

/// Sidebar sources. Filters are applied when chosen, not live, so decided frames do not vanish mid-cull.
enum LibrarySource: Hashable {
    case all
    case album(String)
    case decision(Decision)
    case mark(UInt8)

    var title: String {
        switch self {
        case .all: "All Photos"
        case .album(let name): name
        case .decision(.keep): "Keeps"
        case .decision(.reject): "Rejects"
        case .decision(.undecided): "Undecided"
        case .mark(let m): MarkStyle.name(m)
        }
    }
}

/// Two frames side by side (docs/06 §3 "Compare"). `active` is 0 (left) or 1 (right).
struct ComparePair: Equatable {
    var ids: [Int]
    var active = 0
    /// Incremented by Z; the compare view toggles fit ↔ 1:1 on change.
    var zoomToggles = 0
    var activeID: Int { ids[active] }
    var otherID: Int { ids[1 - active] }
}

/// Non-modal, auto-dismissing notice. Undoable actions offer an Undo button.
struct Toast: Identifiable, Equatable {
    let id = UUID()
    var message: String
    var undoable: Bool
}

/// AppKit views (grid, filmstrip, loupe) observe the model through this protocol instead of SwiftUI
/// observation, so per-item changes on 20k items never invalidate SwiftUI view bodies.
@MainActor protocol LibraryObserver: AnyObject {
    func libraryDidReload()
    /// `positions` index into `AppModel.visible`.
    func itemsDidChange(_ positions: IndexSet)
    func selectionDidChange(scrollToFocus: Bool)
    func adjustmentsDidChange(itemID: Int)
    func thumbnailSizeDidChange()
}

extension LibraryObserver {
    func adjustmentsDidChange(itemID: Int) {}
    func thumbnailSizeDidChange() {}
}

@MainActor @Observable
final class AppModel {
    static let shared = AppModel()

    // MARK: Observed summary state (SwiftUI)

    private(set) var library: any PhotoLibrary = StubLibrary.empty
    private(set) var isLoading = false
    private(set) var counts = CullStore.Counts()
    private(set) var focusedItem: PhotoItem?
    private(set) var focusedState = CullState()
    private(set) var focusedStatus = ItemStatus()
    private(set) var focusedIsBest = false
    private(set) var focusedPosition: Int?
    private(set) var selectionCount = 0
    private(set) var source: LibrarySource = .all
    private(set) var visibleCount = 0
    private(set) var recentFolders: [URL] = []
    private(set) var canUndo = false
    private(set) var canRedo = false
    private(set) var basketTarget = EngineLibrary.defaultBasketTarget
    private(set) var albums: [AlbumSummary] = []
    private(set) var isEngineBacked = false
    private(set) var compare: ComparePair? {
        // The inspector and status bar follow the active side.
        didSet { if let id = compare?.activeID, id != oldValue?.activeID { select(id: id) } }
    }
    var toast: Toast?
    var showDefectSweep = false
    var viewMode: ViewMode = .grid {
        didSet {
            guard viewMode != oldValue else { return }
            if viewMode == .compare, compare == nil {
                // Chosen from the toolbar: build a pair, or refuse.
                viewMode = oldValue
                enterCompare()
                return
            }
            if viewMode != .compare {
                compare = nil
                modeBeforeCompare = viewMode
            }
            notifySelection(scroll: true)
        }
    }
    var autoAdvance = true
    var showInspector = true
    var showFilmstrip = true
    var thumbnailSize: Double = 176 {
        didSet { if thumbnailSize != oldValue { liveObservers.forEach { $0.thumbnailSizeDidChange() } } }
    }
    var statusMessage: String?
    /// Set by the loupe view: colour space and EDR headroom of the current screen.
    var loupeInfo = ""

    // MARK: Unobserved hot state (AppKit)

    @ObservationIgnored let loader = ThumbnailLoader()
    @ObservationIgnored private(set) var cull = StubLibrary.empty.makeCullController()
    /// Visible item ids in display order (group by group), after the sidebar filter.
    @ObservationIgnored private(set) var visible: [Int] = []
    /// id -> position in `visible`, or -1.
    @ObservationIgnored private var positionOfID: [Int] = []
    @ObservationIgnored private(set) var selection = IndexSet()
    @ObservationIgnored private(set) var focus: Int?
    @ObservationIgnored private var anchor: Int?
    /// Reported by the grid layout for ↑/↓ spatial navigation.
    @ObservationIgnored var gridColumns = 1
    @ObservationIgnored private var observers: [WeakObserver] = []
    @ObservationIgnored private var adjustments: [Int: [BasicKey: Double]] = [:]
    @ObservationIgnored private var loadGeneration = 0
    @ObservationIgnored private var modeBeforeCompare: ViewMode = .grid

    private struct WeakObserver { weak var value: (any LibraryObserver)? }

    private static let lastFolderKey = "LastFolderPath"
    private static let recentFoldersKey = "RecentFolderPaths"
    private static let basketTargetKey = "BasketTarget"

    init() {
        recentFolders = (UserDefaults.standard.stringArray(forKey: Self.recentFoldersKey) ?? [])
            .map { URL(fileURLWithPath: $0) }
        basketTarget = UserDefaults.standard.string(forKey: Self.basketTargetKey) ?? EngineLibrary.defaultBasketTarget
    }

    // MARK: Observers

    func addObserver(_ o: any LibraryObserver) {
        observers.removeAll { $0.value == nil }
        observers.append(WeakObserver(value: o))
    }

    private var liveObservers: [any LibraryObserver] { observers.compactMap(\.value) }

    // MARK: Library loading

    var lastFolder: URL? {
        UserDefaults.standard.string(forKey: Self.lastFolderKey).map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    func presentOpenPanel() {
        let panel = NSOpenPanel()
        panel.title = "Open Folder"
        panel.message = "Choose a folder of JPEG or RAW images"
        panel.prompt = "Open"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = false
        panel.directoryURL = lastFolder?.deletingLastPathComponent() ?? lastFolder
        if let window = mainWindow {
            panel.beginSheetModal(for: window) { [weak self] response in
                guard response == .OK, let url = panel.url else { return }
                MainActor.assumeIsolated { self?.openFolder(url) }
            }
        } else if panel.runModal() == .OK, let url = panel.url {
            openFolder(url)
        }
    }

    private var mainWindow: NSWindow? {
        NSApp.keyWindow.flatMap { $0 is NSPanel ? nil : $0 } ?? NSApp.mainWindow
            ?? NSApp.windows.first { $0.isVisible && !($0 is NSPanel) }
    }

    func openFolder(_ url: URL, message: String? = nil) {
        loadGeneration += 1
        let generation = loadGeneration
        let useStub = ProcessInfo.processInfo.arguments.contains("--stub-library")
        let seedScores = ProcessInfo.processInfo.arguments.contains("--seed-scores")
        let target = basketTarget
        isLoading = true
        statusMessage = "Reading \(url.lastPathComponent)…"
        rememberFolder(url)
        Task.detached(priority: .userInitiated) {
            let result = Result<any PhotoLibrary, Error> {
                if useStub { return try StubLibrary.scan(folder: url) }
                let lib = try EngineLibrary.scan(folder: url, basketTarget: target)
                if seedScores { try lib.seedSyntheticScores() }
                return lib
            }
            await MainActor.run {
                guard generation == self.loadGeneration else { return }
                self.isLoading = false
                switch result {
                case .success(let lib):
                    self.install(lib)
                    let raws = lib.items.lazy.filter { $0.kind == .raw }.count
                    let multi = lib.groups.lazy.filter { $0.count > 1 }.count
                    self.statusMessage = message ?? "Opened \(lib.title): \(lib.items.count.formatted()) images (\(raws.formatted()) RAW), "
                        + "\(lib.groups.count.formatted()) groups (\(multi.formatted()) with 2+), \(Self.ms(lib.scanDuration))"
                        + (seedScores ? ", synthetic scores seeded" : "")
                case .failure(let error):
                    self.statusMessage = error.localizedDescription
                }
            }
        }
    }

    func loadStubItems(count: Int) {
        loadGeneration += 1
        isLoading = false
        let lib = StubLibrary.synthetic(count: count)
        install(lib)
        statusMessage = "Generated \(count.formatted()) stub items in \(lib.groups.count.formatted()) groups, \(Self.ms(lib.scanDuration))"
    }

    private func rememberFolder(_ url: URL) {
        UserDefaults.standard.set(url.path, forKey: Self.lastFolderKey)
        var recents = recentFolders.filter { $0.standardizedFileURL != url.standardizedFileURL }
        recents.insert(url, at: 0)
        recentFolders = Array(recents.prefix(8))
        UserDefaults.standard.set(recentFolders.map(\.path), forKey: Self.recentFoldersKey)
    }

    private func install(_ lib: any PhotoLibrary) {
        loader.removeAll()
        library = lib
        cull = lib.makeCullController()
        isEngineBacked = cull.isEngineBacked
        adjustments = [:]
        source = .all
        compare = nil
        if viewMode == .compare { viewMode = modeBeforeCompare }
        rebuildVisible()
        selection = visible.isEmpty ? [] : [0]
        focus = visible.isEmpty ? nil : 0
        anchor = focus
        refreshSummary()
        liveObservers.forEach { $0.libraryDidReload() }
        notifySelection(scroll: true)
    }

    private func rebuildVisible() {
        let items = library.items
        switch source {
        case .all:
            visible = Array(items.indices)
        case .album(let name):
            let members = Set(cull.members(ofAlbum: name))
            visible = items.indices.filter { members.contains($0) }
        case .decision(let d):
            visible = items.indices.filter { cull[$0].decision == d }
        case .mark(let m):
            visible = items.indices.filter { cull[$0].mark == m }
        }
        positionOfID = Array(repeating: -1, count: items.count)
        for (p, id) in visible.enumerated() { positionOfID[id] = p }
        visibleCount = visible.count
    }

    func setSource(_ s: LibrarySource) {
        let focusedID = focus.map { visible[$0] }
        source = s
        rebuildVisible()
        if let focusedID, positionOfID[focusedID] >= 0 {
            focus = positionOfID[focusedID]
        } else {
            focus = visible.isEmpty ? nil : 0
        }
        selection = focus.map { IndexSet(integer: $0) } ?? []
        anchor = focus
        refreshFocusSummary()
        liveObservers.forEach { $0.libraryDidReload() }
        notifySelection(scroll: true)
    }

    // MARK: Accessors for AppKit views

    func item(at position: Int) -> PhotoItem { library.items[visible[position]] }
    func state(at position: Int) -> CullState { cull[visible[position]] }
    func status(at position: Int) -> ItemStatus { cull.statuses[visible[position]] }
    func isSuggestedBest(_ item: PhotoItem) -> Bool { cull.isSuggestedBest(item.id) }
    func groupSize(of item: PhotoItem) -> Int { library.groups[item.groupID].count }
    func indexInGroup(of item: PhotoItem) -> Int { item.id - library.groups[item.groupID].lowerBound }
    func item(id: Int) -> PhotoItem { library.items[id] }
    func state(id: Int) -> CullState { cull[id] }

    // MARK: Selection

    /// From mouse interaction in a collection view.
    func setSelectionFromUI(_ positions: IndexSet, clicked: Int?) {
        selection = positions
        if let clicked { focus = clicked; anchor = clicked }
        else if let f = focus, !positions.contains(f) { focus = positions.first; anchor = focus }
        refreshFocusSummary()
        notifySelection(scroll: false)
    }

    func select(position: Int, extend: Bool = false) {
        guard !visible.isEmpty else { return }
        let p = min(max(position, 0), visible.count - 1)
        if extend, let a = anchor {
            selection = IndexSet(integersIn: min(a, p)...max(a, p))
        } else {
            selection = [p]
            anchor = p
        }
        focus = p
        refreshFocusSummary()
        notifySelection(scroll: true)
    }

    func select(id: Int) {
        if positionOfID.indices.contains(id), positionOfID[id] >= 0 { select(position: positionOfID[id]) }
    }

    func selectAll() {
        guard !visible.isEmpty else { return }
        selection = IndexSet(integersIn: 0..<visible.count)
        refreshFocusSummary()
        notifySelection(scroll: false)
    }

    private func notifySelection(scroll: Bool) {
        for o in liveObservers { o.selectionDidChange(scrollToFocus: scroll) }
    }

    // MARK: Navigation

    enum Move { case left, right, up, down }

    /// Grid: spatial arrows. Loupe (and ⌥ in the grid): ←/→ between groups, ↑/↓ within a group
    /// (docs/06 §3). Unfiltered, group moves come from the Rust session.
    func navigate(_ move: Move, groupwise: Bool, extend: Bool) {
        if viewMode == .compare {
            if move == .left || move == .right { setCompareActive(move == .left ? 0 : 1) }
            return
        }
        guard let f = focus else { if !visible.isEmpty { select(position: 0) }; return }
        if groupwise {
            let groupMove: GroupMove = switch move {
            case .right: .nextGroup
            case .left: .previousGroup
            case .down: .nextInGroup
            case .up: .previousInGroup
            }
            let target: Int?
            if source == .all {
                do { target = try cull.navigate(groupMove, from: visible[f]).map { positionOfID[$0] } }
                catch { statusMessage = "Navigation failed: \(error.localizedDescription)"; return }
            } else {
                target = localGroupMove(groupMove, from: f)
            }
            guard let target, target >= 0 else {
                statusMessage = switch groupMove {
                case .nextGroup: "Last group"
                case .previousGroup: "First group"
                case .nextInGroup: "Last frame in group"
                case .previousInGroup: "First frame in group"
                }
                return
            }
            select(position: target)
            return
        }
        let cols = viewMode == .grid ? max(gridColumns, 1) : 1
        let target: Int = switch move {
        case .left: f - 1
        case .right: f + 1
        case .up: f - cols
        case .down: f + cols
        }
        guard target >= 0, target < visible.count else { return }
        select(position: target, extend: extend)
    }

    private func groupID(at p: Int) -> Int { library.items[visible[p]].groupID }

    /// Same semantics as `crates/cull`, over the filtered positions.
    private func localGroupMove(_ move: GroupMove, from f: Int) -> Int? {
        let g = groupID(at: f)
        switch move {
        case .nextGroup:
            var p = f + 1
            while p < visible.count, groupID(at: p) == g { p += 1 }
            return p < visible.count ? p : nil
        case .previousGroup:
            var start = f
            while start > 0, groupID(at: start - 1) == g { start -= 1 }
            guard start > 0 else { return nil }
            let pg = groupID(at: start - 1)
            var p = start - 1
            while p > 0, groupID(at: p - 1) == pg { p -= 1 }
            return p
        case .nextInGroup:
            return f + 1 < visible.count && groupID(at: f + 1) == g ? f + 1 : nil
        case .previousInGroup:
            return f > 0 && groupID(at: f - 1) == g ? f - 1 : nil
        }
    }

    // MARK: Culling

    func perform(_ action: CullAction) {
        if let pair = compare {
            run { try self.cull.apply(action, to: [pair.activeID]) }
            return
        }
        guard let f = focus else { return }
        let positions = selection.contains(f) ? selection : IndexSet(integer: f)
        let ids = positions.map { visible[$0] }
        guard run({ try self.cull.apply(action, to: ids) }) else { return }
        if autoAdvance, action.advances, positions.count == 1, f + 1 < visible.count {
            select(position: f + 1)
        }
    }

    /// Runs a controller mutation and reflects its changes. Returns false on failure.
    @discardableResult
    private func run(_ body: () throws -> CullChange) -> Bool {
        do {
            didChange(try body())
            return true
        } catch {
            statusMessage = "Could not save decision: \(error.localizedDescription)"
            refreshSummary()
            return false
        }
    }

    /// K: keep the group's suggested best, reject the rest (one undo step), then move on.
    func keepBestRejectRest() {
        guard let f = focus else { return }
        let item = self.item(at: f)
        guard groupSize(of: item) > 1 else {
            statusMessage = "Keep best needs a group of 2 or more frames"
            return
        }
        do {
            let (best, change) = try cull.keepBestRejectRest(group: item.groupID)
            didChange(change)
            showToast("Kept \(library.items[best].name), rejected \(groupSize(of: item) - 1) in G\(item.groupID + 1)", undoable: true)
            if autoAdvance, let next = localGroupMove(.nextGroup, from: f) { select(position: next) }
            else { select(id: best) }
        } catch {
            statusMessage = "Keep best failed: \(error.localizedDescription)"
        }
    }

    func undo() {
        do {
            guard let change = try cull.undo() else { statusMessage = "Nothing to undo"; return }
            didChange(change)
            if let current = change.current ?? (change.ids.count == 1 ? change.ids[0] : nil), compare == nil {
                select(id: current)
            }
            statusMessage = "Undo: \(change.ids.count) image\(change.ids.count == 1 ? "" : "s")"
            if toast?.undoable == true { toast = nil }
        } catch {
            statusMessage = "Undo failed: \(error.localizedDescription)"
        }
    }

    func redo() {
        do {
            guard let change = try cull.redo() else { statusMessage = "Nothing to redo"; return }
            didChange(change)
            if let current = change.current, compare == nil { select(id: current) }
            statusMessage = "Redo: \(change.ids.count) image\(change.ids.count == 1 ? "" : "s")"
        } catch {
            statusMessage = "Redo failed: \(error.localizedDescription)"
        }
    }

    private func didChange(_ change: CullChange) {
        var positions = IndexSet()
        for id in change.ids where positionOfID[id] >= 0 { positions.insert(positionOfID[id]) }
        refreshSummary()
        liveObservers.forEach { $0.itemsDidChange(positions) }
    }

    private func refreshSummary() {
        counts = cull.counts
        canUndo = cull.canUndo
        canRedo = cull.canRedo
        if albums != cull.albums { albums = cull.albums }
        if basketTarget != cull.basketTarget { basketTarget = cull.basketTarget }
        refreshFocusSummary()
    }

    private func refreshFocusSummary() {
        if let f = focus, f < visible.count {
            let it = item(at: f)
            if focusedItem?.id != it.id { focusedItem = it }
            if focusedPosition != f { focusedPosition = f }
            let s = cull[it.id]
            if focusedState != s { focusedState = s }
            let st = cull.statuses[it.id]
            if focusedStatus != st { focusedStatus = st }
            let best = cull.isSuggestedBest(it.id)
            if focusedIsBest != best { focusedIsBest = best }
            cull.setCurrent(it.id)
        } else {
            focusedItem = nil
            focusedPosition = nil
            focusedState = CullState()
            focusedStatus = ItemStatus()
            focusedIsBest = false
        }
        if selectionCount != selection.count { selectionCount = selection.count }
    }

    // MARK: Toast

    func showToast(_ message: String, undoable: Bool) {
        let t = Toast(message: message, undoable: undoable)
        toast = t
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(5))
            if self?.toast?.id == t.id { self?.toast = nil }
        }
    }

    // MARK: Basket and albums

    func setBasketTarget(_ name: String) {
        do {
            try cull.setBasketTarget(name)
            UserDefaults.standard.set(cull.basketTarget, forKey: Self.basketTargetKey)
            refreshSummary()
            liveObservers.forEach { $0.itemsDidChange(IndexSet(integersIn: 0..<visibleCount)) }
            statusMessage = "Basket target: \(cull.basketTarget)"
        } catch {
            statusMessage = "Could not set basket target: \(error.localizedDescription)"
        }
    }

    func promptNewBasketTarget() {
        let alert = NSAlert()
        alert.messageText = "New Basket Album"
        alert.informativeText = "B adds the focused photos to this album. The album is created on the first add."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        field.placeholderString = "Album name"
        alert.accessoryView = field
        alert.addButton(withTitle: "Set Target")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        guard let window = mainWindow else { return }
        alert.beginSheetModal(for: window) { [weak self] response in
            let name = field.stringValue
            MainActor.assumeIsolated {
                if response == .alertFirstButtonReturn { self?.setBasketTarget(name) }
            }
        }
    }

    // MARK: Safe delete (docs/06 §4.2)

    /// ⌫: inside an album removes from that album only, and says so. Elsewhere it does nothing
    /// destructive and points at the separate "Delete from Disk…" command.
    func deletePressed() {
        guard case .album(let name) = source else {
            statusMessage = "Delete only removes photos from an album. To remove files use Cull ▸ Delete from Disk… (⌘⌫)"
            return
        }
        guard let f = focus else { return }
        let positions = selection.contains(f) ? selection : IndexSet(integer: f)
        let ids = positions.map { visible[$0] }
        guard run({ try self.cull.removeFromAlbum(name, ids: ids) }) else { return }
        showToast("Removed \(ids.count) from “\(name)”. Files were not deleted.", undoable: true)
        setSource(.album(name))
    }

    func confirmDeleteFromDisk() {
        guard isEngineBacked, let f = focus, let window = mainWindow else {
            statusMessage = "Delete from Disk needs a folder of real files"
            return
        }
        let positions = compare != nil ? IndexSet() : (selection.contains(f) ? selection : IndexSet(integer: f))
        let ids = compare.map { [$0.activeID] } ?? positions.map { visible[$0] }
        let n = ids.count
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = n == 1 ? "Move “\(library.items[ids[0]].name)” to the Trash?"
            : "Move \(n) photos to the Trash?"
        alert.informativeText = "The original files and their sidecars (.xmp, edits) move to the Trash and are removed "
            + "from every album. This cannot be undone with ⌘Z; use Put Back in the Finder."
        alert.addButton(withTitle: "Move to Trash")
        alert.addButton(withTitle: "Cancel")
        alert.buttons[0].hasDestructiveAction = true
        alert.buttons[0].keyEquivalent = ""
        alert.buttons[1].keyEquivalent = "\r"
        alert.beginSheetModal(for: window) { [weak self] response in
            MainActor.assumeIsolated {
                guard response == .alertFirstButtonReturn else { return }
                self?.deleteFromDisk(ids)
            }
        }
    }

    private func deleteFromDisk(_ ids: [Int]) {
        guard let folder = library.folder else { return }
        do {
            let trashed = try cull.moveToTrash(ids)
            let msg = "Moved \(trashed.count) photo\(trashed.count == 1 ? "" : "s") to the Trash"
            showToast(msg, undoable: false)
            openFolder(folder, message: msg)
        } catch {
            statusMessage = "Delete from disk failed: \(error.localizedDescription)"
            openFolder(folder)
        }
    }

    // MARK: Compare (2-up)

    /// C: the two selected frames, or the focused frame and its neighbour in the group.
    func enterCompare() {
        guard let f = focus else { return }
        var ids: [Int]
        if selection.count >= 2 {
            ids = selection.prefix(2).map { visible[$0] }
        } else {
            let id = visible[f]
            let next = localGroupMove(.nextInGroup, from: f) ?? localGroupMove(.previousInGroup, from: f)
                ?? (f + 1 < visible.count ? f + 1 : (f > 0 ? f - 1 : nil))
            guard let next else { statusMessage = "Compare needs two photos"; return }
            ids = [id, visible[next]].sorted()
        }
        modeBeforeCompare = viewMode == .compare ? modeBeforeCompare : viewMode
        compare = ComparePair(ids: ids, active: ids[0] == visible[f] ? 0 : 1)
        viewMode = .compare
        statusMessage = "Compare: ← → pick side · Return chooses (keep it, reject the other) · Z zoom · Esc back"
    }

    func exitCompare() {
        let focusID = compare?.activeID
        viewMode = modeBeforeCompare
        if let focusID { select(id: focusID) }
    }

    func setCompareActive(_ side: Int) {
        guard var pair = compare, pair.active != side else { return }
        pair.active = side
        compare = pair
    }

    func toggleCompareZoom() {
        compare?.zoomToggles += 1
    }

    /// Return in compare: keep the active frame, reject the other (one undo step). The next
    /// undecided frame of the group takes the rejected side; otherwise compare ends.
    func chooseInCompare() {
        guard let pair = compare else { return }
        let winner = pair.activeID, loser = pair.otherID
        guard run({ try self.cull.decide([(winner, .keep), (loser, .reject)]) }) else { return }
        let group = library.groups[library.items[winner].groupID]
        let challenger = group.first { id in
            id != winner && id != loser && cull[id].decision == .undecided && positionOfID[id] >= 0
        }
        if let challenger {
            var next = pair
            next.ids[1 - pair.active] = challenger
            compare = next
            statusMessage = "Kept \(library.items[winner].name). Next challenger: \(library.items[challenger].name)"
        } else {
            showToast("Kept \(library.items[winner].name), rejected \(library.items[loser].name)", undoable: true)
            exitCompare()
        }
    }

    // MARK: Defect sweep

    func defectFindings(_ rules: [DefectRule]) -> [DefectFinding] {
        do { return try cull.defectSweep(rules) } catch {
            statusMessage = "Defect sweep failed: \(error.localizedDescription)"
            return []
        }
    }

    func applyDefectSweep(_ ids: [Int]) {
        guard !ids.isEmpty else { return }
        guard run({ try self.cull.apply(.reject, to: ids) }) else { return }
        showToast("Rejected \(ids.count) frame\(ids.count == 1 ? "" : "s") from the defect sweep", undoable: true)
    }

    // MARK: Adjustments (stub Basic panel)

    func adjustment(_ key: BasicKey, for itemID: Int) -> Double {
        adjustments[itemID]?[key] ?? key.defaultValue
    }

    /// Hot path from the NSControl slider: no SwiftUI state is touched; observers (the loupe) redraw directly.
    func setAdjustment(_ key: BasicKey, _ value: Double, for itemID: Int) {
        adjustments[itemID, default: [:]][key] = value
        for o in liveObservers { o.adjustmentsDidChange(itemID: itemID) }
    }

    // MARK: Misc

    func requestScrollBenchmark() {
        viewMode = .grid
        for o in liveObservers { (o as? ScrollBenchmarkRunner)?.runScrollBenchmark() }
    }

    static func ms(_ t: TimeInterval) -> String {
        t < 1 ? "\(Int((t * 1000).rounded())) ms" : String(format: "%.2f s", t)
    }
}

@MainActor protocol ScrollBenchmarkRunner: AnyObject {
    func runScrollBenchmark()
}
