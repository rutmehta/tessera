import Foundation

// Auto-Align / Auto-Blend Layers and Photomerge (WP B5-19): the session's stack calls
// (crates/tessera-ffi/src/document/stack.rs), adopted by `EngineDocumentBackend` and, validating only,
// by `StubDocumentBackend`. Every call is one history node and blocks while the engine aligns or blends:
// call it off the main thread.

public protocol DocumentStackBackend: AnyObject, Sendable {
    /// Whether the layers can be aligned / blended, with the reason when not.
    func stackEligibility(ids: [DocLayerID]) throws -> StackEligibilityInfo
    /// Registers top-level pixel layers `ids` (bottom first; `referenceIndex` stays put) and extends the canvas.
    func autoAlignLayers(ids: [DocLayerID], options: StackAlignSettings) throws -> DocumentChange
    /// Panorama seam or focus-stack masks on each layer (plus an optional Content-Aware Fill layer).
    func autoBlendLayers(ids: [DocLayerID], options: StackBlendSettings) throws -> DocumentChange
    /// Library image ids or file paths as named layers of this document, aligned and blended.
    func photomergeIntoLayers(sources: [String], align: StackAlignSettings, blend: StackBlendSettings) throws
        -> DocumentChange
}

/// File ▸ Automate ▸ Photomerge into a new Untitled document.
public protocol DocumentStackEngine: AnyObject, Sendable {
    func photomergeDocument(sources: [String], align: StackAlignSettings, blend: StackBlendSettings) throws
        -> any DocumentBackend
}
