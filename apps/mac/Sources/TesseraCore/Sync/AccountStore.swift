import CoreServices
import Foundation
import Security

/// Where the account records live (SPEC §3, CS-2b): the app's iCloud Drive container, or a
/// folder some other app keeps in sync between the user's Macs (Nextcloud, Dropbox, Google
/// Drive, Syncthing). The choice and the folder persist in user defaults; the folder as a
/// security-scoped bookmark (the app is not sandboxed today, but the bookmark keeps the folder
/// reachable if it ever is), with its path beside it so Settings can still show a folder that
/// no longer resolves.
public enum SyncLocation: Equatable, Sendable {
    case iCloudDrive
    case folder(URL)

    /// "icloud" or "folder".
    public static let kindKey = "Sync.Location"
    public static let bookmarkKey = "Sync.FolderBookmark"
    public static let pathKey = "Sync.FolderPath"

    public var isFolder: Bool { if case .folder = self { true } else { false } }
    public var folderURL: URL? { if case .folder(let url) = self { url } else { nil } }

    /// What Settings ▸ Sync chose. Nil: "a synced folder", with none chosen yet.
    public static func stored(in defaults: UserDefaults) -> SyncLocation? {
        guard defaults.string(forKey: kindKey) == "folder" else { return .iCloudDrive }
        return storedFolder(in: defaults).map { .folder($0) }
    }

    /// The last chosen folder, whichever location is in use.
    public static func storedFolder(in defaults: UserDefaults) -> URL? {
        if let data = defaults.data(forKey: bookmarkKey) {
            var stale = false
            if let url = (try? URL(resolvingBookmarkData: data, options: [.withSecurityScope, .withoutUI], bookmarkDataIsStale: &stale))
                ?? (try? URL(resolvingBookmarkData: data, options: [.withoutUI], bookmarkDataIsStale: &stale)) {
                _ = url.startAccessingSecurityScopedResource()
                if stale { saveBookmark(for: url, in: defaults) }
                return url
            }
        }
        return defaults.string(forKey: pathKey).map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    /// Persists the choice; `.folder` also remembers the folder (see `storedFolder`).
    public func save(in defaults: UserDefaults) {
        switch self {
        case .iCloudDrive:
            defaults.set("icloud", forKey: Self.kindKey)
        case .folder(let url):
            defaults.set("folder", forKey: Self.kindKey)
            Self.saveBookmark(for: url, in: defaults)
        }
    }

    /// "A synced folder" with no folder chosen yet.
    public static func saveFolderPending(in defaults: UserDefaults) { defaults.set("folder", forKey: kindKey) }

    static func saveBookmark(for url: URL, in defaults: UserDefaults) {
        defaults.set(url.path, forKey: pathKey)
        let data = (try? url.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil))
            ?? (try? url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil))
        defaults.set(data, forKey: bookmarkKey)
    }
}

/// The account-resident records (SPEC §3, CS-2): `Tessera/` under the app's iCloud Drive
/// container `iCloud.dev.tessera.app`, or under a synced folder of the user's choosing (CS-2b),
/// one JSON file per record. Writes go through `NSFileCoordinator` and are atomic; `Watcher`
/// runs an `NSMetadataQuery` (iCloud) or an FSEvents stream (folder) so a change made on
/// another Mac applies live. Conflict copies a sync tool leaves beside a record are merged in
/// by the record's own rules and removed. The store is injectable with any base URL, so tests
/// use a temp directory and never need iCloud; with no container at all it is
/// `.unavailable(reason)` and every call is a no-op (the app keeps working locally).
public final class AccountStore: @unchecked Sendable {
    public static let containerIdentifier = "iCloud.dev.tessera.app"
    public static let folderName = "Tessera"
    public static let machineKey = "Sync.MachineID"
    public static let enabledKey = "Sync.Enabled"

