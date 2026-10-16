import Foundation

// Offline editing (docs/design/offline/SPEC.md, OF-2): the app-side contract the background
// builder, Settings ▸ Offline, the folder popover and the reconnect sync are written against.
// The engine half (OF-1) lands separately; `OfflineEngineMethods` is the seam it plugs into.

/// Mirrors the engine's `OfflineCoverage` record: how much of one folder is editable offline.
public struct OfflineCoverage: Sendable, Equatable {
    public let folder: String
    /// Mosaic RAW originals under `folder` (recursive).
    public let raws: UInt64
    /// Of those, with a local preview that is not stale or conflicting.
    public let ready: UInt64
    /// Local previews with journaled edits not yet written to the sidecars.
    public let pendingSync: UInt64
    /// Stale, conflict, failed.
    public let needsAttention: UInt64
    /// On-disk size of the previews counted in `ready` + `pendingSync` + `needsAttention`.
    public let bytes: UInt64

    public init(folder: String, raws: UInt64, ready: UInt64, pendingSync: UInt64, needsAttention: UInt64, bytes: UInt64) {
        self.folder = folder; self.raws = raws; self.ready = ready
        self.pendingSync = pendingSync; self.needsAttention = needsAttention; self.bytes = bytes
    }

    /// Previews on disk for this folder, whatever their state.
    public var previews: UInt64 { ready + pendingSync + needsAttention }
    /// "180 of 226 editable offline" (the popover row detail and the Settings tab).
    public var editableText: String { OfflineCopy.editable(ready: ready + pendingSync, raws: raws) }
}

/// The five OF-1 engine calls, named so they cannot collide with the generated bindings.
/// `Engine` conforms once `TesseraFFI.swift` carries the methods (see `SmartPreviewNative.swift`).
/// Blocking: call off the main actor.
public protocol OfflineEngineMethods: AnyObject, Sendable {
    func offlineCoverageRecord(folder: String) throws -> OfflineCoverage
    func offlineCandidateIDs(folder: String, limit: UInt32) throws -> [String]
    func ensureSmartPreviewSnapshot(imageID: String) throws -> SmartPreviewSnapshot
    func pendingSyncSmartPreviewIDs() throws -> [String]
    func smartPreviewStorageBytes() throws -> UInt64
}

/// Every sentence a person can read about offline editing, in one place (tests pin them).
/// No "declarations", "cached" or "catalog read-only" anywhere.
public enum OfflineCopy {
    /// What to call the drive when its name is unknown.
    public static func driveName(_ volume: String?) -> String {
        volume.map { "“\($0)”" } ?? "The drive"
    }

    /// The banner over an offline library.
    public static func banner(volume: String?) -> String {
        "\(driveName(volume)) isn't attached · editing offline previews · culling resumes when it's back"
    }

    public static let bannerHelp = "Edits to offline previews are kept and written to the drive when it is attached again. Decisions, albums and metadata wait for the drive."

    /// The empty-state title and message of an offline library with nothing to show.
    public static let emptyTitle = "No offline previews"
    public static func emptyMessage(volume: String?) -> String {
        "No offline previews for this folder yet. Attach \(driveName(volume)) and Tessera makes them in the background."
    }

    /// Edit refused: the original is offline and there is no preview to open.
    public static func missingPreview(volume: String?) -> String {
        "\(driveName(volume)) isn't attached and this photo has no offline preview yet. Previews are made in the background while the drive is attached (Settings ▸ Offline)."
    }

    /// The Edit header's position line while the preview, not the original, is open.
    public static let editingOfflinePreview = "Editing offline preview"

    /// The status line after an offline library opens.
    public static func openedOffline(volume: String?, previews: Int) -> String {
        "\(driveName(volume)) isn't attached · \(previews.formatted()) offline preview\(previews == 1 ? "" : "s") ready to edit"
    }

    /// "180 of 226 editable offline".
    public static func editable(ready: UInt64, raws: UInt64) -> String {
        "\(ready.formatted()) of \(raws.formatted()) editable offline"
    }

    public static let budgetReached = "budget reached"

    /// What a decision made offline gets instead of a write.
    public static func decisionsWait(volume: String?) -> String {
        "Decisions wait until \(driveName(volume)) is attached again; edits to offline previews are kept."
    }

    /// The reconnect toast: "Wrote 3 offline edits to the drive", or the failure count with where to look.
    public static func syncToast(wrote: Int, failed: Int) -> String? {
        if wrote == 0, failed == 0 { return nil }
        var parts: [String] = []
        if wrote > 0 { parts.append("Wrote \(wrote) offline edit\(wrote == 1 ? "" : "s") to the drive") }
        if failed > 0 { parts.append("\(failed) edit\(failed == 1 ? "" : "s") could not be written · see Library ▸ Smart Previews") }
        return parts.joined(separator: " · ")
    }
}

