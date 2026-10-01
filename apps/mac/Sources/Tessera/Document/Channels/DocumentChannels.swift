import AppKit
import Observation
import TesseraCore

/// Persistent alpha and spot channels (WP B5-08): the Channels panel's model, its actions (each one
/// history node through `DocumentController.run`), Quick Mask (Q), the Save / Load Selection sheets and
/// the preview overlay. One instance serves the workspace's current document; per-document preview state
/// (component eyes, the Quick Mask channel) is kept by document id. Alpha overlay colour / opacity /
/// indicator live in the document's channel records (B5-17b) and are edited through history.
@MainActor @Observable
final class DocumentChannels {
    static let shared = DocumentChannels()

    @ObservationIgnored weak var workspace: DocumentWorkspace?
    var document: DocumentController? { workspace?.current }

    /// Saved channels of the current document (reloaded on every document change).
    private(set) var records: [SavedChannel] = []
    @ObservationIgnored private var recordsKey: (String, UInt64, DocHistoryID)?
    /// Component eyes per document.
    private(set) var components: [String: ComponentVisibility] = [:]
    /// The temporary Quick Mask channel per document.
    private(set) var quickMask: [String: UInt64] = [:]
    /// Highlighted channel row (the panel's selection).
    var selectedChannel: UInt64?
    /// Row whose name is being edited.
    var renaming: UInt64?
    var sheet: ChannelSheet?

    enum ChannelSheet: Identifiable, Equatable {
        case save, load, options(UInt64), newSpot
        var id: String {
            switch self {
            case .save: "save"
            case .load: "load"
            case .options(let c): "options.\(c)"
            case .newSpot: "newSpot"
            }
        }
    }

    @ObservationIgnored let overlay = ChannelOverlayController()
    @ObservationIgnored private var thumbs: [String: NSImage] = [:]

    private init() {}

    func attach(_ workspace: DocumentWorkspace) {
        guard self.workspace !== workspace else { return }
        self.workspace = workspace
        ChannelsSelfTest.startIfRequested(workspace)
    }

    func backend(_ doc: DocumentController) -> (any DocumentChannelsBackend)? {
        doc.backend as? any DocumentChannelsBackend
    }

    private func say(_ s: String) { document?.report?(s) }

    // MARK: Model

    /// Re-reads the channels after any change of the document (edits, undo, redo, tab switch).
    func reload(_ doc: DocumentController?, force: Bool = false) {
        guard let doc, let b = backend(doc) else {
            records = []
            recordsKey = nil
            overlay.update(nil, channels: self)
            return
        }
        let key = (doc.id, doc.info.epoch, doc.info.historyHead)
        if !force, let k = recordsKey, k == key { return }
        recordsKey = key
        do {
            let r = try b.documentChannels()
            if r != records { records = r }
        } catch {
            records = []
            say("Channels: \(error.localizedDescription)")
        }
        if let q = quickMask[doc.id], !records.contains(where: { $0.id == q }) { quickMask[doc.id] = nil }
        if let s = selectedChannel, !records.contains(where: { $0.id == s }) { selectedChannel = nil }
        overlay.update(doc, channels: self)
    }

    func rows(_ doc: DocumentController) -> [ChannelRow] {
        ChannelsPanelModel.rows(records: records, components: components[doc.id] ?? ComponentVisibility(),
                                quickMask: quickMask[doc.id])
    }

    func componentVisibility(_ doc: DocumentController) -> ComponentVisibility { components[doc.id] ?? ComponentVisibility() }

    /// The overlay style saved in channel `id`'s record.
    func style(_ id: UInt64) -> ChannelOverlayStyle {
        records.first { $0.id == id }?.overlayStyle ?? .alphaDefault
    }

    func isQuickMask(_ doc: DocumentController?) -> Bool { doc.flatMap { quickMask[$0.id] } != nil }

