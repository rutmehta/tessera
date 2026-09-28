import Foundation
import IOSurface
import TesseraFFI

/// A local slider of one mask group (the coalescing key).
public struct MaskParamKey: Hashable, Sendable {
    public let group: UInt32
    public let name: String
    public init(group: UInt32, name: String) { self.group = group; self.name = name }
}

/// Masking on the develop session (M2-14). Structural edits (add, delete, modes) go to the engine at
/// once; drags — local sliders, the amount slider, gradient handles and brush samples — are
/// coalesced to one engine call per display frame through the same `onNeedsFlush` tick as the
/// Basic sliders. Nothing here commits: callers commit one undo step on mouse-up.
extension DevelopController {
    /// `'L008'`: one byte per pixel, imported by Metal as `.r8Unorm`.
    public static let overlayPixelFormat: UInt32 = 0x4C30_3038

    // MARK: Groups

    public func maskGroups() -> [MaskGroupInfo] { (try? session.maskGroups()) ?? [] }

    private func run<T>(_ body: () throws -> T) -> T? {
        flushPending()
        do { return try body() } catch { onFailure?(error.localizedDescription); return nil }
    }

    @discardableResult
    public func addMask(_ definitionJSON: String, interactive: Bool = false) -> UInt32? {
        run { try session.addMask(definitionJson: definitionJSON, interactive: interactive) }
    }

    @discardableResult
    public func addMaskComponent(_ group: UInt32, _ definitionJSON: String, combine: MaskCombineMode,
                                 interactive: Bool = false) -> UInt32? {
        run { try session.addMaskComponent(groupId: group, definitionJson: definitionJSON, combine: combine,
                                           interactive: interactive) }
    }

    public func setMaskComponentMode(_ group: UInt32, _ index: Int, combine: MaskCombineMode, invert: Bool) {
        run { try session.setMaskComponentMode(groupId: group, index: UInt32(index), combine: combine, invert: invert) }
    }

    public func removeMaskComponent(_ group: UInt32, _ index: Int) {
        run { try session.removeMaskComponent(groupId: group, index: UInt32(index)) }
    }

    public func deleteMask(_ group: UInt32) { run { try session.deleteMask(groupId: group) } }

    @discardableResult
    public func duplicateMask(_ group: UInt32) -> UInt32? { run { try session.duplicateMask(groupId: group) } }

    public func resetMaskParams(_ group: UInt32) { run { try session.resetMaskParams(groupId: group) } }

    public func retryAIMask(key: String) { run { try session.retryAiMask(key: key) } }

    /// Name, visibility, amount, invert. Interactive changes (the amount slider) are coalesced.
    public func updateMaskGroup(_ group: UInt32, _ patch: MaskGroupPatch, interactive: Bool) {
        if interactive {
            var merged = pendingMaskGroup[group]?.patch ?? MaskGroupPatch(name: nil, enabled: nil, amount: nil, invert: nil)
            merged.name = patch.name ?? merged.name
            merged.enabled = patch.enabled ?? merged.enabled
            merged.amount = patch.amount ?? merged.amount
            merged.invert = patch.invert ?? merged.invert
            pendingMaskGroup[group] = (merged, true)
            requestFlush()
        } else {
            pendingMaskGroup.removeValue(forKey: group)
            run { try session.updateMaskGroup(groupId: group, patch: patch, interactive: false) }
        }
    }

    /// A local slider value; interactive values are coalesced per display frame.
    public func setMaskParam(_ group: UInt32, _ name: String, _ value: Double, interactive: Bool) {
        let key = MaskParamKey(group: group, name: name)
        if interactive {
            pendingMaskParams[key] = Float(value)
            requestFlush()
        } else {
            pendingMaskParams.removeValue(forKey: key)
            run { try session.setMaskParam(groupId: group, name: name, value: Float(value), interactive: false) }
        }
    }

    /// Gradient handle drags: the latest definition per frame; final values are sent at once.
    public func setMaskComponent(_ group: UInt32, _ index: Int, json: String, interactive: Bool) {
        if interactive {
            pendingComponent = (group, UInt32(index), json)
            requestFlush()
        } else {
            pendingComponent = nil
            run { try session.setMaskComponent(groupId: group, index: UInt32(index), definitionJson: json, interactive: false) }
        }
    }

    // MARK: Brush

    /// Starts a stroke (a new group when `group` is nil); returns the group painted into.
    public func beginBrushStroke(group: UInt32?, radius: Double, feather: Double, flow: Double, erase: Bool) -> UInt32? {
        let id = run {
            try session.beginBrushStroke(groupId: group, brush: BrushSettings(radius: Float(radius), feather: Float(feather),
                                                                             flow: Float(flow), erase: erase))
        }
        // Samples closer than a tenth of the radius add nothing visible.
        if id != nil { pendingStroke = StrokeCoalescer(spacing: radius * 0.1) }
        return id
    }

