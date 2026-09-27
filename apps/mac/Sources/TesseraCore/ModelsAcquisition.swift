import Foundation
import Observation
import TesseraFFI

// MARK: - Model acquisition (M2-51)

/// One pinned model a develop feature needs before it can run (crates/ml-runtime/models.toml).
public struct ModelRequirement: Hashable, Sendable {
    public let id: String
    public let version: String
    /// What the panel calls it ("Depth model").
    public let title: String

    public init(id: String, version: String, title: String) {
        self.id = id; self.version = version; self.title = title
    }

    /// The raw-domain CFA U-Net behind AI Denoise.
    public static let cfaDenoise = ModelRequirement(id: AIDenoise.modelID, version: AIDenoise.modelVersion,
                                                    title: "AI Denoise model")
    /// Depth Anything V2 Small: Lens Blur, its histogram and Visualize Depth (`ml_depth::MODEL_ID`).
    public static let depth = ModelRequirement(id: "depth/anything-v2-small", version: "4472b7362082ad9968fee890ca0f1e5aca36b93d",
                                               title: "Depth model")
    /// Subject focus initialises the engine's segmenter: U2Net plus the SAM encoder / decoder.
    public static let subjectSegmentation: [ModelRequirement] = [
        ModelRequirement(id: "segment/u2net", version: "7fc34deee10329bc039c10a73b98090d0c6f5c59", title: "Subject model"),
        ModelRequirement(id: "segment/sam-encoder", version: "5050a79cd4b912dd745fff83047c4ef6fbd97be5", title: "Segmentation encoder"),
        ModelRequirement(id: "segment/sam-decoder", version: "5050a79cd4b912dd745fff83047c4ef6fbd97be5", title: "Segmentation decoder"),
    ]
    /// Everything Lens Blur ▸ Subject needs.
    public static let lensBlurSubject: [ModelRequirement] = [depth] + subjectSegmentation
}

/// Where one acquisition stands: the `ModelDownloadEvent` sequence (queued → bytes → ready | failed).
public enum ModelAcquisitionState: Equatable, Sendable {
    case idle
    case queued
    case downloading(bytes: UInt64, total: UInt64?)
    case ready(path: String)
    case failed(reason: String)

    public init(_ event: ModelDownloadEvent) {
        switch event {
        case .queued: self = .queued
        case .downloading(let bytes, let total): self = .downloading(bytes: bytes, total: total)
        case .ready(let path): self = .ready(path: path)
        case .failed(let reason): self = .failed(reason: reason)
        }
    }

    public var isReady: Bool { if case .ready = self { true } else { false } }
    public var isBusy: Bool {
        switch self { case .queued, .downloading: true; default: false }
    }
    public var failure: String? { if case .failed(let r) = self { r } else { nil } }

    /// Determinate progress while the size is known.
    public var fraction: Double? {
        guard case .downloading(let bytes, let total?) = self, total > 0 else { return nil }
        return min(Double(bytes) / Double(total), 1)
    }

    /// The inline progress line: "Queued…", "Downloading 12.3 MB of 97.0 MB", "Ready", "Failed: …".
    public var label: String {
        switch self {
        case .idle: return "Not downloaded yet"
        case .queued: return "Queued…"
        case .downloading(let bytes, let total):
            let f = ByteCountFormatter()
            f.countStyle = .file
            return total.map { "Downloading \(f.string(fromByteCount: Int64(bytes))) of \(f.string(fromByteCount: Int64($0)))" }
                ?? "Downloading \(f.string(fromByteCount: Int64(bytes)))"
        case .ready: return "Ready"
        case .failed(let reason): return "Failed: \(reason)"
        }
    }

    /// Several models as one line: any failure wins, then any busy one (bytes summed), then ready
    /// when all are, else idle.
    public static func combined(_ states: [ModelAcquisitionState]) -> ModelAcquisitionState {
        if let failed = states.first(where: { $0.failure != nil }) { return failed }
        let busy = states.filter(\.isBusy)
        if !busy.isEmpty {
            var bytes: UInt64 = 0, total: UInt64? = 0, any = false
            for s in busy {
                if case .downloading(let b, let t) = s {
                    any = true
                    bytes += b
                    total = t.flatMap { t in total.map { $0 + t } }
                }
            }
            return any ? .downloading(bytes: bytes, total: total) : .queued
        }
        if !states.isEmpty, states.allSatisfy(\.isReady) { return states.count == 1 ? states[0] : .ready(path: "") }
        return .idle
    }
}

/// The engine's `ModelDownloads` surface (stubbed in tests).
public protocol ModelDownloadRequesting: AnyObject, Sendable {
    func request(id: String, version: String, listener: ModelDownloadListener) throws
}

extension ModelDownloads: ModelDownloadRequesting {}

/// App-wide model acquisition: one `ModelDownloads` for the library's support folder, a state per
/// pinned model, and the Settings ▸ AI "Allow model downloads" preference (default on). Cache hits
/// are verified and reported ready even with downloads off.
@MainActor @Observable
public final class ModelAcquisition {
    public static let allowDownloadsKey = "ModelDownloadsAllowed"

    public private(set) var states: [ModelRequirement: ModelAcquisitionState] = [:]

