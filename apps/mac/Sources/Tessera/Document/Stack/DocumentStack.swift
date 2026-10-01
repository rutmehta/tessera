import AppKit
import Observation
import SwiftUI
import TesseraCore
import TesseraFFI
import UniformTypeIdentifiers

/// Edit ▸ Auto-Align Layers…, Edit ▸ Auto-Blend Layers… and File ▸ Automate ▸ Photomerge… (WP B5-19).
/// Menu enablement follows the Layers selection (`StackCommandRules`); the sheets collect options; each
/// run is one blocking engine call off the main thread (one history node) with an indeterminate busy
/// sheet and Cancel. Photomerge honours it while the photos are read, before anything changes; the engine
/// has no cancellation point inside alignment or blending yet, so a cancel that arrives there takes effect
/// when the engine returns: the committed step is undone (or the new document discarded), like
/// DocumentRetouch does for a discarded job that succeeded, and the status never says "finished".
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
    /// The running operation's cancel flag. Photomerge's engine call reads it; for Auto-Align / Auto-Blend
    /// only this controller does (their result is undone when the engine returns).
    @ObservationIgnored private(set) var cancelFlag: CancelFlag?
    /// Whether the busy sheet offers Cancel.
    var canCancel: Bool { cancelFlag != nil }

    func cancelBusy() {
        guard let cancelFlag, let busy else { return }
        cancelFlag.cancel()
        say("Cancelling \(busy)…")
    }

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

    /// The engine's own check (it also requires four-channel pixel layers, which the rules cannot see).
    private func engineProblem(_ doc: DocumentController, align: Bool) -> String? {
        guard let b = doc.backend as? any DocumentStackBackend,
              let e = try? b.stackEligibility(ids: selection(doc)) else { return nil }
        return (align ? e.canAlign : e.canBlend) ? nil : e.reason
    }

    func presentAlign() {
        guard let doc = document else { return }
        if let p = StackCommandRules.alignProblem(layers: doc.layers, selected: selection(doc))
            ?? engineProblem(doc, align: true) {
            say("Auto-Align Layers: \(p)"); return
        }
        if align.referenceIndex >= UInt32(selection(doc).count) { align.referenceIndex = 0 }
        sheet = .align
    }

    func presentBlend() {
        guard let doc = document else { return }
        if let p = StackCommandRules.blendProblem(layers: doc.layers, selected: selection(doc))
            ?? engineProblem(doc, align: false) {
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
        // Flat images only: the engine reads their size from the header before decoding anything.
        panel.allowedContentTypes = [.jpeg, .png, .tiff]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK else { return }
        photomerge.sources += panel.urls.map { .file($0) }
        photomerge.removeDuplicates()
    }

    // MARK: Running

    /// Runs `body` off the main thread under the busy sheet, then refreshes the document.
    /// A cancel that arrives after the engine committed undoes that history node.
    private func perform(_ doc: DocumentController, _ what: String, cancel: CancelFlag = CancelFlag(),
                         _ body: @escaping @Sendable () throws -> DocumentChange) {
        busy = what
        cancelFlag = cancel
        sheet = .busy(what)
        say("\(what)…")
        Task { @MainActor in
            let result = await Task.detached(priority: .userInitiated) { Result { try body() } }.value
            self.busy = nil
            self.cancelFlag = nil
            if case .busy = self.sheet { self.sheet = nil }
            let succeeded = if case .success = result { true } else { false }
            switch StackCommandRules.end(cancelRequested: cancel.isCancelled(), succeeded: succeeded) {
            case .cancelled:
                self.say(StackCommandRules.cancelledMessage(what, afterFinishing: false, newDocument: false))
            case .cancelledAfterFinishing:
                // The engine committed before it saw the cancel: undo that step (DocumentRetouch's pattern).
                guard doc.run("Undo cancelled \(what)", { try doc.backend.undo() }) != nil else { return }
                DocumentTools.shared.refreshOutline(doc)
                self.say(StackCommandRules.cancelledMessage(what, afterFinishing: true, newDocument: false))
            case .finished, .failed:
                if doc.run(what, { try result.get() }) != nil {
                    DocumentTools.shared.refreshOutline(doc)
                    self.say("\(what) finished")
                }
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
            let cancel = CancelFlag()
            perform(doc, "Photomerge", cancel: cancel) {
                try b.photomergeIntoLayers(sources: r.sources, align: r.align, blend: r.blend, cancel: cancel)
            }
            return
        }
        // Library images merge on their library's engine; files on the documents' engine.
        let engineImage = model?.selectedItems.compactMap(\.engineImage).first
        let engine: any DocumentEngine = photomerge.sources.contains(where: { if case .image = $0 { true } else { false } })
            ? (engineImage.map { EngineDocumentEngine.for($0.engine) } ?? workspace.engine) : workspace.engine
        guard let stackEngine = engine as? any DocumentStackEngine else {
            say("Photomerge: the document engine cannot merge photos"); return
        }
        let cancel = CancelFlag()
        busy = "Photomerge"
        cancelFlag = cancel
        sheet = .busy("Photomerge")
        say("Photomerge…")
        Task { @MainActor in
            let result = await Task.detached(priority: .userInitiated) {
                Result {
                    try stackEngine.photomergeDocument(sources: r.sources, align: r.align, blend: r.blend,
                                                       cancel: cancel)
                }
            }.value
            self.busy = nil
            self.cancelFlag = nil
            if case .busy = self.sheet { self.sheet = nil }
            let succeeded = if case .success = result { true } else { false }
            switch StackCommandRules.end(cancelRequested: cancel.isCancelled(), succeeded: succeeded) {
            case .cancelled:
                self.say(StackCommandRules.cancelledMessage("Photomerge", afterFinishing: false, newDocument: false))
                return
            case .cancelledAfterFinishing:
                if case .success(let backend) = result { backend.close() }
                self.say(StackCommandRules.cancelledMessage("Photomerge", afterFinishing: true, newDocument: true))
                return
            case .finished, .failed: break
            }
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
