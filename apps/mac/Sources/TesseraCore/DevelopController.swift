import CoreGraphics
import Foundation
import IOSurface
import TesseraFFI

@MainActor
private final class DevelopCloseAttemptContext: @unchecked Sendable {
    weak var controller: DevelopController?
    var result: Result<Void, Error>?
    init(controller: DevelopController) { self.controller = controller }
}

private enum DevelopCloseCallbackContext {
    @TaskLocal static var attempt: DevelopCloseAttemptContext?
}

private enum DevelopCloseAdmissionError: LocalizedError {
    case closing, closed
    var errorDescription: String? {
        switch self {
        case .closing: "Develop is closing; wait for the save result before editing."
        case .closed: "Develop is closed."
        }
    }
}

private struct DevelopSettingsEncodingError: LocalizedError {
    var errorDescription: String? { "Pending Develop settings could not be encoded as JSON." }
}

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
    /// The whole picture at the screen level (cropped extent): what the loupe aspect-fits.
    public let displayWidth: Int
    public let displayHeight: Int
    /// A diagnostic overlay (the sharpening mask), not the developed image.
    public let isOverlay: Bool

    init(_ f: FrameInfo) {
        surfaceID = f.surfaceId; level = Int(f.level); width = Int(f.width); height = Int(f.height)
        firstLevel = Int(f.firstLevel); isFinal = f.isFinal; renderMs = f.renderMs
        generation = f.generation; dirtyStage = f.dirtyStage
        displayWidth = Int(f.displayWidth); displayHeight = Int(f.displayHeight); isOverlay = f.isOverlay
    }

    /// Engine sink time, not app input-to-display latency.
    public var readout: String {
        let levels = firstLevel == level ? "L\(level)" : "L\(firstLevel) → L\(level)"
        return String(format: "engine sink: %@, %.1f ms (not input-to-display)", levels, renderMs)
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
    public var path: [String] { [section, field] }
}

/// Drives one Rust `DevelopSession` for the image on screen (docs/11 §1.2).
///
/// - Slider values are coalesced: `set(_:_:interactive:)` records a JSON merge patch and asks the
///   host for a display-link tick (`onNeedsFlush`); `flushPending()` sends at most one
///   `set_settings` per frame. Final values (mouse-up) are sent at once and committed as one undo step.
/// - The controller allocates the IOSurface ring at the size the session plans for the viewport
///   and attaches it; the engine writes pixels, `onFrame` names the surface to present. The ring is
///   RGBA8 display-encoded sRGB, or RGBA16F display-linear (EDR) while `presentation` says so:
///   an EDR-capable screen (`updateDisplay`) with the recipe's HDR toggle on (M2-22).
/// - Engine callbacks arrive on worker threads and are forwarded to the main actor.
@MainActor
public final class DevelopController {
    /// The library item it develops; follows in-place library updates (`relink`).
    public private(set) var itemID: Int
    public let imageID: String
    public let session: DevelopSession
    public let info: DevelopInfo
    public private(set) var history: HistoryState
    public private(set) var lastFrame: DevelopFrame?
    public private(set) var histogram: Histogram?
    public private(set) var plan: SurfacePlan?
    /// Settings not drawn by this pipeline version (kept in the recipe), as JSON pointers
    /// (`/geometry/upright/mode`). Refreshed on open, history moves and each recorded commit.
    public private(set) var ignoredSettings: [String] = []

    /// Whether the loupe skips any setting under `prefix` (e.g. `/geometry/transform`).
    public func ignores(_ prefix: String) -> Bool { ignoredSettings.contains { $0.hasPrefix(prefix) } }

    public var onFrame: ((DevelopFrame) -> Void)?
    public var onSaved: ((String) -> Void)?
    public var onFailure: ((String) -> Void)?
    /// Called when a coalesced change is waiting; the host calls `flushPending()` on its next
    /// display-link tick. Without a handler changes are sent immediately.
    public var onNeedsFlush: (() -> Void)?
    /// Every JSON merge patch sent to the engine (tests, diagnostics).
    public var onPatchSent: ((String) -> Void)?
    /// Settings changed outside a local slider drag (undo, preset, snapshot, history).
    public var onSettingsReloaded: (() -> Void)?

