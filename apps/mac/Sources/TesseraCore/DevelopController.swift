import CoreGraphics
import Foundation
import IOSurface
import TesseraFFI

/// One finished level of an engine render, written into an attached IOSurface.
public struct DevelopFrame: Sendable, Equatable {
    public let surfaceID: UInt32
    public let level: Int
    /// Valid region, anchored top-left in the surface (sensor orientation).
    public let width: Int
    public let height: Int
    public let firstLevel: Int
    public let isFinal: Bool
    public let renderMs: Double
    public let generation: UInt64
    public let dirtyStage: String?

    init(_ f: FrameInfo) {
        surfaceID = f.surfaceId; level = Int(f.level); width = Int(f.width); height = Int(f.height)
        firstLevel = Int(f.firstLevel); isFinal = f.isFinal; renderMs = f.renderMs
        generation = f.generation; dirtyStage = f.dirtyStage
    }

    /// "L3 → L2, 7.8 ms" (the status bar's debug readout).
    public var readout: String {
        let levels = firstLevel == level ? "L\(level)" : "L\(firstLevel) → L\(level)"
        return String(format: "render: %@, %.1f ms", levels, renderMs)
    }
}

/// A develop setting addressed by its member of the engine's `DevelopSettings` JSON.
public struct DevelopParameter: Hashable, Sendable {
    public let section: String
    public let field: String
    public init(_ section: String, _ field: String) { self.section = section; self.field = field }

    public static let temperature = DevelopParameter("white_balance", "temperature")
    public static let tint = DevelopParameter("white_balance", "tint")
    public static let exposure = DevelopParameter("tone", "exposure")
    public static let contrast = DevelopParameter("tone", "contrast")
    public static let highlights = DevelopParameter("tone", "highlights")
    public static let shadows = DevelopParameter("tone", "shadows")
    public static let whites = DevelopParameter("tone", "whites")
    public static let blacks = DevelopParameter("tone", "blacks")

    public var isWhiteBalance: Bool { section == "white_balance" }
}

/// Drives one Rust `DevelopSession` for the image on screen (docs/11 §1.2).
///
/// - Slider values are coalesced: `set(_:_:interactive:)` records a JSON merge patch and asks the
///   host for a display-link tick (`onNeedsFlush`); `flushPending()` sends at most one
///   `set_settings` per frame. Final values (mouse-up) are sent at once and committed as one undo step.
/// - The controller allocates the RGBA8 IOSurface ring at the size the session plans for the
///   viewport and attaches it; the engine writes pixels, `onFrame` names the surface to present.
/// - Engine callbacks arrive on worker threads and are forwarded to the main actor.
@MainActor
public final class DevelopController {
    public let itemID: Int
    public let imageID: String
    public let session: DevelopSession
    public let info: DevelopInfo
    public private(set) var history: HistoryState
    public private(set) var lastFrame: DevelopFrame?
    public private(set) var histogram: Histogram?
    public private(set) var plan: SurfacePlan?
    /// Settings not drawn by this pipeline version (kept in the recipe).
    public private(set) var ignoredSettings: [String] = []

    public var onFrame: ((DevelopFrame) -> Void)?
    public var onSaved: ((String) -> Void)?
    public var onFailure: ((String) -> Void)?
    /// Called when a coalesced change is waiting; the host calls `flushPending()` on its next
    /// display-link tick. Without a handler changes are sent immediately.
    public var onNeedsFlush: (() -> Void)?

    private var settings: [String: Any] = [:]
    private var pending: [String: [String: Any]] = [:]
    private var pendingInteractive = false
    private var surfaces: [UInt32: IOSurfaceRef] = [:]
    private let events: Events
    private var closed = false

    /// `kCVPixelFormatType_32RGBA`: the engine's RGBA8 display-encoded sRGB contract.
    public static let surfacePixelFormat: UInt32 = 0x5247_4241

    /// Opens the session off the main actor (the RAW is decoded there).
    public static func open(_ ref: EngineImageReference, itemID: Int) async throws -> DevelopController {
        let session = try await Task.detached(priority: .userInitiated) {
            try ref.engine.openDevelopSession(imageId: ref.imageID)
        }.value
        return try DevelopController(session: session, itemID: itemID, imageID: ref.imageID)
    }

    init(session: DevelopSession, itemID: Int, imageID: String) throws {
        self.session = session
        self.itemID = itemID
        self.imageID = imageID
        info = session.info()
        history = try session.historyState()
        events = Events()
        events.owner = self
        session.setListener(listener: events)
        try reloadSettings()
    }

    /// Stops rendering and writes pending edits (off the main actor). Idempotent.
    public func close() async {
        guard !closed else { return }
        closed = true
        _ = flushPending()
        session.setListener(listener: nil)
        let session = session
        await Task.detached(priority: .utility) {
            try? session.close()
        }.value
    }

    // MARK: Surfaces

    /// Allocates and attaches the surface ring for a viewport of `width × height` device pixels
    /// in display orientation. No-op when the planned size is unchanged. Returns the plan.
    @discardableResult
    public func attachSurfaces(viewWidth: Int, viewHeight: Int, count: Int = 3) throws -> SurfacePlan {
        let swap = info.orientation >= 5
        let w = UInt32(max(swap ? viewHeight : viewWidth, 1))
        let h = UInt32(max(swap ? viewWidth : viewHeight, 1))
        let next = session.planSurface(width: w, height: h)
        if next == plan, !surfaces.isEmpty { return next }
        var created: [UInt32: IOSurfaceRef] = [:]
        for _ in 0..<max(count, 1) {
            guard let s = Self.makeSurface(width: Int(next.width), height: Int(next.height)) else {
                throw DevelopError.surface
            }
            created[IOSurfaceGetID(s)] = s
        }
        // Keep the old ring alive until the engine has switched to the new one.
        for (id, _) in created {
            try session.attachSurface(iosurfaceId: id, width: next.width, height: next.height)
        }
        surfaces = created
        plan = next
        return next
    }

