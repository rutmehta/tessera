import Foundation

/// Keeps retired sessions alive until their blocking native shutdown has joined workers.
/// The main actor owns admission and completion; native work always runs detached, one
/// task per session, so a stuck session never delays another one's retirement.
@MainActor
public final class CullShutdownQueue {
    public enum DrainOutcome: Equatable, Sendable {
        /// Every admitted retirement joined its workers.
        case joined
        /// A native shutdown failed (described); the others were still awaited.
        case failed(String)
        /// The bound elapsed first. Retirements keep running in the background.
        case timedOut
    }

    private var pending: [UUID: Task<Void, Error>] = [:]
    private var failures: [Error] = []

    public init() {}

    public func enqueue(_ shutdown: @escaping @Sendable () throws -> Void) {
        let id = UUID()
        let task = Task.detached(priority: .utility) { try shutdown() }
        pending[id] = task
        Task { [weak self] in
            let result = await task.result
            // Whoever removes the entry first records its failure exactly once, so a
            // failure is reported by the next drain and never poisons later ones.
            guard let self, self.pending.removeValue(forKey: id) != nil else { return }
            if case .failure(let error) = result { self.failures.append(error) }
        }
    }

    /// Waits for every retirement, including ones admitted while waiting. Throws the
    /// first failure recorded since the previous drain, after awaiting all the others.
    public func drain() async throws {
        while let (id, task) = pending.first {
            let result = await task.result
            if pending.removeValue(forKey: id) != nil, case .failure(let error) = result {
                failures.append(error)
            }
        }
        if let first = failures.first {
            failures.removeAll()
            throw first
        }
    }

    /// `drain()` bounded by `timeout`, for app termination: a provider stuck on a stalled
    /// volume must not make quitting hang. Never throws.
    public func drain(timeout: Duration) async -> DrainOutcome {
        let (outcomes, sink) = AsyncStream<DrainOutcome>.makeStream()
        Task {
            do {
                try await self.drain()
                sink.yield(.joined)
            } catch {
                sink.yield(.failed(String(describing: error)))
            }
        }
        let timer = Task {
            try? await Task.sleep(for: timeout)
            sink.yield(.timedOut)
        }
        var first = outcomes.makeAsyncIterator()
        let outcome = await first.next() ?? .timedOut
        timer.cancel()
        sink.finish()
        return outcome
    }
}
