import Foundation

/// Nonce-bound identity shared by the app readiness record and runner start permit.
struct TimingVisibleHandshake: Codable, Equatable {
    let nonce: String
    let pid: Int32
    let bundleID: String
    let bundleURL: String
    let launchDate: Double
    let windowNumber: Int
    let session: String
    let time: Double

    enum CodingKeys: String, CodingKey {
        case nonce, pid, session, time
        case bundleID = "bundle_id"
        case bundleURL = "bundle_url"
        case launchDate = "launch_date"
        case windowNumber = "window_number"
    }

    func identifiesSameRun(as other: Self) -> Bool {
        nonce == other.nonce && pid == other.pid && bundleID == other.bundleID
            && bundleURL == other.bundleURL && launchDate == other.launchDate
            && windowNumber == other.windowNumber && session == other.session
    }
}

/// Small deterministic protocol state machine used by the visible P01 app handshake.
struct TimingVisibleIntervalProtocol {
    enum Phase: Equatable { case waitingForStart, measuring, ended, failed }

    let ready: TimingVisibleHandshake
    let timeout: Double
    private(set) var phase: Phase = .waitingForStart
    private(set) var measurementStart: Double?
    private(set) var measurementEnd: Double?

    init(ready: TimingVisibleHandshake, timeout: Double = 45) {
        self.ready = ready
        self.timeout = timeout
    }

    mutating func acceptStart(_ permit: TimingVisibleHandshake, now: Double, visible: Bool) -> Bool {
        guard phase == .waitingForStart,
              permit.identifiesSameRun(as: ready), permit.time.isFinite,
              permit.time >= ready.time, now.isFinite, now >= permit.time,
              now <= ready.time + timeout, visible else {
            phase = .failed
            return false
        }
        measurementStart = now
        phase = .measuring
        return true
    }

    mutating func end(now: Double, identity: TimingVisibleHandshake, visible: Bool) -> Bool {
        guard phase == .measuring, identity.identifiesSameRun(as: ready),
              now.isFinite, let measurementStart, now > measurementStart,
              now <= measurementStart + timeout, visible else {
            if phase != .ended { phase = .failed }
            return false
        }
        measurementEnd = now
        phase = .ended
        return true
    }

    mutating func expire(now: Double) {
        guard phase == .waitingForStart || phase == .measuring else { return }
        let deadline = phase == .waitingForStart ? ready.time + timeout : (measurementStart ?? ready.time) + timeout
        if !now.isFinite || now > deadline { phase = .failed }
    }
}
