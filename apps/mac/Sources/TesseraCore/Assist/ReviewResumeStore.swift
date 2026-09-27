import CryptoKit
import Foundation

/// The small, local manifest needed to restore the latest Review queue for one library.
/// Recipe provenance remains authoritative for each photo's current group and status.
public struct ReviewResumeRecord: Codable, Equatable, Sendable {
    public enum RunState: String, Codable, Sendable {
        case running, completed, partial, cancelled, interrupted, failed
    }

    public struct Target: Codable, Equatable, Sendable {
        public var imageID: String
        public var name: String
        public var ordinal: Int
        public var expectedGroupID: UInt32?
        /// A generic, non-provider error safe to keep in app support.
        public var error: String?

        public init(imageID: String, name: String, ordinal: Int, expectedGroupID: UInt32? = nil,
                    error: String? = nil) {
            self.imageID = imageID
            self.name = name
            self.ordinal = ordinal
            self.expectedGroupID = expectedGroupID
            self.error = error
        }
    }

    public static let currentSchemaVersion = 1
    public var schemaVersion: Int
    public var recordRevision: UInt64
    public var libraryPath: String
    public var queueID: UUID
    public var provider: String
    public var scope: String
    public var sourceDescription: String
    public var state: RunState
    public var startedAt: Date
    public var updatedAt: Date
    public var targets: [Target]
    public var selectedImageID: String?
    public var anchorImageID: String?

    public init(schemaVersion: Int = currentSchemaVersion, recordRevision: UInt64,
                libraryPath: String, queueID: UUID, provider: String, scope: String,
                sourceDescription: String, state: RunState, startedAt: Date, updatedAt: Date,
                targets: [Target], selectedImageID: String? = nil, anchorImageID: String? = nil) {
        self.schemaVersion = schemaVersion
        self.recordRevision = recordRevision
        self.libraryPath = libraryPath
        self.queueID = queueID
        self.provider = provider
        self.scope = scope
        self.sourceDescription = sourceDescription
        self.state = state
        self.startedAt = startedAt
        self.updatedAt = updatedAt
        self.targets = targets
        self.selectedImageID = selectedImageID
        self.anchorImageID = anchorImageID
    }

    public func advanced(state: RunState? = nil, targets: [Target]? = nil,
                         selectedImageID: String?? = nil, anchorImageID: String?? = nil,
                         now: Date = Date()) throws -> Self {
        guard recordRevision < UInt64.max else { throw ReviewResumeStoreError.revisionExhausted }
        return Self(schemaVersion: schemaVersion, recordRevision: recordRevision + 1,
             libraryPath: libraryPath, queueID: queueID, provider: provider, scope: scope,
             sourceDescription: sourceDescription, state: state ?? self.state,
             startedAt: startedAt, updatedAt: now, targets: targets ?? self.targets,
             selectedImageID: selectedImageID ?? self.selectedImageID,
             anchorImageID: anchorImageID ?? self.anchorImageID)
    }

    public func updatingCursor(selectedID: String?, anchorID: String?, now: Date = Date()) throws -> Self {
        var copy = try advanced(now: now)
        copy.selectedImageID = selectedID
        copy.anchorImageID = anchorID
        return copy
    }

    public static func reindexedTargets(_ targets: [Target]) -> [Target] {
        targets.sorted { $0.ordinal == $1.ordinal ? $0.imageID < $1.imageID : $0.ordinal < $1.ordinal }
            .enumerated().map { index, value in
                var value = value
                value.ordinal = index
                return value
            }
    }
}

public enum ReviewResumeStoreError: LocalizedError, Equatable {
    case unsupportedSchema(Int)
    case wrongLibrary
    case staleRevision(current: UInt64, attempted: UInt64)
    case corruptRecord
    case revisionExhausted

    public var errorDescription: String? {
        switch self {
        case .unsupportedSchema(let version): "Review history uses a newer format (version \(version))."
        case .wrongLibrary: "Review history belongs to a different library."
        case .staleRevision: "A newer Review history update already exists."
        case .corruptRecord: "Review history could not be read."
        case .revisionExhausted: "Review history cannot advance its revision."
        }
    }
}