    private var settings: [String: Any] = [:]
    private var pending: [String: Any] = [:]
    private var pendingInteractive = false
    private var settingsFlushInFlight = false
    private var settingsFlushRequested = false
    private var deferredSettingsFlushID = UUID()
    private(set) var deferredSettingsFlush: Task<Void, Never>?
    private var surfaces: [UInt32: IOSurfaceRef] = [:]
    /// The attached ring is RGBA16F (EDR).
    public private(set) var surfacesAreFloat = false
    /// Screen EDR capability and ring format (M2-22); SDR until `updateDisplay`.
    public private(set) var presentation: EDRPresentation = .sdr
    /// Called on the main actor when `presentation` changes.
    public var onPresentationChange: ((EDRPresentation) -> Void)?
    private var screen: EDRScreenValues?
    private var lastView: (width: Int, height: Int)?
    private var reportedHeadroom: Double = 1
    private let events: Events
    public let timingSession = UUID().uuidString
    private var timingInput: UInt64 = 0
    private(set) var closed = false
    private(set) var closing = false
    private var closeTask: Task<Result<Void, Error>, Never>?
    private var reportingAdmissionFailure = false
    /// Internal observation point for a caller joining an in-flight close. Nil in production.
    /// Tests use it to release a backend gate only after the second caller has joined.
    var onCloseWaiterJoined: (() -> Void)?

    // Masking (see DevelopController+Masks.swift): changes coalesced like the sliders.
    var pendingMaskParams: [MaskParamKey: Float] = [:]
    var pendingMaskGroup: [UInt32: (patch: MaskGroupPatch, interactive: Bool)] = [:]
    var pendingComponent: (group: UInt32, index: UInt32, json: String)?
    var pendingStroke: StrokeCoalescer?
    var maskOverlaySurfaces: [UInt32: IOSurfaceRef] = [:]
    /// Overlay frames (the selected mask as an R8 alpha plane), on the main actor.
    public var onMaskOverlay: ((MaskOverlayFrame) -> Void)?
    /// AI mask progress, on the main actor.
    public var onMaskJob: ((MaskJobUpdate) -> Void)?

    /// `kCVPixelFormatType_32RGBA`: the engine's RGBA8 display-encoded sRGB contract.
    public static let surfacePixelFormat: UInt32 = 0x5247_4241
    /// `kCVPixelFormatType_64RGBAHalf` (`'RGhA'`): the engine's RGBA16F EDR contract.
    public static let floatSurfacePixelFormat: UInt32 = 0x5247_6841

