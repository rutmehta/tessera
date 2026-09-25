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
public final class EngineLibrary: PhotoLibrary {
    public let title: String
    public let folder: URL?
    public let items: [PhotoItem]
    public let groups: [Range<Int>]
    public let subfolders: [URL]
    public let scanDuration: TimeInterval
    public let engine: Engine
    public let session: CullSession
    /// Engine image id per item id.
    public let imageIDs: [String]
    let itemOfImage: [String: Int]
    let initialStates: [CullState]
    let initialStatuses: [ItemStatus]
    /// Suggested best item id per group.
    let bestOfGroup: [Int]
    /// Images whose embedded preview could not be hashed (still reviewable).
    public let previewErrors: [String]

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

    private init(title: String, folder: URL, items: [PhotoItem], groups: [Range<Int>], subfolders: [URL],
                 scanDuration: TimeInterval, engine: Engine, session: CullSession, imageIDs: [String],
                 states: [CullState], statuses: [ItemStatus], best: [Int], previewErrors: [String]) {
        self.title = title; self.folder = folder; self.items = items; self.groups = groups
        self.subfolders = subfolders; self.scanDuration = scanDuration; self.engine = engine
        self.session = session; self.imageIDs = imageIDs
        itemOfImage = Dictionary(uniqueKeysWithValues: imageIDs.enumerated().map { ($1, $0) })
        initialStates = states; initialStatuses = statuses; bestOfGroup = best
        self.previewErrors = previewErrors
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
        let rows = try session.images()
        let rustGroups = try session.groups()
        let rowOfImage = Dictionary(uniqueKeysWithValues: rows.enumerated().map { ($1.id, $0) })

        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        var items: [PhotoItem] = []
        var ids: [String] = []
        var states: [CullState] = []
        var ranges: [Range<Int>] = []
        var best: [Int] = []
        items.reserveCapacity(rows.count)
        for (g, group) in rustGroups.enumerated() {
            let start = items.count
            for imageID in group.images {
                guard let r = rowOfImage[imageID] else { continue }
                let row = rows[r]
                let url = URL(fileURLWithPath: row.path)
                let date = row.captureTime.flatMap { value in
                    Double(value).map { Date(timeIntervalSince1970: $0) }
                        ?? formatter.date(from: String(value.prefix(19)))
                } ?? Date(timeIntervalSince1970: 0)
                if imageID == group.best { best.append(items.count) }
                items.append(PhotoItem(id: items.count, url: url, name: url.lastPathComponent,
                                       kind: StubLibrary.kind(forExtension: url.pathExtension) ?? .raw,
                                       captureDate: date, pixelWidth: 0, pixelHeight: 0, groupID: g,
                                       engineImage: EngineImageReference(engine: engine, imageID: row.id,
                                                                                    previewEvents: previewEvents)))
                ids.append(row.id)
                states.append(CullController.state(from: row.selection, inBasket: row.inBasket))
            }
            if best.count == g { best.append(start) }
            ranges.append(start..<items.count)
        }
        let statuses = try session.derivedStatuses(imageIds: ids).map(ItemStatus.init)
        // List the canonical folder: contentsOfDirectory refuses a symlink to a directory.
        let canonical = URL(fileURLWithPath: handle.path, isDirectory: true)
        let subfolders = try fm.contentsOfDirectory(at: canonical, includingPropertiesForKeys: [.isDirectoryKey], options: [.skipsHiddenFiles])
            .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true && $0.lastPathComponent != ".edits" }
            .sorted { $0.path < $1.path }
        return EngineLibrary(title: folder.lastPathComponent, folder: folder, items: items, groups: ranges,
                             subfolders: subfolders, scanDuration: Date().timeIntervalSince(start),
                             engine: engine, session: session, imageIDs: ids, states: states,
                             statuses: statuses, best: best, previewErrors: try session.previewErrors())
    }

    public func makeCullController() -> CullController { CullController(engine: self) }

    /// Test aid behind the hidden `--seed-scores` flag: deterministic synthetic AI signals so the
    /// defect sweep has something to find before ML producers exist. Item n (display order) gets
    /// focus 0.25 when n % 4 == 1 (else 0.85) and closed_eyes 0.92 when n % 5 == 2 (else 0.05).
    public func seedSyntheticScores() throws {
        for (n, imageID) in imageIDs.enumerated() {
            try engine.setScore(imageId: imageID, signal: DefectRule.focus.signal,
                                value: n % 4 == 1 ? 0.25 : 0.85, model: "seed-scores/1")
            try engine.setScore(imageId: imageID, signal: DefectRule.closedEyes.signal,
                                value: n % 5 == 2 ? 0.92 : 0.05, model: "seed-scores/1")
        }
    }
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
