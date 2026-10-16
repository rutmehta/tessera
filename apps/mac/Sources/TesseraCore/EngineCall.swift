import Foundation
import TesseraFFI

/// Engine calls from the UI (ST-2). An FFI call takes the engine's catalog mutex, so a call made
/// on the main actor stalls the UI for as long as a worker holds that lock (a render, an index
/// pass). Views call `EngineCall.detached` instead: the call runs on a worker, the view keeps its
/// previous content, and the result is posted back. UI-safe methods (ST-1: `agent_provenance`,
/// `roots`, `images`, `groups`, `people`, `face_strip`, `assist_status`, `review`) refuse with a
/// `Busy` error rather than wait for the lock; `detached` retries those after a short delay.
public enum EngineCall {
    /// Delay between attempts when the engine answered `Busy`.
    public static let busyRetryDelay: Duration = .milliseconds(60)
    /// Attempts before a `Busy` refusal is reported as the error (about 1.5 s of retries).
    public static let busyAttempts = 25

    /// Whether `error` is the engine's "come back later" refusal (ST-1's UI-safe methods). The
    /// bindings describe their errors by case name, so the check reads the description.
    public static func isBusy(_ error: Error) -> Bool {
        if case BridgeError.Busy = error { return true }
        let text = String(reflecting: error).lowercased()
        return text.contains("busy") || text.contains("would block") || text.contains("try again")
    }

    /// Runs `body` on a worker and returns its result; a `Busy` refusal is retried up to
    /// `attempts` times, `delay` apart. Any other error is thrown at once.
    public static func detached<T: Sendable>(priority: TaskPriority = .userInitiated,
                                             attempts: Int = busyAttempts, delay: Duration = busyRetryDelay,
                                             _ body: @escaping @Sendable () throws -> T) async throws -> T {
        var attempt = 0
        while true {
            attempt += 1
            let result = await Task.detached(priority: priority) { Result { try body() } }.value
            switch result {
            case .success(let value):
                return value
            case .failure(let error):
                guard isBusy(error), attempt < attempts, !Task.isCancelled else { throw error }
                try? await Task.sleep(for: delay)
                try Task.checkCancellation()
            }
        }
    }

    /// `detached` for callers that treat every failure as "nothing": nil on error or cancellation.
    public static func optional<T: Sendable>(priority: TaskPriority = .userInitiated,
                                             _ body: @escaping @Sendable () throws -> T) async -> T? {
        try? await detached(priority: priority, body)
    }
}
