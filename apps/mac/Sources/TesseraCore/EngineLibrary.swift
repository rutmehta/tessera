import Foundation
import TesseraFFI

/// Both real and explicit fallback libraries share the existing keyboard/navigation model.
public protocol PhotoLibrary: Sendable {
    var title: String { get }
    var folder: URL? { get }
    var items: [PhotoItem] { get }
    var groups: [Range<Int>] { get }
    var subfolders: [URL] { get }
    var scanDuration: TimeInterval { get }
    func initialState(for item: PhotoItem) -> CullState
    func persist(_ state: CullState, for item: PhotoItem) throws
}

extension StubLibrary: PhotoLibrary {
    public func initialState(for item: PhotoItem) -> CullState { CullState() }
    public func persist(_ state: CullState, for item: PhotoItem) throws {}
}

/// Retained by immutable items, so in-flight thumbnails cannot switch to a newly opened catalog.
public final class EngineImageReference: Sendable, Hashable {
    public let engine: Engine
    public let imageID: String
    init(engine: Engine, imageID: String) { self.engine = engine; self.imageID = imageID }
    public static func == (lhs: EngineImageReference, rhs: EngineImageReference) -> Bool { lhs === rhs }
    public func hash(into hasher: inout Hasher) { hasher.combine(ObjectIdentifier(self)) }
}

public final class EngineLibrary: PhotoLibrary {
    private let backing: StubLibrary
    private let selections: [String: TesseraFFI.Selection]
    public var title: String { backing.title }
    public var folder: URL? { backing.folder }
    public var items: [PhotoItem] { backing.items }
    public var groups: [Range<Int>] { backing.groups }
    public var subfolders: [URL] { backing.subfolders }
    public var scanDuration: TimeInterval { backing.scanDuration }
    private init(backing: StubLibrary, selections: [String: TesseraFFI.Selection]) {
        self.backing = backing; self.selections = selections
    }

    public static func scan(folder: URL, appSupport: URL? = nil) throws -> EngineLibrary {
        let start = Date()
        let fm = FileManager.default
        let support = appSupport ?? fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Tessera", isDirectory: true)
        let engine = try Engine.open(appSupportDir: support.path)
        let handle = try engine.indexFolder(path: folder.path)
        let rows = try engine.listImages(query: ImageQuery(folder: handle.path, text: nil, decision: nil, limit: 0, offset: 0))
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        let items = rows.enumerated().map { index, row in
            let url = URL(fileURLWithPath: row.path)
            let date = row.captureTime.flatMap { value in
                Double(value).map { Date(timeIntervalSince1970: $0) } ?? formatter.date(from: value)
            } ?? Date(timeIntervalSince1970: 0)
            return PhotoItem(id: index, url: url, name: url.lastPathComponent,
                             kind: StubLibrary.kind(forExtension: url.pathExtension) ?? .raw,
                             captureDate: date, pixelWidth: 0, pixelHeight: 0,
                             engineImage: EngineImageReference(engine: engine, imageID: row.id))
        }
        let subfolders = try fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.isDirectoryKey], options: [.skipsHiddenFiles])
            .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
            .sorted { $0.path < $1.path }
        let backing = StubLibrary(title: folder.lastPathComponent, folder: folder, items: items,
                                  subfolders: subfolders, scanDuration: Date().timeIntervalSince(start))
        return EngineLibrary(backing: backing, selections: Dictionary(uniqueKeysWithValues: rows.map { ($0.id, $0.selection) }))
    }

    public func initialState(for item: PhotoItem) -> CullState {
        guard let ref = item.engineImage, let state = selections[ref.imageID] else { return CullState() }
        let decision: Decision = switch state.decision { case .keep: .keep; case .reject: .reject; case .undecided: .undecided }
        return CullState(decision: decision, grade: state.grade ?? 0, mark: Self.marks.first(where: { $0.value == state.mark })?.key ?? 0)
    }

    // Keep names stable, rather than persisting UI key numbers as mark identifiers.
    private static let marks: [UInt8: String] = [6: "Needs Retouch", 7: "Client Favourite", 8: "Print", 9: "Review"]
    public func persist(_ state: CullState, for item: PhotoItem) throws {
        guard let ref = item.engineImage else { return }
        let decision: TesseraFFI.Decision = switch state.decision { case .keep: .keep; case .reject: .reject; case .undecided: .undecided }
        // Preserve an imported custom mark until the user explicitly chooses a mark key.
        let mark = Self.marks[state.mark] ?? (state.mark == 0 && initialState(for: item).mark == 0 ? selections[ref.imageID]?.mark : nil)
        try ref.engine.setSelection(imageId: ref.imageID, selection: TesseraFFI.Selection(
            decision: decision, grade: state.grade == 0 ? nil : state.grade, mark: mark))
    }
}