    public func surface(_ id: UInt32) -> IOSurfaceRef? { surfaces[id] }

    static func makeSurface(width: Int, height: Int) -> IOSurfaceRef? {
        let props: [CFString: Any] = [
            kIOSurfaceWidth: width,
            kIOSurfaceHeight: height,
            kIOSurfaceBytesPerElement: 4,
            kIOSurfaceBytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
            kIOSurfacePixelFormat: surfacePixelFormat,
        ]
        return IOSurfaceCreate(props as CFDictionary)
    }

    // MARK: Settings

    /// Current value of a Basic slider. White balance in As Shot mode reads the as-shot estimate.
    public func value(_ p: DevelopParameter) -> Double {
        let section = settings[p.section] as? [String: Any]
        if p.isWhiteBalance, (section?["mode"] as? String) == "as_shot" {
            return Double(p == .temperature ? info.asShotTemperature : info.asShotTint)
        }
        return (section?[p.field] as? NSNumber)?.doubleValue ?? 0
    }

    public var isAsShotWhiteBalance: Bool {
        ((settings["white_balance"] as? [String: Any])?["mode"] as? String) == "as_shot"
    }

    /// Records a slider value. Interactive values are coalesced to one engine call per display
    /// frame; a final value is sent at once (call `commit` to make it an undo step).
    public func set(_ p: DevelopParameter, _ value: Double, interactive: Bool) {
        var patch = pending[p.section] ?? [:]
        patch[p.field] = value
        if p.isWhiteBalance {
            // Moving either slider leaves As Shot: pin the other to its displayed value.
            let other: DevelopParameter = p == .temperature ? .tint : .temperature
            if isAsShotWhiteBalance, patch[other.field] == nil { patch[other.field] = self.value(other) }
            patch["mode"] = "custom"
        }
        pending[p.section] = patch
        var section = settings[p.section] as? [String: Any] ?? [:]
        for (k, v) in patch { section[k] = v }
        settings[p.section] = section
        pendingInteractive = interactive
        if interactive, let onNeedsFlush { onNeedsFlush() } else { _ = flushPending() }
    }

    /// White balance back to the camera's as-shot values.
    public func setAsShotWhiteBalance() {
        pending["white_balance"] = ["mode": "as_shot"]
        pendingInteractive = false
        _ = flushPending()
        try? reloadSettings()
    }

    /// Sends the coalesced patch, if any. Returns whether something was sent.
    @discardableResult
    public func flushPending() -> Bool {
        guard !pending.isEmpty, !closed,
              let data = try? JSONSerialization.data(withJSONObject: pending),
              let json = String(data: data, encoding: .utf8) else { return false }
        pending.removeAll()
        do {
            try session.setSettings(jsonPatch: json, interactive: pendingInteractive)
        } catch {
            onFailure?(error.localizedDescription)
        }
        return true
    }

    /// Makes everything since the last commit one undo step labelled `label`.
    @discardableResult
    public func commit(label: String) -> Bool {
        flushPending()
        let recorded = (try? session.commit(label: label)) ?? false
        refreshHistory()
        return recorded
    }

    public func undo() throws -> Bool { try historyMove { try session.undo() } }
    public func redo() throws -> Bool { try historyMove { try session.redo() } }
    public func reset() throws -> Bool { try historyMove { try session.reset() } }

    public func snapshot(named name: String) throws {
        flushPending()
        try session.snapshot(name: name)
        refreshHistory()
    }

    public func restoreSnapshot(named name: String) throws {
        _ = try historyMove { try session.restoreSnapshot(name: name); return true }
    }

    private func historyMove(_ body: () throws -> Bool) throws -> Bool {
        flushPending()
        let moved = try body()
        try reloadSettings()
        return moved
    }

    private func reloadSettings() throws {
        let json = try session.getSettingsJson()
        settings = (try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any]) ?? [:]
        ignoredSettings = (try? session.ignoredSettings()) ?? []
        refreshHistory()
    }

    private func refreshHistory() {
        if let h = try? session.historyState() { history = h }
    }

    // MARK: Engine callbacks (main actor)

    fileprivate func didRender(_ info: FrameInfo) {
        guard !closed else { return }
        let frame = DevelopFrame(info)
        lastFrame = frame
        histogram = try? session.getHistogram()
        onFrame?(frame)
    }

    fileprivate func didSave(_ hash: String) {
        refreshHistory()
        onSaved?(hash)
    }

    fileprivate func didFail(_ message: String) { onFailure?(message) }

    /// Forwards engine worker-thread callbacks to the main actor. Holds its owner weakly.
    private final class Events: DevelopListener, @unchecked Sendable {
        @MainActor weak var owner: DevelopController?
        func frameReady(frame: FrameInfo) {
            DispatchQueue.main.async { MainActor.assumeIsolated { self.owner?.didRender(frame) } }
        }
        func renderFailed(message: String) {
            DispatchQueue.main.async { MainActor.assumeIsolated { self.owner?.didFail(message) } }
        }
        func saved(recipeHash: String) {
            DispatchQueue.main.async { MainActor.assumeIsolated { self.owner?.didSave(recipeHash) } }
        }
    }
}

public enum DevelopError: LocalizedError {
    case surface
    public var errorDescription: String? { "Could not allocate the viewport IOSurface" }
}
