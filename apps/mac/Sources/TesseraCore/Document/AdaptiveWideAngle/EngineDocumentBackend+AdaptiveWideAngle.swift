import CoreGraphics
import Foundation
import TesseraFFI

// `EngineDocumentBackend` and the stub as `DocumentAdaptiveWideAngleBackend` (WP B5-20): each engine call is the
// session call of the same name (crates/tessera-ffi/src/document/adaptive.rs); the stub explains that the filter
// needs the engine. Nothing is simulated.

extension EngineDocumentBackend: DocumentAdaptiveWideAngleBackend {
    public func beginAdaptiveWideAngle(layer: DocLayerID, stageIndex: UInt32?) throws -> AdaptiveWideAngleWorkspaceInfo {
        let s = try bridged { try session.beginAdaptiveWideAngle(layer: layer, stageIndex: stageIndex) }
        return AdaptiveWideAngleWorkspaceInfo(token: s.token, layer: s.layer, stageIndex: s.stageIndex, smartObject: s.smartObject,
                                              width: s.width, height: s.height, previewWidth: s.previewWidth,
                                              previewHeight: s.previewHeight, previewFactor: s.previewFactor,
                                              recipeJson: s.recipeJson, exifFocal35mm: s.exifFocal35mm)
    }

    public func previewAdaptiveWideAngle(token: UInt64, recipeJson: String?) throws -> AdaptiveWideAnglePreviewFrame {
        let p = try bridged { try session.previewAdaptiveWideAngle(token: token, recipeJson: recipeJson) }
        return AdaptiveWideAnglePreviewFrame(surfaceId: p.surfaceId, width: p.width, height: p.height, original: p.original,
                                             millis: p.millis)
    }

    public func commitAdaptiveWideAngle(token: UInt64, recipeJson: String) throws -> DocumentChange {
        try change { try session.commitAdaptiveWideAngle(token: token, recipeJson: recipeJson) }
    }

    public func cancelAdaptiveWideAngle(token: UInt64) { session.cancelAdaptiveWideAngle(token: token) }

    public func adaptiveWideAngleCurve(recipeJson: String, from: CGPoint, to: CGPoint) throws -> [CGPoint] {
        let v = try bridged {
            try TesseraFFI.adaptiveWideAngleCurve(recipeJson: recipeJson, from: [Double(from.x), Double(from.y)],
                                                  to: [Double(to.x), Double(to.y)])
        }
        return stride(from: 0, to: v.count - 1, by: 2).map { CGPoint(x: v[$0], y: v[$0 + 1]) }
    }
}

extension StubDocumentBackend: DocumentAdaptiveWideAngleBackend {
    static let adaptiveNeedsEngine = DocumentError.unsupported(
        "Adaptive Wide Angle needs the engine; open a folder or run without --stub-library")

    public func beginAdaptiveWideAngle(layer: DocLayerID, stageIndex: UInt32?) throws -> AdaptiveWideAngleWorkspaceInfo {
        throw Self.adaptiveNeedsEngine
    }
    public func previewAdaptiveWideAngle(token: UInt64, recipeJson: String?) throws -> AdaptiveWideAnglePreviewFrame {
        throw Self.adaptiveNeedsEngine
    }
    public func commitAdaptiveWideAngle(token: UInt64, recipeJson: String) throws -> DocumentChange { throw Self.adaptiveNeedsEngine }
    public func cancelAdaptiveWideAngle(token: UInt64) {}
    public func adaptiveWideAngleCurve(recipeJson: String, from: CGPoint, to: CGPoint) throws -> [CGPoint] { throw Self.adaptiveNeedsEngine }
}

extension AdaptiveWideAngleFilter {
    /// Largest layer the engine renders, in pixels (one source: `filters::adaptive_lattice::MAX_PIXELS`). Layers over
    /// the dense 16,777,216-vertex lattice render through a coarse solve lattice (B5-20b).
    public static var maxPixels: UInt64 { adaptiveWideAngleMaxPixels() }
}
