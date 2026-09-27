import Foundation

/// Producer-side replacement plus a single pending drain. Values own any resources
/// they need until delivery; replacing a value promptly releases the older one.
final class LatestFrameMailbox<Value: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var latest: Value?
    private var scheduled = false
    private var newestSequence: UInt64 = 0

    /// True only when the caller must enqueue a main-queue drain.
    func offer(_ value: Value, sequence: UInt64 = 0) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard sequence >= newestSequence else { return false }
        newestSequence = sequence
        latest = value
        guard !scheduled else { return false }
        scheduled = true
        return true
    }

    func take() -> Value? {
        lock.lock()
        defer { lock.unlock() }
        let value = latest
        latest = nil
        scheduled = false
        return value
    }
}
