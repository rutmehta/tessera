import AppKit
import Observation
import PhotoEditorCore

enum ViewMode: String, CaseIterable, Identifiable {
    case grid = "Grid"
    case loupe = "Loupe"
    var id: String { rawValue }
}

/// Sidebar sources. Filters are applied when chosen, not live, so decided frames do not vanish mid-cull.
enum LibrarySource: Hashable {
    case all
    case basket
    case decision(Decision)
    case mark(UInt8)

    var title: String {
        switch self {
        case .all: "All Photos"
        case .basket: "Basket"
        case .decision(.keep): "Keeps"
        case .decision(.reject): "Rejects"
        case .decision(.undecided): "Undecided"
        case .mark(let m): MarkStyle.name(m)
        }
    }

    func includes(_ s: CullState) -> Bool {
        switch self {
        case .all: true
        case .basket: s.inBasket
        case .decision(let d): s.decision == d
        case .mark(let m): s.mark == m
        }
    }
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

    private(set) var library: StubLibrary = .empty
    private(set) var isLoading = false
    private(set) var counts = CullStore.Counts()
    private(set) var focusedItem: PhotoItem?
    private(set) var focusedState = CullState()
    private(set) var focusedPosition: Int?
    private(set) var selectionCount = 0
    private(set) var source: LibrarySource = .all
    private(set) var visibleCount = 0
    private(set) var recentFolders: [URL] = []
    private(set) var canUndo = false
    private(set) var canRedo = false
    var viewMode: ViewMode = .grid {
        didSet { if viewMode != oldValue { notifySelection(scroll: true) } }
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
    @ObservationIgnored private(set) var cull = CullStore(count: 0)
    /// Visible item ids in display order (capture time), after the sidebar filter.
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

    private struct WeakObserver { weak var value: (any LibraryObserver)? }

    private static let lastFolderKey = "LastFolderPath"
    private static let recentFoldersKey = "RecentFolderPaths"

    init() {
        recentFolders = (UserDefaults.standard.stringArray(forKey: Self.recentFoldersKey) ?? [])
            .map { URL(fileURLWithPath: $0) }
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
        if let window = NSApp.keyWindow ?? NSApp.mainWindow {
            panel.beginSheetModal(for: window) { [weak self] response in
                guard response == .OK, let url = panel.url else { return }
                MainActor.assumeIsolated { self?.openFolder(url) }
            }
        } else if panel.runModal() == .OK, let url = panel.url {
            openFolder(url)
        }
    }

    func openFolder(_ url: URL) {
        isLoading = true
        statusMessage = "Reading \(url.lastPathComponent)…"
        rememberFolder(url)
        Task.detached(priority: .userInitiated) {
            let result = Result { try StubLibrary.scan(folder: url) }
            await MainActor.run {
                self.isLoading = false
                switch result {
                case .success(let lib):
                    self.install(lib)
                    let raws = lib.items.lazy.filter { $0.kind == .raw }.count
                    self.statusMessage = "Opened \(lib.title): \(lib.items.count.formatted()) images (\(raws.formatted()) RAW), "
                        + "\(lib.groups.count.formatted()) groups, \(Self.ms(lib.scanDuration))"
                case .failure(let error):
                    self.statusMessage = error.localizedDescription
                }
            }
        }
    }

    func loadStubItems(count: Int) {
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

    private func install(_ lib: StubLibrary) {
        loader.removeAll()
        library = lib
        cull = CullStore(count: lib.items.count)
        adjustments = [:]
        source = .all
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
        if source == .all {
            visible = Array(items.indices)
        } else {
            visible = items.indices.filter { source.includes(cull[$0]) }
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
        liveObservers.forEach { $0.libraryDidReload() }
        notifySelection(scroll: true)
    }

    // MARK: Accessors for AppKit views

    func item(at position: Int) -> PhotoItem { library.items[visible[position]] }
    func state(at position: Int) -> CullState { cull[visible[position]] }
    func groupSize(of item: PhotoItem) -> Int { library.groups[item.groupID].count }
    func indexInGroup(of item: PhotoItem) -> Int { item.id - library.groups[item.groupID].lowerBound }

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

    /// Grid: spatial arrows. Loupe: ←/→ between groups, ↑/↓ within a group (docs/06 §3).
    func navigate(_ move: Move, groupwise: Bool, extend: Bool) {
        guard let f = focus else { if !visible.isEmpty { select(position: 0) }; return }
        if groupwise {
            switch move {
            case .right: jumpGroup(+1, from: f)
            case .left: jumpGroup(-1, from: f)
            case .down: stepInGroup(+1, from: f)
            case .up: stepInGroup(-1, from: f)
            }
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

    private func jumpGroup(_ dir: Int, from f: Int) {
        let g = groupID(at: f)
        if dir > 0 {
            var p = f + 1
            while p < visible.count, groupID(at: p) == g { p += 1 }
            guard p < visible.count else { statusMessage = "Last group"; return }
            select(position: p)   // stub "best of group" = first frame
        } else {
            // Start of the current group, then start of the previous group.
            var start = f
            while start > 0, groupID(at: start - 1) == g { start -= 1 }
            guard start > 0 else { statusMessage = "First group"; if f != start { select(position: start) }; return }
            let pg = groupID(at: start - 1)
            var p = start - 1
            while p > 0, groupID(at: p - 1) == pg { p -= 1 }
            select(position: p)
        }
    }

    private func stepInGroup(_ dir: Int, from f: Int) {
        let target = f + dir
        guard target >= 0, target < visible.count, groupID(at: target) == groupID(at: f) else {
            statusMessage = dir > 0 ? "Last frame in group" : "First frame in group"
            return
        }
        select(position: target)
    }

    // MARK: Culling

    func perform(_ action: CullAction) {
        guard let f = focus else { return }
        let positions = selection.contains(f) ? selection : IndexSet(integer: f)
        let ids = positions.map { visible[$0] }
        let changed = cull.apply(action, to: ids)
        if !changed.isEmpty { didChange(ids: changed) }
        if autoAdvance, action.advances, positions.count == 1, f + 1 < visible.count {
            select(position: f + 1)
        }
    }

    func undo() {
        guard let ids = cull.undo() else { return }
        didChange(ids: ids)
        if ids.count == 1, positionOfID[ids[0]] >= 0 { select(position: positionOfID[ids[0]]) }
        statusMessage = "Undo: \(ids.count) image\(ids.count == 1 ? "" : "s")"
    }

    func redo() {
        guard let ids = cull.redo() else { return }
        didChange(ids: ids)
        statusMessage = "Redo: \(ids.count) image\(ids.count == 1 ? "" : "s")"
    }

    private func didChange(ids: [Int]) {
        var positions = IndexSet()
        for id in ids where positionOfID[id] >= 0 { positions.insert(positionOfID[id]) }
        refreshSummary()
        liveObservers.forEach { $0.itemsDidChange(positions) }
    }

    private func refreshSummary() {
        counts = cull.counts
        canUndo = cull.canUndo
        canRedo = cull.canRedo
        refreshFocusSummary()
    }

    private func refreshFocusSummary() {
        if let f = focus, f < visible.count {
            let it = item(at: f)
            if focusedItem?.id != it.id { focusedItem = it }
            if focusedPosition != f { focusedPosition = f }
            let s = cull[it.id]
            if focusedState != s { focusedState = s }
        } else {
            focusedItem = nil
            focusedPosition = nil
            focusedState = CullState()
        }
        if selectionCount != selection.count { selectionCount = selection.count }
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
