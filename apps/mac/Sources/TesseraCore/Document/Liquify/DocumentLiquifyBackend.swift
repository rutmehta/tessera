import CoreGraphics
import Foundation

// Filter ▸ Liquify (WP B5-13): the protocol document backends adopt, mirroring the B5-13 session calls
// (crates/tessera-ffi/src/document/liquify.rs). The mesh and its freeze plane live engine-side; the app keeps
// only UI state. The UI-free parts (tools, brush controls, output rules, view mapping, the overlay geometry, the
// preview throttle) live here and are unit tested (DocumentLiquifyTests).

/// The ten Liquify brush tools (FFI `LiquifyTool`), in Photoshop's order.
public enum LiquifyToolKind: String, CaseIterable, Sendable, Identifiable {
    case forwardWarp, reconstruct, smooth, twirlClockwise, twirlCounterClockwise, pucker, bloat, pushLeft, freeze, thaw
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .forwardWarp: "Forward Warp"
        case .reconstruct: "Reconstruct"
        case .smooth: "Smooth"
        case .twirlClockwise: "Twirl Clockwise"
        case .twirlCounterClockwise: "Twirl Counterclockwise"
        case .pucker: "Pucker"
        case .bloat: "Bloat"
        case .pushLeft: "Push Left"
        case .freeze: "Freeze Mask"
        case .thaw: "Thaw Mask"
        }
    }

    /// Photoshop's Liquify keys (Twirl Counterclockwise: ⌥ with Twirl, or its own button).
    public var key: Character? {
        switch self {
        case .forwardWarp: "w"
        case .reconstruct: "r"
        case .smooth: "e"
        case .twirlClockwise: "c"
        case .twirlCounterClockwise: nil
        case .pucker: "s"
        case .bloat: "b"
        case .pushLeft: "o"
        case .freeze: "f"
        case .thaw: "d"
        }
    }

    public var symbol: String {
        switch self {
        case .forwardWarp: "hand.point.up.left"
        case .reconstruct: "arrow.uturn.backward"
        case .smooth: "water.waves"
        case .twirlClockwise: "arrow.clockwise"
        case .twirlCounterClockwise: "arrow.counterclockwise"
        case .pucker: "arrow.down.right.and.arrow.up.left"
        case .bloat: "arrow.up.left.and.arrow.down.right"
        case .pushLeft: "arrow.left.to.line"
        case .freeze: "lock"
        case .thaw: "lock.open"
        }
    }

    /// Tools that keep acting while the pointer rests (the app repeats the last point on a timer); Rate
    /// scales them. Forward Warp and Push Left only act along the drag.
    public var actsInPlace: Bool { self != .forwardWarp && self != .pushLeft }

    /// Uses the Rate control.
    public var usesRate: Bool { actsInPlace }

    /// The tool for a key, or nil.
    public static func forKey(_ c: Character) -> LiquifyToolKind? {
        allCases.first { $0.key == Character(c.lowercased()) }
    }

    /// ⌥ while twirling turns the other way.
    public func withOption(_ option: Bool) -> LiquifyToolKind {
        guard option else { return self }
        switch self {
        case .twirlClockwise: return .twirlCounterClockwise
        case .twirlCounterClockwise: return .twirlClockwise
        case .pucker: return .bloat
        case .bloat: return .pucker
        case .freeze: return .thaw
        case .thaw: return .freeze
        default: return self
        }
    }
}

/// Brush controls as the panel shows them: size in source pixels, the others in percent.
public struct LiquifyBrushSettings: Equatable, Sendable {
    public static let sizeRange: ClosedRange<Double> = 1...15000
    public private(set) var size: Double = 100
    /// Hard core, 0…100 %.
    public private(set) var density: Double = 50
    /// 1…100 %.
    public private(set) var pressure: Double = 100
    /// 0…100 % (tools that act in place).
    public private(set) var rate: Double = 80

    public init(size: Double = 100, density: Double = 50, pressure: Double = 100, rate: Double = 80) {
        setSize(size); setDensity(density); setPressure(pressure); setRate(rate)
    }

