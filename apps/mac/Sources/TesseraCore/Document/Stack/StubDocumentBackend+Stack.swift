import Foundation

// The stack calls on the stub (WP B5-19; `--stub-library` runs and unit tests). The stub has no pixels
// to register or blend, so it applies the engine's selection rules (the same messages) and then reports
// that alignment, blending and Photomerge need the engine. Nothing changes either way.

extension StubDocumentBackend: DocumentStackBackend {
    private static func needsEngine(_ what: String) -> DocumentError {
        .unsupported("\(what) needs the engine (the stub backend has no pixels)")
    }

    public func stackEligibility(ids: [DocLayerID]) throws -> StackEligibilityInfo {
        let layers = try layers()
        let align = StackCommandRules.alignProblem(layers: layers, selected: ids)
        let blend = StackCommandRules.blendProblem(layers: layers, selected: ids)
        return StackEligibilityInfo(canAlign: align == nil, canBlend: blend == nil, reason: align ?? blend)
    }

    public func autoAlignLayers(ids: [DocLayerID], options: StackAlignSettings) throws -> DocumentChange {
        if let p = StackCommandRules.alignProblem(layers: try layers(), selected: ids) { throw DocumentError.invalid(p) }
        throw Self.needsEngine("Auto-Align Layers")
    }

    public func autoBlendLayers(ids: [DocLayerID], options: StackBlendSettings) throws -> DocumentChange {
        if let p = StackCommandRules.blendProblem(layers: try layers(), selected: ids) { throw DocumentError.invalid(p) }
        throw Self.needsEngine("Auto-Blend Layers")
    }

    public func photomergeIntoLayers(sources: [String], align: StackAlignSettings, blend: StackBlendSettings) throws
        -> DocumentChange {
        if sources.count < 2 { throw DocumentError.invalid("Photomerge needs two or more photos") }
        throw Self.needsEngine("Photomerge")
    }
}

extension StubDocumentEngine: DocumentStackEngine {
    public func photomergeDocument(sources: [String], align: StackAlignSettings, blend: StackBlendSettings) throws
        -> any DocumentBackend {
        if sources.count < 2 { throw DocumentError.invalid("Photomerge needs two or more photos") }
        throw DocumentError.unsupported("Photomerge needs the engine (the stub backend has no pixels)")
    }
}
