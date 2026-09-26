import Foundation
import TesseraFFI

// `EngineDocumentBackend` as a `DocumentFiltersBackend` (WP M5-12): each call is the session call of
// the same name, records converted field by field.

extension FilterCatalogEntry {
    init(_ f: FilterInfo) { self.init(id: f.id, group: f.group, name: f.name, schemaJson: f.paramsSchemaJson) }

    /// The engine's catalogue (`list_filters()`; static, no session needed).
    public static var engineCatalogue: [FilterCatalogEntry] { TesseraFFI.listFilters().map(FilterCatalogEntry.init) }
}

extension SmartFilterRow {
    init(_ r: SmartFilterRecord) {
        self.init(index: r.index, filterId: r.filterId, name: r.name, enabled: r.enabled, filterJson: r.filterJson,
                  opacity: r.opacity, blendMode: r.blendMode, hasMask: r.hasMask)
    }
}

extension SmartFilterChange {
    var ffi: SmartFilterEdit {
        switch self {
        case .enabled(let on): .enabled(enabled: on)
        case .params(let json): .params(filterJson: json)
        case .blending(let mode, let opacity): .blending(mode: mode, opacity: opacity)
        }
    }
}

extension EngineDocumentBackend: DocumentFiltersBackend {
    public func listFilters() -> [FilterCatalogEntry] { FilterCatalogEntry.engineCatalogue }

    public func previewFilter(layer: DocLayerID, filterJson: String, region: CanvasRect?) throws {
        try bridged { try session.previewFilter(layer: layer, filterJson: filterJson, region: region?.ffi) }
    }

    public func previewSmartFilter(layer: DocLayerID, index: UInt32, filterJson: String, region: CanvasRect?) throws {
        try bridged { try session.previewSmartFilter(layer: layer, index: index, filterJson: filterJson, region: region?.ffi) }
    }

    public func previewAdjustment(layer: DocLayerID, adjustmentJson: String) throws {
        try bridged { try session.previewAdjustment(layer: layer, adjustmentJson: adjustmentJson) }
    }

    public func clearPreview() throws { try bridged { try session.clearPreview() } }
    public func filterError() -> String? { session.filterError() }
    public func cancelFilter() { session.cancelFilter() }

    public func filterDetail(layer: DocLayerID, filterJson: String, x: Int64, y: Int64, width: UInt32,
                             height: UInt32) throws -> FilterDetailSurface {
        let d = try bridged {
            try session.filterDetail(layer: layer, filterJson: filterJson, x: x, y: y, width: width, height: height)
        }
        return FilterDetailSurface(surfaceId: d.surfaceId, width: d.width, height: d.height, level: d.level)
    }

    public func applyFilter(layer: DocLayerID, filterJson: String) throws -> DocumentChange {
        try change { try session.applyFilter(layer: layer, filterJson: filterJson) }
    }

    public func applyAdjustment(layer: DocLayerID, adjustmentJson: String) throws -> DocumentChange {
        try change { try session.applyAdjustment(layer: layer, adjustmentJson: adjustmentJson) }
    }

    public func smartFilters(layer: DocLayerID) throws -> [SmartFilterRow] {
        try bridged { try session.smartFilters(layer: layer) }.map(SmartFilterRow.init)
    }

    public func setSmartFilter(layer: DocLayerID, index: UInt32, change edit: SmartFilterChange) throws -> DocumentChange {
        try change { try session.setSmartFilter(layer: layer, index: index, edit: edit.ffi) }
    }

    public func removeSmartFilter(layer: DocLayerID, index: UInt32) throws -> DocumentChange {
        try change { try session.removeSmartFilter(layer: layer, index: index) }
    }

    public func smartFilterMaskThumbnail(layer: DocLayerID, index: UInt32, maxPx: UInt32) throws -> UInt32 {
        try bridged { try session.smartFilterMaskThumbnail(layer: layer, index: index, maxPx: maxPx) }
    }

    public func convertForSmartFilters(layer: DocLayerID) throws -> DocumentChange {
        try change { try session.convertForSmartFilters(layer: layer) }
    }
}
