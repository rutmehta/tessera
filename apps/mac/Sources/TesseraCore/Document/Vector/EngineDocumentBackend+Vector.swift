import Foundation
import TesseraFFI

// `EngineDocumentBackend` as a `DocumentVectorBackend` (WP B5-11): each call is the session call of the
// same name (crates/tessera-ffi/src/document/vector.rs). Models cross as the vector crate's JSON; the
// transform crosses as `TransformMatrix` (row-major, the same layout as `AffineTransform2D`).

extension AffineTransform2D {
    init(_ m: TransformMatrix) { self.init(a: m.a, b: m.b, c: m.c, d: m.d, e: m.e, f: m.f) }
}

extension VectorMaskInfo {
    init(_ r: VectorMaskRecord) throws {
        self.init(path: try VectorJSON.decoder.decode(ShapePath.self, from: Data(r.pathJson.utf8)), enabled: r.enabled,
                  feather: r.feather, density: r.density)
    }
    var ffi: VectorMaskRecord {
        VectorMaskRecord(pathJson: VectorJSON.string(path), enabled: enabled, feather: feather, density: density)
    }
}

extension ShapeLayerInfo {
    init(_ r: ShapeLayerRecord) throws {
        self.init(layer: r.layer, source: try ShapeSource(json: r.modelJson), transform: AffineTransform2D(r.transform),
                  revision: r.revision, liveKind: r.liveKind,
                  bounds: r.bounds.map { CGRect(x: $0.x0, y: $0.y0, width: $0.x1 - $0.x0, height: $0.y1 - $0.y0) },
                  hasOpenSubpaths: r.hasOpenSubpaths, vectorMask: try r.vectorMask.map(VectorMaskInfo.init), notes: r.notes)
    }
}

extension ShapeOperation {
    var ffi: ShapePathOperation {
        switch self {
        case .combine: .combine
        case .subtract: .subtract
        case .intersect: .intersect
        case .exclude: .exclude
        }
    }
}

extension EngineDocumentBackend: DocumentVectorBackend {
    private func decoded<T>(_ body: () throws -> T) throws -> T {
        do { return try body() } catch let e as DecodingError { throw DocumentError.invalid("shape JSON: \(e)") }
    }

    public func shapeLayer(_ layer: DocLayerID) throws -> ShapeLayerInfo {
        let r = try bridged { try session.shapeLayer(layer: layer) }
        return try decoded { try ShapeLayerInfo(r) }
    }

    public func vectorMask(_ layer: DocLayerID) throws -> VectorMaskInfo? {
        let r = try bridged { try session.vectorMask(layer: layer) }
        return try decoded { try r.map(VectorMaskInfo.init) }
    }

    public func shapeHitTest(_ p: CGPoint, includeStroke: Bool, tolerance: Double) throws -> ShapeHit? {
        try bridged { try session.shapeHitTest(x: Double(p.x), y: Double(p.y), includeStroke: includeStroke, tolerance: tolerance) }
            .map { ShapeHit(layer: $0.layer, part: $0.part == .stroke ? .stroke : .fill, local: ShapePoint(x: $0.localX, y: $0.localY)) }
    }

    public func addShapeLayer(name: String, parent: DocLayerID?, index: UInt32?, source: ShapeSource,
                              transform: AffineTransform2D) throws -> DocumentChange {
        try change {
            try session.addShapeLayer(name: name, parent: parent, index: index, modelJson: source.json, transform: transform.ffi)
        }
    }

    public func setShapeLayer(_ layer: DocLayerID, source: ShapeSource, transform: AffineTransform2D,
                              interactive: Bool) throws -> DocumentChange {
        try change {
            try session.setShapeLayer(layer: layer, modelJson: source.json, transform: transform.ffi, interactive: interactive)
        }
    }

    public func editShapePath(_ layer: DocLayerID, commands: [PathCommand], interactive: Bool) throws -> DocumentChange {
        try change { try session.editShapePath(layer: layer, commandJson: PathCommand.json(commands), interactive: interactive) }
    }

    public func transformShapeWithMask(_ layer: DocLayerID, transform: AffineTransform2D, interactive: Bool) throws -> DocumentChange {
        try change { try session.transformShapeWithMask(layer: layer, transform: transform.ffi, interactive: interactive) }
    }

    public func booleanShapes(_ layer: DocLayerID, operands: [DocLayerID], operation: ShapeOperation) throws -> DocumentChange {
        try change { try session.booleanShapePaths(layer: layer, operands: operands, operation: operation.ffi) }
    }

    public func setVectorMask(_ layer: DocLayerID, mask: VectorMaskInfo?, interactive: Bool) throws -> DocumentChange {
        try change { try session.setVectorMask(layer: layer, mask: mask?.ffi, interactive: interactive) }
    }

    public func cancelShapePreview() throws -> DocumentChange { try change { try session.cancelShapePreview() } }

    public func convertShapeToPixels(_ layer: DocLayerID) throws -> DocumentChange {
        try change { try session.convertShapeToPixels(layer: layer) }
    }
}

/// The engine's primitive path for a live shape (`shape_primitive_path`), for checks against
/// `ShapePrimitives`.
public func enginePrimitivePath(_ shape: LiveShape) throws -> ShapePath {
    let json = try bridged { try shapePrimitivePath(shapeJson: VectorJSON.string(shape)) }
    return try VectorJSON.decoder.decode(ShapePath.self, from: Data(json.utf8))
}
