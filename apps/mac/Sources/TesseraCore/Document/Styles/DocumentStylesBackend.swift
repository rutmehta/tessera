import Foundation

// Layer styles and the Global Light (WP B5-07): the third protocol document backends adopt, next to
// `DocumentBackend` and `DocumentFiltersBackend`. It mirrors the B5-07 calls of `DocumentSession`
// (crates/tessera-ffi/src/document/styles.rs) one to one; `EngineDocumentBackend` forwards them and
// `StubDocumentBackend` keeps styles in memory (it stores and lists them but does not draw effects).

public protocol DocumentStylesBackend: AnyObject, Sendable {
    /// The layer's `LayerStyles` JSON (the live state while a drag is pending).
    func layerStylesJson(layer: DocLayerID) throws -> String
    /// Replaces the layer's styles, keeping its other properties. `interactive`: live only, no history
    /// node until `commit` (an inspector slider drag). Throws for locked (Lock All) layers, adjustment
    /// layers and pass-through groups.
    func setLayerStylesJson(layer: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange
    /// Every styled layer (effects top first), in `layers()` order.
    func layerStyleRows() throws -> [LayerStyleRow]
    func globalLight() throws -> DocGlobalLight
    /// Layer ▸ Layer Style ▸ Global Light (every effect with Use Global Light follows it).
    func setGlobalLight(_ light: DocGlobalLight, interactive: Bool) throws -> DocumentChange
    /// Copy Layer Style (application-wide, no history).
    func copyLayerStyles(from layer: DocLayerID) throws
    func canPasteLayerStyles() -> Bool
    /// Paste Layer Style onto `layers`: one history node.
    func pasteLayerStyles(to layers: [DocLayerID]) throws -> DocumentChange
    /// Clear Layer Style: one history node.
    func clearLayerStyles(layer: DocLayerID) throws -> DocumentChange
    /// `style_effects_schema_json()`.
    func styleSchemaJson() -> String
}
