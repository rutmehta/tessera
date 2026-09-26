import Foundation

// Filters, Image ▸ Adjustments and smart filters (WP M5-12): the second protocol document backends
// adopt, next to `DocumentBackend`. It mirrors the M5-12 calls of `DocumentSession`
// (crates/tessera-ffi/src/document/filters.rs) one to one; `EngineDocumentBackend` forwards them and
// `StubDocumentBackend` lists the catalogue but applies nothing (filters need the engine).

/// One smart filter of a smart object (FFI `SmartFilterRecord`), first applied first.
public struct SmartFilterRow: Equatable, Sendable, Identifiable {
    public var index: UInt32
    public var filterId: String
    public var name: String
    public var enabled: Bool
    /// `{"id":…,"params":{…}}`.
    public var filterJson: String
    public var opacity: Float
    public var blendMode: String
    public var hasMask: Bool
    public var id: UInt32 { index }

    public init(index: UInt32, filterId: String, name: String, enabled: Bool, filterJson: String, opacity: Float,
                blendMode: String, hasMask: Bool) {
        self.index = index; self.filterId = filterId; self.name = name; self.enabled = enabled
        self.filterJson = filterJson; self.opacity = opacity; self.blendMode = blendMode; self.hasMask = hasMask
    }
}

/// What `setSmartFilter` changes (FFI `SmartFilterEdit`).
public enum SmartFilterChange: Equatable, Sendable {
    case enabled(Bool)
    /// New filter JSON of the same filter.
    case params(json: String)
    /// Blending options: a blend mode name and opacity 0…1.
    case blending(mode: String, opacity: Float)
}

/// A 1:1 detail crop (FFI `FilterDetail`) in an RGBA8 IOSurface the backend retains.
public struct FilterDetailSurface: Equatable, Sendable {
    public var surfaceId: UInt32
    public var width: UInt32
    public var height: UInt32
    /// 0 = true 1:1; whole-image filters on large layers are filtered on a coarser level.
    public var level: UInt8
    public init(surfaceId: UInt32, width: UInt32, height: UInt32, level: UInt8) {
        self.surfaceId = surfaceId; self.width = width; self.height = height; self.level = level
    }
}

public protocol DocumentFiltersBackend: AnyObject, Sendable {
    /// The Filter menu, in menu order.
    func listFilters() -> [FilterCatalogEntry]
    /// Shows the filter on the viewport level over `region` (level-0 canvas pixels); no history.
    /// Asynchronous and latest-wins: the frame arrives through the listener.
    func previewFilter(layer: DocLayerID, filterJson: String, region: CanvasRect?) throws
    /// Like `previewFilter`, re-editing smart filter `index`.
    func previewSmartFilter(layer: DocLayerID, index: UInt32, filterJson: String, region: CanvasRect?) throws
    /// Shows an Image ▸ Adjustments result (`Adjustment` JSON) live; no history.
    func previewAdjustment(layer: DocLayerID, adjustmentJson: String) throws
    func clearPreview() throws
    /// The last preview / smart filter render error.
    func filterError() -> String?
    /// Cancels a running apply and the preview.
    func cancelFilter()
    /// The filter over `width × height` level-0 pixels at `(x, y)`: the dialog's 1:1 pane. Blocking.
    func filterDetail(layer: DocLayerID, filterJson: String, x: Int64, y: Int64, width: UInt32,
                      height: UInt32) throws -> FilterDetailSurface
    /// One history node: destructive on pixel layers (inside the selection), appended as a smart
    /// filter on smart objects. Blocking (seconds on large layers).
    func applyFilter(layer: DocLayerID, filterJson: String) throws -> DocumentChange
    /// Image ▸ Adjustments on a pixel layer (one history node). Blocking.
    func applyAdjustment(layer: DocLayerID, adjustmentJson: String) throws -> DocumentChange
    func smartFilters(layer: DocLayerID) throws -> [SmartFilterRow]
    func setSmartFilter(layer: DocLayerID, index: UInt32, change: SmartFilterChange) throws -> DocumentChange
    func removeSmartFilter(layer: DocLayerID, index: UInt32) throws -> DocumentChange
    /// The smart filter's mask as a grey RGBA8 IOSurface (white without a mask).
    func smartFilterMaskThumbnail(layer: DocLayerID, index: UInt32, maxPx: UInt32) throws -> UInt32
    /// Filter ▸ Convert for Smart Filters (one history node).
    func convertForSmartFilters(layer: DocLayerID) throws -> DocumentChange
}
