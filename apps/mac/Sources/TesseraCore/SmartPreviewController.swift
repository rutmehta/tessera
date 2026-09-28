import Foundation
import Observation

/// Original is the default until the proxy route has demonstrated a user benefit.
/// Reading never rewrites an explicit stored preference (including an earlier opt-in).
@MainActor
public enum SmartPreviewPreference {
    public static let key = "UseSmartPreviews"
    public static func read(from defaults: UserDefaults = .standard) -> Bool {
        defaults.object(forKey: key) as? Bool ?? false
    }
}

public struct SmartPreviewSnapshot: Sendable, Equatable {
    public enum State: Sendable, Equatable { case missing, ready, originalOffline, dirty, stale, failed, conflict }
    public let imageID: String
    public let state: State
    public let originalAvailable: Bool
    public let dirty: Bool
    public let width: UInt32
    public let height: UInt32
    public let message: String

    public init(imageID: String, state: State, originalAvailable: Bool, dirty: Bool,
                width: UInt32, height: UInt32, message: String) {
        self.imageID = imageID; self.state = state; self.originalAvailable = originalAvailable
        self.dirty = dirty; self.width = width; self.height = height; self.message = message
    }
    public static let libraryThumbnailNotice = "Library thumbnails show the last synchronized image. Smart Preview edits appear there only after Sync."
    public var libraryBadge: String {
        badge + (state != .missing && (hasPendingEdits || !originalAvailable)
                 ? " · Thumbnail: last synchronized image" : "")
    }
    public var hasPendingEdits: Bool { dirty || state == .dirty }
    public var needsAttention: Bool { state == .stale || state == .conflict || state == .failed }
    public var badge: String {
        var parts = [state == .missing ? "No Smart Preview" : "Smart Preview"]
        if !originalAvailable { parts.append("Original offline") }
        if hasPendingEdits { parts.append("Pending edits") }
        switch state {
        case .stale: parts.append("Stale")
        case .conflict: parts.append("Conflict")
        case .failed: parts.append("Failed")
        default: break
        }
        return parts.joined(separator: " · ")
    }
}

public enum DevelopSourceRoute: Sendable, Equatable {
    case original, smartPreview
    public var label: String { self == .original ? "Original" : "Smart Preview" }
}

public enum SmartPreviewUIError: LocalizedError {
    case unavailable(String)
    public var errorDescription: String? { if case .unavailable(let text) = self { text } else { nil } }
}

/// A missing, clean preview may use the original. Failed opens never silently retry a
/// different source, and an explicit Original choice cannot bypass a pending journal.
@MainActor
public enum SmartPreviewRouting {
    public static func route(_ info: SmartPreviewSnapshot, preferPreview: Bool) throws -> DevelopSourceRoute {
        guard !info.needsAttention else {
            throw SmartPreviewUIError.unavailable(info.message.isEmpty ? info.badge : info.message)
        }
        if preferPreview, info.state != .missing { return .smartPreview }
        guard info.originalAvailable else {
            if info.state != .missing {
                throw SmartPreviewUIError.unavailable("Original offline. Choose Library → Smart Previews → Use Smart Preview to edit the existing preview.")
            }
            throw SmartPreviewUIError.unavailable("Original offline and no Smart Preview is available. Reconnect the original to build one.")
        }
        guard !info.hasPendingEdits else {
            throw SmartPreviewUIError.unavailable("Synchronize pending Smart Preview edits before using Original")
        }
        return .original
    }

    public static func open<T>(_ info: SmartPreviewSnapshot, preferPreview: Bool,
                               using opener: @MainActor (DevelopSourceRoute) async throws -> T) async throws -> T {
        try await opener(route(info, preferPreview: preferPreview))
    }
}

