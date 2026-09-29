import AppKit
import CoreGraphics
import IOSurface
import Observation
import SwiftUI
import TesseraCore

/// Filter ▸ Camera Raw Filter… (⇧⌘A) in document mode (WP B5-18): the Develop renderer as a filter over the
/// selected pixel layer (inside the selection) or as a smart filter on a smart object. The sheet edits a
/// `CameraRawDraft` with the Develop panels' controls, previews on the canvas (latest-wins in the engine,
/// drags coalesced here), shows a 1:1 detail crop, and applies as one history node; a smart object's
/// camera_raw row re-opens it (double-click) and OK replaces that smart filter's settings.
@MainActor @Observable
final class DocumentCameraRaw {
    static let shared = DocumentCameraRaw()
    static let menuTitle = CameraRawFilter.title + "…"

    /// Smart filter rows whose double-click opens this sheet rather than the generic filter dialog.
    static func handles(filterId: String) -> Bool { filterId == CameraRawFilter.id }

    var sheet: CameraRawSheetModel?
    /// "Applying Camera Raw Filter…" while a full-resolution apply runs.
    private(set) var busy: String?
    /// The tab the sheet reopens on.
    @ObservationIgnored var lastPanel: CameraRawPanel = .basic

    /// Filter ▸ Camera Raw Filter…: a new filter on the selected layer.
    func open(_ doc: DocumentController) {
        guard busy == nil else { doc.report?("\(busy ?? "") (wait for it to finish)"); return }
        if let why = CameraRawFilter.refusal(kind: doc.primary?.kind, hasSelection: doc.marquee != nil) {
            doc.report?(why)
            return
        }
        guard let layer = doc.primary else { return }
        sheet = CameraRawSheetModel(doc: doc, layer: layer, draft: CameraRawDraft(), smartIndex: nil, owner: self)
    }

    /// Double-click on a camera_raw smart filter: the sheet with its saved settings, previewing the re-edit.
    func edit(_ doc: DocumentController, layer: DocLayerID, row: SmartFilterRow) {
        guard busy == nil else { doc.report?("\(busy ?? "") (wait for it to finish)"); return }
        guard let draft = CameraRawDraft(filterJson: row.filterJson) else {
            doc.report?("\(CameraRawFilter.title): the smart filter's settings could not be read")
            return
        }
        if let why = draft.aiMaskRefusal { doc.report?(why); return }
        guard let node = doc.node(layer) else { return }
        sheet = CameraRawSheetModel(doc: doc, layer: node, draft: draft, smartIndex: row.index, owner: self)
    }

    /// OK: apply (or replace the smart filter's settings) off the main thread; the sheet stays up with a
    /// progress line and Cancel until it lands.
    func apply(_ model: CameraRawSheetModel) {
        guard busy == nil, let backend = DocumentFilters.backend(model.doc) else { return }
        let doc = model.doc, layer = model.layer.id, json = model.draft.filterJson, index = model.smartIndex
        let title = CameraRawFilter.title
        busy = "Applying \(title)…"
        doc.report?(busy ?? "")
        if index != nil { try? backend.clearPreview() }
        let started = Date()
        Task { @MainActor [weak self] in
            let r = await Task.detached(priority: .userInitiated) {
                Result {
                    if let index {
                        try backend.setSmartFilter(layer: layer, index: index, change: .params(json: json))
                    } else {
                        try backend.applyFilter(layer: layer, filterJson: json)
                    }
                }
            }.value
            self?.busy = nil
            if self?.sheet === model { self?.sheet = nil }
            if doc.run(title, { try r.get() }) != nil {
                doc.report?(String(format: "%@ applied (%.1f s)", title, Date().timeIntervalSince(started)))
            } else {
                try? backend.clearPreview()
            }
        }
    }
}

// MARK: - Sheet model