    /// Why there is no container, with the one-line fix Settings ▸ Sync shows.
    public enum UnavailableReason: Equatable, Sendable {
        /// The running build was not signed with the iCloud entitlements (ad-hoc builds, `swift run`).
        case noEntitlement
        /// No iCloud account on this Mac.
        case noAccount
        /// Signed in, but iCloud Drive (or Tessera under it) is off.
        case iCloudDriveOff
        /// An isolated run (`--app-dir`, `TESSERA_APP_DIR`, background audits) never touches the account.
        case isolatedRun
        /// "A synced folder" is chosen, but no folder has been picked yet.
        case noFolder
        /// The chosen folder is not there (the sync app is not running, or the folder moved).
        case folderMissing
        /// The chosen folder is there but cannot be written.
        case folderReadOnly

        public var message: String {
            switch self {
            case .noEntitlement: "This build has no iCloud entitlement."
            case .noAccount: "This Mac is not signed in to iCloud."
            case .iCloudDriveOff: "iCloud Drive is off for Tessera."
            case .isolatedRun: "Sync is off for this run (--app-dir)."
            case .noFolder: "No folder chosen."
            case .folderMissing: "Folder not found: is Nextcloud (or the app that syncs it) running?"
            case .folderReadOnly: "The folder is read-only."
            }
        }
        public var fix: String {
            switch self {
            case .noEntitlement: "Install a Developer ID build of Tessera (Support/release/README.md)."
            case .noAccount: "Sign in to iCloud in System Settings, then turn on iCloud Drive."
            case .iCloudDriveOff: "Turn on iCloud Drive in System Settings ▸ iCloud, and allow Tessera under it."
            case .isolatedRun: "Launch Tessera without --app-dir or TESSERA_APP_DIR."
            case .noFolder: "Click Choose… and pick a folder your sync app keeps on every Mac."
            case .folderMissing: "Start the sync app, or choose the folder again."
            case .folderReadOnly: "Choose a folder you can write to."
            }
        }
    }

    public enum Availability: Equatable, Sendable {
        case available
        case unavailable(UnavailableReason)
    }

    public let availability: Availability
    public let location: SyncLocation
    /// `<container>/Tessera` (or `<folder>/Tessera`); nil when unavailable.
    public let root: URL?
    /// This Mac, for `machine` stamps: a UUID made once and kept in user defaults.
    public let machine: String
    private let defaults: UserDefaults
    private let lock = NSLock()
    /// Modification dates the watcher has reported (or we wrote), per file path, so a file that
    /// did not change (or that we changed ourselves) is not reported again.
    private var seen: [String: Date] = [:]

    /// An iCloud-style store over `baseURL` (the container, or a temp directory in tests).
    public init(baseURL: URL?, reason: UnavailableReason? = nil, defaults: UserDefaults = .standard) {
        self.defaults = defaults
        machine = Self.machineID(in: defaults)
        location = .iCloudDrive
        if let baseURL, reason == nil {
            root = baseURL.appendingPathComponent(Self.folderName, isDirectory: true)
            availability = .available
        } else {
            root = nil
            availability = .unavailable(reason ?? .iCloudDriveOff)
        }
    }

    /// A store over `<folder>/Tessera/` in a folder another app syncs. Available when the folder
    /// is there and writable (`Tessera/` is created if missing); otherwise the reason names what
    /// to fix.
    public init(folder: URL, defaults: UserDefaults = .standard) {
        self.defaults = defaults
        machine = Self.machineID(in: defaults)
        location = .folder(folder)
        let fm = FileManager.default
        var isDirectory: ObjCBool = false
        guard fm.fileExists(atPath: folder.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            root = nil; availability = .unavailable(.folderMissing); return
        }
        let root = folder.appendingPathComponent(Self.folderName, isDirectory: true)
        guard fm.isWritableFile(atPath: folder.path), (try? fm.createDirectory(at: root, withIntermediateDirectories: true)) != nil else {
            self.root = nil; availability = .unavailable(.folderReadOnly); return
        }
        self.root = root
        availability = .available
    }