    /// A grey thumbnail of channel `id` (cached per revision).
    func thumbnail(_ doc: DocumentController, _ row: ChannelRow, px: UInt32 = 48) -> NSImage? {
        guard let id = row.channelID, let b = backend(doc) else { return nil }
        let key = "\(doc.id):\(id):\(row.revision):\(px)"
        if let t = thumbs[key] { return t }
        guard let sid = try? b.channelThumbnail(id: id, maxPx: px), let s = IOSurfaceLookup(sid),
              let cg = ChannelImages.gray(s)?.image else { return nil }
        if thumbs.count > 256 { thumbs.removeAll() }
        let image = NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
        thumbs[key] = image
        return image
    }

    @ObservationIgnored private var componentThumbs: (key: String, images: [NSImage?])?

    /// The composite (RGB row) and its Red / Green / Blue components as grey, from the composite
    /// thumbnail (cached per document epoch).
    func componentThumbnail(_ doc: DocumentController, _ row: ChannelRow, px: UInt32 = 48) -> NSImage? {
        let key = "\(doc.id):\(doc.info.epoch)"
        if componentThumbs?.key != key {
            var images: [NSImage?] = [nil, nil, nil, nil]
            if let sid = try? doc.backend.compositeThumbnail(maxPx: px), let s = IOSurfaceLookup(sid) {
                let variants = [ComponentVisibility(), ComponentVisibility(red: true, green: false, blue: false),
                                ComponentVisibility(red: false, green: true, blue: false),
                                ComponentVisibility(red: false, green: false, blue: true)]
                images = variants.map { v in
                    ChannelImages.components(s, v, space: doc.displayColor.space).map { NSImage(cgImage: $0, size: NSSize(width: $0.width, height: $0.height)) }
                }
            }
            componentThumbs = (key, images)
        }
        switch row.kind {
        case .composite: return componentThumbs?.images[0]
        case .component(let i): return componentThumbs?.images[i + 1]
        case .alpha, .spot: return nil
        }
    }

    // MARK: Edits

    /// One backend edit through the document (errors reach the status bar), then the panel and outline.
    @discardableResult
    private func edit(_ doc: DocumentController, _ what: String,
                      _ body: (any DocumentChannelsBackend) throws -> DocumentChange) -> DocumentChange? {
        guard let b = backend(doc) else {
            say("\(what): channels need a document backend that supports them")
            return nil
        }
        let c = doc.run(what) { try body(b) }
        reload(doc, force: true)
        DocumentTools.shared.refreshOutline(doc)
        return c
    }

    /// Select ▸ Save Selection… (sheet) and the footer's save button.
    func saveSelection(_ form: SaveSelectionForm) {
        guard let doc = document, let r = form.request else { return }
        var made: UInt64?
        edit(doc, "Save Selection") { b in
            let c = try b.saveSelectionChannel(name: r.name, target: r.target, op: r.op)
            made = c.channelID
            return c.change
        }
        if let made { selectedChannel = made; say("Selection saved as a channel") }
    }

    /// Footer button: a new channel from the selection named "Alpha N".
    func quickSaveSelection() {
        saveSelection(SaveSelectionForm(name: ChannelsPanelModel.nextName("Alpha", existing: records)))
    }

    func loadSelection(_ form: LoadSelectionForm) {
        guard let doc = document, let r = form.request else { return }
        edit(doc, "Load Selection") { try $0.loadSelectionChannel(id: r.id, op: r.op, invert: r.invert) }
    }

    /// ⌘-click on a row, footer button: the channel replaces the selection (⇧ adds, ⌥ subtracts, ⇧⌥ intersects).
    func load(_ id: UInt64, op: SelectionCombine = .replace) {
        guard let doc = document else { return }
        edit(doc, "Load Selection") { try $0.loadSelectionChannel(id: id, op: op, invert: false) }
    }

    func newChannel() {
        guard let doc = document else { return }
        let name = ChannelsPanelModel.nextName("Alpha", existing: records)
        var made: UInt64?
        edit(doc, "New Channel") { b in
            let c = try b.newAlphaChannel(name: name, selected: false)
            made = c.channelID
            return c.change
        }
        if let made { selectedChannel = made }
    }

