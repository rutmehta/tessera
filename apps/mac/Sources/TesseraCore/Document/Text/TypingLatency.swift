import Foundation

/// Typing latency of the Type tool (WP B5-10c): keystroke → the renderer's completed frame
/// (`FrameInfo` of the epoch the keystroke's preview produced), in milliseconds.
///
/// * Only keystrokes are measured: a preview carries the oldest keystroke not yet sent, once;
///   previews from box, move or rotate gestures and from inspector edits carry none (before B5-10c
///   they reused the last keystroke's time, which produced minute-long samples).
/// * Time while the document window is not visible or the app is not active is excluded (App Nap
///   and occlusion delay frames of a background window); a keystroke or frame that arrives while
///   inactive is not counted at all. `requireActive = false` (the background self-test) measures
///   wall-clock time instead.
///
/// Times are seconds on a monotonic clock (`ProcessInfo.systemUptime`).
public struct TypingLatencyMeter: Sendable {
    public var requireActive = true
    public private(set) var samples: [Double] = []
    /// Keystrokes that arrived, or whose frame arrived, while the window was inactive.
    public private(set) var excluded = 0
    public static let maxSamples = 400

    /// A keystroke waiting for its preview: wall time, active-clock time, active then.
    public struct Key: Sendable, Equatable {
        public var time: Double
        public var clock: Double
        public var active: Bool
    }
    private var pending: Key?
    private var waiting: [(epoch: UInt64, key: Key)] = []
    /// Previews waiting for their frame.
    public var awaiting: Int { waiting.count }
    private var active = true
    private var inactiveSince: Double?
    private var inactiveTotal = 0.0

    public init(requireActive: Bool = true) { self.requireActive = requireActive }

    /// Seconds of active time (inactive intervals removed) at `t`.
    public func activeClock(_ t: Double) -> Double {
        t - inactiveTotal - (inactiveSince.map { max(t - $0, 0) } ?? 0)
    }

    public mutating func setActive(_ a: Bool, at t: Double) {
        guard a != active else { return }
        active = a
        if a, let s = inactiveSince {
            inactiveTotal += max(t - s, 0)
            inactiveSince = nil
        } else if !a {
            inactiveSince = t
        }
    }

    /// A keystroke changed the draft (the oldest unsent one is kept).
    public mutating func keystroke(at t: Double) {
        if pending == nil { pending = Key(time: t, clock: activeClock(t), active: active) }
    }

    /// Called when a preview starts: the keystroke it carries (nil for gesture / inspector previews).
    public mutating func takePending() -> Key? {
        defer { pending = nil }
        return pending
    }

    /// The preview carrying `key` produced `epoch`.
    public mutating func previewAccepted(epoch: UInt64, key: Key?) {
        if let key { waiting.append((epoch, key)) }
    }

    /// Drops a keystroke no preview will carry (the session ended); previews already sent still
    /// wait for their frame.
    public mutating func reset() { pending = nil }

    /// A frame of `epoch` completed at `t`; returns the new samples (ms).
    @discardableResult
    public mutating func frame(epoch: UInt64, at t: Double) -> [Double] {
        let done = waiting.filter { $0.epoch <= epoch }
        guard !done.isEmpty else { return [] }
        waiting.removeAll { $0.epoch <= epoch }
        var out: [Double] = []
        for d in done {
            if requireActive {
                guard d.key.active, active else { excluded += 1; continue }
                out.append(max(activeClock(t) - d.key.clock, 0) * 1000)
            } else {
                out.append(max(t - d.key.time, 0) * 1000)
            }
        }
        samples.append(contentsOf: out)
        if samples.count > Self.maxSamples { samples.removeFirst(samples.count - Self.maxSamples) }
        return out
    }

    public var median: Double? {
        let s = samples.sorted()
        return s.isEmpty ? nil : s[s.count / 2]
    }

    public var p95: Double? {
        let s = samples.sorted()
        return s.isEmpty ? nil : s[min(s.count - 1, Int(Double(s.count) * 0.95))]
    }

    /// "Keystroke → rendered frame: median 9.8 ms · p95 14.0 ms (42 keys, inactive time excluded)".
    public var readout: String? {
        guard let median, let p95 else { return nil }
        let scope = requireActive ? "inactive time excluded" : "wall clock"
        let skipped = excluded > 0 ? ", \(excluded) while inactive not counted" : ""
        return String(format: "Keystroke → rendered frame: median %.1f ms · p95 %.1f ms (%d keys, %@%@)",
                      median, p95, samples.count, scope, skipped)
    }
}
