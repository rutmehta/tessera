import AppKit
import Observation
import SwiftUI
import TesseraCore

/// Layer styles in document mode (WP B5-07): Layer ▸ Layer Style, the Layer Style inspector (a
/// floating panel, not a modal dialog: edits apply live and each gesture is one history node), the
/// Global Light panel, Copy / Paste / Clear Layer Style and the styled-layer rows the Layers panel
/// shows. One per app; it acts on the document it is handed (the current one).
@MainActor @Observable
final class DocumentStyles {
    static let shared = DocumentStyles()

    private init() {
        StylesSelfTest.startIfRequested()   // --styles-selftest <dir>
    }

    /// What the inspector's right side shows.
    enum Pane: Equatable, Hashable {
        case blending
        /// Effect `index` of the layer's `effects`.
        case effect(Int)
    }

    /// The document the inspector and the Global Light panel show.
    private(set) weak var document: DocumentController?
    var pane: Pane = .blending
    /// Bumped after every styles edit (views re-read the model).
    private(set) var revision = 0
    @ObservationIgnored private var schemaCache: StyleSchema?
    @ObservationIgnored private var inspectorWindow: NSPanel?
    @ObservationIgnored private var lightWindow: NSPanel?
    @ObservationIgnored private var colorDebounce: Task<Void, Never>?
    @ObservationIgnored private var pendingLabel: String?

    // MARK: Backend and model

    static func backend(_ doc: DocumentController?) -> (any DocumentStylesBackend)? {
        doc?.backend as? any DocumentStylesBackend
    }

    /// The engine's effect schema (loaded once).
    var schema: StyleSchema? {
        if let s = schemaCache { return s }
        guard let b = Self.backend(document) ?? Self.backend(DocumentStyles.lastDocument) else { return nil }
        schemaCache = StyleSchema(json: b.styleSchemaJson())
        return schemaCache
    }

    /// Any document seen, for the schema before the inspector opens.
    @ObservationIgnored private static weak var lastDocument: DocumentController?

    /// The layer's styles (the live state while dragging).
    func model(_ doc: DocumentController, _ layer: DocLayerID) -> LayerStyleModel? {
        _ = revision
        _ = doc.revision
        guard let b = Self.backend(doc) else { return nil }
        return (try? b.layerStylesJson(layer: layer)).flatMap { LayerStyleModel(json: $0) }
    }

    func globalLight(_ doc: DocumentController) -> DocGlobalLight {
        _ = revision
        _ = doc.revision
        return (try? Self.backend(doc)?.globalLight()) ?? .default
    }

    /// Styled layers of `doc`, effects top first.
    func rows(_ doc: DocumentController) -> [DocLayerID: LayerStyleRow] {
        guard let b = Self.backend(doc), let rows = try? b.layerStyleRows() else { return [:] }
        return Dictionary(rows.map { ($0.layer, $0) }, uniquingKeysWith: { a, _ in a })
    }

    /// Whether the layer can take effects (not an adjustment layer or a pass-through group).
    static func canStyle(_ n: LayerRecord?) -> Bool {
        guard let n else { return false }
        return n.kind != .adjustment && !(n.kind == .group && n.groupMode == .passThrough)
    }

    // MARK: Edits

    /// Writes `model` for `layer`: live while `final` is false (no history), then one node labelled
    /// `label` on release. Rejected edits (locked layers) report and re-read the model.
    func apply(_ doc: DocumentController, _ layer: DocLayerID, _ model: LayerStyleModel, label: String, final: Bool) {
        guard let b = Self.backend(doc) else { return }
        colorDebounce?.cancel()
        let ok = doc.run(label) { try b.setLayerStylesJson(layer: layer, json: model.json, interactive: true) } != nil
        revision += 1
        if final || !ok { commit(doc, label) }
    }

    /// Colour wells send a stream of values: live, then one node after a short pause.
    func applyColor(_ doc: DocumentController, _ layer: DocLayerID, _ model: LayerStyleModel, label: String) {
        guard let b = Self.backend(doc) else { return }
        colorDebounce?.cancel()
        doc.run(label) { try b.setLayerStylesJson(layer: layer, json: model.json, interactive: true) }
        revision += 1
        colorDebounce = Task { @MainActor [weak self, weak doc] in
            try? await Task.sleep(for: .milliseconds(400))
            guard !Task.isCancelled, let self, let doc else { return }
            self.commit(doc, label)
        }
    }

    func setGlobalLight(_ doc: DocumentController, _ light: DocGlobalLight, final: Bool) {
        guard let b = Self.backend(doc) else { return }
        colorDebounce?.cancel()
        doc.run("Global Light") { try b.setGlobalLight(light, interactive: true) }
        revision += 1
        if final { commit(doc, "Global Light") }
    }

    /// Records the pending drag as one history node.
    func commit(_ doc: DocumentController, _ label: String) {
        doc.run(label) { try doc.backend.commit(label: label) }
        revision += 1
    }

    /// Adds an effect of `kind` (from the schema's defaults) above `above` or on top of its kind,
    /// selects it in the inspector and records one node.
    func addEffect(_ doc: DocumentController, _ layer: DocLayerID, _ kind: StyleEffectKind, above: Int? = nil) {
        guard var m = model(doc, layer) else { return }
        let settings = schema?.defaults(kind, width: Double(doc.info.width)) ?? .object([:])
        guard let i = m.add(kind, settings: settings, above: above) else {
            doc.report?("\(kind.title): a layer takes at most \(LayerStyleModel.maxPerKind) of each effect")
            return
        }
        apply(doc, layer, m, label: kind.title, final: true)
        pane = .effect(i)
    }