    public mutating func setSize(_ v: Double) { size = v.isFinite ? min(max(v, Self.sizeRange.lowerBound), Self.sizeRange.upperBound) : size }
    public mutating func setDensity(_ v: Double) { density = v.isFinite ? min(max(v, 0), 100) : density }
    public mutating func setPressure(_ v: Double) { pressure = v.isFinite ? min(max(v, 1), 100) : pressure }
    public mutating func setRate(_ v: Double) { rate = v.isFinite ? min(max(v, 0), 100) : rate }

    /// `[` / `]`: Photoshop's size steps.
    public mutating func step(larger: Bool) {
        let s = size
        let d: Double = s < 10 ? 1 : s < 100 ? 10 : s < 200 ? 25 : s < 500 ? 50 : 100
        setSize(larger ? s + d : s - d)
    }

    /// The engine's normalized controls (`filters::liquify::Brush`).
    public var engine: (size: Float, density: Float, pressure: Float, rate: Float) {
        (Float(size), Float(density / 100), Float(pressure / 100), Float(rate / 100))
    }
}

/// Where Apply puts the result (FFI `LiquifyDestination`).
public enum LiquifyOutput: String, CaseIterable, Sendable, Identifiable {
    case currentLayer, newLayer, smartFilter
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .currentLayer: "Current layer"
        case .newLayer: "New layer"
        case .smartFilter: "Smart filter"
        }
    }

    /// Which outputs a target allows, with the reason for each one it does not (the panel lists those in
    /// tertiary text rather than hiding them).
    public static func availability(layerKind: LayerKindTag, hasSelection: Bool, reEditing: Bool) -> [(LiquifyOutput, String?)] {
        if layerKind == .smartObject {
            return [
                (.smartFilter, nil),
                (.currentLayer, "A smart object keeps its pixels; Liquify is added as a smart filter"),
                (.newLayer, "New layer output needs a pixel layer"),
            ]
        }
        return [
            (.currentLayer, nil),
            (.newLayer, nil),
            (.smartFilter, reEditing ? nil : hasSelection ? "Works without a selection (the selection only freezes the mesh)" : nil),
        ]
    }

    /// The default output for a target.
    public static func preferred(layerKind: LayerKindTag) -> LiquifyOutput { layerKind == .smartObject ? .smartFilter : .currentLayer }
}

/// An open workspace (FFI `LiquifySessionInfo`).
public struct LiquifyWorkspaceInfo: Equatable, Sendable {
    public var token: UInt64
    public var layer: DocLayerID
    public var stageIndex: UInt32?
    public var smartObject: Bool
    public var width: UInt32
    public var height: UInt32
    public var cellSize: UInt32
    public var columns: UInt32
    public var rows: UInt32
    public var previewWidth: UInt32
    public var previewHeight: UInt32
    public var previewFactor: UInt32
    public var selectionFrozen: Bool
    public var edited: Bool
    public init(token: UInt64, layer: DocLayerID, stageIndex: UInt32?, smartObject: Bool, width: UInt32, height: UInt32,
                cellSize: UInt32, columns: UInt32, rows: UInt32, previewWidth: UInt32, previewHeight: UInt32,
                previewFactor: UInt32, selectionFrozen: Bool, edited: Bool) {
        self.token = token; self.layer = layer; self.stageIndex = stageIndex; self.smartObject = smartObject
        self.width = width; self.height = height; self.cellSize = cellSize; self.columns = columns; self.rows = rows
        self.previewWidth = previewWidth; self.previewHeight = previewHeight; self.previewFactor = previewFactor
        self.selectionFrozen = selectionFrozen; self.edited = edited
    }
    public var sourceSize: CGSize { CGSize(width: Double(width), height: Double(height)) }
}

/// One pointer sample in source pixels (FFI `LiquifyPoint`).
public struct LiquifyInputPoint: Equatable, Sendable {
    public var x: Float
    public var y: Float
    public var pressure: Float
    public init(x: Float, y: Float, pressure: Float = 1) { self.x = x; self.y = y; self.pressure = pressure }
    public init(_ p: CGPoint, pressure: Float = 1) { self.init(x: Float(p.x), y: Float(p.y), pressure: pressure) }
}