    /// Settings ▸ AI ▸ Allow model downloads. Changing it reopens the downloader and forgets
    /// failures so the next use retries.
    public var allowDownloads: Bool {
        didSet {
            guard allowDownloads != oldValue else { return }
            defaults.set(allowDownloads, forKey: Self.allowDownloadsKey)
            downloader = nil
            for (k, v) in states where v.failure != nil { states[k] = .idle }
        }
    }

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let open: (Bool) throws -> any ModelDownloadRequesting
    @ObservationIgnored private var downloader: (any ModelDownloadRequesting)?
    @ObservationIgnored private var waiters: [([ModelRequirement], (ModelAcquisitionState) -> Void)] = []

    /// `open(allowDownloads)` builds the downloader lazily, on the first request.
    public init(defaults: UserDefaults = .standard, open: @escaping (Bool) throws -> any ModelDownloadRequesting) {
        self.defaults = defaults
        self.open = open
        allowDownloads = defaults.object(forKey: Self.allowDownloadsKey) as? Bool ?? true
    }

    /// Downloads into `<support>/models/cache`, the cache the develop session and export read.
    /// The catalog is the engine's copy at `<support>/models/models.toml` (written when a develop
    /// session opens); `TESSERA_MODEL_MANIFEST` points at a packaged manifest instead (needed for
    /// local-artifact entries such as the CFA U-Net, which do not resolve from the copy).
    public static func standard(support: URL, environment: [String: String] = ProcessInfo.processInfo.environment,
                                defaults: UserDefaults = .standard) -> ModelAcquisition {
        let dir = support.appendingPathComponent("models", isDirectory: true)
        let manifest = environment["TESSERA_MODEL_MANIFEST"].flatMap { $0.isEmpty ? nil : $0 }
            ?? dir.appendingPathComponent("models.toml").path
        let cache = dir.appendingPathComponent("cache", isDirectory: true)
        return ModelAcquisition(defaults: defaults) { allow in
            try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true)
            guard FileManager.default.fileExists(atPath: manifest) else {
                throw ModelAcquisitionError.noManifest(manifest)
            }
            return try ModelDownloads.open(manifestPath: manifest, cachePath: cache.path, allowDownloads: allow)
        }
    }

    public static let shared = ModelAcquisition.standard(support: EngineLibrary.defaultSupportDirectory)

    public func state(_ r: ModelRequirement) -> ModelAcquisitionState { states[r] ?? .idle }
    public func state(of rs: [ModelRequirement]) -> ModelAcquisitionState { .combined(rs.map(state)) }

    /// Requests every requirement that is idle (not ready, running or failed: `reset` retries a
    /// failure); `completion` gets the combined terminal state once each one is ready or failed.
    public func acquire(_ rs: [ModelRequirement], completion: ((ModelAcquisitionState) -> Void)? = nil) {
        for r in rs {
            guard state(r) == .idle else { continue }
            states[r] = .queued
            do {
                if downloader == nil { downloader = try open(allowDownloads) }
                try downloader!.request(id: r.id, version: r.version, listener: Relay(owner: self, requirement: r))
            } catch {
                states[r] = .failed(reason: Self.describe(error))
            }
        }
        if let completion { waiters.append((rs, completion)) }
        settleWaiters()
    }

    /// `acquire` as an async call.
    public func ensure(_ rs: [ModelRequirement]) async -> ModelAcquisitionState {
        await withCheckedContinuation { cont in acquire(rs) { cont.resume(returning: $0) } }
    }

    /// Forgets a failure so the next `acquire` retries.
    public func reset(_ rs: [ModelRequirement]) {
        for r in rs where state(r).failure != nil { states[r] = .idle }
    }

    /// One engine event for `r` (on the main actor).
    public func receive(_ event: ModelDownloadEvent, for r: ModelRequirement) {
        var next = ModelAcquisitionState(event)
        if case .failed(let reason) = next, !allowDownloads {
            next = .failed(reason: reason + " (model downloads are off in Settings ▸ AI)")
        }
        // Terminal states stay (late progress from a finished request is ignored).
        if let old = states[r], old.isReady || old.failure != nil, !(next.isReady || next.failure != nil) { return }
        states[r] = next
        settleWaiters()
    }

    private func settleWaiters() {
        var keep: [([ModelRequirement], (ModelAcquisitionState) -> Void)] = []
        var done: [((ModelAcquisitionState) -> Void, ModelAcquisitionState)] = []
        for (rs, completion) in waiters {
            let each = rs.map(state)
            if each.allSatisfy({ $0.isReady || $0.failure != nil }) { done.append((completion, .combined(each))) } else { keep.append((rs, completion)) }
        }
        waiters = keep
        for (completion, s) in done { completion(s) }
    }

    nonisolated static func describe(_ error: Error) -> String {
        if case BridgeError.Failure(let message) = error { return message }
        return (error as? LocalizedError)?.errorDescription ?? "\(error)"
    }

    /// Engine worker thread → main actor. One per request (the engine's contract).
    private final class Relay: ModelDownloadListener, @unchecked Sendable {
        weak var owner: ModelAcquisition?
        let requirement: ModelRequirement
        init(owner: ModelAcquisition, requirement: ModelRequirement) { self.owner = owner; self.requirement = requirement }
        func onEvent(event: ModelDownloadEvent) {
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self.owner?.receive(event, for: self.requirement) }
            }
        }
    }
}

public enum ModelAcquisitionError: LocalizedError, Equatable {
    case noManifest(String)
    public var errorDescription: String? {
        switch self {
        case .noManifest(let path): "No model catalog at \(path) (open a photo in Develop first)"
        }
    }
}
