import Foundation
import TesseraCore

/// Only the profile bytes vary; all state, surfaces and mutations use the real stub backend.
final class ProfileSwitchBackend: DocumentBackend, @unchecked Sendable {
    let base = StubDocumentBackend()
    var profile: Data?
    func id() -> String { base.id() }
    func info() throws -> DocumentSummary { try base.info() }
    func layers() throws -> [LayerRecord] { try base.layers() }
    func layer(id: DocLayerID) throws -> LayerRecord { try base.layer(id: id) }
    func setSelectedLayers(ids: [DocLayerID]) throws { try base.setSelectedLayers(ids: ids) }
    func layerThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 { try base.layerThumbnail(id: id, maxPx: maxPx) }
    func maskThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 { try base.maskThumbnail(id: id, maxPx: maxPx) }
    func compositeThumbnail(maxPx: UInt32) throws -> UInt32 { try base.compositeThumbnail(maxPx: maxPx) }
    func addLayer(kind: NewLayerKind, name: String, parent: DocLayerID?, index: UInt32?) throws -> DocumentChange { try base.addLayer(kind: kind, name: name, parent: parent, index: index) }
    func duplicateLayer(id: DocLayerID) throws -> DocumentChange { try base.duplicateLayer(id: id) }
    func removeLayer(id: DocLayerID) throws -> DocumentChange { try base.removeLayer(id: id) }
    func moveLayer(id: DocLayerID, parent: DocLayerID?, index: UInt32) throws -> DocumentChange { try base.moveLayer(id: id, parent: parent, index: index) }
    func setProps(id: DocLayerID, props: LayerProperties) throws -> DocumentChange { try base.setProps(id: id, props: props) }
    func renameLayer(id: DocLayerID, name: String) throws -> DocumentChange { try base.renameLayer(id: id, name: name) }
    func setVisible(id: DocLayerID, visible: Bool) throws -> DocumentChange { try base.setVisible(id: id, visible: visible) }
    func setOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange { try base.setOpacity(id: id, value: value, interactive: interactive) }
    func setFillOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange { try base.setFillOpacity(id: id, value: value, interactive: interactive) }
    func setBlendMode(id: DocLayerID, mode: String) throws -> DocumentChange { try base.setBlendMode(id: id, mode: mode) }
    func setGroupMode(id: DocLayerID, mode: LayerGroupMode) throws -> DocumentChange { try base.setGroupMode(id: id, mode: mode) }
    func setLocks(id: DocLayerID, locks: LayerLockFlags) throws -> DocumentChange { try base.setLocks(id: id, locks: locks) }
    func setAdjustmentJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange { try base.setAdjustmentJson(id: id, json: json, interactive: interactive) }
    func setFillJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange { try base.setFillJson(id: id, json: json, interactive: interactive) }
    func addMask(id: DocLayerID, mask: LayerMaskInit) throws -> DocumentChange { try base.addMask(id: id, mask: mask) }
    func removeMask(id: DocLayerID) throws -> DocumentChange { try base.removeMask(id: id) }
    func setMaskEnabled(id: DocLayerID, enabled: Bool) throws -> DocumentChange { try base.setMaskEnabled(id: id, enabled: enabled) }
    func setMaskDensity(id: DocLayerID, density: Float) throws -> DocumentChange { try base.setMaskDensity(id: id, density: density) }
    func setMaskLinked(id: DocLayerID, linked: Bool) throws { try base.setMaskLinked(id: id, linked: linked) }
    func setClipped(id: DocLayerID, clipped: Bool) throws -> DocumentChange { try base.setClipped(id: id, clipped: clipped) }
    func mergeDown(id: DocLayerID) throws -> DocumentChange { try base.mergeDown(id: id) }
    func flatten() throws -> DocumentChange { try base.flatten() }
    func groupLayers(ids: [DocLayerID], name: String) throws -> DocumentChange { try base.groupLayers(ids: ids, name: name) }
    func ungroupLayer(id: DocLayerID) throws -> DocumentChange { try base.ungroupLayer(id: id) }
    func setSelectionRect(x: Int64, y: Int64, width: Int64, height: Int64, feather: Float) throws -> DocumentChange { try base.setSelectionRect(x: x, y: y, width: width, height: height, feather: feather) }
    func clearSelection() throws -> DocumentChange { try base.clearSelection() }
    func commit(label: String) throws -> DocumentChange { try base.commit(label: label) }
    func undo() throws -> DocumentChange { try base.undo() }
    func redo() throws -> DocumentChange { try base.redo() }
    func historyItems() throws -> [DocHistoryEntry] { try base.historyItems() }
    func checkoutHistory(id: DocHistoryID) throws -> DocumentChange { try base.checkoutHistory(id: id) }
    func snapshot(name: String) throws { try base.snapshot(name: name) }
    func snapshots() throws -> [String] { try base.snapshots() }
    func restoreSnapshot(name: String) throws -> DocumentChange { try base.restoreSnapshot(name: name) }
    func setMaxStates(maxStates: UInt32) throws { try base.setMaxStates(maxStates: maxStates) }
    func historyMemoryBytes() throws -> UInt64 { try base.historyMemoryBytes() }
    func setListener(listener: (any DocumentBackendListener)?) { base.setListener(listener: listener) }
    func planSurface(width: UInt32, height: UInt32) throws -> DocViewportPlan { try base.planSurface(width: width, height: height) }
    func attachSurface(iosurfaceId: UInt32, width: UInt32, height: UInt32) throws { try base.attachSurface(iosurfaceId: iosurfaceId, width: width, height: height) }
    func setViewport(level: UInt8, x: UInt32, y: UInt32, width: UInt32, height: UInt32, zoom: Double) throws { try base.setViewport(level: level, x: x, y: y, width: width, height: height, zoom: zoom) }
    func setDisplayHeadroom(headroom: Float) throws { try base.setDisplayHeadroom(headroom: headroom) }
    func displayProfileICC() throws -> Data? { profile }
    func refresh() throws { try base.refresh() }
    func detachSurfaces() { base.detachSurfaces() }
    func save() throws { try base.save() }
    func saveAs(path: String) throws { try base.saveAs(path: path) }
    func saveAs(path: String, intent: DocSaveDestinationIntent) throws -> DocSaveAsResult { try base.saveAs(path: path, intent: intent) }
    func exportFlat(path: String, format: DocExportFormat, quality: UInt8, color: DocExportColor) throws { try base.exportFlat(path: path, format: format, quality: quality, color: color) }
    func close() { base.close() }
}
