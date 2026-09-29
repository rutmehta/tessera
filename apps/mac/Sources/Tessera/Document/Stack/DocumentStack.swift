import AppKit
import Observation
import SwiftUI
import TesseraCore

/// Edit ▸ Auto-Align Layers…, Edit ▸ Auto-Blend Layers… and File ▸ Automate ▸ Photomerge… (WP B5-19).
/// Menu enablement follows the Layers selection (`StackCommandRules`); the sheets collect options; each
/// run is one blocking engine call off the main thread (one history node) with an indeterminate busy
/// sheet — the engine has no cancellation point for alignment or blending yet, so there is no Cancel.
@MainActor @Observable
final class DocumentStack {
    static let shared = DocumentStack()

    enum Sheet: Identifiable, Equatable {
        case align, blend, photomerge, busy(String)
        var id: String {
            switch self {
            case .align: "align"
            case .blend: "blend"
            case .photomerge: "photomerge"
            case .busy(let s): "busy.\(s)"
            }
        }
    }

    @ObservationIgnored weak var model: AppModel?
    var sheet: Sheet?
    /// Options remembered for the session.
    var align = StackAlignSettings()
    var blend = StackBlendSettings()
    var photomerge = PhotomergeForm()
    /// The running operation's title.
    private(set) var busy: String?

    private init() {}

    func attach(_ model: AppModel) {
        guard self.model !== model else { return }
        self.model = model
        StackSelfTest.startIfRequested(model)
    }

    private var workspace: DocumentWorkspace? { model?.documents }
    var document: DocumentController? { model?.viewMode == .document ? workspace?.current : nil }
    private func say(_ s: String) { model?.statusMessage = s }

    // MARK: Enablement

    private func selection(_ doc: DocumentController) -> [DocLayerID] {
        StackCommandRules.orderedIDs(layers: doc.layers, selected: doc.selection)
    }

    var canAlign: Bool {
        guard busy == nil, let doc = document, doc.backend is any DocumentStackBackend else { return false }
        return StackCommandRules.canAutoAlign(layers: doc.layers, selected: selection(doc))
    }

    var canBlend: Bool {
        guard busy == nil, let doc = document, doc.backend is any DocumentStackBackend else { return false }
        return StackCommandRules.canAutoBlend(layers: doc.layers, selected: selection(doc))
    }

    var canPhotomerge: Bool { busy == nil && model != nil }

    /// Names of the selected layers, bottom first (the reference picker).
    func selectedNames() -> [String] {
        guard let doc = document else { return [] }
        return selection(doc).compactMap { doc.node($0)?.name }
    }

    // MARK: Presenting

    func presentAlign() {
        guard let doc = document else { return }
        if let p = StackCommandRules.alignProblem(layers: doc.layers, selected: selection(doc)) {
            say("Auto-Align Layers: \(p)"); return
        }
        if align.referenceIndex >= UInt32(selection(doc).count) { align.referenceIndex = 0 }
        sheet = .align
    }

    func presentBlend() {
        guard let doc = document else { return }
        if let p = StackCommandRules.blendProblem(layers: doc.layers, selected: selection(doc)) {
            say("Auto-Blend Layers: \(p)"); return
        }
        sheet = .blend
    }

    /// Starts with the library selection when there is one (two or more photos).
    func presentPhotomerge() {
        guard let model else { return }
        var form = photomerge
        let fromLibrary = libraryPhotos()
        if model.viewMode != .document, fromLibrary.count >= 2 { form.sources = fromLibrary }
        form.intoCurrentDocument = form.intoCurrentDocument && workspace?.current != nil
        photomerge = form
        sheet = .photomerge
    }

    /// The library's selected photos: engine images by id, others by file.
    func libraryPhotos() -> [PhotomergeSource] {
        guard let model else { return [] }
        return model.selectedItems.compactMap { item in
            if let ref = item.engineImage { return .image(id: ref.imageID, name: item.name) }
            return item.url.map { .file($0) }
        }
    }

    func addFiles() {
        let panel = NSOpenPanel()
        panel.title = "Add Photos to Photomerge"
        panel.allowedContentTypes = DocumentWorkspace.documentTypes
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK else { return }
        photomerge.sources += panel.urls.map { .file($0) }
        photomerge.removeDuplicates()
    }

    // MARK: Running

    /// Runs `body` off the main thread under the busy sheet, then refreshes the document.
    private func perform(_ doc: DocumentController, _ what: String,
                         _ body: @escaping @Sendable () throws -> DocumentChange) {
        busy = what
        sheet = .busy(what)
        say("\(what)…")
        Task { @MainActor in
            let result = await Task.detached(priority: .userInitiated) { Result { try body() } }.value
            self.busy = nil
            if case .busy = self.sheet { self.sheet = nil }
            if doc.run(what, { try result.get() }) != nil {
                DocumentTools.shared.refreshOutline(doc)
                self.say("\(what) finished")
            }
        }
    }

    func runAlign() {
        guard let doc = document, let b = doc.backend as? any DocumentStackBackend else { return }
        let ids = selection(doc), options = align
        perform(doc, "Auto-Align Layers") { try b.autoAlignLayers(ids: ids, options: options) }
    }

    func runBlend() {
        guard let doc = document, let b = doc.backend as? any DocumentStackBackend else { return }
        let ids = selection(doc), options = blend
        perform(doc, "Auto-Blend Layers") { try b.autoBlendLayers(ids: ids, options: options) }
    }

    func runPhotomerge() {
        guard let workspace, let r = photomerge.request else { return }
        if photomerge.intoCurrentDocument, let doc = workspace.current {
            guard let b = doc.backend as? any DocumentStackBackend else {
                say("Photomerge: this document cannot merge photos"); return
            }
            perform(doc, "Photomerge") { try b.photomergeIntoLayers(sources: r.sources, align: r.align, blend: r.blend) }
            return
        }
        // Library images merge on their library's engine; files on the documents' engine.
        let engineImage = model?.selectedItems.compactMap(\.engineImage).first
        let engine: any DocumentEngine = photomerge.sources.contains(where: { if case .image = $0 { true } else { false } })
            ? (engineImage.map { EngineDocumentEngine.for($0.engine) } ?? workspace.engine) : workspace.engine
        guard let stackEngine = engine as? any DocumentStackEngine else {
            say("Photomerge: the document engine cannot merge photos"); return
        }
        busy = "Photomerge"
        sheet = .busy("Photomerge")
        say("Photomerge…")
        Task { @MainActor in
            let result = await Task.detached(priority: .userInitiated) {
                Result { try stackEngine.photomergeDocument(sources: r.sources, align: r.align, blend: r.blend) }
            }.value
            self.busy = nil
            if case .busy = self.sheet { self.sheet = nil }
            switch result {
            case .success(let backend):
                do {
                    try workspace.install(backend)
                    self.say("Photomerge: \(r.sources.count) photos merged into layers")
                } catch {
                    backend.close()
                    self.say("Photomerge: \(error.localizedDescription)")
                }
            case .failure(let error):
                self.say("Photomerge: \(error.localizedDescription)")
            }
        }
    }
}
