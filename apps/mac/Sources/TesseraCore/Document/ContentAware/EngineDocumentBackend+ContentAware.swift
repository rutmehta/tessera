import Foundation
import TesseraFFI

// `EngineDocumentBackend` and the stub as `DocumentContentAwareBackend` (WP B5-13): the session calls of the same
// name (crates/tessera-ffi/src/document/content_aware.rs); the stub explains that the move needs the engine.

extension ContentAwareMoveMode {
    var ffi: ContentAwareMode { self == .move ? .move : .extend }
}

extension ContentAwareSeamLevel {
    var ffi: ContentAwareSeam {
        switch self {
        case .none: .none
        case .standard: .default
        case .high: .high
        case .veryHigh: .veryHigh
        }
    }
}

extension EngineDocumentBackend: DocumentContentAwareBackend {
    public func beginContentAwareMove(layer: DocLayerID, mode: ContentAwareMoveMode) throws -> ContentAwareMoveSession {
        let i = try bridged { try session.beginContentAwareMove(layer: layer, mode: mode.ffi) }
        return ContentAwareMoveSession(token: i.token, layer: i.layer, mode: mode, smartObject: i.smartObject,
                                       selectionBounds: CanvasRect(i.selectionBounds), width: i.width, height: i.height)
    }

    public func previewContentAwareMove(token: UInt64, dx: Int32, dy: Int32, fillJson: String,
                                        seam: ContentAwareSeamLevel) throws -> ContentAwarePreviewInfo {
        let p = try bridged {
            try session.previewContentAwareMove(token: token, dx: dx, dy: dy, fillJson: fillJson, seam: seam.ffi)
        }
        return ContentAwarePreviewInfo(dx: p.dx, dy: p.dy, affected: p.affected.map(CanvasRect.init), millis: p.millis)
    }

    public func commitContentAwareMove(token: UInt64) throws -> DocumentChange {
        try change { try session.commitContentAwareMove(token: token) }
    }

    public func cancelContentAwareMove(token: UInt64) { session.cancelContentAwareMove(token: token) }
}

extension StubDocumentBackend: DocumentContentAwareBackend {
    static let contentAwareNeedsEngine = DocumentError.unsupported(
        "Content-Aware Move needs the engine; open a folder or run without --stub-library")

    public func beginContentAwareMove(layer: DocLayerID, mode: ContentAwareMoveMode) throws -> ContentAwareMoveSession {
        throw Self.contentAwareNeedsEngine
    }
    public func previewContentAwareMove(token: UInt64, dx: Int32, dy: Int32, fillJson: String,
                                        seam: ContentAwareSeamLevel) throws -> ContentAwarePreviewInfo {
        throw Self.contentAwareNeedsEngine
    }
    public func commitContentAwareMove(token: UInt64) throws -> DocumentChange { throw Self.contentAwareNeedsEngine }
    public func cancelContentAwareMove(token: UInt64) {}
}
