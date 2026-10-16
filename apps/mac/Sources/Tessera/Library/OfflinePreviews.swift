import Foundation
import Observation
import TesseraCore

/// OF-2: makes an offline preview for every RAW in the open folder while its drive is attached,
/// in the background, keeps and newest first, inside a disk budget (docs/design/offline/SPEC.md).
///
/// One controller per library (`AppModel.install` makes it next to `SmartPreviewController`).
/// `start()` waits `Timing.startDelay`, then builds one photo at a time through the injected
/// `SmartPreviewAPI.ensure`, re-checking the budget before each. It waits while the host's
/// `gate` names a reason (a photo opening, an export or print, a menu Smart Preview batch, the
/// culling measurement) and for `Timing.frameQuiet` after a loupe frame, re-checking every
/// `Timing.idleRetry`; `buildNow()` runs the same queue ignoring those gates (the budget still
/// holds). `cancel()` stops it for good when the library changes.
@MainActor @Observable
final class OfflinePreviews {
    struct Timing: Sendable {
        var startDelay: Duration = .seconds(3)
        var idleRetry: Duration = .seconds(2)
        var frameQuiet: Duration = .seconds(2)
        static let live = Timing()
    }

    /// The folder's coverage (refreshed when a photo finishes).
    private(set) var coverage: OfflineCoverage?
    /// Coverage of detached roots by path, for the folder popover rows.
    private(set) var rootCoverage: [String: OfflineCoverage] = [:]
    private(set) var isBuilding = false
    /// The image id being built.
    private(set) var current: String?
    private(set) var budget: UInt64
    private(set) var mode: OfflinePreviewsMode
    /// Bytes under `smart-previews/` (all folders).
    private(set) var storageBytes: UInt64?
    private(set) var lastError: String?
    /// Why the queue is waiting, while it is.
    private(set) var waitingReason: String?
    private(set) var budgetReached = false
    /// The queue drained: every candidate has a preview or was skipped.
    private(set) var finished = false
    /// Previews made by this controller.
    private(set) var built = 0

    @ObservationIgnored private let api: SmartPreviewAPI?
    @ObservationIgnored let folder: String?
    /// The original folder is reachable (nothing can be built from an offline library).
    @ObservationIgnored let originalAvailable: Bool
    @ObservationIgnored private let isKeeper: @MainActor (String) -> Bool
    @ObservationIgnored private let gate: @MainActor () -> String?
    @ObservationIgnored private let timing: Timing
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var lastFrame: ContinuousClock.Instant?
    /// Candidates that failed or were refused: not asked for again this session.
    @ObservationIgnored private var skipped: Set<String> = []
    @ObservationIgnored private var cancelled = false
    /// A photo's preview changed: the host refreshes its status badge and thumbnail.
    @ObservationIgnored var onPhotoBuilt: (@MainActor (String) -> Void)?

    init(api: SmartPreviewAPI? = nil, folder: String? = nil, originalAvailable: Bool = true,
         isKeeper: @escaping @MainActor (String) -> Bool = { _ in false },
         gate: @escaping @MainActor () -> String? = { nil },
         timing: Timing = .live, defaults: UserDefaults = .standard,
         freeBytes: UInt64 = OfflinePreviewsSettings.bootVolumeFreeBytes()) {
        self.api = api; self.folder = folder; self.originalAvailable = originalAvailable
        self.isKeeper = isKeeper; self.gate = gate; self.timing = timing; self.defaults = defaults
        mode = OfflinePreviewsSettings.readMode(from: defaults)
        budget = OfflinePreviewsSettings.readBudget(from: defaults, freeBytes: freeBytes)
    }

    /// There is a folder on an attached drive to make previews for.
    var canBuild: Bool { api != nil && folder != nil && originalAvailable }
    var isRunning: Bool { task != nil }

    /// "180 of 226 editable offline", with "budget reached" while the budget stops the queue.
    var coverageText: String? {
        guard let coverage else { return nil }
        return coverage.editableText + (budgetReached ? " · " + OfflineCopy.budgetReached : "")
    }

    /// One line for Settings ▸ Offline.
    var statusText: String {
        if let lastError, !isBuilding, waitingReason == nil { return "Last problem: \(lastError)" }
        if isBuilding { return "Making preview \(built + 1)" + (coverage.map { " · \($0.editableText)" } ?? "") }
        if let waitingReason { return "Waiting: \(waitingReason)" }
        if budgetReached {
            return "Budget reached · \(Self.bytesText(storageBytes ?? 0)) used of \(Self.bytesText(budget))"
        }
        guard canBuild else { return "Open a folder on an attached drive to make previews" }
        if mode == .off { return "Off · previews are made only with “Make this folder available offline now”" }
        if finished { return "Up to date" + (coverage.map { " · \($0.editableText)" } ?? "") }
        return isRunning ? "Starting soon" : "Idle"
    }

    /// "Used: 2.3 GB · 226 previews".
    var usageText: String {
        let bytes = Self.bytesText(storageBytes ?? coverage?.bytes ?? 0)
        guard let coverage else { return "Used: \(bytes)" }
        return "Used: \(bytes) · \(coverage.previews.formatted()) preview\(coverage.previews == 1 ? "" : "s")"
    }