@MainActor @Observable
final class CameraRawSheetModel: Identifiable {
    let id = UUID()
    let doc: DocumentController
    let layer: LayerRecord
    /// Re-editing smart filter `index` (nil: a new filter).
    let smartIndex: UInt32?
    private(set) var draft: CameraRawDraft
    var panel: CameraRawPanel { didSet { owner?.lastPanel = panel } }
    /// Before: the canvas and detail pane show the layer without the filter.
    var showBefore = false { didSet { if showBefore != oldValue { pushPreview(); refreshDetail() } } }
    /// Bumped when values change from outside a slider (Reset), so sliders take the new value.
    private(set) var revision = 0
    private(set) var detail: CGImage?
    private(set) var detailLevel: UInt8 = 0
    private(set) var error: String?
    var applying: Bool { owner?.busy != nil }
    @ObservationIgnored private weak var owner: DocumentCameraRaw?
    @ObservationIgnored private let backend: (any DocumentFiltersBackend)?
    @ObservationIgnored private var previewTask: Task<Void, Never>?
    @ObservationIgnored private var detailGate = LatestRequestBuffer<String>()
    @ObservationIgnored private var detailPixels = CGSize(width: 320, height: 320)
    @ObservationIgnored private(set) var detailCenter: CGPoint
    @ObservationIgnored private var closed = false

    init(doc: DocumentController, layer: LayerRecord, draft: CameraRawDraft, smartIndex: UInt32?, owner: DocumentCameraRaw) {
        self.doc = doc
        self.layer = layer
        self.draft = draft
        self.smartIndex = smartIndex
        self.owner = owner
        panel = owner.lastPanel
        backend = DocumentFilters.backend(doc)
        let r = doc.lastFrame?.canvasRect ?? CanvasRect(x: 0, y: 0, width: Int64(doc.info.width), height: Int64(doc.info.height))
        detailCenter = CGPoint(x: Double(r.x) + Double(r.width) / 2, y: Double(r.y) + Double(r.height) / 2)
    }

    var title: String { CameraRawFilter.title }
    var subtitle: String {
        if smartIndex != nil { return "Editing a smart filter of “\(layer.name)”" }
        return layer.kind == .smartObject ? "Smart filter on “\(layer.name)”"
            : (doc.marquee != nil ? "Layer “\(layer.name)”, inside the selection" : "Layer “\(layer.name)”")
    }

    func value(_ c: DevelopControl) -> Double { draft.value(c) }

    func set(_ c: DevelopControl, _ v: Double, final: Bool) {
        var d = draft
        d.set(c, v)
        guard d != draft else { return }
        draft = d
        if showBefore { showBefore = false }
        schedulePreview(final: final)
    }

    var amountPercent: Double { draft.amountPercent }

    /// Pyramid level of the last submitted canvas preview (the engine's `filter_preview_level`).
    private(set) var previewLevel = 0

    /// B5-18b: when the submitted preview's level is above 0 (zoom at or below 50 %), the canvas preview leaves
    /// out Sharpening, Noise Reduction, Texture and Clarity.
    var detailPreviewNote: String? { showBefore ? nil : draft.detailPreviewNote(previewLevel: previewLevel) }

    func setAmount(_ percent: Double, final: Bool) {
        var d = draft
        d.amountPercent = percent
        guard d != draft else { return }
        draft = d
        schedulePreview(final: final)
    }

    func reset() {
        draft.reset()
        revision += 1
        schedulePreview(final: true)
    }

    func start() {
        pushPreview()
        refreshDetail()
    }

    // MARK: Preview