    /// Opens the session off the main actor (the RAW is decoded there).
    /// The library renumbered its items in place; the session is unchanged.
    public func relink(itemID: Int) {
        guard admitsMutation() else { return }
        self.itemID = itemID
    }

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
        events = Events(session: timingSession)
        events.owner = self
        session.setListener(listener: events)
        session.setMaskListener(listener: events)
        try reloadSettings()
    }

    /// Stops rendering and writes pending edits (off the main actor). A failed attempt
    /// leaves the controller and native session available for an explicit retry.
    @discardableResult
    public func close() async -> Result<Void, Error> {
        // A child task created by a synchronous close callback belongs to that
        // attempt, even if it first runs after the failing attempt has completed.
        if let inherited = DevelopCloseCallbackContext.attempt,
           inherited.controller === self, let result = inherited.result {
            return result
        }
        if let closeTask {
            onCloseWaiterJoined?()
            return await closeTask.value
        }
        guard !closed else { return .success(()) }
        closing = true
        invalidateDeferredSettingsFlush()
        invalidateDeferredMaskFlush()
        let attempt = DevelopCloseAttemptContext(controller: self)
        let task = Task { @MainActor [self] in
            await DevelopCloseCallbackContext.$attempt.withValue(attempt) {
                await performClose(attempt: attempt)
            }
        }
        closeTask = task
        let result = await task.value
        closeTask = nil
        return result
    }

    private func performClose(attempt: DevelopCloseAttemptContext) async -> Result<Void, Error> {
        if let error = drainPendingForClose() {
            let result: Result<Void, Error> = .failure(error)
            attempt.result = result
            closing = false
            return result
        }
        let session = session
        do {
            try await Task.detached(priority: .utility) { try session.close() }.value
        } catch {
            reportCloseFailure(error)
            let result: Result<Void, Error> = .failure(error)
            attempt.result = result
            closing = false
            return result
        }
        closed = true
        session.setListener(listener: nil)
        session.setMaskListener(listener: nil)
        onFrame = nil
        onSaved = nil
        onFailure = nil
        onNeedsFlush = nil
        onPatchSent = nil
        onSettingsReloaded = nil
        onMaskOverlay = nil
        onMaskJob = nil
        onPresentationChange = nil
        attempt.result = .success(())
        return .success(())
    }

    private func drainPendingForClose() -> Error? {
        let mask = flushMaskPendingResult(allowClosing: true)
        if let error = mask.error { return error }
        return flushSettingsPendingResult().error
    }

    /// Admission errors are reported once across synchronous callback reentry.
    @discardableResult
    func admitsMutation() -> Bool {
        guard closing || closed else { return true }
        guard !reportingAdmissionFailure else { return false }
        reportingAdmissionFailure = true
        defer { reportingAdmissionFailure = false }
        reportCloseFailure(closed ? DevelopCloseAdmissionError.closed : .closing)
        return false
    }

    func requireMutation() throws {
        guard admitsMutation() else {
            throw closed ? DevelopCloseAdmissionError.closed : .closing
        }
    }

    private func reportCloseFailure(_ error: Error) {
        onFailure?(error.localizedDescription)
    }

    // MARK: Surfaces

    /// Allocates and attaches the surface ring for a viewport of `width × height` device pixels
    /// in display orientation, in the format `presentation` asks for. No-op when the planned
    /// size and the format are unchanged. Returns the plan.
    @discardableResult
    public func attachSurfaces(viewWidth: Int, viewHeight: Int, count: Int = 3) throws -> SurfacePlan {
        try requireMutation()
        lastView = (viewWidth, viewHeight)
        let swap = info.orientation >= 5
        let w = UInt32(max(swap ? viewHeight : viewWidth, 1))
        let h = UInt32(max(swap ? viewWidth : viewHeight, 1))
        let next = session.planSurface(width: w, height: h)
        let float = presentation.floatSurfaces
        if next == plan, !surfaces.isEmpty, float == surfacesAreFloat { return next }
        var created: [UInt32: IOSurfaceRef] = [:]
        for _ in 0..<max(count, 1) {
            let s = float
                ? Self.makeSurface(width: Int(next.width), height: Int(next.height), bytesPerElement: 8,
                                   pixelFormat: Self.floatSurfacePixelFormat)
                : Self.makeSurface(width: Int(next.width), height: Int(next.height))
            guard let s else { throw DevelopError.surface }
            created[IOSurfaceGetID(s)] = s
        }
        // Keep the old ring alive until the engine has switched to the new one.
        for (id, _) in created {
            try session.attachSurface(iosurfaceId: id, width: next.width, height: next.height)
        }
        surfaces = created
        surfacesAreFloat = float
        plan = next
        if !maskOverlaySurfaces.isEmpty { try attachMaskOverlaySurfaces() }
        return next
    }

    public func surface(_ id: UInt32) -> IOSurfaceRef? { surfaces[id] }

    // MARK: EDR presentation (M2-22)

    /// Whether the recipe's HDR toggle is on (live value).
    public var hdrEnabled: Bool { (value(at: HDRControls.enabledPath) as? NSNumber)?.boolValue ?? false }

    /// The recipe's HDR headroom in stops (live value).
    public var hdrStops: Double { number(at: HDRControls.stopsPath) ?? 0 }

    /// Reports the screen showing the viewport (call on screen changes and periodically: the
    /// current headroom follows the display brightness). Switches the ring format when the
    /// presentation changes and tells the engine the display's current headroom.
    public func updateDisplay(_ screen: EDRScreen?) {
        guard admitsMutation() else { return }
        self.screen = screen.map(EDRScreenValues.init)
        syncPresentation()
    }

    /// The patch switching HDR on (with the display's full headroom when none is stored) or off.
    public func hdrPatch(_ on: Bool) -> [String: Any] {
        var output: [String: Any] = ["hdr": on]
        if on, hdrStops <= 0, presentation.defaultStops > 0 { output["hdr_headroom_stops"] = presentation.defaultStops }
        return ["output": output]
    }

    /// Re-resolves the presentation from the last screen and the live HDR toggle.
    func syncPresentation() {
        guard admitsMutation() else { return }
        let next = EDRPresentation.resolve(screen: screen, hdrEnabled: hdrEnabled)
        let old = presentation
        presentation = next
        if EDRPresentation.headroomChanged(next.displayHeadroom, reportedHeadroom)
            || (next.displayHeadroom == 1) != (reportedHeadroom == 1) {
            reportedHeadroom = next.displayHeadroom
            do { try session.setDisplayHeadroom(headroom: Float(next.displayHeadroom)) } catch {
                onFailure?(error.localizedDescription)
            }
        }
        if next.floatSurfaces != surfacesAreFloat, !surfaces.isEmpty, let v = lastView {
            do { try attachSurfaces(viewWidth: v.width, viewHeight: v.height) } catch {
                onFailure?(error.localizedDescription)
            }
        }
        if next != old { onPresentationChange?(next) }
    }

    static func makeSurface(width: Int, height: Int, bytesPerElement: Int = 4,
                            pixelFormat: UInt32 = surfacePixelFormat) -> IOSurfaceRef? {
        let props: [CFString: Any] = [
            kIOSurfaceWidth: width,
            kIOSurfaceHeight: height,
            kIOSurfaceBytesPerElement: bytesPerElement,
            kIOSurfaceBytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * bytesPerElement),
            kIOSurfacePixelFormat: pixelFormat,
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
        guard admitsMutation() else { return }
        var patch: [String: Any] = [p.field: value]
        if p.isWhiteBalance {
            // Moving either slider leaves As Shot: pin the other to its displayed value.
            let other: DevelopParameter = p == .temperature ? .tint : .temperature
            let pendingWB = pending[p.section] as? [String: Any]
            if isAsShotWhiteBalance, pendingWB?[other.field] == nil { patch[other.field] = self.value(other) }
            patch["mode"] = "custom"
        }
        apply(patch: [p.section: patch], interactive: interactive)
    }

    // MARK: Settings by JSON path (the develop panels)

    /// The live settings document (engine `DevelopSettings` JSON), including unsent changes.
    public var settingsObject: [String: Any] { settings }

    /// Value at a member path, e.g. `["color", "hsl", "hue", "red"]`.
    public func value(at path: [String]) -> Any? {
        var node: Any? = settings
        for key in path { node = (node as? [String: Any])?[key] }
        return node
    }

    /// Numeric value at a member path, or nil when absent.
    public func number(at path: [String]) -> Double? { (value(at: path) as? NSNumber)?.doubleValue }

    /// Sets one member. Interactive values are coalesced like `set(_:_:interactive:)`.
    public func set(path: [String], _ value: Any, interactive: Bool) {
        guard admitsMutation() else { return }
        apply(patch: Self.patch(path, value), interactive: interactive)
    }

    /// Merges an RFC 7386 patch (nested objects merge, arrays and scalars replace, `NSNull`
    /// removes) into the live settings and queues it for the engine.
    public func apply(patch: [String: Any], interactive: Bool) {
        guard admitsMutation() else { return }
        timingInput &+= 1
        PerformanceTrace.shared.record("input", session: timingSession, input: timingInput, backend: info.backend)
        pending = Self.merge(pending, patch, keepNulls: true)
        settings = Self.merge(settings, patch, keepNulls: false)
        pendingInteractive = interactive
        if interactive, let onNeedsFlush { onNeedsFlush() } else { _ = flushPending() }
        if patch["output"] != nil { syncPresentation() }
    }

    /// `["a","b"] + v` → `{"a":{"b":v}}`.
    nonisolated public static func patch(_ path: [String], _ value: Any) -> [String: Any] {
        guard let first = path.first else { return [:] }
        return [first: path.count == 1 ? value : patch(Array(path.dropFirst()), value)]
    }

    /// Compact JSON with sorted keys and shortest round-trip decimals (`0.6`, not
    /// `0.59999999999999998`), as sent to the engine and stored in presets.
    nonisolated public static func encode(_ obj: [String: Any], pretty: Bool = false) -> String? {
        let options: JSONSerialization.WritingOptions = pretty ? [.sortedKeys, .prettyPrinted] : [.sortedKeys]
        guard let data = try? JSONSerialization.data(withJSONObject: decimalized(obj), options: options) else { return nil }
        return String(decoding: data, as: UTF8.self)
    }

    nonisolated static func decimalized(_ v: Any) -> Any {
        switch v {
        case let d as [String: Any]: return d.mapValues(decimalized)
        case let a as [Any]: return a.map(decimalized)
        case let n as NSNumber where CFGetTypeID(n) != CFBooleanGetTypeID() && CFNumberIsFloatType(n):
            let x = n.doubleValue
            return x.isFinite ? NSDecimalNumber(string: "\(x)") : n
        default: return v
        }
    }

    /// RFC 7386 merge. With `keepNulls` (accumulating a patch) removals stay as `NSNull`.
    nonisolated public static func merge(_ target: [String: Any], _ patch: [String: Any], keepNulls: Bool) -> [String: Any] {
        var out = target
        for (k, v) in patch {
            if v is NSNull {
                if keepNulls { out[k] = v } else { out.removeValue(forKey: k) }
            } else if let obj = v as? [String: Any] {
                out[k] = merge(out[k] as? [String: Any] ?? [:], obj, keepNulls: keepNulls)
            } else {
                out[k] = v
            }
        }
        return out
    }

    /// White balance back to the camera's as-shot values.
    public func setAsShotWhiteBalance() {
        guard admitsMutation() else { return }
        pending["white_balance"] = ["mode": "as_shot"]
        pendingInteractive = false
        _ = flushPending()
        try? reloadSettings()
    }

    /// Sends the coalesced patch, if any. Returns whether a patch was attempted. Rejected
    /// engine writes are reported through `onFailure`.
    @discardableResult
    public func flushPending() -> Bool {
        guard !closing, !closed else { return false }
        let masks = flushMaskPendingResult(allowClosing: false)
        let settings = flushSettingsPendingResult()
        return masks.attempted || settings.attempted
    }

    private func flushSettingsPendingResult() -> (attempted: Bool, error: Error?) {
        let span = PerformanceTrace.shared.begin("flush", session: timingSession, input: timingInput)
        defer { PerformanceTrace.shared.end(span) }
        guard !pending.isEmpty, !closed else { return (false, nil) }
        guard let json = Self.encode(pending) else {
            let error = DevelopSettingsEncodingError()
            reportCloseFailure(error)
            return (false, error)
        }
        guard !settingsFlushInFlight else {
            settingsFlushRequested = true
            return (false, nil)
        }
        invalidateDeferredSettingsFlush()
        let patch = pending
        let interactive = pendingInteractive
        pending.removeAll(keepingCapacity: true)
        pendingInteractive = false
        settingsFlushInFlight = true
        onPatchSent?(json)
        let ffiSpan = PerformanceTrace.shared.begin("ffi", session: timingSession, input: timingInput)
        defer { PerformanceTrace.shared.end(ffiSpan) }
        var accepted = false
        var failure: Error?
        do {
            try session.setSettings(jsonPatch: json, interactive: interactive)
            accepted = true
        } catch {
            failure = error
            let newerPending = !pending.isEmpty
            let newerInteractive = pendingInteractive
            pending = Self.merge(patch, pending, keepNulls: true)
            pendingInteractive = newerPending ? (interactive && newerInteractive) : interactive
            onFailure?(error.localizedDescription)
        }
        settingsFlushInFlight = false
        let shouldFlushNewerSettings = settingsFlushRequested
        settingsFlushRequested = false
        if accepted, shouldFlushNewerSettings, !pending.isEmpty {
            scheduleDeferredSettingsFlush()
        }
        return (true, failure)
    }

    private func invalidateDeferredSettingsFlush() {
        deferredSettingsFlushID = UUID()
        deferredSettingsFlush?.cancel()
        deferredSettingsFlush = nil
    }

    /// Handles a requested reentrant noninteractive update after the current settings call
    /// returns. Deferring both the host notification and fallback flush prevents recursive
    /// backend calls when a host handles `onNeedsFlush` synchronously.
    private func scheduleDeferredSettingsFlush() {
        deferredSettingsFlushID = UUID()
        let generation = deferredSettingsFlushID
        deferredSettingsFlush?.cancel()
        deferredSettingsFlush = Task { @MainActor [weak self] in
            await Task.yield()
            guard let self, !Task.isCancelled,
                  self.deferredSettingsFlushID == generation, !self.closed, !self.closing else { return }
            self.deferredSettingsFlush = nil
            if let onNeedsFlush = self.onNeedsFlush {
                onNeedsFlush()
            } else {
                _ = self.flushPending()
            }
        }
    }

    /// Makes everything since the last commit one undo step labelled `label`.
    @discardableResult
    public func commit(label: String) -> Bool {
        guard admitsMutation() else { return false }
        flushPending()
        let recorded = (try? session.commit(label: label)) ?? false
        if recorded { ignoredSettings = (try? session.ignoredSettings()) ?? ignoredSettings }
        refreshHistory()
        return recorded
    }

    public func undo() throws -> Bool { try requireMutation(); return try historyMove { try session.undo() } }
    public func redo() throws -> Bool { try requireMutation(); return try historyMove { try session.redo() } }
    public func reset() throws -> Bool { try requireMutation(); return try historyMove { try session.reset() } }

    public func snapshot(named name: String) throws {
        try requireMutation()
        flushPending()
        try session.snapshot(name: name)
        refreshHistory()
    }

    public func restoreSnapshot(named name: String) throws {
        _ = try historyMove { try session.restoreSnapshot(name: name); return true }
    }

    /// Applied steps oldest first, then the undone steps redo would reapply.
    public func historyItems() -> [HistoryItem] { (try? session.historyItems()) ?? [] }

    /// Moves to the state after history step `id` (nil: the original state).
    public func checkoutHistory(_ id: UInt64?) throws -> Bool {
        try requireMutation()
        return try historyMove { try session.checkoutHistory(id: id) }
    }

    /// Turns a step's changes off or back on (recorded as a new step).
    public func setHistoryStep(_ id: UInt64, enabled: Bool) throws -> Bool {
        try requireMutation()
        return try historyMove { try session.setHistoryStepEnabled(id: id, enabled: enabled) }
    }

    /// Named history groups (the agent's "Agent base edit" and redos) with their amount.
    public func historyGroups() -> [HistoryGroupState] { (try? session.historyGroups()) ?? [] }

    /// Amount slider drag: previews `amount` of `group` through the coalesced patch path.
    public func previewGroupAmount(_ group: HistoryGroupState, _ amount: Double) {
        guard admitsMutation() else { return }
        guard let patch = AgentFade.patch(current: settings, withoutJSON: group.withoutJson,
                                          withJSON: group.withJson, amount: amount), !patch.isEmpty else { return }
        apply(patch: patch, interactive: true)
    }

    /// Amount slider release: the engine records the amount as one undo step.
    @discardableResult
    public func commitGroupAmount(_ groupID: UInt32, _ amount: Double) throws -> Bool {
        try requireMutation()
        return try historyMove { try session.commitGroupAmount(groupId: groupID, amount: min(max(amount, 0), 1)) }
    }

    /// Applies a partial recipe (preset) as one undo step labelled `label`.
    @discardableResult
    public func applyPreset(_ patch: [String: Any], label: String) -> Bool {
        guard admitsMutation() else { return false }
        apply(patch: patch, interactive: false)
        let recorded = commit(label: label)
        onSettingsReloaded?()
        return recorded
    }

    // MARK: Tools

    /// Crop tool: the engine renders the whole frame while on.
    public func setCropEditing(_ on: Bool) {
        guard admitsMutation() else { return }
        flushPending()
        do { try session.setCropEditing(editing: on) } catch { onFailure?(error.localizedDescription) }
    }

    /// ⌥ on the Masking slider: frames show the sharpening mask while on.
    public func setMaskingPreview(_ on: Bool) {
        guard admitsMutation() else { return }
        do { try session.setMaskingPreview(enabled: on) } catch { onFailure?(error.localizedDescription) }
    }

    /// 1:1 crop into `surface` (blocking; call off the main actor through `DetailPreviewRenderer`).
    nonisolated public static func renderDetail(session: DevelopSession, into surface: IOSurfaceRef,
                                                centerX: Double, centerY: Double) throws -> DetailPreview {
        try session.renderDetailPreview(iosurfaceId: IOSurfaceGetID(surface),
                                        width: UInt32(IOSurfaceGetWidth(surface)),
                                        height: UInt32(IOSurfaceGetHeight(surface)),
                                        centerX: Float(centerX), centerY: Float(centerY))
    }

    public static func makeDetailSurface(width: Int, height: Int) -> IOSurfaceRef? {
        makeSurface(width: width, height: height)
    }

    private func historyMove(_ body: () throws -> Bool) throws -> Bool {
        try requireMutation()
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
        syncPresentation()
        onSettingsReloaded?()
    }

    private func refreshHistory() {
        if let h = try? session.historyState() { history = h }
    }

    // MARK: Engine callbacks (main actor)

    fileprivate func didRender(_ info: FrameInfo) {
        let span = PerformanceTrace.shared.begin("callback_drain", session: timingSession)
        defer { PerformanceTrace.shared.end(span) }
        PerformanceTrace.shared.record("callback_drain", session: timingSession, generation: info.generation,
                                       width: Int(info.width), height: Int(info.height), level: Int(info.level),
                                       backend: self.info.backend, engineSinkMs: info.renderMs)
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
    private final class Events: DevelopListener, MaskListener, @unchecked Sendable {
        let timingSession: String
        init(session: String) { timingSession = session }
        @MainActor weak var owner: DevelopController?
        func frameReady(frame: FrameInfo) {
            PerformanceTrace.shared.record("callback_enqueue", session: timingSession, generation: frame.generation,
                                           width: Int(frame.width), height: Int(frame.height), level: Int(frame.level),
                                           engineSinkMs: frame.renderMs)
            DispatchQueue.main.async { MainActor.assumeIsolated { self.owner?.didRender(frame) } }
        }
        func renderFailed(message: String) {
            DispatchQueue.main.async { MainActor.assumeIsolated { self.owner?.didFail(message) } }
        }
        func saved(recipeHash: String) {
            DispatchQueue.main.async { MainActor.assumeIsolated { self.owner?.didSave(recipeHash) } }
        }
        func overlayReady(frame: MaskOverlayFrame) {
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let o = self.owner, !o.closed else { return }
                    o.onMaskOverlay?(frame)
                }
            }
        }
        func aiProgress(update: MaskJobUpdate) {
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let o = self.owner, !o.closed else { return }
                    o.onMaskJob?(update)
                }
            }
        }
    }
}

public enum DevelopError: LocalizedError {
    case surface
    public var errorDescription: String? { "Could not allocate the viewport IOSurface" }
}
