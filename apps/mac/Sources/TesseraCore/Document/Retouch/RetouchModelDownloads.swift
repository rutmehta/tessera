import Foundation
import Observation

// MARK: - Retouch model downloads (WP B5-09b)

/// Where one retouch model stands for the UI: the Remove options bar (LaMa) and the Neural Filters sheet
/// (DDColor, DRUNet) show this inline, next to the control it gates.
public enum RetouchDownloadPhase: Equatable, Sendable {
    /// Installed (or nothing needed).
    case installed
    /// Missing; downloads are allowed: offer "Download", or download when the operation is asked for.
    case available(RetouchModelInfo)
    /// Missing; Settings ▸ AI ▸ Allow model downloads is off: say so and link to the setting.
    case downloadsOff(RetouchModelInfo)
    /// Queued or downloading (`ModelAcquisitionState` has the bytes).
    case downloading(RetouchModelInfo, ModelAcquisitionState)
    /// The download failed; Retry requests it again.
    case failed(RetouchModelInfo, String)

    public var model: RetouchModelInfo? {
        switch self {
        case .installed: nil
        case .available(let m), .downloadsOff(let m), .downloading(let m, _), .failed(let m, _): m
        }
    }
    public var isDownloading: Bool { if case .downloading = self { true } else { false } }
    /// The inline progress row's state (downloading or failed; idle otherwise).
    public var progress: ModelAcquisitionState {
        switch self {
        case .downloading(_, let s): s
        case .failed(_, let reason): .failed(reason: reason)
        default: .idle
        }
    }
}

/// Routes LaMa, DDColor and DRUNet through the app's `ModelAcquisition` (the `ModelDownloads` flow M2-51
/// uses for AI Denoise and Lens Blur): same cache (`<support>/models/cache`, where the engine's retouch calls
/// look), same Settings ▸ AI "Allow model downloads" preference. Nothing downloads without a user request;
/// with downloads off nothing downloads at all. A request remembers the operation that asked for the model
/// and runs it once the model is ready (or hands it the failure).
@MainActor @Observable
public final class RetouchModelDownloads {
    public enum Outcome: Equatable, Sendable {
        /// The model is ready: run the operation now.
        case ready
        /// Downloading; the operation runs when it completes.
        case started
        /// Downloads are off (Settings ▸ AI): nothing was requested.
        case downloadsOff
    }

    @ObservationIgnored public let acquisition: ModelAcquisition
    /// Operation titles waiting for a model, by model id ("Remove", "Colorize").
    public private(set) var waiting: [String: String] = [:]
    @ObservationIgnored private var actions: [String: [@MainActor (Result<Void, RetouchModelDownloadError>) -> Void]] = [:]

    public init(acquisition: ModelAcquisition) { self.acquisition = acquisition }

    public var allowDownloads: Bool { acquisition.allowDownloads }

    public static func requirement(_ m: RetouchModelInfo) -> ModelRequirement {
        ModelRequirement(id: m.modelId, version: m.version, title: m.usedBy)
    }

    /// The model a Remove backend needs before it can run: LaMa for LaMa; nothing for Auto (it falls back
    /// to PatchMatch) or PatchMatch.
    public static func modelId(for engine: RemoveEngine) -> String? { engine == .lama ? "remove/lama" : nil }

    /// The model a neural filter needs (Skin Smoothing needs none; JPEG Artifact Removal and Photo
    /// Restoration share DRUNet).
    public static func modelId(for kind: NeuralKind) -> String? {
        switch kind {
        case .skinSmoothing: nil
        case .colorize: "filters/ddcolor"
        case .jpegArtifactRemoval, .photoRestoration: "enhance/drunet-color"
        }
    }

    /// `m`'s phase (`nil` or installed: `.installed`).
    public func phase(_ m: RetouchModelInfo?) -> RetouchDownloadPhase {
        guard let m, !m.installed else { return .installed }
        let s = acquisition.state(Self.requirement(m))
        switch s {
        case .queued, .downloading: return .downloading(m, s)
        case .failed(let reason): return .failed(m, reason)
        case .ready:
            // Downloaded, yet the engine's lookup does not find it: say so rather than pretend.
            return .failed(m, "downloaded, but the engine does not find it at \(m.cachePath)")
        case .idle: return acquisition.allowDownloads ? .available(m) : .downloadsOff(m)
        }
    }

    /// A user request for an operation that needs `m`. Installed: `.ready` (run now). Downloads off:
    /// `.downloadsOff`, nothing requested. Otherwise the download starts (or keeps going) with inline
    /// progress, and `then` runs once on the main actor when it is ready (`.success`) or failed.
    @discardableResult
    public func request(_ m: RetouchModelInfo, for operation: String,
                        then: @escaping @MainActor (Result<Void, RetouchModelDownloadError>) -> Void) -> Outcome {
        let r = Self.requirement(m)
        // Installed, or downloaded already (the operation then reports whatever the engine finds).
        if m.installed || acquisition.state(r).isReady { return .ready }
        guard acquisition.allowDownloads else { return .downloadsOff }
        acquisition.reset([r])   // a failed download is retried on a new request
        waiting[m.modelId] = operation
        actions[m.modelId, default: []].append(then)
        acquisition.acquire([r]) { [weak self] state in
            guard let self else { return }
            let pending = self.actions.removeValue(forKey: m.modelId) ?? []
            self.waiting[m.modelId] = nil
            let result: Result<Void, RetouchModelDownloadError> = state.isReady
                ? .success(()) : .failure(RetouchModelDownloadError(modelId: m.modelId, reason: state.failure ?? state.label))
            for a in pending { a(result) }
        }
        return .started
    }

    /// Forgets the operations waiting for `modelId` (the download itself continues into the cache).
    public func forgetWaiting(_ modelId: String) {
        actions[modelId] = nil
        waiting[modelId] = nil
    }

    /// Options bar / sheet: "LaMa model: Downloading 12 MB of 208 MB — Remove runs when it is ready".
    public func line(_ m: RetouchModelInfo) -> String? {
        switch phase(m) {
        case .installed: return nil
        case .available: return "\(m.usedBy) is not installed. Download it (\(m.modelId)) when asked, or now."
        case .downloadsOff: return "\(m.usedBy) is not installed and model downloads are off (Settings ▸ AI). Nothing is downloaded."
        case .downloading(_, let s):
            let then = waiting[m.modelId].map { " · \($0) runs when it is ready" } ?? ""
            return "\(m.modelId): \(s.label)\(then)"
        case .failed(_, let reason): return "\(m.modelId) download failed: \(reason)"
        }
    }
}

public struct RetouchModelDownloadError: Error, Equatable, LocalizedError {
    public let modelId: String
    public let reason: String
    public var errorDescription: String? { "The \(modelId) download failed: \(reason)" }
}
