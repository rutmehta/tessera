import AppKit
import Observation
import TesseraCore
import struct TesseraFFI.HistoryState

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
    func thumbnailSizeDidChange()
    /// The items' previews changed (a saved edit); cells should request them again.
    func thumbnailsDidChange(_ positions: IndexSet)
    /// `AppModel.develop` was opened, replaced or closed.
    func developDidChange()
    /// The engine finished a level into one of the session's surfaces (hot path, main actor).
    func developDidRender(_ frame: DevelopFrame, controller: DevelopController)
}

extension LibraryObserver {
    func thumbnailSizeDidChange() {}
    func thumbnailsDidChange(_ positions: IndexSet) {}
    func developDidChange() {}
    func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {}
}

/// State of the develop session for the focused image, for the inspector.
enum DevelopStatus: Equatable {
    case none
    case loading
    case ready
    case unavailable(String)
}

/// Which history ⌘Z addresses: the last kind of change the user made.
private enum UndoDomain { case cull, develop }

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
    private(set) var developStatus: DevelopStatus = .none
    /// Bumped when develop values change outside a slider drag (open, undo, reset, snapshot).
    private(set) var developRevision = 0
    private(set) var developHistory: HistoryState?
    /// "render: L3 → L2, 7.8 ms", throttled to 10 Hz; shown when `showRenderReadout`.
    private(set) var renderReadout: String?
    var showRenderReadout = UserDefaults.standard.bool(forKey: AppModel.renderReadoutKey) {
        didSet { UserDefaults.standard.set(showRenderReadout, forKey: Self.renderReadoutKey) }
    }

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
    /// Develop session for the focused RAW while the loupe shows it (docs/11 §1.2).
    @ObservationIgnored private(set) var develop: DevelopController?
    @ObservationIgnored private var developTask: Task<Void, Never>?
    @ObservationIgnored private var undoDomain = UndoDomain.cull
    @ObservationIgnored private var pendingReadout: String?
    @ObservationIgnored private var developSelfTestRan = false
    @ObservationIgnored private var selfTestFrames: [DevelopFrame]?
    @ObservationIgnored private var readoutTask: Task<Void, Never>?
    @ObservationIgnored private var loadGeneration = 0
    @ObservationIgnored private var modeBeforeCompare: ViewMode = .grid

    private struct WeakObserver { weak var value: (any LibraryObserver)? }

    private static let lastFolderKey = "LastFolderPath"
    private static let recentFoldersKey = "RecentFolderPaths"
    private static let basketTargetKey = "BasketTarget"
    static let renderReadoutKey = "ShowRenderReadout"

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
        closeDevelop()
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
        if let d = develop, d.itemID != focusedItem?.id { closeDevelop() }
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
        if let d = develop, d.history.canUndo, undoDomain == .develop || !cull.canUndo {
            developHistoryMove("Undo", label: d.history.headLabel) { try d.undo() }
            return
        }
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
        if let d = develop, d.history.canRedo, undoDomain == .develop || !cull.canRedo {
            developHistoryMove("Redo", label: nil) { try d.redo() }
            return
        }
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
        undoDomain = .cull
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

    // MARK: Develop (Basic panel on the engine)

    /// Opens the session for `item` (RAW only). Called by the loupe when it shows an image.
    func openDevelop(for item: PhotoItem) {
        if develop?.itemID == item.id {
            liveObservers.forEach { $0.developDidChange() }
            return
        }
        closeDevelop()
        guard item.kind == .raw, let ref = item.engineImage else {
            developStatus = .unavailable(item.kind == .synthetic
                ? "Stub items have no pixels to develop" : "Develop needs a RAW file (\(item.kind.rawValue))")
            return
        }
        developStatus = .loading
        let generation = loadGeneration
        developTask = Task { [weak self] in
            do {
                let controller = try await DevelopController.open(ref, itemID: item.id)
                guard let self, !Task.isCancelled, generation == self.loadGeneration,
                      self.focusedItem?.id == item.id else {
                    await controller.close()
                    return
                }
                self.install(develop: controller)
            } catch {
                guard let self, !Task.isCancelled, self.focusedItem?.id == item.id else { return }
                self.developStatus = .unavailable(error.localizedDescription)
            }
        }
    }

    private func install(develop controller: DevelopController) {
        developTask = nil
        develop = controller
        developStatus = .ready
        developHistory = controller.history
        developRevision += 1
        controller.onFrame = { [weak self, weak controller] frame in
            guard let self, let controller else { return }
            self.developDidRender(frame, controller)
        }
        let itemID = controller.itemID
        controller.onSaved = { [weak self] _ in self?.developDidSave(itemID: itemID) }
        controller.onFailure = { [weak self] message in self?.statusMessage = "Develop: \(message)" }
        if ProcessInfo.processInfo.arguments.contains("--develop-selftest"), !developSelfTestRan {
            developSelfTestRan = true
            runDevelopSelfTest(controller)
        }
        if !controller.ignoredSettings.isEmpty {
            statusMessage = "Develop: \(controller.ignoredSettings.count) imported setting(s) are kept but not rendered yet"
        }
        liveObservers.forEach { $0.developDidChange() }
    }

    /// Stops rendering and writes pending edits of the current session (in the background).
    func closeDevelop() {
        developTask?.cancel()
        developTask = nil
        if developStatus != .none { developStatus = .none }
        guard let controller = develop else { return }
        develop = nil
        controller.onFrame = nil
        controller.onFailure = nil
        developHistory = nil
        renderReadout = nil
        liveObservers.forEach { $0.developDidChange() }
        Task { await controller.close() }
    }

    private func developDidRender(_ frame: DevelopFrame, _ controller: DevelopController) {
        for o in liveObservers { o.developDidRender(frame, controller: controller) }
        selfTestFrames?.append(frame)
        guard showRenderReadout, frame.isFinal else { return }
        pendingReadout = frame.readout
        if readoutTask == nil {
            readoutTask = Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(100))
                guard let self else { return }
                self.renderReadout = self.pendingReadout
                self.readoutTask = nil
            }
        }
    }

    /// Recipe + XMP were written: refresh the grid thumbnail (recipe-hash keyed) and the status.
    private func developDidSave(itemID: Int) {
        if let d = develop, d.itemID == itemID { developHistory = d.history }
        guard library.items.indices.contains(itemID) else { return }
        loader.invalidate(library.items[itemID])
        cull.refreshStatuses([itemID])
        var positions = IndexSet()
        if positionOfID.indices.contains(itemID), positionOfID[itemID] >= 0 { positions.insert(positionOfID[itemID]) }
        refreshFocusSummary()
        liveObservers.forEach { $0.thumbnailsDidChange(positions); $0.itemsDidChange(positions) }
    }

    /// `--develop-selftest`: the Exposure slider's own path (coalesced per display frame, then a
    /// final mouse-up commit), timed by the engine's `render_ms` per frame.
    private func runDevelopSelfTest(_ controller: DevelopController) {
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(1.5))   // first paint settles
            guard let self, self.develop === controller else { return }
            self.selfTestFrames = []
            let id = controller.itemID
            for i in 0...60 {
                self.setAdjustment(.exposure, 1.5 * Double(i) / 60, final: i == 60, for: id)
                try? await Task.sleep(for: .milliseconds(16))
            }
            try? await Task.sleep(for: .seconds(1))
            self.developRevision += 1   // the sliders were bypassed: show the new values
            let frames = self.selfTestFrames ?? []
            self.selfTestFrames = nil
            let drag = frames.filter { $0.dirtyStage == "Tone" }.map(\.renderMs).sorted()
            guard !drag.isEmpty else { FileHandle.standardError.write(Data("develop-selftest: no frames\n".utf8)); return }
            let q = { (p: Double) in drag[Int(Double(drag.count - 1) * p)] }
            let level = frames.last.map { "L\($0.level)" } ?? "?"
            let line = String(format: "develop-selftest: %d tone frames at %@, render median %.1f ms, p90 %.1f ms, max %.1f ms; backend %@; history: %@",
                              drag.count, level, q(0.5), q(0.9), q(1), controller.info.backend,
                              controller.history.headLabel ?? "-")
            FileHandle.standardError.write(Data((line + "\n").utf8))
            self.statusMessage = line
        }
    }

    func adjustment(_ key: BasicKey, for itemID: Int) -> Double {
        guard let d = develop, d.itemID == itemID, let p = key.parameter else { return defaultValue(key) }
        return d.value(p)
    }

    /// Double-click target: as-shot white balance, zero otherwise.
    func defaultValue(_ key: BasicKey) -> Double {
        guard let d = develop else { return key.defaultValue }
        switch key {
        case .temperature: return Double(d.info.asShotTemperature)
        case .tint: return Double(d.info.asShotTint)
        default: return key.defaultValue
        }
    }

    /// Hot path from the NSControl slider: no SwiftUI state is touched while dragging. Values are
    /// coalesced per display frame; the final value (mouse-up) becomes one undo step.
    func setAdjustment(_ key: BasicKey, _ value: Double, final: Bool, for itemID: Int) {
        guard let d = develop, d.itemID == itemID, let p = key.parameter else { return }
        if final, p.isWhiteBalance, value == defaultValue(key) {
            d.setAsShotWhiteBalance()
        } else {
            d.set(p, value, interactive: !final)
        }
        guard final else { return }
        d.commit(label: key.historyLabel(value))
        undoDomain = .develop
        developHistory = d.history
    }

    func resetDevelop() {
        guard let d = develop else { return }
        developHistoryMove("Reset", label: nil) { try d.reset() }
    }

    func restoreSnapshot(_ name: String) {
        guard let d = develop else { return }
        developHistoryMove("Snapshot “\(name)”", label: nil) { try d.restoreSnapshot(named: name); return true }
    }

    func promptSnapshot() {
        guard let d = develop, let window = mainWindow else {
            statusMessage = "Snapshots need a RAW open in the loupe"
            return
        }
        let alert = NSAlert()
        alert.messageText = "New Snapshot"
        alert.informativeText = "Names the current develop state so you can return to it."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        field.stringValue = "Snapshot \(d.history.snapshots.count + 1)"
        alert.accessoryView = field
        alert.addButton(withTitle: "Save")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { [weak self] response in
            let name = field.stringValue.trimmingCharacters(in: .whitespaces)
            MainActor.assumeIsolated {
                guard response == .alertFirstButtonReturn, let self, let d = self.develop else { return }
                do {
                    try d.snapshot(named: name)
                    self.developHistory = d.history
                    self.statusMessage = "Snapshot “\(name)” saved"
                } catch {
                    self.statusMessage = "Snapshot failed: \(error.localizedDescription)"
                }
            }
        }
    }

    private func developHistoryMove(_ verb: String, label: String?, _ body: () throws -> Bool) {
        guard let d = develop else { return }
        do {
            let moved = try body()
            undoDomain = .develop
            developHistory = d.history
            developRevision += 1
            statusMessage = moved ? [verb, label].compactMap { $0 }.joined(separator: ": ") : "Nothing to \(verb.lowercased())"
        } catch {
            statusMessage = "\(verb) failed: \(error.localizedDescription)"
        }
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
