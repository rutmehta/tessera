import Foundation
import QuartzCore

/// Opt-in, bounded in-memory audit trace. No per-frame disk writes or logging in normal use.
/// Input sequence and engine generation are deliberately distinct: today's FFI cannot join them.
public final class PerformanceTrace: @unchecked Sendable {
    public struct Event: Codable, Sendable {
        public let name: String
        public let time: Double
        public let mainThread: Bool
        public let session: String?
        public let input: UInt64?
        public let generation: UInt64?
        public let width: Int?
        public let height: Int?
        public let level: Int?
        public let backend: String?
        public let residency: String
        public let span: String?
        public let durationMs: Double?
        public let engineSinkMs: Double?
    }
    public struct Snapshot: Codable, Sendable {
        public let events: [Event]
        public let dropped: Int
    }
    public struct Span: Sendable {
        let name: String
        let id: String
        let start: Double
        let session: String?
        let input: UInt64?
    }
    public static let outputPath: String? = {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: "--timing-output"), i + 1 < args.count else { return nil }
        return args[i + 1]
    }()
    public static let shared = PerformanceTrace(enabled: outputPath != nil)
    public let enabled: Bool
    private let capacity: Int
    private let lock = NSLock()
    private var events: [Event] = []
    private var dropped = 0

    public init(enabled: Bool, capacity: Int = 100_000) {
        self.enabled = enabled
        self.capacity = max(0, capacity)
    }

    public func record(_ name: String, session: String? = nil, input: UInt64? = nil,
                       generation: UInt64? = nil, width: Int? = nil, height: Int? = nil,
                       level: Int? = nil, backend: String? = nil, span: String? = nil,
                       durationMs: Double? = nil, engineSinkMs: Double? = nil, time: Double? = nil) {
        guard enabled else { return }
        let event = Event(name: name, time: time ?? CACurrentMediaTime(), mainThread: Thread.isMainThread,
                          session: session, input: input, generation: generation, width: width, height: height,
                          level: level, backend: backend, residency: "unavailable", span: span,
                          durationMs: durationMs, engineSinkMs: engineSinkMs)
        lock.lock()
        defer { lock.unlock() }
        if events.count < capacity { events.append(event) } else { dropped += 1 }
    }

    public func begin(_ name: String, session: String? = nil, input: UInt64? = nil) -> Span? {
        guard enabled else { return nil }
        let span = Span(name: name, id: UUID().uuidString, start: CACurrentMediaTime(), session: session, input: input)
        record(name + "_start", session: session, input: input, span: span.id, time: span.start)
        return span
    }

    public func end(_ span: Span?) {
        guard let span else { return }
        let now = CACurrentMediaTime()
        record(span.name + "_end", session: span.session, input: span.input, span: span.id,
               durationMs: (now - span.start) * 1000, time: now)
    }

    public func snapshot() -> Snapshot {
        lock.lock()
        defer { lock.unlock() }
        return Snapshot(events: events, dropped: dropped)
    }

    /// Call once when the audit ends; callers keep file encoding/publication off the main actor.
    public func write(to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        try encoder.encode(snapshot()).write(to: url, options: .atomic)
    }
}

/// Known callback identity, carried unchanged into drawable submission and actual presentation.
public struct FrameTiming: Sendable {
    public let session: String
    public let generation: UInt64
    public let width: Int
    public let height: Int
    public let level: Int
    public let backend: String

    public init(session: String, generation: UInt64, width: Int, height: Int, level: Int, backend: String) {
        self.session = session; self.generation = generation
        self.width = width; self.height = height; self.level = level; self.backend = backend
    }

    public func record(_ name: String, time: Double? = nil) {
        PerformanceTrace.shared.record(name, session: session, generation: generation, width: width,
                                       height: height, level: level, backend: backend, time: time)
    }
}
