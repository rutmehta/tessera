import Foundation

// The layered editor's tools (WP M5-11): painting, selections, Free Transform, fill / clear and the
// eyedropper. A second protocol next to `DocumentBackend` so the document protocol itself stays
// M5-09's call list: `EngineDocumentBackend` adopts it over the engine session's M5-11 calls
// (crates/tessera-ffi/src/document/tools.rs) and `StubDocumentBackend` adopts a geometric subset.
// Records carry TesseraCore names distinct from the FFI's (as in M5-13): `ToolColor` ⇄ `PaintColor`,
// `CanvasPoint` ⇄ `ToolPoint`, `BrushOptions` ⇄ `PaintBrush`, `PenSample` ⇄ `StrokeSample`,
// `StrokeFrameResult` ⇄ `StrokeFrame`, `SelectionCombine` ⇄ `SelectionOp`, `MarqueeKind` ⇄
// `MarqueeShape`, `LassoMode` ⇄ `LassoKind`, `SelectionModifyKind` ⇄ `SelectionModify`,
// `RefineEdgeSettings` ⇄ `RefineEdgeParams`, `SelectionOutline` ⇄ `OutlinePolyline`,
// `AffineTransform2D` ⇄ `TransformMatrix`, `ResampleMode` ⇄ `TransformInterpolation`,
// `TransformStart` ⇄ `TransformInfo`, `SelectionFillKind` ⇄ `SelectionFill`, `BrushTip` ⇄
// `BrushTipInfo`, `BrushTipBitmap` ⇄ `BrushTipImage`, `BrushToolKind` ⇄ `StrokeTool`,
// `BrushStrokeTarget` ⇄ `StrokeTarget`, `BrushSymmetry` ⇄ `PaintSymmetry`.

/// A straight, display-encoded RGB colour (0…1 per channel).
public struct ToolColor: Equatable, Sendable, Codable {
    public var r: Float
    public var g: Float
    public var b: Float
    public init(r: Float, g: Float, b: Float) { self.r = r; self.g = g; self.b = b }
    public static let black = ToolColor(r: 0, g: 0, b: 0)
    public static let white = ToolColor(r: 1, g: 1, b: 1)
    /// Rec. 709 luma (what a mask stroke paints).
    public var luminance: Float { 0.2126 * r + 0.7152 * g + 0.0722 * b }
    /// `#RRGGBB`.
    public var hex: String {
        let q = { (v: Float) in Int((min(max(v, 0), 1) * 255).rounded()) }
        return String(format: "#%02X%02X%02X", q(r), q(g), q(b))
    }
}

/// A point in level-0 canvas pixels.
public struct CanvasPoint: Equatable, Sendable, Codable {
    public var x: Float
    public var y: Float
    public init(x: Float, y: Float) { self.x = x; self.y = y }
    public init(_ p: CGPoint) { self.init(x: Float(p.x), y: Float(p.y)) }
    public var cgPoint: CGPoint { CGPoint(x: Double(x), y: Double(y)) }
}

public enum BrushStrokeTarget: String, Sendable, CaseIterable { case pixels, mask }

public enum BrushToolKind: String, Sendable, CaseIterable {
    case brush, eraser, clone, heal
    /// Photoshop's history label for the stroke.
    public var historyLabel: String {
        switch self {
        case .brush: "Brush Tool"
        case .eraser: "Eraser"
        case .clone: "Clone Stamp"
        case .heal: "Healing Brush"
        }
    }
}

public enum BrushSymmetry: String, Sendable, CaseIterable, Codable {
    case none, vertical, horizontal, dual, diagonal, radial, mandala
    public var title: String {
        switch self {
        case .none: "Off"
        case .vertical: "Vertical"
        case .horizontal: "Horizontal"
        case .dual: "Dual Axis"
        case .diagonal: "Diagonal"
        case .radial: "Radial"
        case .mandala: "Mandala"
        }
    }
}

