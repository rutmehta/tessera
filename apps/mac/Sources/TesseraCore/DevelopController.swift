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
    /// Every JSON merge patch sent to the engine (tests, diagnostics).
    public var onPatchSent: ((String) -> Void)?
    /// Settings changed outside a local slider drag (undo, preset, snapshot, history).
    public var onSettingsReloaded: (() -> Void)?

    private var settings: [String: Any] = [:]
    private var pending: [String: Any] = [:]
    private var pendingInteractive = false
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
    private(set) var closed = false

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
        session.setMaskListener(listener: events)
        try reloadSettings()
    }

    /// Stops rendering and writes pending edits (off the main actor). Idempotent.
    public func close() async {
        guard !closed else { return }
        closed = true
        _ = flushPending()
        session.setListener(listener: nil)
        session.setMaskListener(listener: nil)
        let session = session
        await Task.detached(priority: .utility) {
            try? session.close()
        }.value
    }

    // MARK: Surfaces

    /// Allocates and attaches the surface ring for a viewport of `width × height` device pixels
    /// in display orientation, in the format `presentation` asks for. No-op when the planned
    /// size and the format are unchanged. Returns the plan.
    @discardableResult
    public func attachSurfaces(viewWidth: Int, viewHeight: Int, count: Int = 3) throws -> SurfacePlan {
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
        apply(patch: Self.patch(path, value), interactive: interactive)
    }

    /// Merges an RFC 7386 patch (nested objects merge, arrays and scalars replace, `NSNull`
    /// removes) into the live settings and queues it for the engine.
    public func apply(patch: [String: Any], interactive: Bool) {
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
        pending["white_balance"] = ["mode": "as_shot"]
        pendingInteractive = false
        _ = flushPending()
        try? reloadSettings()
    }

    /// Sends the coalesced patch, if any. Returns whether something was sent.
    @discardableResult
    public func flushPending() -> Bool {
        let masks = flushMaskPending()
        guard !pending.isEmpty, !closed, let json = Self.encode(pending) else { return masks }
        pending.removeAll()
        onPatchSent?(json)
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

    /// Applied steps oldest first, then the undone steps redo would reapply.
    public func historyItems() -> [HistoryItem] { (try? session.historyItems()) ?? [] }

    /// Moves to the state after history step `id` (nil: the original state).
    public func checkoutHistory(_ id: UInt64?) throws -> Bool {
        try historyMove { try session.checkoutHistory(id: id) }
    }

    /// Turns a step's changes off or back on (recorded as a new step).
    public func setHistoryStep(_ id: UInt64, enabled: Bool) throws -> Bool {
        try historyMove { try session.setHistoryStepEnabled(id: id, enabled: enabled) }
    }

    /// Applies a partial recipe (preset) as one undo step labelled `label`.
    @discardableResult
    public func applyPreset(_ patch: [String: Any], label: String) -> Bool {
        apply(patch: patch, interactive: false)
        let recorded = commit(label: label)
        onSettingsReloaded?()
        return recorded
    }

    // MARK: Tools

    /// Crop tool: the engine renders the whole frame while on.
    public func setCropEditing(_ on: Bool) {
        flushPending()
        do { try session.setCropEditing(editing: on) } catch { onFailure?(error.localizedDescription) }
    }

    /// ⌥ on the Masking slider: frames show the sharpening mask while on.
    public func setMaskingPreview(_ on: Bool) {
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