    /// Records a sample; sent with the next display-frame flush.
    public func addBrushSample(x: Double, y: Double, pressure: Double) {
        guard pendingStroke != nil else { return }
        if pendingStroke?.add(x: x, y: y, pressure: pressure) == true { requestFlush() }
    }

    /// Sends the remaining samples and ends the stroke (then commit).
    public func endBrushStroke() {
        guard var stroke = pendingStroke else { return }
        pendingStroke = nil
        let rest = stroke.finish()
        run {
            if !rest.isEmpty { try session.addBrushPoints(points: rest) }
            try session.endBrushStroke()
        }
    }

    // MARK: Ranges and AI

    /// Samples the image at a mask-space point (blocking in the engine; call from a task).
    public func addRangeMask(group: UInt32?, kind: RangeKind, x: Double, y: Double, combine: MaskCombineMode) -> UInt32? {
        run { try session.addRangeMask(groupId: group, kind: kind, x: Float(x), y: Float(y), combine: combine) }
    }

    public func addColorRangeSample(group: UInt32, index: Int, x: Double, y: Double) {
        run { try session.addColorRangeSample(groupId: group, index: UInt32(index), x: Float(x), y: Float(y)) }
    }

    @discardableResult
    public func addAIMask(group: UInt32?, _ request: AiMaskRequest, combine: MaskCombineMode) -> UInt32? {
        run { try session.addAiMask(groupId: group, request: request, combine: combine) }
    }

    // MARK: Overlay

    /// Allocates two R8 overlay surfaces of the planned size (idempotent per size).
    public func attachMaskOverlaySurfaces() throws {
        guard let plan else { return }
        if let s = maskOverlaySurfaces.values.first, IOSurfaceGetWidth(s) == Int(plan.width),
           IOSurfaceGetHeight(s) == Int(plan.height), maskOverlaySurfaces.count == 2 { return }
        var created: [UInt32: IOSurfaceRef] = [:]
        for _ in 0..<2 {
            guard let s = Self.makeSurface(width: Int(plan.width), height: Int(plan.height), bytesPerElement: 1,
                                           pixelFormat: Self.overlayPixelFormat) else { throw DevelopError.surface }
            created[IOSurfaceGetID(s)] = s
        }
        for id in created.keys {
            try session.attachMaskOverlaySurface(iosurfaceId: id, width: plan.width, height: plan.height)
        }
        maskOverlaySurfaces = created
    }

    public func maskOverlaySurface(_ id: UInt32) -> IOSurfaceRef? { maskOverlaySurfaces[id] }

    /// Shows `group` in the overlay after every frame (nil: off).
    public func setMaskOverlay(_ group: UInt32?) {
        do {
            if group != nil { try attachMaskOverlaySurfaces() }
            try session.setMaskOverlay(groupId: group)
        } catch {
            onFailure?(error.localizedDescription)
        }
    }

    public func maskThumbnail(_ group: UInt32, maxPixels: Int) -> MaskThumbnail? {
        (try? session.maskThumbnail(groupId: group, maxPx: UInt32(maxPixels))) ?? nil
    }

    // MARK: Coalescing

    private func requestFlush() {
        if let onNeedsFlush { onNeedsFlush() } else { flushMaskPending() }
    }

    /// Whether mask changes are waiting for the next display frame.
    public var hasPendingMaskChanges: Bool {
        !pendingMaskParams.isEmpty || !pendingMaskGroup.isEmpty || pendingComponent != nil
            || pendingStroke?.isEmpty == false
    }

    /// Sends the coalesced mask changes (called from `flushPending`). Returns whether any was sent.
    @discardableResult
    func flushMaskPending() -> Bool {
        guard !closed, hasPendingMaskChanges else { return false }
        do {
            if var stroke = pendingStroke, !stroke.isEmpty {
                try session.addBrushPoints(points: stroke.take())
                pendingStroke = stroke
            }
            if let c = pendingComponent {
                pendingComponent = nil
                try session.setMaskComponent(groupId: c.group, index: c.index, definitionJson: c.json, interactive: true)
            }
            let groups = pendingMaskGroup
            pendingMaskGroup.removeAll()
            for (id, p) in groups.sorted(by: { $0.key < $1.key }) {
                try session.updateMaskGroup(groupId: id, patch: p.patch, interactive: p.interactive)
            }
            let params = pendingMaskParams
            pendingMaskParams.removeAll()
            for (key, value) in params.sorted(by: { ($0.key.group, $0.key.name) < ($1.key.group, $1.key.name) }) {
                try session.setMaskParam(groupId: key.group, name: key.name, value: value, interactive: true)
            }
        } catch {
            onFailure?(error.localizedDescription)
        }
        return true
    }
}
