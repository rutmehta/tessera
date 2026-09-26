import Foundation

// The stub backend (`--stub-library`, unit tests) and filters (WP M5-12): the menu is the engine's
// catalogue (static data), previews show nothing and applying explains that filters need the engine.
// The stub has no smart objects, so its smart filter lists are empty.

extension StubDocumentBackend: DocumentFiltersBackend {
    static let needsEngine = DocumentError.unsupported("Filters need the engine; open a folder or run without --stub-library")

    public func listFilters() -> [FilterCatalogEntry] { FilterCatalogEntry.engineCatalogue }
    public func previewFilter(layer: DocLayerID, filterJson: String, region: CanvasRect?) throws {}
    public func previewSmartFilter(layer: DocLayerID, index: UInt32, filterJson: String, region: CanvasRect?) throws {}
    public func previewAdjustment(layer: DocLayerID, adjustmentJson: String) throws {}
    public func clearPreview() throws {}
    public func filterError() -> String? { nil }
    public func cancelFilter() {}
    public func filterDetail(layer: DocLayerID, filterJson: String, x: Int64, y: Int64, width: UInt32,
                             height: UInt32) throws -> FilterDetailSurface { throw Self.needsEngine }
    public func applyFilter(layer: DocLayerID, filterJson: String) throws -> DocumentChange { throw Self.needsEngine }
    public func applyAdjustment(layer: DocLayerID, adjustmentJson: String) throws -> DocumentChange { throw Self.needsEngine }
    public func smartFilters(layer: DocLayerID) throws -> [SmartFilterRow] { [] }
    public func setSmartFilter(layer: DocLayerID, index: UInt32, change: SmartFilterChange) throws -> DocumentChange {
        throw Self.needsEngine
    }
    public func removeSmartFilter(layer: DocLayerID, index: UInt32) throws -> DocumentChange { throw Self.needsEngine }
    public func smartFilterMaskThumbnail(layer: DocLayerID, index: UInt32, maxPx: UInt32) throws -> UInt32 {
        throw Self.needsEngine
    }
    public func convertForSmartFilters(layer: DocLayerID) throws -> DocumentChange { throw Self.needsEngine }
}