    func removeEffect(_ doc: DocumentController, _ layer: DocLayerID, _ index: Int) {
        guard var m = model(doc, layer), m.effects.indices.contains(index) else { return }
        let kind = m.effects[index].kind
        m.remove(at: index)
        apply(doc, layer, m, label: "Delete \(kind.title)", final: true)
        pane = m.indices(of: kind).first.map { .effect($0) } ?? m.displayOrder.first.map { .effect($0) } ?? .blending
    }

    func setEnabled(_ doc: DocumentController, _ layer: DocLayerID, _ index: Int, _ on: Bool) {
        guard var m = model(doc, layer), m.effects.indices.contains(index) else { return }
        m.effects[index].enabled = on
        apply(doc, layer, m, label: (on ? "Show " : "Hide ") + m.effects[index].kind.title, final: true)
    }

    /// Checking a kind in the inspector's list: adds it when absent, else toggles every instance.
    func toggleKind(_ doc: DocumentController, _ layer: DocLayerID, _ kind: StyleEffectKind) {
        guard var m = model(doc, layer) else { return }
        let ids = m.indices(of: kind)
        if ids.isEmpty { addEffect(doc, layer, kind); return }
        let on = !ids.contains { m.effects[$0].enabled }
        for i in ids { m.effects[i].enabled = on }
        apply(doc, layer, m, label: (on ? "Show " : "Hide ") + kind.title, final: true)
    }

    func copy(_ doc: DocumentController) {
        guard let b = Self.backend(doc), let p = doc.primary else { return }
        do {
            try b.copyLayerStyles(from: p.id)
            doc.report?("Copied the layer style of \(p.name)")
        } catch { doc.report?("Copy Layer Style: \(error.localizedDescription)") }
        revision += 1
    }

    func paste(_ doc: DocumentController) {
        guard let b = Self.backend(doc), !doc.selection.isEmpty else { return }
        let ids = doc.selection
        doc.run("Paste Layer Style") { try b.pasteLayerStyles(to: ids) }
        revision += 1
    }

    func clear(_ doc: DocumentController) {
        guard let b = Self.backend(doc) else { return }
        for id in doc.selection { doc.run("Clear Layer Style") { try b.clearLayerStyles(layer: id) } }
        pane = .blending
        revision += 1
    }

    func canPaste(_ doc: DocumentController?) -> Bool {
        _ = revision
        return Self.backend(doc)?.canPasteLayerStyles() ?? false
    }

    // MARK: Windows

    /// Opens the Layer Style inspector on the primary layer (selecting `layer` first when given),
    /// showing `kind`'s first effect (adding one when the layer has none) or Blending Options.
    func open(_ doc: DocumentController, layer: DocLayerID? = nil, kind: StyleEffectKind? = nil, effect: Int? = nil) {
        if let layer, doc.selection.last != layer { doc.select(layer) }
        document = doc
        Self.lastDocument = doc
        guard let p = doc.primary else { doc.report?("Layer Style: select a layer"); return }
        if let effect {
            pane = .effect(effect)
        } else if let kind {
            if !Self.canStyle(p) {
                doc.report?("Layer Style: \(p.kind == .adjustment ? "adjustment layers" : "pass-through groups") cannot have effects")
            } else if let first = model(doc, p.id)?.indices(of: kind).first {
                pane = .effect(first)
            } else {
                addEffect(doc, p.id, kind)
            }
        } else {
            pane = .blending
        }
        showInspector()
    }

    /// Layer ▸ Layer Style ▸ Global Light…
    func openGlobalLight(_ doc: DocumentController) {
        document = doc
        Self.lastDocument = doc
        if lightWindow == nil {
            lightWindow = panel(title: "Global Light", size: NSSize(width: 360, height: 196), identifier: "document.globalLight") {
                GlobalLightPanel(styles: self)
            }
        }
        lightWindow?.makeKeyAndOrderFront(nil)
    }

    func closeInspector() {
        if let doc = document { commitIfPending(doc) }
        inspectorWindow?.orderOut(nil)
    }

    func closeGlobalLight() { lightWindow?.orderOut(nil) }

    private func commitIfPending(_ doc: DocumentController) {
        guard colorDebounce != nil else { return }
        colorDebounce?.cancel()
        colorDebounce = nil
        commit(doc, "Layer Style")
    }

    private func showInspector() {
        if inspectorWindow == nil {
            inspectorWindow = panel(title: "Layer Style", size: NSSize(width: 640, height: 560), identifier: "document.layerStyle") {
                LayerStyleInspector(styles: self)
            }
        }
        inspectorWindow?.makeKeyAndOrderFront(nil)
    }

    private func panel<V: View>(title: String, size: NSSize, identifier: String, @ViewBuilder _ root: () -> V) -> NSPanel {
        let p = NSPanel(contentRect: NSRect(origin: .zero, size: size),
                        styleMask: [.titled, .closable, .resizable, .utilityWindow, .nonactivatingPanel], backing: .buffered,
                        defer: true)
        p.title = title
        p.isFloatingPanel = true
        p.hidesOnDeactivate = false
        p.isReleasedWhenClosed = false
        p.becomesKeyOnlyIfNeeded = false
        p.contentView = NSHostingView(rootView: root())
        p.contentMinSize = size
        p.setAccessibilityIdentifier(identifier)
        if let main = NSApp.mainWindow {
            let f = main.frame
            p.setFrameOrigin(NSPoint(x: f.midX - size.width / 2, y: f.midY - size.height / 2))
        } else {
            p.center()
        }
        return p
    }
}
