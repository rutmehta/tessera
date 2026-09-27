import Foundation

// The stub backend (`--stub-library`, unit tests) and retouching (WP B5-09): the neural catalogue is the
// engine's static data; every apply explains that retouching needs the engine. Nothing is simulated.

extension StubDocumentBackend: DocumentRetouchBackend {
    static let retouchNeedsEngine = DocumentError.unsupported(
        "Remove, Content-Aware Fill and neural filters need the engine; open a folder or run without --stub-library")

    public func neuralFilterSpecs() -> [NeuralFilterSpec] { NeuralFilterSpec.engineCatalogue }
    public func retouchModels() throws -> [RetouchModelInfo] { [] }
    public func contentAwareFill(layer: DocLayerID, paramsJson: String) throws -> RetouchOutcome { throw Self.retouchNeedsEngine }
    public func removeSelection(layer: DocLayerID, engine: RemoveEngine, paramsJson: String) throws -> RetouchOutcome {
        throw Self.retouchNeedsEngine
    }
    public func beginRemoveStroke(layer: DocLayerID, size: Float, engine: RemoveEngine) throws { throw Self.retouchNeedsEngine }
    public func removeStrokePoints(_ points: [CanvasPoint]) throws -> CanvasRect? { throw Self.retouchNeedsEngine }
    public func cancelRemoveStroke() {}
    public func endRemoveStroke(paramsJson: String) throws -> RetouchOutcome { throw Self.retouchNeedsEngine }
    public func detectDistractions(layer: DocLayerID, paramsJson: String) throws -> DistractionScanResult {
        throw Self.retouchNeedsEngine
    }
    public func removeDistractions(layer: DocLayerID, accepted: [UInt32], engine: RemoveEngine,
                                   paramsJson: String) throws -> RetouchOutcome { throw Self.retouchNeedsEngine }
    public func clearDistractions() {}
    public func neuralFilter(layer: DocLayerID, kind: NeuralKind, paramsJson: String,
                             output: NeuralOutput) throws -> RetouchOutcome { throw Self.retouchNeedsEngine }
    public func cancelRetouch() {}
}