/// Brush options (options bar, Brushes panel).
public struct BrushOptions: Equatable, Sendable, Codable {
    /// Diameter, canvas pixels.
    public var size: Float = 40
    /// 0 soft … 1 hard.
    public var hardness: Float = 0.8
    public var opacity: Float = 1
    public var flow: Float = 1
    /// Fraction of the diameter.
    public var spacing: Float = 0.1
    /// Degrees.
    public var angle: Float = 0
    public var roundness: Float = 1
    public var blendMode = "normal"
    public var pressureSize = true
    public var pressureOpacity = false
    public var pressureFlow = false
    /// Pulled-string length, pixels.
    public var smoothing: Float = 0
    public var symmetry: BrushSymmetry = .none
    public var symmetryX: Float = 0
    public var symmetryY: Float = 0
    public var symmetryCount: UInt32 = 6
    /// A sampled or imported tip; nil = computed round.
    public var tipId: String?
    /// Clone / heal sample the composite.
    public var sampleAllLayers = false
    public init() {}
}

/// One pointer sample (canvas pixels; pressure after the pressure curve).
public struct PenSample: Equatable, Sendable {
    public var x: Float
    public var y: Float
    public var pressure: Float
    public var tiltX: Float
    public var tiltY: Float
    public var timestamp: Double
    public init(x: Float, y: Float, pressure: Float = 1, tiltX: Float = 0, tiltY: Float = 0, timestamp: Double = 0) {
        self.x = x; self.y = y; self.pressure = pressure; self.tiltX = tiltX; self.tiltY = tiltY; self.timestamp = timestamp
    }
}

public struct StrokeFrameResult: Equatable, Sendable {
    public var dirtyRect: CanvasRect?
    public var dabs: UInt32
    public var totalDabs: UInt32
    public var rasterMs: Double
    public var epoch: UInt64
    public init(dirtyRect: CanvasRect?, dabs: UInt32, totalDabs: UInt32, rasterMs: Double, epoch: UInt64) {
        self.dirtyRect = dirtyRect; self.dabs = dabs; self.totalDabs = totalDabs; self.rasterMs = rasterMs; self.epoch = epoch
    }
}

/// How a new selection combines with the current one (⇧ add, ⌥ subtract, ⇧⌥ intersect).
public enum SelectionCombine: String, Sendable, CaseIterable {
    case replace, add, subtract, intersect
    public var title: String {
        switch self {
        case .replace: "New Selection"
        case .add: "Add to Selection"
        case .subtract: "Subtract from Selection"
        case .intersect: "Intersect with Selection"
        }
    }
    public var symbol: String {
        switch self {
        case .replace: "square"
        case .add: "plus.square"
        case .subtract: "minus.square"
        case .intersect: "square.on.square.intersection.dashed"
        }
    }
}

public enum MarqueeKind: String, Sendable, CaseIterable { case rect, ellipse, row, column }
public enum LassoMode: String, Sendable, CaseIterable { case free, polygon, magnetic }

public enum SelectionModifyKind: String, Sendable, CaseIterable, Identifiable {
    case border, smooth, expand, contract, feather
    public var id: String { rawValue }
    public var title: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }
}

/// Select and Mask global refinements, canvas pixels.
public struct RefineEdgeSettings: Equatable, Sendable {
    public var radius: Float = 0
    public var smartRadius = false
    public var smooth: Float = 0
    public var feather: Float = 0
    /// 0…1.
    public var contrast: Float = 0
    public var shiftEdge: Float = 0
    public init() {}
    public init(radius: Float, smartRadius: Bool, smooth: Float, feather: Float, contrast: Float, shiftEdge: Float) {
        self.radius = radius; self.smartRadius = smartRadius; self.smooth = smooth; self.feather = feather
        self.contrast = contrast; self.shiftEdge = shiftEdge
    }
}

/// One marching-ants loop in canvas pixels.
public struct SelectionOutline: Equatable, Sendable {
    public var points: [CanvasPoint]
    public var closed: Bool
    public init(points: [CanvasPoint], closed: Bool) { self.points = points; self.closed = closed }
}

public enum ResampleMode: String, Sendable, CaseIterable {
    case nearest, bilinear, bicubic
    public var title: String {
        switch self {
        case .nearest: "Nearest Neighbor"
        case .bilinear: "Bilinear"
        case .bicubic: "Bicubic"
        }
    }
}

public struct TransformStart: Equatable, Sendable {
    public var layers: [DocLayerID]
    public var bounds: CanvasRect?
    public init(layers: [DocLayerID], bounds: CanvasRect?) { self.layers = layers; self.bounds = bounds }
}

public enum SelectionFillKind: Equatable, Sendable {
    case color(ToolColor)
    /// The engine's Content-Aware Fill placeholder (smooth fill from the surroundings).
    case contentAware
}