    /// The store for a location: the real container, or the chosen folder (nil: "a synced folder"
    /// with none chosen yet). `url(forUbiquityContainerIdentifier:)` may block while iCloud sets
    /// the container up, so call this off the main actor.
    public static func live(location: SyncLocation? = .iCloudDrive, defaults: UserDefaults = .standard) -> AccountStore {
        switch location {
        case .folder(let url): return AccountStore(folder: url, defaults: defaults)
        case nil: return AccountStore(baseURL: nil, reason: .noFolder, defaults: defaults)
        case .iCloudDrive: break
        }
        guard hasEntitlement() else { return AccountStore(baseURL: nil, reason: .noEntitlement, defaults: defaults) }
        guard FileManager.default.ubiquityIdentityToken != nil else {
            return AccountStore(baseURL: nil, reason: .noAccount, defaults: defaults)
        }
        guard let url = FileManager.default.url(forUbiquityContainerIdentifier: containerIdentifier) else {
            return AccountStore(baseURL: nil, reason: .iCloudDriveOff, defaults: defaults)
        }
        return AccountStore(baseURL: url, defaults: defaults)
    }

    /// The signed binary carries our container in `com.apple.developer.icloud-container-identifiers`.
    public static func hasEntitlement() -> Bool {
        guard let task = SecTaskCreateFromSelf(nil),
              let value = SecTaskCopyValueForEntitlement(task, "com.apple.developer.icloud-container-identifiers" as CFString, nil)
        else { return false }
        return (value as? [String])?.contains(containerIdentifier) == true
    }

    static func machineID(in defaults: UserDefaults) -> String {
        if let id = defaults.string(forKey: machineKey), !id.isEmpty { return id }
        let id = UUID().uuidString.lowercased()
        defaults.set(id, forKey: machineKey)
        return id
    }

    public var isAvailable: Bool { availability == .available }
    public var unavailableReason: UnavailableReason? {
        if case .unavailable(let r) = availability { r } else { nil }
    }