/// What one brush call did (FFI `LiquifyStrokeResult`).
public struct LiquifyStrokeInfo: Equatable, Sendable {
    public var dabs: UInt32
    public var dirty: CanvasRect?
    public var millis: Double
    public init(dabs: UInt32, dirty: CanvasRect?, millis: Double) { self.dabs = dabs; self.dirty = dirty; self.millis = millis }
}

/// A preview surface (FFI `LiquifyPreview`): RGBA8, straight alpha.
public struct LiquifyPreviewFrame: Equatable, Sendable {
    public var surfaceId: UInt32
    public var width: UInt32
    public var height: UInt32
    public var original: Bool
    public var millis: Double
    public init(surfaceId: UInt32, width: UInt32, height: UInt32, original: Bool, millis: Double) {
        self.surfaceId = surfaceId; self.width = width; self.height = height; self.original = original; self.millis = millis
    }
}

/// The engine mesh for the overlay (FFI `LiquifyMeshRecord`). Displacements are inverse: output (x, y)
/// samples source (x + dx, y + dy).
public struct LiquifyMeshData: Equatable, Sendable {
    public var columns: Int
    public var rows: Int
    public var cellSize: Int
    /// Interleaved (dx, dy) per node, row-major.
    public var displacement: [Float]
    /// Freeze weight per node, 0…1.
    public var freeze: [Float]
    public var maxDisplacement: Float
    public init(columns: Int, rows: Int, cellSize: Int, displacement: [Float], freeze: [Float], maxDisplacement: Float) {
        self.columns = columns; self.rows = rows; self.cellSize = cellSize; self.displacement = displacement
        self.freeze = freeze; self.maxDisplacement = maxDisplacement
    }

    public var isEdited: Bool { maxDisplacement > 0 }
    public var hasFreeze: Bool { freeze.contains { $0 > 0 } }

    /// Where node (c, r)'s content appears after the warp, in source pixels: the node minus its inverse
    /// displacement (first-order forward map; exact where the field is locally constant).
    public func warpedNode(_ c: Int, _ r: Int) -> CGPoint {
        let i = r * columns + c
        let (dx, dy) = (Double(displacement[2 * i]), Double(displacement[2 * i + 1]))
        return CGPoint(x: Double(c * cellSize) - dx, y: Double(r * cellSize) - dy)
    }

    /// Mesh lines every `step` nodes (rows then columns), warped, in source pixels.
    public func gridLines(step: Int) -> [[CGPoint]] {
        let s = max(step, 1)
        var lines: [[CGPoint]] = []
        for r in stride(from: 0, to: rows, by: s) { lines.append((0..<columns).map { warpedNode($0, r) }) }
        for c in stride(from: 0, to: columns, by: s) { lines.append((0..<rows).map { warpedNode(c, $0) }) }
        return lines
    }

    /// The node step that keeps mesh lines at least `minSpacing` view points apart at `scale` points per
    /// source pixel (Photoshop's small / medium / large mesh are multiples of it).
    public func lineStep(scale: Double, minSpacing: Double) -> Int {
        let spacing = Double(cellSize) * scale
        guard spacing > 0 else { return 1 }
        return max(1, Int((minSpacing / spacing).rounded(.up)))
    }

    /// The freeze plane as 8-bit grey (255 = frozen), one byte per node, row-major.
    public var freezeBytes: [UInt8] { freeze.map { UInt8((min(max($0, 0), 1) * 255).rounded()) } }
}

/// View mapping of the workspace canvas: `scale` view points per source pixel, `origin` the view point of the
/// source's top-left corner (the view is flipped: y grows down, like the source).
public struct LiquifyViewTransform: Equatable, Sendable {
    public static let scaleRange: ClosedRange<Double> = 0.01...32
    public var scale: Double
    public var origin: CGPoint

    public init(scale: Double, origin: CGPoint) {
        self.scale = min(max(scale, Self.scaleRange.lowerBound), Self.scaleRange.upperBound)
        self.origin = origin
    }

