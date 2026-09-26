import AppKit
import CoreGraphics
import IOSurface
import Observation
import SwiftUI
import TesseraCore

/// Filter menu, Image ▸ Adjustments and smart filters in document mode (WP M5-12). One per
/// workspace: it owns the open filter / adjustment / blending sheet, the Last Filter memory, and
/// runs full-resolution applies off the main thread.
@MainActor @Observable
final class DocumentFilters {
    /// Last Filter (⌃F) and the values each dialog reopens with.
    @ObservationIgnored let memory: FilterMemory
    /// The open filter dialog.
    var filterSheet: FilterSheetModel?
    /// The open Image ▸ Adjustments dialog.
    var adjustmentSheet: AdjustmentSheetModel?
    /// The open smart filter blending options dialog.
    var blendingSheet: SmartFilterBlendingModel?
    /// "Applying Gaussian Blur…" while a full-resolution apply runs.
    private(set) var busy: String?
    @ObservationIgnored private var catalogueCache: [FilterCatalogEntry]?

    /// The workspace's instance (the Layers outline reaches smart filters through it).
    static weak var active: DocumentFilters?

    init(memory: FilterMemory = FilterMemory(defaults: .standard)) {
        self.memory = memory
        Self.active = self
        if !Self.selfTestStarted {
            Self.selfTestStarted = true
            FilterSelfTest.startIfRequested()   // --filter-selftest <dir>
        }
    }
    private static var selfTestStarted = false

    // MARK: Catalogue

    static func backend(_ doc: DocumentController?) -> (any DocumentFiltersBackend)? {
        doc?.backend as? any DocumentFiltersBackend
    }

    /// The Filter menu's entries (the engine catalogue; loaded once).
    func catalogue(_ doc: DocumentController?) -> [FilterCatalogEntry] {
        if let c = catalogueCache { return c }
        guard let b = Self.backend(doc) else { return [] }
        let c = b.listFilters()
        catalogueCache = c
        return c
    }

    func entry(_ id: String, _ doc: DocumentController?) -> FilterCatalogEntry? { catalogue(doc).first { $0.id == id } }

    /// The layer filters apply to: the primary selection when it is a pixel layer or a smart object.
    static func target(_ doc: DocumentController?) -> LayerRecord? {
        guard let p = doc?.primary, p.kind == .pixel || p.kind == .smartObject else { return nil }
        return p
    }

    var lastFilterTitle: String {
        guard let last = memory.last, let e = catalogueCache?.first(where: { $0.id == last.id }) else { return "Last Filter" }
        return "Last Filter: \(e.name)"
    }

    // MARK: Filters

    /// Filter ▸ <filter>…: the dialog, or an immediate apply for filters without settings.
    func open(_ entry: FilterCatalogEntry, _ doc: DocumentController) {
        guard busy == nil else { doc.report?("\(busy ?? "") (wait for it to finish)"); return }
        guard let layer = Self.target(doc) else {
            doc.report?("\(entry.name): select a pixel layer or a smart object")
            return
        }
        let settings = memory.settings(for: entry)
        if entry.params.isEmpty {
            apply(settings, entry: entry, layer: layer.id, doc)
            return
        }
        filterSheet = FilterSheetModel(doc: doc, layer: layer, entry: entry, settings: settings, smartIndex: nil, owner: self)
    }

    /// Filter ▸ Last Filter (⌃F): the last filter again with its settings, no dialog.
    func reapplyLast(_ doc: DocumentController) {
        guard let last = memory.last, let entry = entry(last.id, doc) else { doc.report?("No filter has been applied yet"); return }
        guard let layer = Self.target(doc) else { doc.report?("\(entry.name): select a pixel layer or a smart object"); return }
        apply(FilterSettings(entry, values: last.values), entry: entry, layer: layer.id, doc)
    }

