import Foundation
import TesseraFFI

// `EngineDocumentBackend` and the stub as `DocumentLiquifyBackend` (WP B5-13): each engine call is the session
// call of the same name (crates/tessera-ffi/src/document/liquify.rs); the stub explains that Liquify needs the
// engine. Nothing is simulated.

extension LiquifyToolKind {
    var ffi: LiquifyTool {
        switch self {
        case .forwardWarp: .forwardWarp
        case .reconstruct: .reconstruct
        case .smooth: .smooth
        case .twirlClockwise: .twirlClockwise
        case .twirlCounterClockwise: .twirlCounterClockwise
        case .pucker: .pucker
        case .bloat: .bloat
        case .pushLeft: .pushLeft
        case .freeze: .freeze
        case .thaw: .thaw
        }
    }
}

extension LiquifyOutput {
    var ffi: LiquifyDestination {
        switch self {
        case .currentLayer: .currentLayer
        case .newLayer: .newLayer
        case .smartFilter: .smartFilter
        }
    }
}

extension LiquifyWorkspaceInfo {
    init(_ s: LiquifySessionInfo) {
        self.init(token: s.token, layer: s.layer, stageIndex: s.stageIndex, smartObject: s.smartObject, width: s.width,
                  height: s.height, cellSize: s.cellSize, columns: s.columns, rows: s.rows, previewWidth: s.previewWidth,
                  previewHeight: s.previewHeight, previewFactor: s.previewFactor, selectionFrozen: s.selectionFrozen,
                  edited: s.edited)
    }
}

extension EngineDocumentBackend: DocumentLiquifyBackend {
    public func beginLiquify(layer: DocLayerID, stageIndex: UInt32?) throws -> LiquifyWorkspaceInfo {
        LiquifyWorkspaceInfo(try bridged { try session.beginLiquify(layer: layer, stageIndex: stageIndex) })
    }

    public func liquifyBrush(token: UInt64, tool: LiquifyToolKind, brush: LiquifyBrushSettings,
                             points: [LiquifyInputPoint]) throws -> LiquifyStrokeInfo {
        let b = brush.engine
        let r = try bridged {
            try session.liquifyBrushPoints(token: token, tool: tool.ffi,
                                           brush: LiquifyBrush(size: b.size, density: b.density, pressure: b.pressure, rate: b.rate),
                                           points: points.map { LiquifyPoint(x: $0.x, y: $0.y, pressure: $0.pressure) })
        }
        return LiquifyStrokeInfo(dabs: r.dabs, dirty: r.dirty.map(CanvasRect.init), millis: r.millis)
    }

    public func liquifyEndStroke(token: UInt64) throws { try bridged { try session.liquifyEndStroke(token: token) } }

    public func liquifyMesh(token: UInt64) throws -> LiquifyMeshData {
        let m = try bridged { try session.liquifyMesh(token: token) }
        return LiquifyMeshData(columns: Int(m.columns), rows: Int(m.rows), cellSize: Int(m.cellSize), displacement: m.displacement,
                               freeze: m.freeze, maxDisplacement: m.maxDisplacement)
    }

    public func liquifyReconstructAll(token: UInt64, amount: Double) throws {
        try bridged { try session.liquifyReconstructAll(token: token, amount: Float(amount)) }
    }

    public func liquifyReset(token: UInt64, keepFreeze: Bool) throws {
        try bridged { try session.liquifyReset(token: token, keepFreeze: keepFreeze) }
    }

    public func liquifyFreezeAll(token: UInt64, frozen: Bool) throws {
        try bridged { try session.liquifyFreezeAll(token: token, frozen: frozen) }
    }

    public func previewLiquify(token: UInt64, original: Bool) throws -> LiquifyPreviewFrame {
        let p = try bridged { try session.previewLiquify(token: token, original: original) }
        return LiquifyPreviewFrame(surfaceId: p.surfaceId, width: p.width, height: p.height, original: p.original, millis: p.millis)
    }

    public func commitLiquify(token: UInt64, output: LiquifyOutput) throws -> DocumentChange {
        try change { try session.commitLiquify(token: token, destination: output.ffi) }
    }

    public func cancelLiquify(token: UInt64) { session.cancelLiquify(token: token) }
}

extension StubDocumentBackend: DocumentLiquifyBackend {
    static let liquifyNeedsEngine = DocumentError.unsupported(
        "Liquify needs the engine; open a folder or run without --stub-library")

    public func beginLiquify(layer: DocLayerID, stageIndex: UInt32?) throws -> LiquifyWorkspaceInfo { throw Self.liquifyNeedsEngine }
    public func liquifyBrush(token: UInt64, tool: LiquifyToolKind, brush: LiquifyBrushSettings,
                             points: [LiquifyInputPoint]) throws -> LiquifyStrokeInfo { throw Self.liquifyNeedsEngine }
    public func liquifyEndStroke(token: UInt64) throws { throw Self.liquifyNeedsEngine }
    public func liquifyMesh(token: UInt64) throws -> LiquifyMeshData { throw Self.liquifyNeedsEngine }
    public func liquifyReconstructAll(token: UInt64, amount: Double) throws { throw Self.liquifyNeedsEngine }
    public func liquifyReset(token: UInt64, keepFreeze: Bool) throws { throw Self.liquifyNeedsEngine }
    public func liquifyFreezeAll(token: UInt64, frozen: Bool) throws { throw Self.liquifyNeedsEngine }
    public func previewLiquify(token: UInt64, original: Bool) throws -> LiquifyPreviewFrame { throw Self.liquifyNeedsEngine }
    public func commitLiquify(token: UInt64, output: LiquifyOutput) throws -> DocumentChange { throw Self.liquifyNeedsEngine }
    public func cancelLiquify(token: UInt64) {}
}
