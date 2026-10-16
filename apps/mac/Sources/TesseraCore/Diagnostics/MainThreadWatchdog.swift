import Darwin
import Foundation
import MachO

/// A main-thread hang watchdog (ST-2) for debug runs and tests: a background thread pings the main
/// run loop every `interval` (100 ms); a ping answered more than `threshold` (150 ms) late is a
/// gap. Each gap records the last location the UI noted on the main thread (`note(_:)`) and a
/// signal-free stack of the main thread, sampled through the Mach thread API while the ping is
/// overdue (the thread is suspended for the few microseconds of the frame walk, then resumed).
///
/// `TESSERA_HANG_WATCHDOG=1` starts it at launch and logs every gap on stderr; tests start it
/// themselves and read `gaps`.
public final class MainThreadWatchdog: @unchecked Sendable {
    public struct Gap: Sendable, CustomStringConvertible {
        /// When the overdue ping was posted.
        public let start: Date
        public let duration: TimeInterval
        /// The last `note(_:)` on the main thread before the gap.
        public let location: String?
        /// The main thread's frames while the ping was overdue (innermost first), symbolicated.
        public let stack: [String]

        public var description: String {
            let ms = Int((duration * 1000).rounded())
            var lines = ["main thread blocked \(ms) ms" + (location.map { " at \($0)" } ?? "")]
            lines += stack.prefix(24).map { "    " + $0 }
            return lines.joined(separator: "\n")
        }
    }

    public static let shared = MainThreadWatchdog()
    /// Whether the environment asked for the watchdog at launch.
    public static var isRequested: Bool { ProcessInfo.processInfo.environment["TESSERA_HANG_WATCHDOG"] == "1" }

    public let interval: TimeInterval
    public let threshold: TimeInterval
    /// Called off the main thread with every gap (the launch hook logs them).
    public var onGap: (@Sendable (Gap) -> Void)?

    private let lock = NSLock()
    private var thread: Thread?
    private var running = false
    private var mainThread: pthread_t?
    private var pingSentAt: CFAbsoluteTime?
    private var sampled = false
    private var sample: [String] = []
    private var location: String?
    private var locationAtPing: String?
    private var recorded: [Gap] = []
    private var latest: TimeInterval = 0
    private var paused = 0
    /// The ping in flight was sent during a deliberate stall: its lateness is not a gap.
    private var ignorePending = false

    public init(interval: TimeInterval = 0.1, threshold: TimeInterval = 0.15) {
        self.interval = interval
        self.threshold = threshold
    }

    /// Gaps recorded since `start()` / `reset()`, oldest first (bounded to the last 256).
    public var gaps: [Gap] { lock.withLock { recorded } }
    public var longest: TimeInterval { gaps.map(\.duration).max() ?? 0 }
    /// The latest any ping was answered since `start()` / `reset()`, gaps or not.
    public var longestLateness: TimeInterval { lock.withLock { latest } }

    /// Records where the main thread is (call from the main thread at interesting entry points).
    public func note(_ location: String) {
        lock.withLock { self.location = location }
    }

    public func reset() { lock.withLock { recorded.removeAll(); latest = 0 } }

    /// Starts pinging. Must be called on the main thread (it is the thread being watched).
    public func start() {
        precondition(Thread.isMainThread, "the watchdog is started from the thread it watches")
        lock.withLock {
            guard !running else { return }
            running = true
            mainThread = pthread_self()
            pingSentAt = nil
            recorded.removeAll()
        }
        let t = Thread { [weak self] in self?.loop() }
        t.name = "dev.tessera.hang-watchdog"
        t.qualityOfService = .userInteractive
        lock.withLock { thread = t }
        t.start()
    }

    public func stop() {
        lock.withLock {
            running = false
            thread = nil
        }
    }

    /// Suspends gap recording for the duration of `body` (a deliberately blocking test step).
    public func withoutRecording<T>(_ body: () throws -> T) rethrows -> T {
        lock.withLock { paused += 1 }
        defer {
            lock.withLock {
                paused -= 1
                if pingSentAt != nil { ignorePending = true }
            }
        }
        return try body()
    }

    private func loop() {
        while lock.withLock({ running }) {
            let overdue: Bool = lock.withLock {
                guard let sent = pingSentAt else { return false }
                return CFAbsoluteTimeGetCurrent() - sent > threshold && !sampled
            }
            if overdue { sampleMainThread() }
            let needsPing = lock.withLock { pingSentAt == nil }
            if needsPing {
                lock.withLock {
                    pingSentAt = CFAbsoluteTimeGetCurrent()
                    sampled = false
                    sample = []
                    locationAtPing = location
                }
                DispatchQueue.main.async { [weak self] in self?.pong() }
            }
            Thread.sleep(forTimeInterval: interval)
        }
    }