/// Atomic JSON storage keyed by the canonical library path. This is local resume state, not a
/// portable library sidecar or a second authority for recipe status.
public final class ReviewResumeStore: @unchecked Sendable {
    private static let lock = NSLock()
    public let directory: URL

    public init(directory: URL) { self.directory = directory }

    public func fileURL(for libraryFolder: URL) -> URL {
        let path = Self.canonicalPath(libraryFolder)
        let digest = SHA256.hash(data: Data(path.utf8)).map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent("ReviewRuns", isDirectory: true)
            .appendingPathComponent(digest + ".json")
    }

    public func load(libraryFolder: URL) throws -> ReviewResumeRecord? {
        try Self.lock.withLock { try readStoredUnlocked(libraryFolder: libraryFolder) }
    }

    /// Called when installing a library into a new application session. It is intentionally
    /// separate from ordinary reads so status/cursor updates cannot interrupt a live run.
    public func restore(libraryFolder: URL) throws -> ReviewResumeRecord? {
        try Self.lock.withLock {
            guard let record = try readStoredUnlocked(libraryFolder: libraryFolder) else { return nil }
            guard record.state == .running else { return record }
            let interrupted = try record.advanced(state: .interrupted)
            try saveUnlocked(interrupted)
            return interrupted
        }
    }

    public func save(_ record: ReviewResumeRecord) throws {
        try Self.lock.withLock { try saveUnlocked(record) }
    }

    private func saveUnlocked(_ record: ReviewResumeRecord) throws {
            guard record.schemaVersion == ReviewResumeRecord.currentSchemaVersion,
                  record.libraryPath == Self.canonicalPath(URL(fileURLWithPath: record.libraryPath, isDirectory: true)) else {
                throw ReviewResumeStoreError.wrongLibrary
            }
            guard Self.hasUnambiguousTargets(record.targets) else { throw ReviewResumeStoreError.corruptRecord }
            let file = fileURL(for: URL(fileURLWithPath: record.libraryPath, isDirectory: true))
            if FileManager.default.fileExists(atPath: file.path) {
                guard let existing = try readStoredUnlocked(libraryFolder: URL(fileURLWithPath: record.libraryPath, isDirectory: true)) else {
                    throw ReviewResumeStoreError.corruptRecord
                }
                guard record.recordRevision > existing.recordRevision else {
                    throw ReviewResumeStoreError.staleRevision(current: existing.recordRevision,
                                                               attempted: record.recordRevision)
                }
            }
            try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            let data = try encoder.encode(record)
            try data.write(to: file, options: .atomic)
    }

    public static func canonicalPath(_ folder: URL) -> String {
        folder.standardizedFileURL.resolvingSymlinksInPath().path
    }

    private func readStoredUnlocked(libraryFolder: URL) throws -> ReviewResumeRecord? {
        let file = fileURL(for: libraryFolder)
        guard FileManager.default.fileExists(atPath: file.path) else { return nil }
        let data = try Data(contentsOf: file)
        let schema: Int
        do {
            guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let value = object["schemaVersion"] as? Int else { throw ReviewResumeStoreError.corruptRecord }
            schema = value
        } catch let error as ReviewResumeStoreError { throw error }
        catch { throw ReviewResumeStoreError.corruptRecord }
        guard schema == ReviewResumeRecord.currentSchemaVersion else {
            throw ReviewResumeStoreError.unsupportedSchema(schema)
        }
        let record: ReviewResumeRecord
        do { record = try JSONDecoder().decode(ReviewResumeRecord.self, from: data) }
        catch { throw ReviewResumeStoreError.corruptRecord }
        guard record.libraryPath == Self.canonicalPath(libraryFolder) else {
            throw ReviewResumeStoreError.wrongLibrary
        }
        guard Self.hasUnambiguousTargets(record.targets) else { throw ReviewResumeStoreError.corruptRecord }
        return record
    }

    private static func hasUnambiguousTargets(_ targets: [ReviewResumeRecord.Target]) -> Bool {
        let ids = targets.map(\.imageID)
        let ordinals = targets.map(\.ordinal)
        return ids.allSatisfy { !$0.isEmpty } && ordinals.allSatisfy { $0 >= 0 }
            && Set(ids).count == ids.count && Set(ordinals).count == ordinals.count
    }
}
