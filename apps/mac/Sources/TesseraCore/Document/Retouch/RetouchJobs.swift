import Foundation
import Observation

// MARK: - Long retouch applies (WP B5-09b)

/// One blocking retouch apply at a time (Remove, Content-Aware Fill, Remove Distractions, a neural filter),
/// run off the main thread. Cancel never waits for the engine: the job is marked abandoned and the caller's
/// UI returns to idle at once, the engine is asked to stop on a background queue, and whatever the abandoned
/// job returns later is handed back as `.discarded` (never applied to history). Until the abandoned job has
/// actually returned, a new job is refused with a message (the engine's cancel flags are per session, so a
/// second apply would re-arm them under the first).
@MainActor @Observable
public final class RetouchJobs {
    public struct Job: Equatable, Sendable {
        public let id: UInt64
        /// The busy line ("Removing…").
        public let title: String
        /// The history label and message prefix ("Remove").
        public let operation: String
        public let started: Date
    }

    /// How a job ended.
    public enum End<T: Sendable>: Sendable {
        /// It ran to completion while still wanted: apply a success, report a failure.
        case finished(Result<T, Error>)
        /// It was cancelled from the UI and returned later: do not apply it.
        case discarded(Result<T, Error>)
    }

    public private(set) var running: Job?
    /// Cancelled from the UI, still running in the engine.
    public private(set) var abandoned: Job?
    @ObservationIgnored private var nextID: UInt64 = 0
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let stopQueue = DispatchQueue(label: "dev.tessera.retouch-cancel", qos: .userInitiated)

    public init(now: @escaping () -> Date = Date.init) { self.now = now }

    public var isBusy: Bool { running != nil }

    /// Why a new job cannot start now, or nil.
    public var refusal: String? {
        if let r = running { return "\(r.operation) is still running: wait for it or Cancel it (Esc)" }
        if let a = abandoned {
            let s = Int(now().timeIntervalSince(a.started).rounded())
            return "The cancelled \(a.operation) is still stopping in the engine (\(s) s); try again when it has stopped"
        }
        return nil
    }

    /// Starts `body` off the main thread unless another job is running or still stopping (then returns the
    /// refusal and runs nothing). `completion` runs on the main actor exactly once.
    @discardableResult
    public func start<T: Sendable>(_ title: String, operation: String,
                                   _ body: @escaping @Sendable () throws -> T,
                                   completion: @escaping @MainActor (End<T>) -> Void) -> String? {
        if let why = refusal { return why }
        nextID += 1
        let job = Job(id: nextID, title: title, operation: operation, started: now())
        running = job
        Task { @MainActor in
            let r = await Task.detached(priority: .userInitiated) { Result { try body() } }.value
            if self.running?.id == job.id {
                self.running = nil
                completion(.finished(r))
            } else {
                if self.abandoned?.id == job.id { self.abandoned = nil }
                completion(.discarded(r))
            }
        }
        return nil
    }

    /// Cancel button / Esc: the running job becomes abandoned and `stop` (the engine's cancel) runs on a
    /// background queue, so this returns at once even when the engine call blocks. Returns the job, or nil
    /// when nothing was running.
    @discardableResult
    public func cancel(stop: @escaping @Sendable () -> Void) -> Job? {
        guard let job = running else { return nil }
        running = nil
        abandoned = job
        stopQueue.async { stop() }
        return job
    }

    /// Seconds the running job (or else the abandoned one) has taken.
    public var seconds: Double {
        guard let j = running ?? abandoned else { return 0 }
        return now().timeIntervalSince(j.started)
    }
}

// MARK: - Edit ▸ Content-Aware Fill enablement (WP B5-09b)

public enum RetouchMenuState {
    /// Edit ▸ Content-Aware Fill: a pixel layer or smart object with an active selection, and no apply
    /// running. A cancelled job that is still stopping does not disable it (it used to: the old Cancel kept
    /// the job "busy" until the engine returned, minutes on a large PatchMatch); invoking it then gets the
    /// "still stopping" message instead.
    public static func contentAwareFillEnabled(layerKind: LayerKindTag?, hasSelection: Bool, jobRunning: Bool) -> Bool {
        guard let k = layerKind, k == .pixel || k == .smartObject else { return false }
        return hasSelection && !jobRunning
    }
}
