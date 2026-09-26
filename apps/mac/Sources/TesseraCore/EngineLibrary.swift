import Foundation
import TesseraFFI

/// What the shell needs from a loaded library. Culling state and history live in the
/// `CullController` the library makes, not in the library itself.
public protocol PhotoLibrary: Sendable {
    var title: String { get }
    var folder: URL? { get }
    /// Display order: groups are contiguous, members in review-queue order.
    var items: [PhotoItem] { get }
    var groups: [Range<Int>] { get }
    var subfolders: [URL] { get }
    var scanDuration: TimeInterval { get }
    /// Engine-backed libraries persist through a Rust `CullSession`; the synthetic stub
    /// library keeps decisions in memory.
    func makeCullController() -> CullController
}

extension StubLibrary: PhotoLibrary {
    public func makeCullController() -> CullController { CullController(memory: self) }
}

/// Retained by immutable items, so in-flight thumbnails cannot switch to a newly opened catalog.
public final class EngineImageReference: Sendable, Hashable {
    public let engine: Engine
    public let imageID: String
    let previewEvents: PreviewEvents
    init(engine: Engine, imageID: String, previewEvents: PreviewEvents) {
        self.engine = engine; self.imageID = imageID; self.previewEvents = previewEvents
    }
    public static func == (lhs: EngineImageReference, rhs: EngineImageReference) -> Bool { lhs === rhs }
    public func hash(into hasher: inout Hasher) { hasher.combine(ObjectIdentifier(self)) }
}

/// A folder opened through the Rust index and a `CullSession`. Groups (bursts and
/// near-duplicates), the suggested best frame, decisions, the basket and undo history all
/// come from `crates/cull`; this type only arranges them in display order.
///
/// The layout follows the catalog in place (M2-28): `apply(_:)` takes a `QueueDelta` from
/// `CullSession.syncChanges()` and rebuilds the display order around the same session, so
/// its undo history, the host's filters and selection survive new frames, imports and
/// rescans. Item ids stay dense display positions; an update reports how they moved.
/// Mutated on the main actor only.
public final class EngineLibrary: PhotoLibrary, @unchecked Sendable {
    public let title: String
    public let folder: URL?
    public private(set) var items: [PhotoItem]
    public private(set) var groups: [Range<Int>]
    public let subfolders: [URL]
    public let scanDuration: TimeInterval
    public let engine: Engine
    public let session: CullSession
    /// Engine image id per item id.
    public private(set) var imageIDs: [String]
    public private(set) var itemOfImage: [String: Int]
    private(set) var initialStates: [CullState]
    private(set) var initialStatuses: [ItemStatus]
    /// Suggested best item id per group.
    private(set) var bestOfGroup: [Int]
    /// Images whose embedded preview could not be hashed (still reviewable).
    public private(set) var previewErrors: [String]
    /// Catalog change sequence the layout reflects.
    public private(set) var changeSequence: UInt64
    let previewEvents: PreviewEvents
    /// Latest session row per image id, and the (identity-stable) reference items carry.
    private var rows: [String: SessionImage] = [:]
    private var references: [String: EngineImageReference] = [:]
    /// Group layout in engine terms (image ids per group, suggested best).
    private var layout: [CullGroup]

    public static let defaultBasketTarget = "Selects"

