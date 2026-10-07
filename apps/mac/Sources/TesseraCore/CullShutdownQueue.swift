import Foundation

/// Keeps retired sessions alive until their blocking native shutdown has joined workers.
/// The main actor owns admission and completion; native work always runs detached.
@MainActor
public final class CullShutdownQueue {
    private var pending: [UUID: Task<Void, Error>] = [:]

    public init() {}

    public func enqueue(_ shutdown: @escaping @Sendable () throws -> Void) {
        let id = UUID()
        let task = Task.detached(priority: .utility) { try shutdown() }
        pending[id] = task
        Task { [weak self] in
            // Keep failures visible to drain rather than silently reporting joined shutdown.
            if case .success = await task.result { self?.pending.removeValue(forKey: id) }
        }
    }

    /// Includes retirements admitted while a previous worker is still draining.
    public func drain() async throws {
        while let (id, task) = pending.first {
            try await task.value
            pending.removeValue(forKey: id)
        }
    }
}
