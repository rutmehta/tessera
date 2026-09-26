import Foundation
import TesseraFFI

/// One frame that arrived from the camera: downloaded, renamed, indexed and scored by the
/// engine (`TetherFrame`), reduced to what the incoming strip shows.
public struct IncomingFrame: Sendable, Equatable, Identifiable {
    public var sequence: UInt64
    public var url: URL
    public var imageID: String?
    public var preview: URL?
    /// Whole-frame sharpness (ml-quality), [0, 1].
    public var sharpness: Double?
    /// nil: faces were not analysed (see `faceWarning`), which is not the same as none found.
    public var faces: Int?
    /// Sharpness of the largest face.
    public var faceFocus: Double?
    /// Lowest eyes-open proxy across the faces.
    public var eyesOpen: Double?
    public var faceWarning: String?
    public var error: String?
    public var id: UInt64 { sequence }

    public init(sequence: UInt64, url: URL, imageID: String? = nil, preview: URL? = nil, sharpness: Double? = nil,
                faces: Int? = nil, faceFocus: Double? = nil, eyesOpen: Double? = nil,
                faceWarning: String? = nil, error: String? = nil) {
        self.sequence = sequence; self.url = url; self.imageID = imageID; self.preview = preview
        self.sharpness = sharpness; self.faces = faces; self.faceFocus = faceFocus; self.eyesOpen = eyesOpen
        self.faceWarning = faceWarning; self.error = error
    }

    public init(_ f: TetherFrame) {
        self.init(sequence: f.sequence, url: URL(fileURLWithPath: f.path), imageID: f.imageId,
                  preview: f.preview.map { URL(fileURLWithPath: $0) }, sharpness: f.sharpness,
                  faces: f.faces.map(Int.init), faceFocus: f.faceFocus, eyesOpen: f.eyesOpen,
                  faceWarning: f.faceWarning, error: f.error)
    }

    public var name: String { url.lastPathComponent }
    /// Usable in the library (the file is saved and indexed), even if scoring was partial.
    public var isInLibrary: Bool { imageID != nil }
    public var failed: Bool { error != nil }

    /// The subject's focus when a face was found, otherwise the whole frame's sharpness.
    public var focus: Double? { faceFocus ?? sharpness }
    public var focusIsFace: Bool { faceFocus != nil }
    /// Same thresholds as the face strip (docs/06 §3): green ≥ 0.55, yellow ≥ 0.30, red below.
    public var focusLevel: FaceChip.Level {
        guard let f = focus else { return .unknown }
        return f >= 0.55 ? .good : f >= 0.30 ? .fair : .poor
    }
    /// Open ≥ 0.5, closed < 0.3; unknown without faces or without the proxy.
    public var eyesLevel: FaceChip.Level {
        guard let e = eyesOpen else { return .unknown }
        return e >= 0.5 ? .good : e >= 0.3 ? .fair : .poor
    }
    /// Whether the eyes badge applies (a face was found).
    public var hasFaces: Bool { (faces ?? 0) > 0 }
}

/// The live "incoming" strip: newest first, capped, with placeholders for shutter requests
/// still on their way (download + index + score). Pure value type (unit tested).
public struct IncomingStrip: Sendable, Equatable {
    public private(set) var frames: [IncomingFrame] = []
    /// Shutter requests sent from the app that have not produced a frame yet. Physical-shutter
    /// frames arrive without a request and never go negative.
    public private(set) var pending = 0
    public private(set) var received = 0
    public private(set) var failed = 0
    public let capacity: Int

    public init(capacity: Int = 24) { self.capacity = max(1, capacity) }

    public mutating func captureRequested() { pending += 1 }
    /// The camera refused the request: no frame is coming for it.
    public mutating func captureFailed() { pending = max(0, pending - 1) }

    /// Adds frames (engine order = arrival order). Returns the newest frame that is in the
    /// library, the auto-advance target.
    @discardableResult
    public mutating func receive(_ new: [IncomingFrame]) -> IncomingFrame? {
        var target: IncomingFrame?
        for frame in new.sorted(by: { $0.sequence < $1.sequence }) {
            received += 1
            if frame.failed { failed += 1 }
            pending = max(0, pending - 1)
            frames.removeAll { $0.sequence == frame.sequence }
            frames.insert(frame, at: 0)
            if frame.isInLibrary { target = frame }
        }
        if frames.count > capacity { frames.removeLast(frames.count - capacity) }
        return target
    }

    public var latest: IncomingFrame? { frames.first }

    /// One line for the panel: "12 frames · 1 failed · 1 on the way".
    public var summary: String {
        var parts = [received == 1 ? "1 frame" : "\(received) frames"]
        if failed > 0 { parts.append("\(failed) failed") }
        if pending > 0 { parts.append("\(pending) on the way") }
        return parts.joined(separator: " · ")
    }

    public mutating func reset() { self = IncomingStrip(capacity: capacity) }
}

/// Interval capture ("every N s, M frames"): the schedule only, driven by the controller's timer.
public struct IntervalPlan: Sendable, Equatable {
    public var seconds: Double
    /// 0: until stopped.
    public var count: Int
    public private(set) var taken = 0

    public init(seconds: Double, count: Int) {
        self.seconds = max(1, seconds); self.count = max(0, count)
    }
    public var isFinished: Bool { count > 0 && taken >= count }
    public var remaining: Int? { count > 0 ? max(0, count - taken) : nil }
    /// Records one shot; returns false once the plan is complete.
    public mutating func shoot() -> Bool {
        guard !isFinished else { return false }
        taken += 1
        return true
    }
}
