import Foundation
import Security

/// The account-resident records in the app's iCloud Drive container (SPEC §3, CS-2): `Tessera/`
/// under `iCloud.dev.tessera.app`, one JSON file per record. Writes go through
/// `NSFileCoordinator` and are atomic; `Watcher` runs an `NSMetadataQuery` so a change made on
/// another Mac applies live. The store is injectable with any base URL, so tests use a temp
/// directory and never need iCloud; with no container at all it is `.unavailable(reason)` and
/// every call is a no-op (the app keeps working locally).
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

        public var message: String {
            switch self {
            case .noEntitlement: "This build has no iCloud entitlement."
            case .noAccount: "This Mac is not signed in to iCloud."
            case .iCloudDriveOff: "iCloud Drive is off for Tessera."
            case .isolatedRun: "Sync is off for this run (--app-dir)."
            }
        }
        public var fix: String {
            switch self {
            case .noEntitlement: "Install a Developer ID build of Tessera (Support/release/README.md)."
            case .noAccount: "Sign in to iCloud in System Settings, then turn on iCloud Drive."
            case .iCloudDriveOff: "Turn on iCloud Drive in System Settings ▸ iCloud, and allow Tessera under it."
            case .isolatedRun: "Launch Tessera without --app-dir or TESSERA_APP_DIR."
            }
        }
    }

    public enum Availability: Equatable, Sendable {
        case available
        case unavailable(UnavailableReason)
    }

    public let availability: Availability
    /// `<container>/Tessera`; nil when unavailable.
    public let root: URL?
    /// This Mac, for `machine` stamps: a UUID made once and kept in user defaults.
    public let machine: String
    private let defaults: UserDefaults
    private let lock = NSLock()
    /// Modification dates the watcher has reported (or we wrote), per file path, so a file that
    /// did not change (or that we changed ourselves) is not reported again.
    private var seen: [String: Date] = [:]

    public init(baseURL: URL?, reason: UnavailableReason? = nil, defaults: UserDefaults = .standard) {
        self.defaults = defaults
        machine = Self.machineID(in: defaults)
        if let baseURL, reason == nil {
            root = baseURL.appendingPathComponent(Self.folderName, isDirectory: true)
            availability = .available
        } else {
            root = nil
            availability = .unavailable(reason ?? .iCloudDriveOff)
        }
    }

    /// The real container. `url(forUbiquityContainerIdentifier:)` may block while iCloud sets the
    /// container up, so call this off the main actor.
    public static func live(defaults: UserDefaults = .standard) -> AccountStore {
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

    /// Runs a metadata query over the container so a record written on another Mac is reported
    /// (and downloaded first when iCloud has only its metadata). Main-actor: `NSMetadataQuery`
    /// needs the main run loop.
    @MainActor
    public final class Watcher {
        private let store: AccountStore
        private var query: NSMetadataQuery?
        private var observers: [NSObjectProtocol] = []
        /// Called on the main actor with each changed record.
        public var onChange: ((RecordKey) -> Void)?

        public init(store: AccountStore) { self.store = store }

        public func start() {
            guard query == nil, store.isAvailable else { return }
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

        public func stop() {
            query?.stop()
            query = nil
            observers.forEach { NotificationCenter.default.removeObserver($0) }
            observers = []
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
    }
}