    /// Applies at full resolution off the main thread; one history node.
    func apply(_ settings: FilterSettings, entry: FilterCatalogEntry, layer: DocLayerID, _ doc: DocumentController) {
        guard let backend = Self.backend(doc), busy == nil else { return }
        let json = settings.json, name = entry.name
        busy = "Applying \(name)…"
        doc.report?(busy ?? "")
        let started = Date()
        Task { @MainActor [weak self] in
            let r = await Task.detached(priority: .userInitiated) {
                Result { try backend.applyFilter(layer: layer, filterJson: json) }
            }.value
            self?.busy = nil
            if doc.run(name, { try r.get() }) != nil {
                self?.memory.record(settings)
                doc.report?(String(format: "%@ applied (%.1f s)", name, Date().timeIntervalSince(started)))
            } else {
                try? backend.clearPreview()
            }
        }
    }

    /// Filter ▸ Convert for Smart Filters.
    func convertForSmartFilters(_ doc: DocumentController) {
        guard let p = doc.primary, let b = Self.backend(doc) else { return }
        if doc.run("Convert for Smart Filters", { try b.convertForSmartFilters(layer: p.id) }) != nil {
            doc.report?("\(p.name) is a smart object: filters now apply as smart filters")
        }
    }

    // MARK: Smart filters

    func smartFilters(_ doc: DocumentController, layer: DocLayerID) -> [SmartFilterRow] {
        (try? Self.backend(doc)?.smartFilters(layer: layer)) ?? []
    }

    /// Double-click on a smart filter: its dialog, previewing the re-edit.
    func editSmartFilter(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        guard let node = doc.node(layer), let entry = entry(row.filterId, doc) else { return }
        guard !entry.params.isEmpty else { doc.report?("\(entry.name) has no settings"); return }
        let saved = FilterSettings(json: row.filterJson) ?? FilterSettings(entry)
        filterSheet = FilterSheetModel(doc: doc, layer: node, entry: entry, settings: FilterSettings(entry, values: saved.values),
                                       smartIndex: row.index, owner: self)
    }

    func toggleSmartFilter(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        guard let b = Self.backend(doc) else { return }
        doc.run(row.enabled ? "Disable Smart Filter" : "Enable Smart Filter") {
            try b.setSmartFilter(layer: layer, index: row.index, change: .enabled(!row.enabled))
        }
    }

    func deleteSmartFilter(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        guard let b = Self.backend(doc) else { return }
        doc.run("Delete Smart Filter") { try b.removeSmartFilter(layer: layer, index: row.index) }
    }

    func blendingOptions(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        blendingSheet = SmartFilterBlendingModel(doc: doc, layer: layer, row: row)
    }

    // MARK: Image ▸ Adjustments

    func openAdjustment(_ kind: AdjustmentModel.Kind, _ doc: DocumentController) {
        guard busy == nil else { return }
        guard let p = doc.primary, p.kind == .pixel else {
            doc.report?("\(kind.title): select a pixel layer (use an adjustment layer for other layers)")
            return
        }
        if kind == .invert {
            applyAdjustment(kind.neutral, layer: p.id, doc)
            return
        }
        adjustmentSheet = AdjustmentSheetModel(doc: doc, layer: p, model: kind.neutral)
    }

    func applyAdjustment(_ model: AdjustmentModel, layer: DocLayerID, _ doc: DocumentController) {
        guard let backend = Self.backend(doc), busy == nil else { return }
        let json = model.json, title = model.kind.title
        busy = "Applying \(title)…"
        doc.report?(busy ?? "")
        Task { @MainActor [weak self] in
            let r = await Task.detached(priority: .userInitiated) {
                Result { try backend.applyAdjustment(layer: layer, adjustmentJson: json) }
            }.value
            self?.busy = nil
            if doc.run(title, { try r.get() }) != nil { doc.report?("\(title) applied") } else { try? backend.clearPreview() }
        }
    }
}

// MARK: - Filter dialog model

/// One open filter dialog: its values, the canvas preview and the 1:1 detail pane.
@MainActor @Observable
final class FilterSheetModel: Identifiable {
    let id = UUID()
    let doc: DocumentController
    let layer: LayerRecord
    let entry: FilterCatalogEntry
    /// Re-editing smart filter `index` (nil: a new filter).
    let smartIndex: UInt32?
    private(set) var settings: FilterSettings
    /// Preview on the canvas (the detail pane always shows the filter).
    var preview = true { didSet { preview ? pushPreview() : clear() } }
    /// Bumped when values change from outside a control (Reset), so sliders take the new value.
    private(set) var revision = 0
    private(set) var detail: CGImage?
    private(set) var detailLevel: UInt8 = 0
    /// Canvas point at the centre of the detail pane.
    private(set) var detailCenter: CGPoint
    private(set) var error: String?
    @ObservationIgnored private weak var owner: DocumentFilters?
    @ObservationIgnored private var detailPixels = CGSize(width: 360, height: 360)
    @ObservationIgnored private var detailTask: Task<Void, Never>?
    @ObservationIgnored private var detailGeneration = 0
    @ObservationIgnored private let backend: (any DocumentFiltersBackend)?