/// Injectable actor-isolated boundary. Live implementations MUST dispatch blocking
/// native work off MainActor; deterministic tests may suspend with continuations.
@MainActor
public struct SmartPreviewAPI {
    public var info: @MainActor (String) async throws -> SmartPreviewSnapshot
    public var build: @MainActor (String) async throws -> SmartPreviewSnapshot
    public var discard: @MainActor (String) async throws -> Void
    public var synchronize: @MainActor (String) async throws -> SmartPreviewSnapshot
    public init(info: @escaping @MainActor (String) async throws -> SmartPreviewSnapshot,
                build: @escaping @MainActor (String) async throws -> SmartPreviewSnapshot,
                discard: @escaping @MainActor (String) async throws -> Void,
                synchronize: @escaping @MainActor (String) async throws -> SmartPreviewSnapshot) {
        self.info = info; self.build = build; self.discard = discard; self.synchronize = synchronize
    }
}

public struct SmartPreviewTarget: Sendable, Identifiable {
    public let id: String
    public let name: String
    public init(id: String, name: String) { self.id = id; self.name = name }
}

public struct SmartPreviewOutcome: Identifiable {
    public let id: String
    public let name: String
    public let succeeded: Bool
    public let message: String
}

/// Presentation-only evidence from a successful save on a known proxy session.
/// Never used by routing or native admission; original availability is historical.
private struct SmartPreviewLocalSavePresentation {
    let originalLastCheckedOffline: Bool
    func badge(source: EnginePreviewSource) -> String {
        "Smart Preview · Local edits saved · Status needs refresh"
            + (source == .smartPreview ? " · Thumbnail source: Smart Preview" : " · Thumbnail: last synchronized image")
            + (originalLastCheckedOffline ? " · Original last checked offline" : "")
    }
}

@MainActor @Observable
public final class SmartPreviewController {
    public enum Action: String, CaseIterable { case build = "Build", discard = "Discard", synchronize = "Sync" }
    public private(set) var snapshots: [String: SmartPreviewSnapshot] = [:]
    private var localSavePresentations: [String: SmartPreviewLocalSavePresentation] = [:]
    public private(set) var selectedImageID: String?
    public var selectedInfo: SmartPreviewSnapshot? { selectedImageID.flatMap { snapshots[$0] } }
    public var selectedPresentationWarning: String? {
        guard let id = selectedImageID, snapshots[id] == nil else { return nil }
        return localSavePresentations[id]?.badge(source: thumbnailSource)
    }
    public func libraryBadge(imageID: String) -> String? {
        guard let info = snapshots[imageID] else { return localSavePresentations[imageID]?.badge(source: thumbnailSource) }
        if thumbnailSource == .smartPreview {
            // Identify the renderer, not an assertion that a pending/failed image is current.
            return info.badge + " · Thumbnail source: Smart Preview"
        }
        // A failed/stale/conflicting recheck cannot establish thumbnail freshness.
        let needsSavedWarning = localSavePresentations[imageID] != nil
            && !info.hasPendingEdits && info.originalAvailable
        return info.libraryBadge + (needsSavedWarning ? " · Thumbnail: last synchronized image" : "")
    }

    private func acceptStatus(_ info: SmartPreviewSnapshot) {
        snapshots[info.imageID] = info
        // Keep the save evidence until fresh state establishes clean synchronization
        // (or clean removal); a dirty/offline/conflicting refresh cannot clear it.
        if !info.needsAttention, !info.hasPendingEdits,
           info.state == .missing || (info.state == .ready && info.originalAvailable) {
            localSavePresentations[info.imageID] = nil
        }
    }