public struct BrushTip: Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var width: UInt32
    public var height: UInt32
    public var diameter: Float
    public var spacing: Float?
    public var sampled: Bool
    public init(id: String, name: String, width: UInt32, height: UInt32, diameter: Float, spacing: Float?, sampled: Bool) {
        self.id = id; self.name = name; self.width = width; self.height = height; self.diameter = diameter
        self.spacing = spacing; self.sampled = sampled
    }
}

/// A grey tip preview (0 = no paint, 255 = full paint), row-major.
public struct BrushTipBitmap: Equatable, Sendable {
    public var width: UInt32
    public var height: UInt32
    public var pixels: [UInt8]
    public init(width: UInt32, height: UInt32, pixels: [UInt8]) { self.width = width; self.height = height; self.pixels = pixels }
}

/// The session's M5-11 calls, one for one (crates/tessera-ffi/src/document/tools.rs).
public protocol DocumentToolsBackend: AnyObject, Sendable {
    // Painting
    func beginStroke(layer: DocLayerID, target: BrushStrokeTarget, tool: BrushToolKind, brush: BrushOptions,
                     color: ToolColor) throws
    /// The pointer samples of one display frame.
    func strokePoints(_ points: [PenSample]) throws -> StrokeFrameResult
    /// One history node (nothing when no dab was placed).
    func endStroke() throws -> DocumentChange
    func cancelStroke() throws -> DocumentChange
    /// The pixel painted at `p` is sampled at `p + (dx, dy)` of `layer`.
    func setCloneSource(layer: DocLayerID, dx: Float, dy: Float) throws

    // Selections (each one history node)
    func selectMarquee(_ kind: MarqueeKind, rect: CGRect, feather: Float, antialias: Bool, op: SelectionCombine) throws
        -> DocumentChange
    func selectLasso(_ points: [CanvasPoint], mode: LassoMode, feather: Float, antialias: Bool, op: SelectionCombine) throws
        -> DocumentChange
    func magneticPath(from: CanvasPoint, to: CanvasPoint) throws -> [CanvasPoint]
    func selectWand(at p: CanvasPoint, tolerance: Float, contiguous: Bool, sampleAll: Bool, antialias: Bool,
                    op: SelectionCombine) throws -> DocumentChange
    func selectQuick(stroke: [CanvasPoint], radius: Float, sampleAll: Bool, op: SelectionCombine) throws -> DocumentChange
    func selectColorRange(_ color: ToolColor, fuzziness: Float, op: SelectionCombine) throws -> DocumentChange
    func selectSubject(op: SelectionCombine) throws -> DocumentChange
    func selectSky(op: SelectionCombine) throws -> DocumentChange
    func selectObject(at p: CanvasPoint, op: SelectionCombine) throws -> DocumentChange
    func selectAll() throws -> DocumentChange
    func selectInverse() throws -> DocumentChange
    func modifySelection(_ kind: SelectionModifyKind, px: Float) throws -> DocumentChange
    /// Interactive: live without history until a final call (one node) or `cancelRefineEdge`.
    func refineEdge(_ settings: RefineEdgeSettings, interactive: Bool) throws -> DocumentChange
    func cancelRefineEdge() throws -> DocumentChange
    /// Marching ants traced at pyramid `level`, canvas pixels.
    func selectionOutline(level: UInt8) throws -> [SelectionOutline]
    func saveSelection(name: String) throws
    func loadSelection(name: String, op: SelectionCombine) throws -> DocumentChange
    func selectionChannels() throws -> [String]

    // Free Transform
    func beginTransform(layers: [DocLayerID]) throws -> TransformStart
    /// Live, no history.
    func setTransform(_ m: AffineTransform2D, interpolation: ResampleMode) throws -> DocumentChange
    func commitTransform() throws -> DocumentChange
    func cancelTransform() throws -> DocumentChange

    // Fill, clear, eyedropper
    func fillSelection(layer: DocLayerID, fill: SelectionFillKind, opacity: Float) throws -> DocumentChange
    func deleteSelection(layer: DocLayerID, background: ToolColor) throws -> DocumentChange
    func sampleColor(at p: CanvasPoint, sampleAll: Bool, layer: DocLayerID?, radius: UInt32) throws -> ToolColor

    // Brush tips (process-wide library)
    func brushTips() -> [BrushTip]
    func importAbr(path: String) throws -> [BrushTip]
    func brushTipPreview(id: String, maxPx: UInt32) throws -> BrushTipBitmap
}