    /// The whole source centred in `view` with `margin` points around it.
    public static func fit(source: CGSize, in view: CGSize, margin: Double) -> LiquifyViewTransform {
        guard source.width > 0, source.height > 0 else { return LiquifyViewTransform(scale: 1, origin: .zero) }
        let w = max(Double(view.width) - 2 * margin, 1), h = max(Double(view.height) - 2 * margin, 1)
        let s = min(w / Double(source.width), h / Double(source.height))
        let t = LiquifyViewTransform(scale: s, origin: .zero)
        let x = (Double(view.width) - Double(source.width) * t.scale) / 2
        let y = (Double(view.height) - Double(source.height) * t.scale) / 2
        return LiquifyViewTransform(scale: t.scale, origin: CGPoint(x: x, y: y))
    }

    public func viewPoint(source p: CGPoint) -> CGPoint {
        CGPoint(x: Double(origin.x) + Double(p.x) * scale, y: Double(origin.y) + Double(p.y) * scale)
    }

    public func sourcePoint(view p: CGPoint) -> CGPoint {
        CGPoint(x: (Double(p.x) - Double(origin.x)) / scale, y: (Double(p.y) - Double(origin.y)) / scale)
    }

    public func viewRect(source size: CGSize) -> CGRect {
        CGRect(x: origin.x, y: origin.y, width: Double(size.width) * scale, height: Double(size.height) * scale)
    }

    /// Zoom by `factor` keeping the source point under `anchor` (view) fixed.
    public mutating func zoom(by factor: Double, around anchor: CGPoint) {
        guard factor.isFinite, factor > 0 else { return }
        let s = sourcePoint(view: anchor)
        scale = min(max(scale * factor, Self.scaleRange.lowerBound), Self.scaleRange.upperBound)
        origin = CGPoint(x: Double(anchor.x) - Double(s.x) * scale, y: Double(anchor.y) - Double(s.y) * scale)
    }

    public mutating func pan(dx: Double, dy: Double) {
        origin = CGPoint(x: Double(origin.x) + dx, y: Double(origin.y) + dy)
    }

    /// Brush diameter in view points.
    public func viewLength(_ source: Double) -> Double { source * scale }
}

/// Latest-wins scheduling for slow renders (Liquify previews, Content-Aware Move): at most one in flight; a
/// request while one runs marks it dirty so exactly one more runs after it, with the newest state.
public struct PreviewThrottle: Equatable, Sendable {
    public private(set) var inFlight = false
    public private(set) var dirty = false
    public init() {}

    /// A new state to show. True: start a render now.
    public mutating func request() -> Bool {
        if inFlight { dirty = true; return false }
        inFlight = true
        return true
    }

    /// The running render ended. True: start another one now (the state changed meanwhile).
    public mutating func finished() -> Bool {
        if dirty { dirty = false; return true }
        inFlight = false
        return false
    }

    /// Forget pending work (cancel, close).
    public mutating func reset() { inFlight = false; dirty = false }
}

/// Filter ▸ Liquify… enablement.
public enum LiquifyMenuState {
    public static func enabled(layerKind: LayerKindTag?, busy: Bool) -> Bool {
        guard let k = layerKind, k == .pixel || k == .smartObject else { return false }
        return !busy
    }
}

public protocol DocumentLiquifyBackend: AnyObject, Sendable {
    func beginLiquify(layer: DocLayerID, stageIndex: UInt32?) throws -> LiquifyWorkspaceInfo
    func liquifyBrush(token: UInt64, tool: LiquifyToolKind, brush: LiquifyBrushSettings,
                      points: [LiquifyInputPoint]) throws -> LiquifyStrokeInfo
    func liquifyEndStroke(token: UInt64) throws
    func liquifyMesh(token: UInt64) throws -> LiquifyMeshData
    func liquifyReconstructAll(token: UInt64, amount: Double) throws
    func liquifyReset(token: UInt64, keepFreeze: Bool) throws
    func liquifyFreezeAll(token: UInt64, frozen: Bool) throws
    func previewLiquify(token: UInt64, original: Bool) throws -> LiquifyPreviewFrame
    /// Blocking full-resolution render and one history node.
    func commitLiquify(token: UInt64, output: LiquifyOutput) throws -> DocumentChange
    func cancelLiquify(token: UInt64)
}