    /// Settings ▸ Sync's switch (on by default). Off: nothing is written or applied.
    public var isEnabled: Bool {
        get { defaults.object(forKey: Self.enabledKey) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Self.enabledKey) }
    }
    public var isActive: Bool { isAvailable && isEnabled }

    // MARK: Paths

    /// Ids become file names: anything outside `[A-Za-z0-9._-]` is percent-encoded.
    static let fileSafe = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "._-"))

    public func url(for key: RecordKey) -> URL? {
        guard let root else { return nil }
        if key.kind.isKeyed {
            guard let id = key.id, !id.isEmpty, let name = id.addingPercentEncoding(withAllowedCharacters: Self.fileSafe) else { return nil }
            return root.appendingPathComponent(key.kind.rawValue, isDirectory: true).appendingPathComponent("\(name).json")
        }
        return root.appendingPathComponent("\(key.kind.rawValue).json")
    }

    /// The record a container path stands for (nil for anything that is not ours).
    public func key(for url: URL) -> RecordKey? {
        guard let root, url.pathExtension == "json" else { return nil }
        let parts = url.standardizedFileURL.pathComponents
        let base = root.standardizedFileURL.pathComponents
        guard parts.count > base.count, Array(parts.prefix(base.count)) == base else { return nil }
        let rel = Array(parts.dropFirst(base.count))
        let stem = String(url.lastPathComponent.dropLast(5))
        if rel.count == 1, let kind = RecordKind(rawValue: stem), !kind.isKeyed { return RecordKey(kind) }
        if rel.count == 2, let kind = RecordKind(rawValue: rel[0]), kind.isKeyed, let id = stem.removingPercentEncoding {
            return RecordKey(kind, id: id)
        }
        return nil
    }

    // MARK: Codec

    nonisolated(unsafe) static let iso8601: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
    nonisolated(unsafe) static let iso8601Plain: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    static func date(_ text: String) -> Date? { iso8601.date(from: text) ?? iso8601Plain.date(from: text) }

    public static func encoder() -> JSONEncoder {
        let e = JSONEncoder()
        e.outputFormatting = [.prettyPrinted, .sortedKeys]
        e.dateEncodingStrategy = .custom { date, encoder in
            var c = encoder.singleValueContainer()
            try c.encode(iso8601.string(from: date))
        }
        return e
    }

    public static func decoder() -> JSONDecoder {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .custom { decoder in
            let text = try decoder.singleValueContainer().decode(String.self)
            guard let date = date(text) else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "not an ISO 8601 date: \(text)"))
            }
            return date
        }
        return d
    }

    // MARK: Records

    /// The record on disk, through file coordination (iCloud may be writing it). Nil when there
    /// is none, the store is inactive, or the file does not decode (a newer schema).
    public func read<R: SyncRecord>(_ type: R.Type, id: String? = nil) -> R? {
        guard isActive, let url = url(for: RecordKey(R.kind, id: id)) else { return nil }
        return readFile(type, at: url)
    }

    private func readFile<R: SyncRecord>(_ type: R.Type, at url: URL) -> R? {
        var data: Data?
        var error: NSError?
        NSFileCoordinator(filePresenter: nil).coordinate(readingItemAt: url, options: [], error: &error) { u in
            data = try? Data(contentsOf: u)
        }
        guard let data else { return nil }
        return try? Self.decoder().decode(R.self, from: data)
    }

    /// Every record of a keyed kind.
    public func list<R: SyncRecord>(_ type: R.Type) -> [R] {
        guard isActive, R.kind.isKeyed, let root else { return [] }
        let folder = root.appendingPathComponent(R.kind.rawValue, isDirectory: true)
        let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
        return files.filter { $0.pathExtension == "json" }.compactMap { readFile(type, at: $0) }
    }

    /// Merges `record` with what is on disk and writes the result atomically under coordination.
    /// Returns what is now on disk (nil when inactive or the write failed). A record equal to the
    /// stored one apart from its stamp is not written, so applying a remote change never bounces
    /// a fresh stamp back.
    @discardableResult
    public func write<R: SyncRecord>(_ record: R, id: String? = nil) -> R? {
        guard isActive, let url = url(for: RecordKey(R.kind, id: id)) else { return nil }
        var written: R?
        var error: NSError?
        let coordinator = NSFileCoordinator(filePresenter: nil)
        coordinator.coordinate(readingItemAt: url, options: [], writingItemAt: url, options: .forReplacing, error: &error) { readURL, writeURL in
            let existing: R? = (try? Data(contentsOf: readURL)).flatMap { try? Self.decoder().decode(R.self, from: $0) }
            if let existing, record.sameContent(as: existing) { written = existing; return }
            let merged = existing.map { R.merge($0, record) } ?? record
            if let existing, merged == existing { written = existing; return }
            do {
                try FileManager.default.createDirectory(at: writeURL.deletingLastPathComponent(), withIntermediateDirectories: true)
                try Self.encoder().encode(merged).write(to: writeURL, options: .atomic)
                written = merged
                let stamp = (try? writeURL.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
                self.lock.withLock { self.seen[writeURL.standardizedFileURL.path] = stamp ?? merged.modified }
            } catch {
                NSLog("Sync: could not write %@: %@", url.lastPathComponent, error.localizedDescription)
            }
        }
        if let error { NSLog("Sync: coordination failed for %@: %@", url.lastPathComponent, error.localizedDescription) }
        return written
    }

    /// For the watcher: the keys whose files changed since last reported (our own writes excluded).
    func changed(_ files: [(url: URL, modified: Date?)]) -> [RecordKey] {
        lock.withLock {
            files.compactMap { file in
                guard let key = key(for: file.url) else { return nil }
                let path = file.url.standardizedFileURL.path
                let stamp = file.modified ?? .distantPast
                if let last = seen[path], last >= stamp { return nil }
                seen[path] = stamp
                return key
            }
        }
    }

    /// Every record file under the root with its modification date (what the folder watcher feeds
    /// to `changed`).
    func recordFiles() -> [(url: URL, modified: Date?)] {
        guard let root, let files = FileManager.default.enumerator(at: root, includingPropertiesForKeys: [.contentModificationDateKey]) else { return [] }
        var out: [(url: URL, modified: Date?)] = []
        for case let url as URL in files where key(for: url) != nil {
            out.append((url, (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate))
        }
        return out
    }

    // MARK: Conflict copies

    /// A sync tool that cannot merge leaves the other Mac's copy beside the record: Dropbox and
    /// Nextcloud "name (conflicted copy 2026-10-15).json" / "name (conflict …).json", Syncthing
    /// "name.sync-conflict-20261015-120000-ABCDEFG.json", older Nextcloud "name_conflict-….json".
    /// A record name never contains a space or "_conflict-" (ids are percent-encoded), so the stem
    /// before the marker is the record's.
    nonisolated(unsafe) static let conflictCopy = try! NSRegularExpression(
        pattern: #"^(.+?)(?: \([^)]*conflict[^)]*\)|\.sync-conflict-[^.]+|_conflict-[^.]+)\.json$"#, options: .caseInsensitive)

    /// The record file a conflict copy belongs to; nil for a regular file.
    public func canonicalURL(forConflictCopy url: URL) -> URL? {
        let name = url.lastPathComponent
        guard let m = Self.conflictCopy.firstMatch(in: name, range: NSRange(name.startIndex..., in: name)),
              let stem = Range(m.range(at: 1), in: name) else { return nil }
        return url.deletingLastPathComponent().appendingPathComponent("\(name[stem]).json")
    }

    /// Merges every conflict copy under the root into its record (the record's own rules decide)
    /// and removes the copy. Returns the keys merged into; a copy that does not decode (a newer
    /// schema) stays where it is.
    @discardableResult
    public func absorbConflictCopies() -> [RecordKey] {
        guard isActive, let root, let files = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil) else { return [] }
        var merged: [RecordKey] = []
        for case let url as URL in files {
            guard url.pathExtension == "json", let canonical = canonicalURL(forConflictCopy: url), let key = key(for: canonical) else { continue }
            let absorbed = switch key.kind {
            case .preferences: absorb(PreferencesRecord.self, copy: url, key: key)
            case .keywords: absorb(KeywordsRecord.self, copy: url, key: key)
            case .styleProfile: absorb(StyleProfileRecord.self, copy: url, key: key)
            case .learning: absorb(LearningRecord.self, copy: url, key: key)
            case .person: absorb(PersonRecord.self, copy: url, key: key)
            case .drive: absorb(DriveRecord.self, copy: url, key: key)
            }
            if absorbed, !merged.contains(key) { merged.append(key) }
        }
        return merged
    }

    private func absorb<R: SyncRecord>(_ type: R.Type, copy: URL, key: RecordKey) -> Bool {
        guard let record = readFile(type, at: copy), write(record, id: key.id) != nil else { return false }
        do {
            try FileManager.default.removeItem(at: copy)
        } catch {
            NSLog("Sync: could not remove conflict copy %@: %@", copy.lastPathComponent, error.localizedDescription)
        }
        return true
    }

    // MARK: Stats (Settings ▸ Sync)

    public struct Stats: Equatable, Sendable {
        /// The newest file in the container (the last write from any Mac that reached this one).
        public var lastSync: Date?
        public var bytes: Int64
        public var files: Int
        public init(lastSync: Date? = nil, bytes: Int64 = 0, files: Int = 0) { self.lastSync = lastSync; self.bytes = bytes; self.files = files }
    }

    public func stats() -> Stats {
        guard isAvailable, let root else { return Stats() }
        let keys: Set<URLResourceKey> = [.contentModificationDateKey, .fileSizeKey, .isRegularFileKey]
        guard let files = FileManager.default.enumerator(at: root, includingPropertiesForKeys: Array(keys)) else { return Stats() }
        var stats = Stats()
        for case let url as URL in files where url.pathExtension == "json" {
            guard let v = try? url.resourceValues(forKeys: keys), v.isRegularFile == true else { continue }
            stats.files += 1
            stats.bytes += Int64(v.fileSize ?? 0)
            if let m = v.contentModificationDate, stats.lastSync.map({ m > $0 }) ?? true { stats.lastSync = m }
        }
        return stats
    }

    // MARK: Watching

    /// Reports records written on another Mac. iCloud: a metadata query over the container (a
    /// record iCloud has only the metadata of is downloaded first). Folder: an FSEvents stream on
    /// `Tessera/`, coalesced to 1 s, after which the folder is rescanned (the sync app may land
    /// several files and a conflict copy at once). Main-actor: `NSMetadataQuery` needs the main
    /// run loop, and the stream delivers on the main queue.
    @MainActor
    public final class Watcher {
        private let store: AccountStore
        private var query: NSMetadataQuery?
        /// Touched only on the main actor and in deinit (which cannot be isolated on this toolchain).
        nonisolated(unsafe) private var stream: FSEventStreamRef?
        private var observers: [NSObjectProtocol] = []
        /// Called on the main actor with each changed record.
        public var onChange: ((RecordKey) -> Void)?

        public init(store: AccountStore) { self.store = store }

        public func start() {
            guard query == nil, stream == nil, store.isAvailable else { return }
            if store.location.isFolder { startStream() } else { startQuery() }
        }

        public func stop() {
            query?.stop()
            query = nil
            observers.forEach { NotificationCenter.default.removeObserver($0) }
            observers = []
            if let stream {
                FSEventStreamStop(stream)
                FSEventStreamInvalidate(stream)
                FSEventStreamRelease(stream)
                self.stream = nil
            }
        }

        deinit {
            // The stream holds an unretained pointer to the watcher; it must not outlive it.
            if let stream {
                FSEventStreamStop(stream)
                FSEventStreamInvalidate(stream)
                FSEventStreamRelease(stream)
            }
        }

        // MARK: iCloud

        private func startQuery() {
            let q = NSMetadataQuery()
            q.searchScopes = [NSMetadataQueryUbiquitousDataScope, NSMetadataQueryUbiquitousDocumentsScope]
            q.predicate = NSPredicate(format: "%K LIKE '*.json'", NSMetadataItemFSNameKey)
            q.notificationBatchingInterval = 1
            let center = NotificationCenter.default
            for name in [NSNotification.Name.NSMetadataQueryDidFinishGathering, .NSMetadataQueryDidUpdate] {
                observers.append(center.addObserver(forName: name, object: q, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.collect() }
                })
            }
            query = q
            q.start()
        }

        private func collect() {
            guard let q = query else { return }
            q.disableUpdates()
            defer { q.enableUpdates() }
            var files: [(url: URL, modified: Date?)] = []
            for case let item as NSMetadataItem in q.results {
                guard let url = item.value(forAttribute: NSMetadataItemURLKey) as? URL, store.key(for: url) != nil else { continue }
                let status = item.value(forAttribute: NSMetadataUbiquitousItemDownloadingStatusKey) as? String
                if let status, status != NSMetadataUbiquitousItemDownloadingStatusCurrent {
                    // Metadata only: ask for the bytes; the query reports again when they land.
                    try? FileManager.default.startDownloadingUbiquitousItem(at: url)
                    continue
                }
                files.append((url, item.value(forAttribute: NSMetadataItemFSContentChangeDateKey) as? Date))
            }
            for key in store.changed(files) { onChange?(key) }
        }

        // MARK: Folder

        /// The first scan stands in for the query's initial gather. Our own writes are not ignored
        /// at the stream (tests write "remotely" from this process); `changed` filters them.
        private func startStream() {
            guard let root = store.root else { return }
            var context = FSEventStreamContext(version: 0, info: Unmanaged.passUnretained(self).toOpaque(),
                                               retain: nil, release: nil, copyDescription: nil)
            let callback: FSEventStreamCallback = { _, info, _, _, _, _ in
                guard let info else { return }
                let watcher = Unmanaged<Watcher>.fromOpaque(info).takeUnretainedValue()
                MainActor.assumeIsolated { watcher.scanFolder() }
            }
            let flags = FSEventStreamCreateFlags(kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagUseCFTypes)
            guard let s = FSEventStreamCreate(nil, callback, &context, [root.path] as CFArray,
                                              FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 1, flags) else { return }
            FSEventStreamSetDispatchQueue(s, .main)
            stream = s
            FSEventStreamStart(s)
            scanFolder()
        }

        /// Conflict copies first (merging one writes the record, which the scan then sees as our
        /// own write), then whatever else changed.
        private func scanFolder() {
            var keys = store.absorbConflictCopies()
            for key in store.changed(store.recordFiles()) where !keys.contains(key) { keys.append(key) }
            for key in keys { onChange?(key) }
        }
    }
}
