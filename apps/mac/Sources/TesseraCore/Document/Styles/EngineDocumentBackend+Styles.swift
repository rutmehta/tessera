import Foundation
import TesseraFFI

// `EngineDocumentBackend` as a `DocumentStylesBackend` (WP B5-07): each call is the session call of the
// same name, records converted field by field.

extension LayerStyleRow {
    init?(_ s: LayerStyleSummary) {
        self.init(layer: s.layer, effects: s.effects.compactMap { e in
            StyleEffectKind(rawValue: e.kind).map { Effect(index: Int(e.index), kind: $0, enabled: e.enabled) }
        })
    }
}

extension EngineDocumentBackend: DocumentStylesBackend {
    public func layerStylesJson(layer: DocLayerID) throws -> String {
        try bridged { try session.layerStylesJson(layer: layer) }
    }

    public func setLayerStylesJson(layer: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange {
        try change { try session.setLayerStylesJson(layer: layer, json: json, interactive: interactive) }
    }

    public func layerStyleRows() throws -> [LayerStyleRow] {
        try bridged { try session.layerStyleSummaries() }.compactMap(LayerStyleRow.init)
    }

    public func globalLight() throws -> DocGlobalLight {
        let g = try bridged { try session.globalLight() }
        return DocGlobalLight(angle: Double(g.angle), altitude: Double(g.altitude))
    }

    public func setGlobalLight(_ light: DocGlobalLight, interactive: Bool) throws -> DocumentChange {
        try change {
            try session.setGlobalLight(angle: Float(light.angle), altitude: Float(light.altitude), interactive: interactive)
        }
    }

    public func copyLayerStyles(from layer: DocLayerID) throws { try bridged { try session.copyLayerStyles(from: layer) } }
    public func canPasteLayerStyles() -> Bool { session.canPasteLayerStyles() }

    public func pasteLayerStyles(to layers: [DocLayerID]) throws -> DocumentChange {
        try change { try session.pasteLayerStyles(to: layers) }
    }

    public func clearLayerStyles(layer: DocLayerID) throws -> DocumentChange {
        try change { try session.clearLayerStyles(layer: layer) }
    }

    public func styleSchemaJson() -> String { TesseraFFI.styleEffectsSchemaJson() }
}