    /// Slider drags are coalesced (every camera_raw preview is a full-resolution render); mouse-up at once.
    private func schedulePreview(final: Bool) {
        previewTask?.cancel()
        if final {
            pushPreview()
            refreshDetail()
            return
        }
        previewTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(120))
            guard !Task.isCancelled, let self else { return }
            self.pushPreview()
            self.refreshDetail()
        }
    }

    private func pushPreview() {
        guard let backend, !closed else { return }
        do {
            if showBefore {
                try backend.clearPreview()
            } else if let i = smartIndex {
                try backend.previewSmartFilter(layer: layer.id, index: i, filterJson: draft.filterJson, region: doc.lastFrame?.canvasRect)
            } else {
                try backend.previewFilter(layer: layer.id, filterJson: draft.filterJson, region: doc.lastFrame?.canvasRect)
            }
            if !showBefore {
                // Same viewport as the submit just above (both on the main thread): the level it renders at.
                previewLevel = Int(try backend.filterPreviewLevel(layer: layer.id, smartIndex: smartIndex,
                                                                  filterJson: draft.filterJson))
            }
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    // MARK: Detail pane (1:1)

    func setDetailPixels(_ size: CGSize) {
        guard size.width >= 1, size.height >= 1, size != detailPixels else { return }
        detailPixels = size
        refreshDetail()
    }

    /// Drag in the pane: `dx, dy` in pane pixels.
    func panDetail(dx: Double, dy: Double) {
        let scale = Double(1 << detailLevel)
        detailCenter = CGPoint(x: min(max(detailCenter.x - dx * scale, 0), Double(doc.info.width)),
                               y: min(max(detailCenter.y - dy * scale, 0), Double(doc.info.height)))
        refreshDetail()
    }

    private var detailJson: String {
        guard showBefore else { return draft.filterJson }
        var before = draft
        before.amountPercent = 0
        return before.filterJson
    }

    /// Latest-wins: one `filter_detail` runs at a time; a newer draft replaces the pending one.
    private func refreshDetail() {
        guard !closed, let request = detailGate.submit(detailJson) else { return }
        run(request)
    }

    private func run(_ request: LatestRequestBuffer<String>.Request) {
        guard let backend else { return }
        let (w, h) = (UInt32(detailPixels.width), UInt32(detailPixels.height))
        let x = Int64(detailCenter.x) - Int64(w / 2), y = Int64(detailCenter.y) - Int64(h / 2)
        let layer = layer.id, json = request.value, index = smartIndex
        Task { @MainActor [weak self] in
            let result = await Task.detached(priority: .userInitiated) { () -> Result<(CGImage?, UInt8), Error> in
                Result {
                    // B5-18b: re-editing replaces the saved filter in the pane (no double apply).
                    let d = try backend.filterDetail(layer: layer, smartIndex: index, filterJson: json, x: x, y: y,
                                                     width: w, height: h)
                    return (IOSurfaceLookup(d.surfaceId).flatMap { FilterSheetModel.image($0, width: Int(d.width), height: Int(d.height)) },
                            d.level)
                }
            }.value
            guard let self else { return }
            let (accept, next) = self.detailGate.finish(request.generation)
            if accept, !self.closed {
                switch result {
                case .success(let (image, level)):
                    self.detail = image
                    self.detailLevel = level
                    if self.error == nil { self.error = backend.filterError() }
                case .failure(let e):
                    self.error = e.localizedDescription
                }
            }
            if let next, !self.closed { self.run(next) }
        }
    }

    // MARK: Closing

    func cancel() {
        if applying {
            backend?.cancelFilter()
            return
        }
        close()
        try? backend?.clearPreview()
        owner?.sheet = nil
    }

    func ok() {
        guard !applying else { return }
        if showBefore { showBefore = false }
        previewTask?.cancel()
        closed = true
        detailGate.invalidate()
        owner?.apply(self)
    }

    private func close() {
        closed = true
        previewTask?.cancel()
        detailGate.invalidate()
    }
}

// MARK: - Views

/// The sheet: detail pane, Amount and Before/After on the left; tabbed Develop controls on the right.
/// Identifiers: `document.cameraRaw.<panel>`, `.<control id>`, `.amount`, `.before`, `.reset`, `.cancel`, `.ok`, `.detail`,
/// `.detailNote`.
struct CameraRawSheet: View {
    @Bindable var model: CameraRawSheetModel
    private let ident = "document.cameraRaw"

    var body: some View {
        SheetScaffold(title: model.title, subtitle: model.subtitle) {
            EmptyView()
        } content: {
            HStack(alignment: .top, spacing: 0) {
                VStack(alignment: .leading, spacing: Theme.Space.m) {
                    CameraRawDetailPane(model: model)
                        .frame(width: 280, height: 280)
                        .accessibilityIdentifier("\(ident).detail")
                    DocSlider(title: "Amount", value: model.amountPercent, range: 0...100, defaultValue: 100, format: "%.0f %%",
                              step: 1, identifier: "\(ident).amount", revision: model.revision) { v, final in
                        model.setAmount(v, final: final)
                    }
                    .frame(width: 280, height: Theme.Height.slider)
                    Toggle("Show before", isOn: $model.showBefore)
                        .toggleStyle(.checkbox)
                        .font(Theme.Fonts.caption)
                        .help("Shows the layer without the filter (canvas and detail pane)")
                        .accessibilityIdentifier("\(ident).before")
                    if let note = model.detailPreviewNote {
                        Text(note)
                            .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            .help("Sharpening, Noise Reduction, Texture and Clarity show on the canvas at 100 % and in the 1:1 pane; OK applies them at any zoom.")
                            .accessibilityIdentifier("\(ident).detailNote")
                    }
                    if model.applying {
                        HStack(spacing: Theme.Space.xs) {
                            ProgressView().controlSize(.small)
                            Text("Applying at full resolution… Cancel stops it.")
                                .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        }
                        .accessibilityIdentifier("document.cameraRaw.busy")
                    }
                    if let e = model.error { StatusLine(text: e, kind: .error) }
                    Spacer(minLength: 0)
                }
                .padding(Theme.Space.l)
                Hairline(vertical: true)
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    SegmentedPicker(selection: $model.panel,
                                    segments: CameraRawPanel.allCases.map { .init(value: $0, title: $0.title) },
                                    height: Theme.Height.small)
                        .accessibilityIdentifier("\(ident).panel")
                    ScrollView {
                        VStack(alignment: .leading, spacing: Theme.Space.m) {
                            ForEach(model.panel.sections) { section in
                                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                                    Text(section.title).font(Theme.Fonts.caption.weight(.semibold)).foregroundStyle(Theme.textPrimary)
                                    ForEach(section.controls) { c in
                                        DocSlider(title: c.title, value: model.value(c), range: c.range, defaultValue: c.defaultValue,
                                                  format: c.format, step: c.step, identifier: "\(ident).\(c.id)",
                                                  revision: model.revision) { v, final in model.set(c, v, final: final) }
                                            .frame(height: Theme.Height.slider)
                                    }
                                }
                            }
                        }
                        .padding(.trailing, Theme.Space.s)
                    }
                    .scrollIndicators(.automatic)
                    .accessibilityIdentifier("\(ident).\(model.panel.rawValue)")
                }
                .padding(Theme.Space.l)
                .frame(maxWidth: .infinity, alignment: .topLeading)
            }
            .disabled(model.applying)
        } leading: {
            EmptyView()
        } actions: {
            Button("Reset") { model.reset() }
                .sheetButton()
                .disabled(model.applying)
                .accessibilityIdentifier("\(ident).reset")
            Button("Cancel") { model.cancel() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
                .accessibilityIdentifier("\(ident).cancel")
            Button("OK") { model.ok() }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .disabled(model.applying)
                .accessibilityIdentifier("\(ident).ok")
        }
        .frame(width: 860, height: 600)
        .onAppear { model.start() }
    }
}