    init(doc: DocumentController, layer: LayerRecord, entry: FilterCatalogEntry, settings: FilterSettings, smartIndex: UInt32?,
         owner: DocumentFilters) {
        self.doc = doc
        self.layer = layer
        self.entry = entry
        self.settings = settings
        self.smartIndex = smartIndex
        self.owner = owner
        backend = DocumentFilters.backend(doc)
        let r = doc.lastFrame?.canvasRect ?? CanvasRect(x: 0, y: 0, width: Int64(doc.info.width), height: Int64(doc.info.height))
        detailCenter = CGPoint(x: Double(r.x) + Double(r.width) / 2, y: Double(r.y) + Double(r.height) / 2)
    }

    var title: String { entry.name }
    var subtitle: String { smartIndex == nil ? (layer.kind == .smartObject ? "Smart filter on “\(layer.name)”" : "Layer “\(layer.name)”")
        : "Editing a smart filter of “\(layer.name)”" }

    func value(_ p: FilterParam) -> FilterValue { settings.values[p.key] ?? p.control.defaultValue }

    func set(_ p: FilterParam, _ v: FilterValue, final: Bool = true) {
        let c = p.control.clamped(v)
        guard settings.values[p.key] != c else { return }
        settings.values[p.key] = c
        if preview { pushPreview() }
        refreshDetail(debounce: !final)
    }

    func reset() {
        settings = FilterSettings(entry)
        revision += 1
        if preview { pushPreview() }
        refreshDetail(debounce: false)
    }

    /// The visible canvas (what the preview renders).
    private var region: CanvasRect? { doc.lastFrame?.canvasRect }

    func start() {
        if preview { pushPreview() }
        refreshDetail(debounce: false)
    }