    public func didSave(imageID: String, source: DevelopSourceRoute) {
        if source == .smartPreview {
            let lastOffline = snapshots[imageID].map { !$0.originalAvailable }
                ?? localSavePresentations[imageID]?.originalLastCheckedOffline ?? false
            localSavePresentations[imageID] = .init(originalLastCheckedOffline: lastOffline)
        }
        // Keep presentation separate; routing must wait for fresh validation.
        invalidateStatus(imageID: imageID)
    }
    public private(set) var selectionError: String?
    public private(set) var results: [SmartPreviewOutcome] = []
    public private(set) var isRunning = false
    public private(set) var cancelRequested = false
    public private(set) var activeName: String?
    public private(set) var total = 0
    @ObservationIgnored private let api: SmartPreviewAPI?
    @ObservationIgnored private var selectionGeneration = UUID()
    @ObservationIgnored private var selectionIdentity = UUID()
    @ObservationIgnored private var selectionTask: Task<Void, Never>?
    @ObservationIgnored private var selectionNeedsRead = true
    @ObservationIgnored private var revisions: [String: UUID] = [:]
    @ObservationIgnored public var onChange: ((String) -> Void)?

    @ObservationIgnored public var onThumbnailInvalidation: ((String) -> Void)?
    public let thumbnailSource: EnginePreviewSource
    public var libraryThumbnailNotice: String {
        thumbnailSource == .smartPreview
            ? "Cached Library thumbnails render validated Smart Previews with local saved edits. Invalid or missing previews may have no thumbnail."
            : SmartPreviewSnapshot.libraryThumbnailNotice
    }
    public init(api: SmartPreviewAPI? = nil, thumbnailSource: EnginePreviewSource = .original) {
        self.api = api; self.thumbnailSource = thumbnailSource
    }
    public var progressLabel: String {
        if isRunning {
            if cancelRequested { return "Stopping after current photo · \(results.count)/\(total)" }
            return "\(results.count)/\(total) · \(activeName ?? "Preparing")"
        }
        return results.isEmpty ? "No batch started" : "\(results.filter(\.succeeded).count)/\(total) succeeded"
    }

    /// Full asset validation is expensive: keep one active selection read, let it
    /// drain, then read only the latest queued selection. Repeated notifications and
    /// the Develop opener share the same result. No native work from cell drawing.
    @discardableResult
    public func select(imageID: String?, refresh: Bool = false) -> Task<Void, Never>? {
        if selectedImageID == imageID, !refresh, !selectionNeedsRead { return selectionTask }
        let previous = selectionTask
        previous?.cancel() // stops queued work, not a native call already in progress
        if selectedImageID != imageID { selectionIdentity = UUID() }
        selectedImageID = imageID; selectionError = nil
        let generation = UUID(); selectionGeneration = generation
        selectionNeedsRead = true
        guard let imageID, let api, !isRunning else { return nil }
        snapshots[imageID] = nil
        onChange?(imageID)
        selectionNeedsRead = false
        let revision = revisions[imageID]
        let task = Task { [weak self] in
            await previous?.value
            guard !Task.isCancelled, let self, self.selectionGeneration == generation else { return }
            do {
                let info = try await api.info(imageID)
                guard !Task.isCancelled, self.selectionGeneration == generation,
                      self.revisions[imageID] == revision else { return }
                guard info.imageID == imageID else { throw SmartPreviewUIError.unavailable("Smart Preview identity mismatch") }
                self.acceptStatus(info); self.onChange?(imageID)
            } catch {
                guard !Task.isCancelled, self.selectionGeneration == generation,
                      self.revisions[imageID] == revision else { return }
                self.snapshots[imageID] = nil
                self.selectionError = error.localizedDescription
                self.onChange?(imageID)
            }
        }
        selectionTask = task
        return task
    }

    /// The snapshot chooses the requested route; native open must independently
    /// validate current source/journal identity and writer admission. An error never
    /// triggers Original fallback. Save callbacks invalidate without hashing again.
    public func statusForOpening(imageID: String) async throws -> SmartPreviewSnapshot {
        try await statusForOpening(imageID: imageID, willWait: {})
    }