    static func bytesText(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }

    // MARK: Lifecycle

    /// Called once the folder is installed: the queue starts after `Timing.startDelay` (and after
    /// the gates clear). Coverage is read either way, so an offline library still reports it.
    func start() {
        guard !cancelled else { return }
        refreshCoverage()
        guard canBuild, task == nil, mode != .off else { return }
        task = Task { @MainActor [weak self, timing] in
            try? await Task.sleep(for: timing.startDelay)
            guard let self, !Task.isCancelled else { return }
            await self.run(ignoringGates: false)
            self.task = nil
        }
    }

    /// Settings ▸ "Make this folder available offline now": the queue runs at once, ignoring the
    /// idle gates and the mode, still inside the budget.
    func buildNow() {
        guard !cancelled, canBuild else { return }
        task?.cancel()
        task = Task { @MainActor [weak self] in
            guard let self else { return }
            await self.run(ignoringGates: true)
            self.task = nil
        }
    }

    /// The library changed: nothing more is built on this controller.
    func cancel() {
        cancelled = true
        task?.cancel()
        task = nil
        isBuilding = false
        current = nil
        waitingReason = nil
    }

    /// A loupe frame was drawn: the queue stays quiet for `Timing.frameQuiet`.
    func noteLoupeFrame() { lastFrame = .now }

    func setMode(_ newMode: OfflinePreviewsMode) {
        guard mode != newMode else { return }
        mode = newMode
        OfflinePreviewsSettings.write(newMode, to: defaults)
        finished = false
        if newMode == .off {
            task?.cancel(); task = nil
        } else if task == nil {
            start()
        }
    }

    func setBudget(_ bytes: UInt64) {
        guard budget != bytes else { return }
        budget = bytes
        OfflinePreviewsSettings.write(budget: bytes, to: defaults)
        if budgetReached, task == nil { budgetReached = false; start() }
    }

    /// Coverage and storage for the folder (and the Settings tab), off the main actor.
    func refreshCoverage() {
        guard let api else { return }
        let folder = folder
        Task { @MainActor [weak self] in
            if let folder, let coverage = try? await api.coverage(folder) {
                guard let self, !self.cancelled else { return }
                self.coverage = coverage
            }
            if let storage = try? await api.storage() {
                guard let self, !self.cancelled else { return }
                self.storageBytes = storage
            }
        }
    }

    /// Coverage for roots whose drive is not attached (the folder popover rows).
    func refreshRootCoverage(paths: [String]) {
        guard let api else { return }
        Task { @MainActor [weak self] in
            for path in paths {
                guard let coverage = try? await api.coverage(path) else { continue }
                guard let self, !self.cancelled else { return }
                self.rootCoverage[path] = coverage
            }
        }
    }

    // MARK: The queue

    private func blockedReason() -> String? {
        if let reason = gate() { return reason }
        if let lastFrame, ContinuousClock.now - lastFrame < timing.frameQuiet { return "the loupe is drawing" }
        return nil
    }

    private func run(ignoringGates: Bool) async {
        guard let api, let folder else { return }
        defer { isBuilding = false; current = nil; waitingReason = nil }
        while !Task.isCancelled, !cancelled {
            if !ignoringGates, let reason = blockedReason() {
                waitingReason = reason
                try? await Task.sleep(for: timing.idleRetry)
                continue
            }
            waitingReason = nil
            let storage: UInt64
            do { storage = try await api.storage() } catch { lastError = error.localizedDescription; return }
            guard !Task.isCancelled, !cancelled else { return }
            storageBytes = storage
            guard OfflinePreviewsSettings.fits(storage: storage, budget: budget) else { budgetReached = true; return }
            budgetReached = false
            let next: String?
            do { next = try await nextCandidate(api: api, folder: folder, ignoringMode: ignoringGates) }
            catch { lastError = error.localizedDescription; return }
            guard !Task.isCancelled, !cancelled else { return }
            guard let next else { finished = true; return }
            isBuilding = true
            current = next
            do {
                let info = try await api.ensure(next)
                if info.imageID != next || info.state == .missing || info.needsAttention {
                    skipped.insert(next)
                    lastError = info.message.isEmpty ? info.badge : info.message
                } else {
                    built += 1
                    lastError = nil
                }
            } catch {
                skipped.insert(next)
                lastError = error.localizedDescription
            }
            isBuilding = false
            current = nil
            guard !cancelled else { return }
            onPhotoBuilt?(next)
            if let coverage = try? await api.coverage(folder), !cancelled { self.coverage = coverage }
        }
    }

    /// The next RAW without a usable preview (keeps first, then newest). With `.keepersOnly` the
    /// queue ends at the first candidate that is not a keep; `ignoringMode` (build now) takes all.
    private func nextCandidate(api: SmartPreviewAPI, folder: String, ignoringMode: Bool) async throws -> String? {
        let ids = try await api.candidates(folder, UInt32(clamping: skipped.count + 1))
        guard let id = ids.first(where: { !skipped.contains($0) }) else { return nil }
        if !ignoringMode, mode == .keepersOnly, !isKeeper(id) { return nil }
        return id
    }
}