    func newSpot(name: String, color: ToolColor, solidity: Float, fromSelection: Bool) {
        guard let doc = document else { return }
        var made: UInt64?
        edit(doc, "New Spot Channel") { b in
            let c = try b.newSpotChannel(name: name, color: color, solidity: solidity, fromSelection: fromSelection)
            made = c.channelID
            return c.change
        }
        if let made { selectedChannel = made }
    }

    func delete(_ id: UInt64) {
        guard let doc = document else { return }
        edit(doc, "Delete Channel") { try $0.deleteDocumentChannel(id: id) }
    }

    func duplicate(_ id: UInt64) {
        guard let doc = document else { return }
        var made: UInt64?
        edit(doc, "Duplicate Channel") { b in
            let c = try b.duplicateDocumentChannel(id: id)
            made = c.channelID
            return c.change
        }
        if let made { selectedChannel = made }
    }

    func rename(_ id: UInt64, to name: String) {
        renaming = nil
        guard let doc = document, let old = records.first(where: { $0.id == id }), old.name != name,
              !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        edit(doc, "Rename Channel") { try $0.renameDocumentChannel(id: id, name: name) }
    }

    /// Channel Options: the name (Rename Channel), then at most one display edit (Channel Options): spot
    /// colour and solidity, or the alpha overlay colour / opacity / indicator. Both are history nodes saved
    /// with the document (B5-17b); the overlay re-reads them from the records.
    func applyOptions(_ id: UInt64, _ form: ChannelOptionsForm) {
        guard let doc = document, let old = records.first(where: { $0.id == id }) else { return }
        rename(id, to: form.name)
        if let e = form.displayEdit(from: old) {
            edit(doc, "Channel Options") { try e.apply($0, id: id) }
        }
    }

    // MARK: Preview (session state)

    func toggleVisible(_ row: ChannelRow) {
        guard let doc = document else { return }
        switch row.kind {
        case .composite:
            var v = componentVisibility(doc)
            v.setComposite(!v.all)
            components[doc.id] = v
        case .component(let i):
            var v = componentVisibility(doc)
            v[i].toggle()
            components[doc.id] = v
        case .alpha, .spot:
            guard let id = row.channelID, let b = backend(doc) else { return }
            do { try b.setChannelVisible(id: id, visible: !row.visible) } catch {
                say("Channels: \(error.localizedDescription)")
            }
            reload(doc, force: true)
            return
        }
        overlay.update(doc, channels: self)
    }

    // MARK: Quick Mask

    /// Q: enter (the selection becomes a temporary channel shown as an overlay and is dropped, B5-17d) or
    /// exit (the mask becomes the selection again). Each is one "Quick Mask" history node.
    func toggleQuickMask() {
        guard let doc = document, let b = backend(doc) else { return }
        if let id = quickMask[doc.id] {
            quickMask[doc.id] = nil
            doc.run("Quick Mask") { try QuickMask.exit(b, channel: id) ?? doc.backend.info().change }
            say("Quick Mask off: the mask is the selection")
        } else {
            var made: UInt64?
            doc.run("Quick Mask") {
                let c = try QuickMask.enter(b)
                made = c.channelID
                return c.change
            }
            if let made {
                quickMask[doc.id] = made
                say("Quick Mask on: Q again turns the mask back into the selection")
            }
        }
        reload(doc, force: true)
        DocumentTools.shared.refreshOutline(doc)
    }

    /// A closed document forgets its preview state.
    func forget(_ id: String) {
        components[id] = nil
        quickMask[id] = nil
    }
}

private extension DocumentSummary {
    /// A no-op change at the current head (Quick Mask exit when its channel is gone).
    var change: DocumentChange {
        DocumentChange(layersChanged: [], created: [], historyHead: historyHead, dirtyRect: nil, epoch: epoch, dirty: dirty)
    }
}