    private func pushPreview() {
        guard let backend else { return }
        do {
            if let i = smartIndex {
                try backend.previewSmartFilter(layer: layer.id, index: i, filterJson: settings.json, region: region)
            } else {
                try backend.previewFilter(layer: layer.id, filterJson: settings.json, region: region)
            }
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func clear() { try? backend?.clearPreview() }

    // MARK: Detail pane

    func setDetailPixels(_ size: CGSize) {
        guard size.width >= 1, size.height >= 1, size != detailPixels else { return }
        detailPixels = size
        refreshDetail(debounce: false)
    }

    /// Drag in the detail pane: `dx, dy` in pane pixels (1:1 canvas pixels).
    func panDetail(dx: Double, dy: Double) {
        let scale = Double(1 << detailLevel)
        let w = Double(doc.info.width), h = Double(doc.info.height)
        detailCenter = CGPoint(x: min(max(detailCenter.x - dx * scale, 0), w), y: min(max(detailCenter.y - dy * scale, 0), h))
        refreshDetail(debounce: true)
    }

    /// Clicking the canvas area of the pane jumps there (canvas point).
    func centerDetail(at p: CGPoint) {
        detailCenter = p
        refreshDetail(debounce: false)
    }

    private func refreshDetail(debounce: Bool) {
        guard let backend else { return }
        detailTask?.cancel()
        detailGeneration += 1
        let gen = detailGeneration
        let (w, h) = (UInt32(detailPixels.width), UInt32(detailPixels.height))
        let x = Int64(detailCenter.x) - Int64(w / 2), y = Int64(detailCenter.y) - Int64(h / 2)
        let json = settings.json, layer = layer.id
        detailTask = Task { @MainActor [weak self] in
            if debounce { try? await Task.sleep(for: .milliseconds(40)) }
            guard !Task.isCancelled else { return }
            let result = await Task.detached(priority: .userInitiated) { () -> Result<(CGImage?, UInt8), Error> in
                Result {
                    let d = try backend.filterDetail(layer: layer, filterJson: json, x: x, y: y, width: w, height: h)
                    return (IOSurfaceLookup(d.surfaceId).flatMap { FilterSheetModel.image($0, width: Int(d.width), height: Int(d.height)) }, d.level)
                }
            }.value
            guard let self, gen == self.detailGeneration else { return }
            switch result {
            case .success(let (image, level)):
                self.detail = image
                self.detailLevel = level
            case .failure(let e):
                self.error = e.localizedDescription
            }
        }
    }

    nonisolated static func image(_ s: IOSurfaceRef, width: Int, height: Int) -> CGImage? {
        IOSurfaceLock(s, .readOnly, nil)
        defer { IOSurfaceUnlock(s, .readOnly, nil) }
        let stride = IOSurfaceGetBytesPerRow(s)
        let data = Data(bytes: IOSurfaceGetBaseAddress(s), count: stride * height)
        guard let provider = CGDataProvider(data: data as CFData) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: stride,
                       space: CGColorSpace(name: CGColorSpace.sRGB)!,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }

    // MARK: Closing

    func cancel() {
        detailTask?.cancel()
        clear()
        owner?.filterSheet = nil
    }

    func ok() {
        detailTask?.cancel()
        guard let owner else { return }
        owner.filterSheet = nil
        if let i = smartIndex {
            clear()
            guard let backend else { return }
            let json = settings.json
            if doc.run("Edit Smart Filter", { try backend.setSmartFilter(layer: layer.id, index: i, change: .params(json: json)) }) != nil {
                owner.memory.record(settings)
            }
        } else {
            // The engine ends the preview when the apply lands (the preview stays up meanwhile).
            owner.apply(settings, entry: entry, layer: layer.id, doc)
        }
    }
}

// MARK: - Image ▸ Adjustments model

@MainActor @Observable
final class AdjustmentSheetModel: Identifiable {
    let id = UUID()
    let doc: DocumentController
    let layer: LayerRecord
    private(set) var model: AdjustmentModel
    private(set) var revision = 0
    var preview = true { didSet { preview ? push() : clear() } }
    private(set) var error: String?

    init(doc: DocumentController, layer: LayerRecord, model: AdjustmentModel) {
        self.doc = doc
        self.layer = layer
        self.model = model
    }

    func start() { push() }

    func edit(_ m: AdjustmentModel, final: Bool) {
        model = m
        if final { revision += 1 }
        if preview { push() }
    }

    func reset() {
        model = model.kind.neutral
        revision += 1
        if preview { push() }
    }

    private func push() {
        do {
            try DocumentFilters.backend(doc)?.previewAdjustment(layer: layer.id, adjustmentJson: model.json)
            error = nil
        } catch { self.error = error.localizedDescription }
    }

    private func clear() { try? DocumentFilters.backend(doc)?.clearPreview() }

    func cancel(_ owner: DocumentFilters) {
        clear()
        owner.adjustmentSheet = nil
    }

    func ok(_ owner: DocumentFilters) {
        owner.adjustmentSheet = nil
        owner.applyAdjustment(model, layer: layer.id, doc)
    }
}

// MARK: - Smart filter blending options

@MainActor @Observable
final class SmartFilterBlendingModel: Identifiable {
    let id = UUID()
    let doc: DocumentController
    let layer: DocLayerID
    let row: SmartFilterRow
    var mode: DocBlendMode
    /// Percent.
    var opacity: Double

    init(doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        self.doc = doc
        self.layer = layer
        self.row = row
        mode = DocBlendMode(backendName: row.blendMode) ?? .normal
        opacity = Double(row.opacity) * 100
    }

    func ok(_ owner: DocumentFilters) {
        owner.blendingSheet = nil
        guard let b = DocumentFilters.backend(doc) else { return }
        let (mode, opacity, layer, index) = (mode.backendName, Float(opacity / 100), layer, row.index)
        doc.run("Smart Filter Blending Options") {
            try b.setSmartFilter(layer: layer, index: index, change: .blending(mode: mode, opacity: opacity))
        }
    }
}