/// The 1:1 pane: the filter on the layer's own pixels, one image pixel per device pixel. Drag to move.
struct CameraRawDetailPane: View {
    let model: CameraRawSheetModel
    @Environment(\.displayScale) private var scale
    @State private var last: CGPoint?

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .bottomLeading) {
                RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.plotWell))
                if let image = model.detail {
                    Image(decorative: image, scale: scale)
                        .interpolation(.none)
                        .frame(width: g.size.width, height: g.size.height)
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                }
                Text((model.detailLevel == 0 ? "1:1" : "1:\(1 << Int(model.detailLevel))") + (model.showBefore ? " before" : ""))
                    .font(Theme.Fonts.captionNumeric)
                    .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                    .padding(.horizontal, Theme.Space.xs)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                    .padding(Theme.Space.xs)
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 1)
                .onChanged { v in
                    let prev = last ?? v.startLocation
                    model.panDetail(dx: (v.location.x - prev.x) * scale, dy: (v.location.y - prev.y) * scale)
                    last = v.location
                }
                .onEnded { _ in last = nil })
            .onAppear { model.setDetailPixels(CGSize(width: g.size.width * scale, height: g.size.height * scale)) }
        }
        .help("The filter at 100 %. Drag to look at another part of the layer.")
        .accessibilityElement()
        .accessibilityLabel("Camera Raw detail preview")
    }
}

/// Filter ▸ Camera Raw Filter… (⇧⌘A).
struct CameraRawMenuItem: View {
    let doc: DocumentController?

    var body: some View {
        let cr = DocumentCameraRaw.shared
        let target = doc?.primary.map { $0.kind == .pixel || $0.kind == .smartObject } ?? false
        Button(DocumentCameraRaw.menuTitle) { if let doc { cr.open(doc) } }
            .shortcut(doc != nil, "a", [.command, .shift])
            .disabled(!target || cr.busy != nil)
    }
}

/// The sheet, hung off the document view.
struct CameraRawSheets: ViewModifier {
    @Bindable var cameraRaw: DocumentCameraRaw

    func body(content: Content) -> some View {
        let _ = CameraRawSelfTest.startIfRequested()
        return content.sheet(item: $cameraRaw.sheet) { CameraRawSheet(model: $0) }
    }
}