    /// The ping landed on the main thread: measure how late it is.
    private func pong() {
        let gap: Gap? = lock.withLock {
            guard let sent = pingSentAt else { return nil }
            pingSentAt = nil
            let ignored = ignorePending
            ignorePending = false
            let late = CFAbsoluteTimeGetCurrent() - sent
            if paused == 0, !ignored { latest = max(latest, late) }
            guard late > threshold, paused == 0, !ignored else { return nil }
            let gap = Gap(start: Date(timeIntervalSinceReferenceDate: sent), duration: late,
                          location: locationAtPing, stack: sample)
            recorded.append(gap)
            if recorded.count > 256 { recorded.removeFirst(recorded.count - 256) }
            return gap
        }
        if let gap, let onGap { DispatchQueue.global(qos: .utility).async { onGap(gap) } }
    }

    /// One frame-pointer walk of the main thread, taken while it is suspended (no signals, no
    /// debugger). arm64 only; elsewhere the gap carries the noted location alone.
    private func sampleMainThread() {
        let frames = Self.captureMainThreadFrames(lock.withLock { mainThread })
        lock.withLock {
            sampled = true
            sample = frames
        }
    }

    nonisolated static func captureMainThreadFrames(_ main: pthread_t?) -> [String] {
        #if arch(arm64)
        guard let main else { return [] }
        let port = pthread_mach_thread_np(main)
        guard thread_suspend(port) == KERN_SUCCESS else { return [] }
        var state = arm_thread_state64_t()
        var count = mach_msg_type_number_t(MemoryLayout<arm_thread_state64_t>.size / MemoryLayout<natural_t>.size)
        let got = withUnsafeMutablePointer(to: &state) { pointer in
            pointer.withMemoryRebound(to: natural_t.self, capacity: Int(count)) { raw in
                thread_get_state(port, ARM_THREAD_STATE64, raw, &count)
            }
        }
        var addresses: [UInt] = []
        if got == KERN_SUCCESS {
            addresses.append(UInt(state.__pc))
            addresses.append(UInt(state.__lr))
            // Walk the frame chain: fp → [previous fp, return address]. The stack is the main
            // thread's; bounds keep a corrupt frame from reading elsewhere.
            var fp = UInt(state.__fp)
            let stackTop = UInt(bitPattern: pthread_get_stackaddr_np(main))
            let stackBottom = stackTop - UInt(pthread_get_stacksize_np(main))
            var steps = 0
            while fp >= stackBottom, fp + 16 <= stackTop, fp % 8 == 0, steps < 64 {
                let frame = UnsafePointer<UInt>(bitPattern: fp)!
                let next = frame[0]
                let ret = frame[1]
                if ret == 0 { break }
                addresses.append(ret)
                guard next > fp else { break }
                fp = next
                steps += 1
            }
        }
        thread_resume(port)
        return addresses.enumerated().map { i, address in Self.symbol(address, index: i) }
        #else
        return []
        #endif
    }

    /// "12  Tessera  0x… AgentEditPanel.load() + 40", like `Thread.callStackSymbols`.
    private nonisolated static func symbol(_ address: UInt, index: Int) -> String {
        var info = dl_info()
        guard dladdr(UnsafeRawPointer(bitPattern: address), &info) != 0 else {
            return String(format: "%-3d ??? 0x%016lx", index, address)
        }
        let image = info.dli_fname.map { URL(fileURLWithPath: String(cString: $0)).lastPathComponent } ?? "???"
        let symbol = info.dli_sname.map { Self.demangled(String(cString: $0)) } ?? "???"
        let offset = address - UInt(bitPattern: info.dli_saddr)
        return String(format: "%-3d %@ 0x%016lx %@ + %lu", index, image, address, symbol, offset)
    }

    private nonisolated static func demangled(_ name: String) -> String {
        guard name.hasPrefix("$s") || name.hasPrefix("_$s") else { return name }
        // Swift's runtime demangler, resolved at run time (not part of the public SDK).
        typealias Demangle = @convention(c) (UnsafePointer<CChar>, Int, UnsafeMutablePointer<CChar>?, UnsafeMutablePointer<Int>?, UInt32) -> UnsafeMutablePointer<CChar>?
        guard let handle = dlopen(nil, RTLD_LAZY), let sym = dlsym(handle, "swift_demangle") else { return name }
        let demangle = unsafeBitCast(sym, to: Demangle.self)
        guard let out = demangle(name, name.utf8.count, nil, nil, 0) else { return name }
        defer { free(out) }
        return String(cString: out)
    }
}

/// The process's physical footprint, the number `footprint(1)` and `vmmap --summary` report
/// (ST-2: the memory before/after evidence is read from the harness itself).
public enum MemoryFootprint {
    public struct Reading: Sendable, CustomStringConvertible {
        public let physical: UInt64
        public let peak: UInt64
        public var description: String {
            String(format: "footprint %.1f MB (peak %.1f MB)", Double(physical) / 1_048_576, Double(peak) / 1_048_576)
        }
    }

    public static func current() -> Reading? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { raw in
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), raw, &count)
            }
        }
        guard result == KERN_SUCCESS else { return nil }
        return Reading(physical: UInt64(max(info.phys_footprint, 0)), peak: UInt64(max(info.ledger_phys_footprint_peak, 0)))
    }
}
