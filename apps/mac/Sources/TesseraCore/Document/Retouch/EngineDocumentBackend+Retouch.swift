import Foundation
import TesseraFFI

// `EngineDocumentBackend` as a `DocumentRetouchBackend` (WP B5-09): each call is the session call of the
// same name (crates/tessera-ffi/src/document/retouch.rs), records converted field by field.

extension RemoveEngine {
    var ffi: RemoveBackend {
        switch self {
        case .auto: .auto
        case .patchMatch: .patchMatch
        case .lama: .lama
        }
    }
}

extension NeuralKind {
    var ffi: NeuralFilterKind {
        switch self {
        case .skinSmoothing: .skinSmoothing
        case .colorize: .colorize
        case .jpegArtifactRemoval: .jpegArtifactRemoval
        case .photoRestoration: .photoRestoration
        }
    }
    init(_ k: NeuralFilterKind) {
        switch k {
        case .skinSmoothing: self = .skinSmoothing
        case .colorize: self = .colorize
        case .jpegArtifactRemoval: self = .jpegArtifactRemoval
        case .photoRestoration: self = .photoRestoration
        }
    }
}

extension NeuralOutput {
    var ffi: NeuralDestination {
        switch self {
        case .currentLayer: .currentLayer
        case .newLayer: .newLayer
        case .smartFilter: .smartFilter
        }
    }
}

extension NeuralFilterSpec {
    init(_ f: NeuralFilterInfo) {
        self.init(kind: NeuralKind(f.kind), name: f.name,
                  controls: f.params.map {
                      NeuralControl(key: $0.key, label: $0.label, min: Double($0.min), max: Double($0.max),
                                    defaultValue: Double($0.defaultValue))
                  },
                  requiresWeights: f.requiresWeights, limitation: f.limitation)
    }

    /// The engine's neural catalogue (static, no session needed).
    public static var engineCatalogue: [NeuralFilterSpec] { TesseraFFI.neuralFilters().map(NeuralFilterSpec.init) }
}

extension EngineDocumentBackend: DocumentRetouchBackend {
    /// An apply: the update through `change` (history ids mapped), the rest of the record alongside.
    private func outcome(_ body: () throws -> RetouchResult) throws -> RetouchOutcome {
        var result: RetouchResult?
        let c = try change {
            let r = try body()
            result = r
            return r.update
        }
        guard let r = result else { throw DocumentError.invalid("no result") }
        return RetouchOutcome(change: c, backend: r.backend, note: r.note, millis: r.millis)
    }

    public func neuralFilterSpecs() -> [NeuralFilterSpec] { NeuralFilterSpec.engineCatalogue }

    public func retouchModels() throws -> [RetouchModelInfo] {
        try bridged { try session.retouchModels() }.map {
            RetouchModelInfo(modelId: $0.modelId, usedBy: $0.usedBy, installed: $0.installed, cachePath: $0.cachePath,
                             sourceURL: $0.sourceUrl, version: $0.version)
        }
    }

    public func contentAwareFill(layer: DocLayerID, paramsJson: String) throws -> RetouchOutcome {
        try outcome { try session.contentAwareFillSelection(layer: layer, paramsJson: paramsJson) }
    }

    public func removeSelection(layer: DocLayerID, engine: RemoveEngine, paramsJson: String) throws -> RetouchOutcome {
        try outcome { try session.removeWithSelection(layer: layer, backend: engine.ffi, paramsJson: paramsJson) }
    }

    public func beginRemoveStroke(layer: DocLayerID, size: Float, engine: RemoveEngine) throws {
        try bridged { try session.beginRemoveStroke(layer: layer, size: size, backend: engine.ffi) }
    }

    public func removeStrokePoints(_ points: [CanvasPoint]) throws -> CanvasRect? {
        try bridged { try session.removeStrokePoints(points: points.map(\.ffi)) }.map(CanvasRect.init)
    }

    public func cancelRemoveStroke() { session.cancelRemoveStroke() }

    public func endRemoveStroke(paramsJson: String) throws -> RetouchOutcome {
        try outcome { try session.endRemoveStroke(paramsJson: paramsJson) }
    }

    public func detectDistractions(layer: DocLayerID, paramsJson: String) throws -> DistractionScanResult {
        let s = try bridged { try session.detectDistractions(layer: layer, paramsJson: paramsJson) }
        return DistractionScanResult(
            candidates: s.suggestions.map {
                DistractionCandidate(id: $0.id, kind: $0.kind == .wire ? .wire : .faceBox, bounds: CanvasRect($0.bounds),
                                     pixels: $0.pixels)
            },
            faces: s.faces, limitation: s.limitation)
    }

    public func removeDistractions(layer: DocLayerID, accepted: [UInt32], engine: RemoveEngine,
                                   paramsJson: String) throws -> RetouchOutcome {
        try outcome {
            try session.removeDistractionSuggestions(layer: layer, accepted: accepted, backend: engine.ffi, paramsJson: paramsJson)
        }
    }

    public func clearDistractions() { session.clearDistractionSuggestions() }

    public func neuralFilter(layer: DocLayerID, kind: NeuralKind, paramsJson: String,
                             output: NeuralOutput) throws -> RetouchOutcome {
        try outcome { try session.neuralFilter(layer: layer, kind: kind.ffi, paramsJson: paramsJson, destination: output.ffi) }
    }

    public func cancelRetouch() { session.cancelRetouch() }
}