    /// Explicit launch argument wins over the environment; otherwise use macOS Application Support.
    public static func supportDirectory(arguments: [String] = ProcessInfo.processInfo.arguments,
                                        environment: [String: String] = ProcessInfo.processInfo.environment) -> URL {
        if let flag = arguments.firstIndex(of: "--app-dir"), arguments.indices.contains(flag + 1) {
            return URL(fileURLWithPath: (arguments[flag + 1] as NSString).expandingTildeInPath, isDirectory: true)
        }
        if let path = environment["TESSERA_APP_DIR"], !path.isEmpty {
            return URL(fileURLWithPath: (path as NSString).expandingTildeInPath, isDirectory: true)
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Tessera", isDirectory: true)
    }

    /// The index and preview caches; overridable for tests and acceptance runs.
    public static var defaultSupportDirectory: URL { supportDirectory() }

    private init(title: String, folder: URL, subfolders: [URL], scanDuration: TimeInterval, engine: Engine,
                 session: CullSession, previewEvents: PreviewEvents, rows: [SessionImage], layout: [CullGroup],
                 statuses: [String: ItemStatus], previewErrors: [String], sequence: UInt64) {
        self.title = title; self.folder = folder; self.subfolders = subfolders
        self.scanDuration = scanDuration; self.engine = engine; self.session = session
        self.previewEvents = previewEvents; self.previewErrors = previewErrors
        self.layout = layout; changeSequence = sequence
        items = []; groups = []; imageIDs = []; itemOfImage = [:]; initialStates = []; initialStatuses = []
        bestOfGroup = []
        for row in rows { self.rows[row.id] = row }
        arrange()
        initialStates = imageIDs.map { state(of: $0) }
        initialStatuses = imageIDs.map { statuses[$0] ?? ItemStatus() }
    }

    private func state(of id: String) -> CullState {
        rows[id].map { CullController.state(from: $0.selection, inBasket: $0.inBasket) } ?? CullState()
    }

    /// Indexes `folder`, opens a review session and orders items group by group.
    /// Blocking (index scan, sidecar reconciliation, preview hashing): call off the main actor.
    public static func scan(folder: URL, appSupport: URL? = nil,
                            basketTarget: String = defaultBasketTarget) throws -> EngineLibrary {
        let start = Date()
        let fm = FileManager.default
        let support = appSupport ?? defaultSupportDirectory
        let engine = try Engine.open(appSupportDir: support.path)
        let previewEvents = PreviewEvents()
        engine.setEventListener(listener: previewEvents)
        let handle = try engine.indexFolder(path: folder.path)
        let session = try engine.openCullSession(folder: handle.path)
        try session.setBasketTarget(name: basketTarget)
        let sequence = try session.changeSequence()
        let rows = try session.images()
        let layout = try session.groups()
        // Display order: group by group.
        var ids: [String] = []
        ids.reserveCapacity(rows.count)
        for group in layout { ids.append(contentsOf: group.images) }
        let statuses = try session.derivedStatuses(imageIds: ids)
        // List the canonical folder: contentsOfDirectory refuses a symlink to a directory.
        let canonical = URL(fileURLWithPath: handle.path, isDirectory: true)
        let subfolders = try fm.contentsOfDirectory(at: canonical, includingPropertiesForKeys: [.isDirectoryKey], options: [.skipsHiddenFiles])
            .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true && $0.lastPathComponent != ".edits" }
            .sorted { $0.path < $1.path }
        return EngineLibrary(title: folder.lastPathComponent, folder: folder, subfolders: subfolders,
                             scanDuration: Date().timeIntervalSince(start), engine: engine, session: session,
                             previewEvents: previewEvents, rows: rows, layout: layout,
                             statuses: Dictionary(statuses.map { ($0.imageId, ItemStatus($0)) }, uniquingKeysWith: { a, _ in a }),
                             previewErrors: try session.previewErrors(), sequence: sequence)
    }

    public func makeCullController() -> CullController { CullController(engine: self) }

    // MARK: In-place updates

