import Foundation
import Observation
import TesseraFFI

// MARK: - Session-only render modes (M2-51)

/// The develop-session calls behind Lens Blur's depth tools and Guided Upright's uncorrected view
/// (M2-49). `DevelopSession` conforms; tests use a stub.
public protocol DevelopRenderModes: AnyObject, Sendable {
    /// 256 near → far bins for the current recipe's pre-geometry image (blocking: runs depth).
    func depthHistogram() throws -> [UInt64]
    /// Grayscale depth overlay instead of the picture (session only; never downloads).
    func setRenderDepthVisualisation(enabled: Bool) throws
    /// Focal range around the segmented subject's depth, written to the live settings (blocking).
    func focusLensBlurOnSubject() throws -> [Float]
    /// The uncorrected image (no lens, Upright, transform or crop) for placing Guided guides.
    func setRenderUncorrected(enabled: Bool) throws
    /// Re-renders the live settings (e.g. once a model has arrived).
    func refresh() throws
}

extension DevelopSession: DevelopRenderModes {}

/// Guided Upright draws its guides on the uncorrected image: the tool turns the session's
/// uncorrected view on when it is armed and back off, on that same session, when it ends.
@MainActor
public final class UncorrectedPlacement {
    public private(set) var session: (any DevelopRenderModes)?
    public var onFailure: ((String) -> Void)?

    public init() {}

    public var isActive: Bool { session != nil }

    public func enter(_ s: any DevelopRenderModes) {
        if session === s { return }
        exit()
        do {
            try s.setRenderUncorrected(enabled: true)
            session = s
        } catch {
            onFailure?(ModelAcquisition.describe(error))
        }
    }

    /// Forgets a session that has closed (nothing to restore).
    public func abandon() { session = nil }

    public func exit() {
        guard let s = session else { return }
        session = nil
        do { try s.setRenderUncorrected(enabled: false) } catch { onFailure?(ModelAcquisition.describe(error)) }
    }
}

// MARK: - Lens Blur depth (M2-51)

/// Lens Blur's depth side for the open develop session: the near → far histogram under the Focal
/// Range strip, Visualize Depth and Subject. Each first acquires the pinned weights through
/// `ModelAcquisition` (the same inline progress as AI Denoise); a failure is shown in the panel
/// and leaves the recipe unchanged.
@MainActor @Observable
public final class LensBlurDepthModel {
    public enum Busy: Equatable, Sendable { case histogram, subject, visualize }

    /// Normalised 0…1 bins (near → far), nil until depth has been estimated.
    public private(set) var histogram: [Double]?
    public private(set) var visualize = false
    public private(set) var busy: Busy?
    public private(set) var error: String?

    @ObservationIgnored public let models: ModelAcquisition
    @ObservationIgnored public private(set) var session: (any DevelopRenderModes)?
    @ObservationIgnored private var generation = 0

    public init(models: ModelAcquisition) { self.models = models }

    /// Depth weights state (the panel's progress line).
    public var weights: ModelAcquisitionState { models.state(.depth) }
    public var subjectWeights: ModelAcquisitionState { models.state(of: ModelRequirement.lensBlurSubject) }

    /// Follows the open develop session; the old one's depth overlay is switched off.
    public func bind(_ s: (any DevelopRenderModes)?) {
        if session === s { return }
        if visualize, let old = session { try? old.setRenderDepthVisualisation(enabled: false) }
        session = s
        generation += 1
        histogram = nil
        visualize = false
        busy = nil
        error = nil
    }

    /// Makes sure the depth weights are cached (downloading them when allowed). Sets `error` on failure.
    public func ensureWeights(_ rs: [ModelRequirement] = [.depth]) async -> Bool {
        models.reset(rs)
        let s = await models.ensure(rs)
        if let reason = s.failure { error = reason; return false }
        return true
    }

    /// Estimates depth (off the main actor) and publishes the 256-bin histogram.
    public func refreshHistogram() async {
        guard let s = session, busy == nil else { return }
        let gen = generation
        busy = .histogram
        error = nil
        defer { if gen == generation { busy = nil } }
        guard await ensureWeights(), gen == generation else { return }
        let result = await Task.detached(priority: .userInitiated) { Result { try s.depthHistogram() } }.value
        guard gen == generation else { return }
        switch result {
        case .success(let bins): histogram = Self.normalize(bins)
        case .failure(let e): error = ModelAcquisition.describe(e)
        }
    }

    /// Visualize Depth: on acquires the weights first; off is immediate.
    public func setVisualize(_ on: Bool) async {
        guard let s = session else { return }
        error = nil
        if on {
            let gen = generation
            busy = .visualize
            defer { if gen == generation, busy == .visualize { busy = nil } }
            guard await ensureWeights(), gen == generation else { return }
        }
        do {
            try s.setRenderDepthVisualisation(enabled: on)
            visualize = on
        } catch {
            self.error = ModelAcquisition.describe(error)
        }
    }

    /// Subject: acquires depth + segmentation weights, asks the engine for the subject's focal
    /// range and hands it to `apply` (one history step). Returns the range, or nil on failure.
    @discardableResult
    public func focusOnSubject(apply: (FocalRange) -> Void) async -> FocalRange? {
        guard let s = session, busy == nil else { return nil }
        let gen = generation
        busy = .subject
        error = nil
        defer { if gen == generation { busy = nil } }
        guard await ensureWeights(ModelRequirement.lensBlurSubject), gen == generation else { return nil }
        let result = await Task.detached(priority: .userInitiated) { Result { try s.focusLensBlurOnSubject() } }.value
        guard gen == generation else { return nil }
        switch result {
        case .success(let v) where v.count == 2:
            let r = FocalRange(near: Double(v[0]), far: Double(v[1]))
            apply(r)
            if let bins = try? await Task.detached(priority: .utility, operation: { try s.depthHistogram() }).value,
               gen == generation {
                histogram = Self.normalize(bins)
            }
            return r
        case .success(let v):
            error = "The engine returned no subject range (\(v.count) values)"
        case .failure(let e):
            error = ModelAcquisition.describe(e)
        }
        return nil
    }

    /// Counts → 0…1 of the tallest bin (all zero stays zero).
    nonisolated public static func normalize(_ bins: [UInt64]) -> [Double] {
        let peak = Double(bins.max() ?? 0)
        return bins.map { peak > 0 ? Double($0) / peak : 0 }
    }
}
