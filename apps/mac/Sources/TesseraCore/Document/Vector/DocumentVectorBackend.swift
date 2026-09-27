import Foundation

// Live shapes, Pen / Direct Selection and vector masks (WP B5-11): the protocol document backends
// adopt. It mirrors crates/tessera-ffi/src/document/vector.rs. Every mutation returns a
// `DocumentChange`; `interactive` edits preview only and the next final call of the same layer records
// the net edit as one history node; `cancelShapePreview` (Esc) drops a draft with no history change.

/// A shape layer's document-space vector mask (FFI `VectorMaskRecord`).
public struct VectorMaskInfo: Equatable, Sendable {
    /// Closed paths in document pixels.
    public var path: ShapePath
    public var enabled: Bool
    /// Level-0 pixel radius.
    public var feather: Float
    /// 0…1: effective mask = 1 − density · (1 − coverage).
    public var density: Float
    public init(path: ShapePath, enabled: Bool = true, feather: Float = 0, density: Float = 1) {
        self.path = path; self.enabled = enabled; self.feather = feather; self.density = density
    }
    public static let featherRange: ClosedRange<Float> = 0...250
    /// The engine's validation (feather finite and ≥ 0, density in 0…1).
    public var isValid: Bool { feather.isFinite && feather >= 0 && density.isFinite && (0...1).contains(density) }
}

/// A shape layer as the inspector and overlays read it (FFI `ShapeLayerRecord`).
public struct ShapeLayerInfo: Equatable, Sendable {
    public var layer: DocLayerID
    public var source: ShapeSource
    /// Local → document.
    public var transform: AffineTransform2D
    public var revision: UInt64
    /// `rectangle`, `ellipse`, `polygon`, `line`, `custom`; nil after a custom path edit.
    public var liveKind: String?
    /// Document-space bounds including the stroke.
    public var bounds: CGRect?
    public var hasOpenSubpaths: Bool
    public var vectorMask: VectorMaskInfo?
    /// Interchange / colour limitations stated by the engine.
    public var notes: [String]
    public init(layer: DocLayerID, source: ShapeSource, transform: AffineTransform2D, revision: UInt64, liveKind: String?,
                bounds: CGRect?, hasOpenSubpaths: Bool, vectorMask: VectorMaskInfo?, notes: [String]) {
        self.layer = layer; self.source = source; self.transform = transform; self.revision = revision
        self.liveKind = liveKind; self.bounds = bounds; self.hasOpenSubpaths = hasOpenSubpaths
        self.vectorMask = vectorMask; self.notes = notes
    }
}

public enum ShapeHitKind: String, Sendable { case fill, stroke }

public struct ShapeHit: Equatable, Sendable {
    public var layer: DocLayerID
    public var part: ShapeHitKind
    public var local: ShapePoint
    public init(layer: DocLayerID, part: ShapeHitKind, local: ShapePoint) { self.layer = layer; self.part = part; self.local = local }
}

/// Photoshop's path operations.
public enum ShapeOperation: String, CaseIterable, Sendable, Identifiable {
    case combine, subtract, intersect, exclude
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .combine: "Combine Shapes"
        case .subtract: "Subtract Front Shape"
        case .intersect: "Intersect Shape Areas"
        case .exclude: "Exclude Overlapping Shapes"
        }
    }
}

public protocol DocumentVectorBackend: AnyObject, Sendable {
    func shapeLayer(_ layer: DocLayerID) throws -> ShapeLayerInfo
    func vectorMask(_ layer: DocLayerID) throws -> VectorMaskInfo?
    /// Topmost visible shape whose geometry contains the document point (`tolerance` document pixels).
    func shapeHitTest(_ p: CGPoint, includeStroke: Bool, tolerance: Double) throws -> ShapeHit?
    /// One node ("<Kind> Tool" / "Pen"); the change's `created` holds the new id.
    func addShapeLayer(name: String, parent: DocLayerID?, index: UInt32?, source: ShapeSource,
                       transform: AffineTransform2D) throws -> DocumentChange
    func setShapeLayer(_ layer: DocLayerID, source: ShapeSource, transform: AffineTransform2D, interactive: Bool) throws -> DocumentChange
    func editShapePath(_ layer: DocLayerID, commands: [PathCommand], interactive: Bool) throws -> DocumentChange
    /// The explicit linked-mask gesture: shape transform and vector mask in one Batch.
    func transformShapeWithMask(_ layer: DocLayerID, transform: AffineTransform2D, interactive: Bool) throws -> DocumentChange
    func booleanShapes(_ layer: DocLayerID, operands: [DocLayerID], operation: ShapeOperation) throws -> DocumentChange
    func setVectorMask(_ layer: DocLayerID, mask: VectorMaskInfo?, interactive: Bool) throws -> DocumentChange
    func cancelShapePreview() throws -> DocumentChange
    func convertShapeToPixels(_ layer: DocLayerID) throws -> DocumentChange
}