    /// Rebuilds items, groups and ids from `layout` and `rows`, reusing each image's reference.
    private func arrange() {
        let formatter = Self.formatter
        var newItems: [PhotoItem] = []
        var ids: [String] = []
        var ranges: [Range<Int>] = []
        var best: [Int] = []
        newItems.reserveCapacity(rows.count)
        ids.reserveCapacity(rows.count)
        for (g, group) in layout.enumerated() {
            let start = newItems.count
            for imageID in group.images {
                guard let row = rows[imageID] else { continue }
                let url = URL(fileURLWithPath: row.path)
                let date = row.captureTime.flatMap { value in
                    Double(value).map { Date(timeIntervalSince1970: $0) }
                        ?? formatter.date(from: String(value.prefix(19)))
                } ?? Date(timeIntervalSince1970: 0)
                if imageID == group.best { best.append(newItems.count) }
                let reference = references[imageID] ?? {
                    let r = EngineImageReference(engine: engine, imageID: imageID, previewEvents: previewEvents)
                    references[imageID] = r
                    return r
                }()
                newItems.append(PhotoItem(id: newItems.count, url: url, name: url.lastPathComponent,
                                          kind: StubLibrary.kind(forExtension: url.pathExtension) ?? .raw,
                                          captureDate: date, pixelWidth: 0, pixelHeight: 0, groupID: g,
                                          engineImage: reference))
                ids.append(imageID)
            }
            if best.count == g { best.append(start) }
            ranges.append(start..<newItems.count)
        }
        items = newItems
        imageIDs = ids
        groups = ranges
        bestOfGroup = best
        itemOfImage = Dictionary(uniqueKeysWithValues: ids.enumerated().map { ($1, $0) })
    }

    private static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        return f
    }()

    /// `handler` runs on an engine thread whenever the catalog changed (own writes, tethered
    /// frames, other writers); pull with `session.syncChanges()` and `apply(_:)`.
    public func onCatalogChange(_ handler: (@Sendable (UInt64) -> Void)?) {
        previewEvents.onLibraryChanged(handler)
    }

    /// Current row (selection, basket flag, path) of an image in the layout.
    public func row(for imageID: String) -> SessionImage? { rows[imageID] }

    /// Applies a queue delta in place. Returns how item ids moved and what changed, or nil when
    /// the delta asks for a reload (`reset`). Call on the main actor, one delta at a time.
    public func apply(_ delta: QueueDelta) -> LibraryUpdate? {
        guard !delta.reset else { return nil }
        let oldIDs = imageIDs
        for id in delta.removed {
            rows[id] = nil
            references[id] = nil
        }
        for row in delta.added { rows[row.id] = row }
        for u in delta.updated { rows[u.image.id] = u.image }
        if let groups = delta.groups { layout = groups }
        let moved = !delta.added.isEmpty || !delta.removed.isEmpty || delta.groups != nil
        let geometry = delta.updated.contains { $0.fields.file || $0.fields.captureTime || $0.fields.metadata }
        if moved || geometry { arrange() }
        changeSequence = max(changeSequence, delta.sequence)
        let remap = oldIDs.map { itemOfImage[$0] }
        let inserted = delta.added.compactMap { itemOfImage[$0.id] }
        let updated = delta.updated.compactMap { u in itemOfImage[u.image.id].map { ($0, u.fields) } }
        return LibraryUpdate(remap: remap, inserted: inserted, removed: delta.removed, updated: updated,
                             groupsChanged: delta.groups != nil, sequence: delta.sequence)
    }
}

/// How an in-place library update moved and changed items (see `EngineLibrary.apply`).
public struct LibraryUpdate {
    /// Old item id → new item id; nil for items that left the library.
    public let remap: [Int?]
    /// New items (new ids), in display order.
    public let inserted: [Int]
    /// Engine image ids that left.
    public let removed: [String]
    /// Remaining items (new ids) whose catalog rows changed.
    public let updated: [(id: Int, fields: ChangedFields)]
    /// Group layout (membership, order or suggested best) changed.
    public let groupsChanged: Bool
    public let sequence: UInt64

    public var isEmpty: Bool { inserted.isEmpty && removed.isEmpty && updated.isEmpty && !groupsChanged }
    /// Whether any item id changed.
    public var idsMoved: Bool { remap.enumerated().contains { $0.element != $0.offset } || !inserted.isEmpty }
    public func newID(_ old: Int) -> Int? { remap.indices.contains(old) ? remap[old] : nil }
}

extension ItemStatus {
    init(_ status: ImageStatus) {
        let phase: DerivedPhase = switch status.phase {
        case .unedited: .unedited
        case .edited: .edited
        case .exported: .exported
        case .published: .published
        }
        self.init(phase: phase, albums: status.inAlbum)
    }
}