    /// Internal deterministic test seam fires only after capturing the awaited task
    /// and generation. The production entry point supplies a no-op observer.
    func statusForOpening(imageID: String, willWait: @MainActor () -> Void) async throws -> SmartPreviewSnapshot {
        let identity = selectionIdentity
        while true {
            try Task.checkCancellation()
            guard selectedImageID == imageID, selectionIdentity == identity else { throw CancellationError() }
            guard !isRunning else {
                throw SmartPreviewUIError.unavailable("Wait for the Smart Preview operation before opening this photo")
            }
            let task = select(imageID: imageID)
            let generation = selectionGeneration
            willWait()
            await task?.value
            try Task.checkCancellation()
            guard selectedImageID == imageID, selectionIdentity == identity else { throw CancellationError() }
            // Check Status may replace a same-photo read while this await drains.
            // Follow that actual replacement; do not mistake its missing snapshot
            // for failure, start a duplicate read, or revive an older selection.
            guard selectionGeneration == generation else { continue }
            guard !isRunning, let info = snapshots[imageID] else {
                throw SmartPreviewUIError.unavailable(selectionError ?? "Check Smart Preview status before opening this photo")
            }
            return info
        }
    }

    public func invalidateStatus(imageID: String) {
        revisions[imageID] = UUID()
        snapshots[imageID] = nil
        if selectedImageID == imageID {
            selectionTask?.cancel()
            selectionGeneration = UUID()
            selectionNeedsRead = true
            selectionError = nil
        }
        onChange?(imageID)
    }

    public func cancel() { if isRunning { cancelRequested = true } }

    public func run(_ action: Action, targets: [SmartPreviewTarget]) async {
        guard !isRunning, let api else { return }
        var seen = Set<String>()
        let targets = targets.filter { seen.insert($0.id).inserted }
        guard !targets.isEmpty else { return }
        isRunning = true; cancelRequested = false; results = []; total = targets.count
        selectionTask?.cancel()
        selectionGeneration = UUID()
        selectionNeedsRead = true
        // A queued batch must not overlap a full asset validation already running.
        await selectionTask?.value
        defer { isRunning = false; activeName = nil }
        for target in targets {
            if cancelRequested || Task.isCancelled {
                results.append(.init(id: target.id, name: target.name, succeeded: false, message: "Cancelled before starting"))
                continue
            }
            activeName = target.name
            revisions[target.id] = UUID() // invalidate older selection reads
            onThumbnailInvalidation?(target.id)
            do {
                let info: SmartPreviewSnapshot
                switch action {
                case .build: info = try await api.build(target.id)
                case .synchronize: info = try await api.synchronize(target.id)
                case .discard:
                    let before = try await api.info(target.id)
                    guard before.imageID == target.id, !before.hasPendingEdits else {
                        throw SmartPreviewUIError.unavailable("Pending Smart Preview edits must be synchronized before discard")
                    }
                    // Native discard must also enforce this atomically; this UI check is not a lock.
                    try await api.discard(target.id)
                    info = try await api.info(target.id)
                }
                guard info.imageID == target.id else { throw SmartPreviewUIError.unavailable("Smart Preview identity mismatch") }
                acceptStatus(info)
                if selectedImageID == target.id { selectionNeedsRead = false; selectionError = nil }
                let success: Bool
                switch action {
                case .build: success = !info.needsAttention && info.state != .missing
                case .synchronize: success = !info.needsAttention && !info.hasPendingEdits && info.originalAvailable && info.state == .ready
                case .discard: success = !info.hasPendingEdits && info.state == .missing
                }
                results.append(.init(id: target.id, name: target.name, succeeded: success,
                                     message: info.message.isEmpty ? info.badge : info.message))
            } catch {
                snapshots[target.id] = nil // old ready badges must not mask a failed operation
                if selectedImageID == target.id { selectionNeedsRead = false; selectionError = error.localizedDescription }
                results.append(.init(id: target.id, name: target.name, succeeded: false, message: error.localizedDescription))
            }
            revisions[target.id] = UUID() // suppress reads started during the native operation
            // Also retire work started during the mutation, including partial native failure.
            onThumbnailInvalidation?(target.id)
            onChange?(target.id)
        }
    }
}