/// Journaled offline edits are written to the sidecars when the original is reachable again:
/// pending ids ∩ the open library, one at a time, never for a photo whose Develop session is open.
public enum OfflineEditSync {
    public struct Outcome: Sendable, Equatable {
        public var wrote = 0
        public var failed = 0
        /// Pending photos left alone because their editor was open.
        public var skipped = 0
        public init(wrote: Int = 0, failed: Int = 0, skipped: Int = 0) {
            self.wrote = wrote; self.failed = failed; self.skipped = skipped
        }
        public var toast: String? { OfflineCopy.syncToast(wrote: wrote, failed: failed) }
    }

    /// `isOpen`: a Develop session for the photo is open (that photo waits for the next reconnect).
    /// `shouldContinue`: the library is still the one the pending list was read for.
    /// `didSynchronize`: the fresh status for a photo (ready or failed), for badges and thumbnails.
    @MainActor
    public static func run(api: SmartPreviewAPI, libraryIDs: Set<String>,
                           isOpen: @MainActor (String) -> Bool,
                           shouldContinue: @MainActor () -> Bool = { true },
                           didSynchronize: @MainActor (String, SmartPreviewSnapshot?) -> Void = { _, _ in }) async -> Outcome {
        var outcome = Outcome()
        guard let pending = try? await api.pendingSync() else { return outcome }
        for id in pending where libraryIDs.contains(id) {
            guard shouldContinue() else { break }
            if isOpen(id) { outcome.skipped += 1; continue }
            do {
                let info = try await api.synchronize(id)
                let clean = info.imageID == id && !info.needsAttention && !info.hasPendingEdits && info.originalAvailable
                if clean { outcome.wrote += 1 } else { outcome.failed += 1 }
                didSynchronize(id, info)
            } catch {
                outcome.failed += 1
                didSynchronize(id, nil)
            }
        }
        return outcome
    }
}

/// Settings ▸ Offline: the mode and the disk budget, with the spec's defaults.
public enum OfflinePreviewsMode: String, CaseIterable, Sendable {
    case everyRaw, keepersOnly, off
    public var title: String {
        switch self {
        case .everyRaw: "Every RAW in the folders you open"
        case .keepersOnly: "Keepers only"
        case .off: "Off"
        }
    }
}

public enum OfflinePreviewsSettings {
    public static let modeKey = "OfflinePreviewsMode"
    public static let budgetKey = "OfflinePreviewsBudgetBytes"
    public static let gigabyte: UInt64 = 1_000_000_000
    /// The budget's ceiling and its share of the boot volume's free space.
    public static let budgetCap: UInt64 = 25 * gigabyte
    public static let budgetShareOfFreeSpace: UInt64 = 10
    /// What one preview costs (a 33 MP ARW at 2048 px, f16 + zstd), for "fits about N photos".
    public static let bytesPerPreview: UInt64 = 10_000_000
    /// Headroom the builder keeps: it stops when `storage + headroom > budget`.
    public static let headroomBytes: UInt64 = 12_000_000

    public static func readMode(from defaults: UserDefaults = .standard) -> OfflinePreviewsMode {
        defaults.string(forKey: modeKey).flatMap(OfflinePreviewsMode.init(rawValue:)) ?? .everyRaw
    }

    public static func write(_ mode: OfflinePreviewsMode, to defaults: UserDefaults = .standard) {
        defaults.set(mode.rawValue, forKey: modeKey)
    }

    /// `min(25 GB, 10 % of the free space on the boot volume)`.
    public static func defaultBudget(freeBytes: UInt64) -> UInt64 {
        min(budgetCap, freeBytes / budgetShareOfFreeSpace)
    }

    /// Free space on the boot volume (what the system would let an important download take).
    public static func bootVolumeFreeBytes() -> UInt64 {
        let values = try? URL(fileURLWithPath: "/").resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
        return UInt64(max(0, values?.volumeAvailableCapacityForImportantUsage ?? 0))
    }

    /// A stored budget wins; otherwise the default is computed from `freeBytes` and not stored.
    public static func readBudget(from defaults: UserDefaults = .standard,
                                  freeBytes: @autoclosure () -> UInt64 = bootVolumeFreeBytes()) -> UInt64 {
        if let stored = defaults.object(forKey: budgetKey) as? NSNumber, stored.uint64Value > 0 {
            return stored.uint64Value
        }
        return defaultBudget(freeBytes: freeBytes())
    }

    public static func write(budget: UInt64, to defaults: UserDefaults = .standard) {
        defaults.set(NSNumber(value: budget), forKey: budgetKey)
    }

    /// "fits about 2,500 photos".
    public static func fitsText(budget: UInt64) -> String {
        "fits about \((budget / bytesPerPreview).formatted()) photos"
    }

    /// Whether another preview fits: `storage + headroom > budget` is the stop.
    public static func fits(storage: UInt64, budget: UInt64) -> Bool {
        storage + headroomBytes <= budget
    }
}
