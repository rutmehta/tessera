import AppKit
import Observation
import QuartzCore
import SwiftUI
import TesseraCore
import TesseraFFI
import UniformTypeIdentifiers

/// Typed handoff callers may own status through their own owner/intent guards.
enum DocumentLoadStatusPublication: Sendable {
    case workspace
    case caller
}

/// Terminal settlement of backend source capture and optional workspace install.
enum DocumentLoadOutcome: Equatable, Sendable {
    case installed
    case rejected(String)
    case failed(String)
    case workspaceReleased
}

@MainActor
private final class DocumentLoadSettlement {
    let engine: any DocumentEngine
    private var completed = false
    private var completion: (@MainActor (DocumentLoadOutcome) -> Void)?
    init(engine: any DocumentEngine, completion: @escaping @MainActor (DocumentLoadOutcome) -> Void) {
        self.engine = engine; self.completion = completion
    }
    func claim() -> Bool {
        guard !completed else { return false }
        completed = true
        return true
    }
    func finish(_ outcome: DocumentLoadOutcome) {
        let callback = completion
        completion = nil
        callback?(outcome)
    }
}

/// A save result is distinct from permission to continue a cancelled close intent.
enum DocumentSaveOutcome: Equatable {
    case saved(URL?, continuationCancelled: Bool)
    case cancelled
    case destinationConflict(URL)
    case failed(String)
}

@MainActor
private final class DocumentSaveOperation {
    enum Phase { case choosing, waitingForDismissal, replacing, writing }
    let id: UUID
    let document: DocumentController
    let requiresWindow: Bool
    var phase: Phase = .choosing
    var continuationCancelled = false
    var completion: ((DocumentSaveOutcome) -> Void)?
    var host: DocumentSaveHostIdentity?
    var pendingRequest: SaveAsRequest?
    var needsReplacement = false
    var destinationIntent: DocSaveDestinationIntent?
    init(id: UUID, document: DocumentController, requiresWindow: Bool,
         completion: @escaping (DocumentSaveOutcome) -> Void) {
        self.id = id; self.document = document; self.requiresWindow = requiresWindow
        self.completion = completion
    }
}

/// Document mode's open documents (WP B5-02): the tab switcher, New / Open / Edit in Layers,
/// Save / Save As / Export Flat, close with a save prompt, panels (Tab) and screen modes (F).
@MainActor @Observable
final class DocumentWorkspace {
    /// Where documents come from when no engine-backed library is open (WP B5-03).
    enum BackendPolicy: Equatable {
        /// The stub backend (`--stub-library`, unit tests).
        case stub
        /// A standalone engine opened on first use in the app-support directory.
        case engine
    }

    /// The app sets `.engine` at launch unless `--stub-library` is given; unit tests keep `.stub`.
    @ObservationIgnored var policy: BackendPolicy = .stub
    /// An explicit backend factory (tests); nil = `resolvedEngine`.
    @ObservationIgnored private var engineOverride: (any DocumentEngine)?
    @ObservationIgnored private var standaloneEngine: EngineDocumentEngine?
    @ObservationIgnored weak var app: AppModel?

    /// The backend factory: an explicit one when set, else the engine of an engine-backed library,
    /// else the policy's (a standalone engine, or the stub).
    var engine: any DocumentEngine {
        get { engineOverride ?? resolvedEngine }
        set { engineOverride = newValue }
    }

    /// Engine vs stub, per library and policy (the `engine` getter without an override).
    var resolvedEngine: any DocumentEngine { resolveEngine(statusPublication: .workspace) }

    private func resolveEngine(statusPublication: DocumentLoadStatusPublication) -> any DocumentEngine {
        Self.selectEngine(library: app?.library, policy: policy) { [weak self] in
            if let e = self?.standaloneEngine { return e.engine }
            do {
                let e = try Engine.open(appSupportDir: EngineLibrary.defaultSupportDirectory.path)
                self?.standaloneEngine = EngineDocumentEngine.for(e)
                return e
            } catch {
                self?.publishDocumentLoadStatus("Documents: the engine did not open (\(error.localizedDescription)); using the stub backend", policy: statusPublication)
                return nil
            }
        }
    }

    /// The engine adapter of an engine-backed library; otherwise, under `.engine`, a standalone engine
    /// (`standalone()`, nil when it cannot open), and the stub under `.stub` or as the fallback.
    static func selectEngine(library: (any PhotoLibrary)?, policy: BackendPolicy,
                             standalone: () -> Engine?) -> any DocumentEngine {
        if let lib = library as? EngineLibrary { return EngineDocumentEngine.for(lib.engine) }
        if policy == .engine, let e = standalone() { return EngineDocumentEngine.for(e) }
        return StubDocumentEngine.shared
    }

    var usesStub: Bool { engine is StubDocumentEngine }
    /// A document is being opened off the main thread (engine opens decode or render).
    private(set) var opening: String?

    private(set) var documents: [DocumentController] = []
    private(set) var current: DocumentController? {
        didSet {
            if oldValue !== current { DocumentTools.shared.activeDocumentChanged(in: self) }
        }
    }
    var showNewDocument = false
    var showExportFlat = false
    /// Tab: sidebar and inspector hidden.
    private(set) var panelsHidden = false
    var columnVisibility: NavigationSplitViewVisibility = .all
    /// 0 standard, 1 full screen, 2 full screen without panels (F cycles).
    private(set) var screenMode = 0
    /// Space held: the viewport pans on drag.
    var spaceHeld = false
    /// New Document sheet values, remembered for the session.
    var newSettings = NewDocumentSettings()
    var exportSettings = ExportFlatSettings()
    /// Filter menu, Image ▸ Adjustments and smart filters (WP B5-05).
    let filters = DocumentFilters()
    /// B5-16: the inspector's sub-tab (Stack · Properties · Channels; ⌃1 / ⌃2 / ⌃3), remembered.
    var inspectorTab: DocumentInspectorTab = DocumentWorkspace.storedInspectorTab {
        didSet { UserDefaults.standard.set(inspectorTab.rawValue, forKey: Self.inspectorTabKey) }
    }
    static let inspectorTabKey = "DocumentInspector.tab"
    private static var storedInspectorTab: DocumentInspectorTab {
        UserDefaults.standard.string(forKey: inspectorTabKey).flatMap(DocumentInspectorTab.init(rawValue:)) ?? .stack
    }

    static let documentTypes: [UTType] = [
        UTType(exportedAs: "dev.tessera.document", conformingTo: .data),
        UTType(filenameExtension: "psd") ?? .image, UTType(filenameExtension: "psb") ?? .data,
        .jpeg, .png, .tiff,
    ]
    static let documentExtensions: Set<String> = ["tessera-doc", "psd", "psb", "jpg", "jpeg", "png", "tif", "tiff"]

    private func say(_ message: String) { app?.statusMessage = message }

    /// The main window; nil without a running application (unit tests).
    private var window: NSWindow? { (NSApp as NSApplication?) == nil ? nil : app?.mainWindow }

    // MARK: Opening

    func install(_ backend: any DocumentBackend, activateDocument: Bool = true) throws {
        if let existing = documents.first(where: { $0.backend === backend }) {
            select(existing, activateDocument: activateDocument)
            return
        }
        let doc = try DocumentController(backend: backend)
        doc.report = { [weak self] in self?.say($0) }
        documents.append(doc)
        // Construct the native controls during document setup, not the first export.
        // The unattached HUD is reused, including after an export finishes.
        if preparedExportHUD == nil, (NSApp as NSApplication?) != nil {
            preparedExportHUD = FlatExportProgressView(workspace: self)
        }
        select(doc, activateDocument: activateDocument)
    }

    func select(_ doc: DocumentController, activateDocument: Bool = true) {
        // B5-10 begin: switching documents applies the text being edited (Esc first to discard it).
        if current !== doc, let c = current, DocumentText.shared.isEditing(c) { DocumentText.shared.documentWillChange() }
        // B5-10 end
        current = doc
        if activateDocument { app?.viewMode = .document }
    }

    func newDocument(_ s: NewDocumentSettings) {
        let engine = self.engine
        do {
            try install(engine.newDocument(width: UInt32(s.width), height: UInt32(s.height), depth: s.depth, profile: s.profile))
            say("New document \(s.width) × \(s.height) px, \(s.depth.title), \(s.profile)"
                + (engine is StubDocumentEngine ? " (stub backend: sample layers)" : ""))
        } catch {
            say("New document: \(error.localizedDescription)")
        }
    }

    private func publishDocumentLoadStatus(_ message: String, policy: DocumentLoadStatusPublication) {
        if case .workspace = policy { say(message) }
    }

    // Tests hold backend completion without native decode, disk or GPU work.
    @ObservationIgnored var documentLoadExecutor: ((any DocumentEngine,
        @escaping @Sendable (any DocumentEngine) throws -> any DocumentBackend,
        @escaping @MainActor (Result<any DocumentBackend, Error>) -> Void) -> Void)?
    @ObservationIgnored private var openingToken: UUID?

    /// Completion retains the captured engine until actual backend return and
    /// installation/orphan cleanup, even if the workspace no longer exists.
    private func load(_ what: String, engine: any DocumentEngine, done: String,
                      statusPublication: DocumentLoadStatusPublication = .workspace,
                      activateDocument: Bool = true,
                      completion: @escaping @MainActor (DocumentLoadOutcome) -> Void = { _ in },
                      _ body: @escaping @Sendable (any DocumentEngine) throws -> any DocumentBackend) {
        let token = UUID()
        openingToken = token
        opening = what
        publishDocumentLoadStatus("\(what)…", policy: statusPublication)
        let settlement = DocumentLoadSettlement(engine: engine, completion: completion)
        let receive: @MainActor (Result<any DocumentBackend, Error>) -> Void = { [weak self, settlement] result in
            guard settlement.claim() else { return }
            guard let self else {
                if case .success(let backend) = result { backend.close() }
                // Failure is still reported faithfully when no workspace remains.
                if case .failure(let error) = result { settlement.finish(.failed(error.localizedDescription)) }
                else { settlement.finish(.workspaceReleased) }
                return
            }
            if self.openingToken == token { self.openingToken = nil; self.opening = nil }
            switch result {
            case .success(let backend):
                do {
                    try self.install(backend, activateDocument: activateDocument)
                    self.publishDocumentLoadStatus(done, policy: statusPublication)
                    settlement.finish(.installed)
                } catch {
                    backend.close()
                    self.publishDocumentLoadStatus("\(what): \(error.localizedDescription)", policy: statusPublication)
                    settlement.finish(.failed(error.localizedDescription))
                }
            case .failure(let error):
                self.publishDocumentLoadStatus("\(what): \(error.localizedDescription)", policy: statusPublication)
                settlement.finish(.failed(error.localizedDescription))
            }
        }
        if let documentLoadExecutor { documentLoadExecutor(engine, body, receive) }
        else if engine is StubDocumentEngine { receive(Result { try body(engine) }) }
        else {
            Task { @MainActor in
                let result = await Task.detached(priority: .userInitiated) { Result { try body(engine) } }.value
                receive(result)
            }
        }
    }

    func presentOpen() {
        let panel = NSOpenPanel()
        panel.title = "Open Document"
        panel.allowedContentTypes = Self.documentTypes
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        let handle: @MainActor (NSApplication.ModalResponse) -> Void = { [weak self] r in
            guard r == .OK else { return }
            for url in panel.urls { self?.open(url) }
        }
        if let window = self.window {
            panel.beginSheetModal(for: window) { r in MainActor.assumeIsolated { handle(r) } }
        } else {
            handle(panel.runModal())
        }
    }

    func open(_ url: URL) {
        let path = url.path
        load("Open \(url.lastPathComponent)", engine: engine, done: "Opened \(url.lastPathComponent)") {
            try $0.openDocument(path: path)
        }
    }

    /// Library ▸ Edit in Layers (⌘E): reuse the focused image's open document, or open it developed. Engine images
    /// open on their own engine by image id (`open_document_from_image(id, developed: true)`); the stub
    /// takes the file.
    /// Release saved-pixel reservations from completion, never on method return.
    /// Rejections/stub loads may settle synchronously. There is no early cancellation
    /// signal: completion follows backend return, including when the UI disappears.
    /// Set activateDocument to false when the caller owns navigation admission.
    /// Installation/current-document selection and backend settlement still complete.
    func editInLayers(_ item: PhotoItem?,
                      statusPublication: DocumentLoadStatusPublication = .workspace,
                      activateDocument: Bool = true,
                      completion: @escaping @MainActor (DocumentLoadOutcome) -> Void = { _ in }) {
        guard let item else {
            let message = "Edit in Layers: select a photo first"
            publishDocumentLoadStatus(message, policy: statusPublication); completion(.rejected(message)); return
        }
        let what = "Edit \(item.name) in Layers", done = "Editing \(item.name) in layers"
        // Image opens create fresh backends, so install's backend-identity check cannot reuse them.
        // Use the recorded source identity, not the library's transient row index or document title.
        let sourceImageID = (!(engineOverride is StubDocumentEngine) ? item.engineImage?.imageID : nil)
            ?? item.url.map { "file:\($0.path)" }
        if let existing = documents.first(where: { doc in
            if let sourceImageID, doc.info.sourceImageId == sourceImageID { return true }
            guard let url = item.url, let path = doc.info.path else { return false }
            return URL(fileURLWithPath: path).standardizedFileURL == url.standardizedFileURL
        }) {
            select(existing, activateDocument: activateDocument)
            publishDocumentLoadStatus(done, policy: statusPublication)
            completion(.installed)
            return
        }
        if !(engineOverride is StubDocumentEngine), let ref = item.engineImage {
            let id = ref.imageID
            load(what, engine: EngineDocumentEngine.for(ref.engine), done: done, statusPublication: statusPublication, activateDocument: activateDocument, completion: completion) {
                try $0.openDocumentFromImage(imageId: id, developed: true)
            }
            return
        }
        let engine = engineOverride ?? resolveEngine(statusPublication: statusPublication)
        guard let url = item.url else {
            let message = "Edit in Layers needs a photo file (stub items have none)"
            publishDocumentLoadStatus(message, policy: statusPublication); completion(.rejected(message)); return
        }
        if engine is StubDocumentEngine {
            let id = "file:\(url.path)"
            load(what, engine: engine, done: done, statusPublication: statusPublication, activateDocument: activateDocument, completion: completion) { try $0.openDocumentFromImage(imageId: id, developed: true) }
        } else if item.kind == .raw {
            let message = "Edit in Layers: open the photo's folder to edit a RAW on the engine"
            publishDocumentLoadStatus(message, policy: statusPublication); completion(.rejected(message))
        } else {
            let path = url.path
            load(what, engine: engine, done: done, statusPublication: statusPublication, activateDocument: activateDocument, completion: completion) { try $0.openDocument(path: path) }
        }
    }

    // MARK: Saving

    @ObservationIgnored private var saveOperations: [UUID: DocumentSaveOperation] = [:]
    @ObservationIgnored private var activeSavePrompt: UUID?
    @ObservationIgnored private var latestSaveRequest: UUID?
    @ObservationIgnored let savePresenter = DocumentSavePresenter()
    @ObservationIgnored private var presentedSaveAs: SaveAsRequest?
    @ObservationIgnored private var savePresentation: DocumentSavePresentationToken?
    @ObservationIgnored private var queuedSaveAs: SaveAsRequest?
    var saveAsRequest: SaveAsRequest? { presentedSaveAs }
    @ObservationIgnored var lastSaveFolder: URL?
    // Injected only by deterministic source tests; native session is injected at
    // the presenter's driver boundary, never by synthesizing dismissal callbacks.
    @ObservationIgnored var saveHasWindow: (() -> Bool)?
    @ObservationIgnored var saveFileExists: ((URL) -> Bool)?
    @ObservationIgnored var checkedSaveWriter: ((DocumentController, URL, DocSaveDestinationIntent, @escaping @MainActor (Result<DocSaveAsResult, Error>) -> Void) -> Void)?
    @ObservationIgnored var saveWriter: ((DocumentController, URL?, @escaping @MainActor (Result<Void, Error>) -> Void) -> Void)?

    /// Preparation-facing seam only: does not close, finalize drafts or enable Quit.
    @discardableResult
    func saveForPreparation(_ doc: DocumentController, saveAs: Bool = false,
                            completion: @escaping (DocumentSaveOutcome) -> Void) -> UUID {
        beginDocumentSave(doc, saveAs: saveAs, requiresWindow: true, completion: completion)
    }

    /// Explicit compatibility wrapper: only this legacy entry permits headless Save As.
    func save(_ doc: DocumentController? = nil, then: (@MainActor () -> Void)? = nil) {
        guard let doc = doc ?? current else { return }
        beginDocumentSave(doc, saveAs: false, requiresWindow: false) { outcome in
            if case .saved(_, continuationCancelled: false) = outcome { then?() }
        }
    }

    func saveAs(_ doc: DocumentController? = nil, then: (@MainActor () -> Void)? = nil) {
        guard let doc = doc ?? current else { return }
        beginDocumentSave(doc, saveAs: true, requiresWindow: false) { outcome in
            if case .saved(_, continuationCancelled: false) = outcome { then?() }
        }
    }

    @discardableResult
    private func beginDocumentSave(_ doc: DocumentController, saveAs: Bool, requiresWindow: Bool,
                                   completion: @escaping (DocumentSaveOutcome) -> Void) -> UUID {
        let id = UUID()
        savePresenter.onHostLost = { [weak self] host in
            guard let self else { return }
            let affected = self.saveOperations.values.filter { $0.host == host }.map(\.id)
            for id in affected { self.documentSaveWindowLost(id) }
        }
        guard !doc.isClosed else { completion(.failed("Document is closed")); return id }
        guard !saveOperations.values.contains(where: { $0.document === doc && $0.phase == .writing }) else {
            completion(.failed("A save for this document is still running")); return id
        }
        let old = activeSavePrompt
        let operation = DocumentSaveOperation(id: id, document: doc, requiresWindow: requiresWindow, completion: completion)
        operation.host = savePresenter.host
        saveOperations[id] = operation
        latestSaveRequest = id
        activeSavePrompt = id
        if let old { cancelDocumentSave(old) }
        // Cancelling the old prompt may synchronously start another request.
        guard saveOperations[id] === operation, activeSavePrompt == id else { return id }
        // An existing GUI window with an unavailable bridge must fail presentation,
        // never fall through to the legacy headless automatic destination.
        let hasWindow = saveHasWindow?() ?? (savePresenter.host != nil || window != nil)
        guard !requiresWindow || hasWindow else {
            settleDocumentSave(id, .failed("Save requires a document window")); return id
        }
        if !saveAs, let path = doc.info.path, path.hasSuffix(".tessera-doc") {
            admitDocumentWrite(operation, url: nil, folder: nil)
        } else {
            let request = SaveAsRequest(id: id, doc: doc,
                name: SaveAsRequest.defaultName(doc.title, path: doc.info.path), folder: saveFolder(for: doc))
            if !hasWindow { // Explicit legacy-only automatic destination.
                admitDocumentWrite(operation, url: request.url, folder: request.folder)
            } else { presentDocumentSaveSheet(request) }
        }
        return id
    }

    private func saveFolder(for doc: DocumentController) -> URL {
        if let p = doc.info.path { return URL(fileURLWithPath: p).deletingLastPathComponent() }
        if let f = lastSaveFolder ?? app?.recentFolders.first { return f }
        return FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser
    }

    func cancelDocumentSave(_ id: UUID) {
        guard let operation = saveOperations[id] else { return }
        if operation.phase == .writing {
            operation.continuationCancelled = true
        } else { settleDocumentSave(id, .cancelled) }
    }

    func documentSaveWindowLost(_ id: UUID) {
        if let operation = saveOperations[id] {
            if operation.phase == .writing { operation.continuationCancelled = true }
            else { settleDocumentSave(id, .failed("Document window closed before save")) }
        }
    }

    private func presentDocumentSaveSheet(_ request: SaveAsRequest) {
        guard saveOperations[request.id]?.phase == .choosing, activeSavePrompt == request.id else { return }
        if savePresenter.isBusy || savePresentation != nil {
            queuedSaveAs = request
            return
        }
        presentedSaveAs = request
        presentOwnedSave(request, replacement: false)
    }

    private func presentOwnedSave(_ request: SaveAsRequest, replacement: Bool) {
        let token = DocumentSavePresentationToken(requestID: request.id)
        savePresentation = token
        let actions = DocumentSavePresentationActions(cancel: { [weak self] in
            guard self?.savePresentation == token else { return }
            self?.cancelDocumentSave(request.id)
        }, submit: { [weak self] submitted in
            guard self?.savePresentation == token else { return }
            self?.finishSaveAs(submitted)
        }, chooseFolder: { [weak self] folder, done in
            guard let self, self.savePresentation == token else { return }
            self.savePresenter.chooseFolder(token, folder: folder, completion: done)
        })
        savePresenter.present(token, content: replacement ? .replacement(request) : .form(request), actions: actions) { [weak self] event in
            self?.savePresentationEvent(event)
        }
    }

    private func savePresentationEvent(_ event: DocumentSavePresentationEvent) {
        switch event {
        case .hostLost(let token):
            guard savePresentation == token else { return }
            // Invalidate queued work before callbacks may reenter. The owned
            // presenter retains its old native drain independently of operations.
            let queued = queuedSaveAs?.id
            queuedSaveAs = nil
            if let queued { documentSaveWindowLost(queued) }
            documentSaveWindowLost(token.requestID)
        case .failed(let token, let message):
            guard savePresentation == token else { return }
            savePresentation = nil
            settleDocumentSave(token.requestID, .failed(message))
            presentQueuedSaveIfReady()
        case .drained(let token, let response):
            guard savePresentation == token else { return }
            savePresentation = nil
            if presentedSaveAs?.id == token.requestID { presentedSaveAs = nil }
            if let operation = saveOperations[token.requestID], activeSavePrompt == operation.id {
                switch operation.phase {
                case .choosing: settleDocumentSave(operation.id, .cancelled)
                case .waitingForDismissal:
                    if let request = operation.pendingRequest {
                        if operation.needsReplacement {
                            operation.phase = .replacing
                            presentOwnedSave(request, replacement: true)
                        } else { admitDocumentWrite(operation, url: request.url, folder: request.folder) }
                    }
                case .replacing:
                    if response == NSApplication.ModalResponse.alertFirstButtonReturn.rawValue,
                       let request = operation.pendingRequest {
                        operation.destinationIntent = .replaceConfirmed
                        admitDocumentWrite(operation, url: request.url, folder: request.folder)
                    } else { settleDocumentSave(operation.id, .cancelled) }
                case .writing: break
                }
            }
            presentQueuedSaveIfReady()
        }
    }

    private func presentQueuedSaveIfReady() {
        guard savePresentation == nil, !savePresenter.isBusy, let request = queuedSaveAs else { return }
        queuedSaveAs = nil
        presentDocumentSaveSheet(request)
    }

    func finishSaveAs(_ request: SaveAsRequest) {
        guard let operation = saveOperations[request.id], operation.document === request.doc,
              operation.phase == .choosing, activeSavePrompt == request.id,
              let token = savePresentation, token.requestID == request.id else { return }
        guard request.isValid else { settleDocumentSave(request.id, .failed("Invalid file name")); return }
        operation.needsReplacement = saveFileExists?(request.url) ?? FileManager.default.fileExists(atPath: request.url.path)
        operation.destinationIntent = operation.needsReplacement ? nil : .createIfAbsent
        operation.pendingRequest = request
        operation.phase = .waitingForDismissal
        presentedSaveAs = nil
        savePresenter.end(token)
    }

    private func admitDocumentWrite(_ operation: DocumentSaveOperation, url: URL?, folder: URL?) {
        guard saveOperations[operation.id] === operation, operation.phase != .writing else { return }
        guard !operation.document.isClosed else { settleDocumentSave(operation.id, .failed("Document is closed")); return }
        if operation.requiresWindow, !(saveHasWindow?() ?? (savePresenter.host != nil)) {
            settleDocumentSave(operation.id, .failed("Document window closed before save")); return
        }
        guard operation.pendingRequest == nil || (operation.destinationIntent != nil && url != nil) else {
            settleDocumentSave(operation.id, .failed("Save As requires a confirmed destination intent")); return
        }
        operation.phase = .writing
        if activeSavePrompt == operation.id { activeSavePrompt = nil }
        if presentedSaveAs?.id == operation.id { presentedSaveAs = nil }
        // Strong self/operation ownership lasts until an admitted writer settles.
        let done: @MainActor (Result<DocSaveAsResult, Error>) -> Void = { [self, operation] result in
            guard saveOperations[operation.id] === operation, operation.phase == .writing else { return }
            switch result {
            case .success(.destinationExists):
                guard let url else {
                    settleDocumentSave(operation.id, .failed("Save returned a conflict without a destination")); return
                }
                settleDocumentSave(operation.id, .destinationConflict(url))
            case .success(.saved):
                operation.document.reloadModel()
                operation.document.reloadHistory()
                if latestSaveRequest == operation.id {
                    if let folder { lastSaveFolder = folder }
                    say("Saved \(operation.document.title)")
                }
                settleDocumentSave(operation.id, .saved(url ?? operation.document.info.path.map { URL(fileURLWithPath: $0) },
                    continuationCancelled: operation.continuationCancelled))
            case .failure(let error): settleDocumentSave(operation.id, .failed(error.localizedDescription))
            }
        }
        if let intent = operation.destinationIntent, let url {
            if let checkedSaveWriter { checkedSaveWriter(operation.document, url, intent, done) }
            else { done(Result { try operation.document.backend.saveAs(path: url.path, intent: intent) }) }
        } else if let saveWriter {
            // Legacy headless Save As and ordinary Save preserve their Void route.
            saveWriter(operation.document, url) { result in done(result.map { .saved }) }
        } else {
            done(Result {
                if let url { try operation.document.backend.saveAs(path: url.path) }
                else { try operation.document.backend.save() }
                return .saved
            })
        }
    }

    private func settleDocumentSave(_ id: UUID, _ outcome: DocumentSaveOutcome) {
        guard let operation = saveOperations.removeValue(forKey: id) else { return }
        operation.pendingRequest = nil
        if queuedSaveAs?.id == id { queuedSaveAs = nil }
        if activeSavePrompt == id { activeSavePrompt = nil }
        if presentedSaveAs?.id == id { presentedSaveAs = nil }
        let completion = operation.completion
        operation.completion = nil // Latch before reentrant observers.
        if latestSaveRequest == id {
            switch outcome {
            case .failed(let message): say("Save: \(message)")
            case .destinationConflict:
                say("Save: A file appeared at this destination. Choose another name or confirm Replace.")
            default: break
            }
        }
        // Clear logical ownership before native end or caller completion reenters.
        let token = savePresentation.flatMap { $0.requestID == id ? $0 : nil }
        if let token { savePresenter.end(token) }
        completion?(outcome)
    }

    /// Save As to `url` (`.tessera-doc`, `.psd`, `.psb`); the document takes that path.
    @discardableResult
    func write(_ doc: DocumentController, to url: URL) -> Bool {
        do {
            try doc.backend.saveAs(path: url.path)
            doc.reloadModel()
            doc.reloadHistory()
            say("Saved \(url.lastPathComponent)")
            return true
        } catch {
            say("Save As: \(error.localizedDescription)")
            return false
        }
    }

    /// Export Flat of `doc` to `url` with `s`, synchronously (self-tests and scripts). The menu command runs
    /// `startExportFlat` instead, which keeps the main thread free.
    @discardableResult
    func exportFlat(_ doc: DocumentController, _ s: ExportFlatSettings, to url: URL) -> Bool {
        do {
            try doc.backend.exportFlat(path: url.path, format: s.format.documentFormat, quality: UInt8(s.quality),
                                       color: s.color.documentColor)
            say("Exported \(url.lastPathComponent) (\(s.format.title), \(s.color.title))")
            return true
        } catch {
            say("Export Flat: \(error.localizedDescription)")
            return false
        }
    }

    /// File ▸ Export Flat… : the sheet's settings, then a save panel.
    func exportFlat(_ s: ExportFlatSettings) {
        guard let doc = current else { return }
        exportSettings = s
        let panel = NSSavePanel()
        panel.title = "Export Flat"
        panel.allowedContentTypes = [s.format.utType]
        panel.nameFieldStringValue = (doc.title as NSString).deletingPathExtension + "." + s.format.fileExtension
        let handle: @MainActor (NSApplication.ModalResponse) -> Void = { [weak self] r in
            guard r == .OK, let url = panel.url else { return }
            self?.startExportFlat(doc, s, to: url)   // B5-15: in the background
        }
        if let window = self.window {
            panel.beginSheetModal(for: window) { r in MainActor.assumeIsolated { handle(r) } }
        } else {
            handle(panel.runModal())
        }
    }

    // MARK: Background Export Flat (B5-15, perf audit P16)

    /// Exports running in the background, oldest first (shown in the document window with Cancel).
    private(set) var flatExports: [FlatExportTask] = []
    @ObservationIgnored private let flatExportGroup = DispatchGroup()
    @ObservationIgnored private var terminationObserver: NSObjectProtocol?
    @ObservationIgnored private var exportAccessories: [FlatExportProgressView] = []
    @ObservationIgnored private var preparedExportHUD: FlatExportProgressView?
    // Diagnostic injection: exercise the real progress UI in an unordered test window.
    @ObservationIgnored var exportWindow: NSWindow?
    @ObservationIgnored var exportTrace = PerformanceTrace.shared

    /// Export Flat of `doc` to `url` with `s` without blocking the main thread: reserve the session now,
    /// then snapshot, composite, convert, encode and write on a background
    /// task with progress and Cancel in the window. Cancelling leaves the destination untouched (the file is
    /// renamed into place only when complete). Closing the document does not stop the export; quitting
    /// cancels it. The snapshot includes edits committed before the worker acquires it; later edits do not
    /// affect the export. `then` runs on the main actor with the outcome.
    @discardableResult
    func startExportFlat(_ doc: DocumentController, _ s: ExportFlatSettings, to url: URL,
                         then: (@MainActor (FlatExportTask.Outcome) -> Void)? = nil) -> FlatExportTask? {
        let trace = exportTrace
        let setupSpan = trace.begin("export_flat_setup")
        defer { trace.end(setupSpan) }
        let backend = doc.backend
        let (format, quality, color) = (s.format.documentFormat, UInt8(s.quality), s.color.documentColor)
        let path = url.path
        let run: @Sendable (@escaping @Sendable (Double, String) -> Void) throws -> Void
        let cancel: @Sendable () -> Void
        if let exporter = backend as? DocumentFlatExporting {
            let preparation: DocumentFlatExportPreparation
            do {
                preparation = try exporter.prepareExportFlat(path: path, format: format, quality: quality, color: color)
            } catch {
                say("Export Flat: \(error.localizedDescription)")
                then?(.failed(error.localizedDescription))
                return nil
            }
            run = { progress in
                let job: DocumentFlatExport
                do {
                    let snapshotSpan = trace.begin("export_flat_snapshot")
                    defer { trace.end(snapshotSpan) }
                    job = try preparation.snapshot()
                }
                try job.run(progress: progress)
            }
            cancel = { preparation.cancel() }
        } else {
            // Backends without a background exporter (the stub): their synchronous export, off the main thread.
            let flag = CancelBox()
            run = { _ in
                // The only checkpoint is before the write; once it has started the file is exported.
                if flag.isSet { throw CancellationError() }
                try backend.exportFlat(path: path, format: format, quality: quality, color: color)
            }
            cancel = { flag.set() }
        }
        let task = FlatExportTask(fileName: url.lastPathComponent, documentTitle: doc.title, cancel: cancel)
        // Capture the exporting document's host once. A later main-window change must not
        // move its progress UI. The explicit fallback is only for unordered diagnostic hosts.
        task.progressHost = doc.viewport ?? exportWindow?.contentView
        flatExports.append(task)
        observeTermination()
        updateExportAccessory()
        let summary = "\(url.lastPathComponent) (\(s.format.title), \(s.color.title))"
        let group = flatExportGroup
        let onProgress: @MainActor @Sendable (Double, String) -> Void = { [weak self, weak task] f, phase in
            guard let self, let task, self.flatExports.contains(where: { $0 === task }), !task.cancelling else { return }
            let span = trace.begin("export_flat_progress")
            defer { trace.end(span) }
            task.update(f, phase)
        }
        let progress = FlatExportProgressPublisher(publish: onProgress)
        let onDone: @MainActor @Sendable (Result<Void, any Error>) -> Void = { [weak self, weak task] result in
            progress.finish()
            let span = trace.begin("export_flat_completion")
            defer { trace.end(span) }
            guard let task else { return }
            let outcome: FlatExportTask.Outcome
            switch result {
            // A Cancel that arrived after the last checkpoint could not stop the write: the file is in place,
            // so report it as exported rather than cancelled.
            case .success: outcome = .exported
            case .failure(let e): outcome = task.cancelling ? .cancelled : .failed(e.localizedDescription)
            }
            self?.finishExportFlat(task, outcome, summary: summary)
            then?(outcome)
        }
        group.enter()
        Task.detached(priority: .userInitiated) {
            let workSpan = trace.begin("export_flat_work")
            defer { trace.end(workSpan) }
            let result = Result {
                // Task priority alone does not prevent App Nap when the document window is covered.
                // Keep this user-requested export active only until the worker finishes (including
                // cancellation/errors), without preventing the Mac from sleeping.
                let activity = ProcessInfo.processInfo.beginActivity(
                    options: .userInitiatedAllowingIdleSystemSleep, reason: "Exporting document")
                defer { ProcessInfo.processInfo.endActivity(activity) }
                try run { f, phase in progress.receive(f, phase) }
            }
            group.leave()
            await onDone(result)
        }
        return task
    }

    /// Cancels `task` (the Cancel button); the destination is left as it was, unless the export had already
    /// passed its last checkpoint, in which case it completes and is reported as exported.
    func cancelExportFlat(_ task: FlatExportTask) {
        task.cancelNow()
        say("Cancelling export of \(task.fileName)…")
    }

    private func finishExportFlat(_ task: FlatExportTask, _ outcome: FlatExportTask.Outcome, summary: String) {
        flatExports.removeAll { $0 === task }
        updateExportAccessory()
        switch outcome {
        case .exported: say("Exported \(summary)")
        case .cancelled: say("Export of \(task.fileName) cancelled")
        case .failed(let message): say("Export Flat: \(message)")
        }
    }

    /// Quitting cancels running exports and waits (bounded) for them to stop, so no temporary file is left
    /// half-written next to a destination.
    private func observeTermination() {
        guard terminationObserver == nil else { return }
        terminationObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.willTerminateNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                for t in self.flatExports { t.cancelNow() }
                _ = self.flatExportGroup.wait(timeout: .now() + 5)
            }
        }
    }

    /// Keep progress outside the SwiftUI layout tree. Adding/removing a titlebar accessory changes
    /// contentLayoutRect, resizes the document viewport, and schedules another expensive render/layout.
    /// This native overlay never changes the window, canvas, or hosting view's sizing constraints.
    private func updateExportAccessory() {
        let span = exportTrace.begin("export_flat_hud_update")
        defer { exportTrace.end(span) }
        for hud in exportAccessories {
            let tasks = flatExports.filter { $0.progressHost != nil && $0.progressHost === hud.superview }
            if tasks.isEmpty {
                hud.removeFromSuperview()
                if preparedExportHUD == nil { preparedExportHUD = hud }
            }
        }
        exportAccessories.removeAll { $0.superview == nil }
        var updated = Set<ObjectIdentifier>()
        for task in flatExports {
            guard let parent = task.progressHost, updated.insert(ObjectIdentifier(parent)).inserted else { continue }
            let hud: FlatExportProgressView
            if let existing = exportAccessories.first(where: { $0.superview === parent }) {
                hud = existing
            } else {
                hud = preparedExportHUD ?? FlatExportProgressView(workspace: self)
                preparedExportHUD = nil
                parent.addSubview(hud, positioned: .above, relativeTo: nil)
                exportAccessories.append(hud)
            }
            hud.update(flatExports.filter { $0.progressHost === parent })
            hud.placeInViewport()
        }
    }

    // MARK: Closing

    /// ⌘W in document mode: asks to save unsaved changes.
    func close(_ doc: DocumentController? = nil) {
        guard let doc = doc ?? current else { return }
        guard doc.isDirty, let window = self.window else { discard(doc); return }
        let alert = NSAlert()
        alert.messageText = "Do you want to save the changes made to “\(doc.title)”?"
        alert.informativeText = "Your changes will be lost if you don’t save them."
        alert.addButton(withTitle: "Save…")
        alert.addButton(withTitle: "Cancel")
        alert.addButton(withTitle: "Don’t Save")
        alert.beginSheetModal(for: window) { [weak self] response in
            MainActor.assumeIsolated {
                switch response {
                case .alertFirstButtonReturn: self?.save(doc) { self?.discard(doc) }
                case .alertThirdButtonReturn: self?.discard(doc)
                default: break
                }
            }
        }
    }

    func discard(_ doc: DocumentController) {
        guard let i = documents.firstIndex(where: { $0 === doc }) else { return }
        documents.remove(at: i)
        DocumentText.shared.documentClosing(doc)   // B5-10: no stale caret or draft
        DocumentTools.shared.documentClosing(doc)
        doc.close()
        if current === doc {
            current = documents.isEmpty ? nil : documents[min(i, documents.count - 1)]
        }
        say("Closed \(doc.title)")
    }

    /// ⌘W: the current document in document mode, otherwise the window.
    func closeCommand() {
        if app?.viewMode == .document, current != nil { close() } else { NSApp.keyWindow?.performClose(nil) }
    }

    // MARK: Panels and screen modes

    func setPanelsHidden(_ hidden: Bool) {
        panelsHidden = hidden
        columnVisibility = hidden ? .detailOnly : .all
        app?.showInspector = !hidden
    }

    func togglePanels() { setPanelsHidden(!panelsHidden) }

    /// F: standard → full screen → full screen without panels → standard.
    func cycleScreenMode() {
        let window = self.window
        let isFull = window?.styleMask.contains(.fullScreen) == true
        screenMode = (screenMode + 1) % 3
        switch screenMode {
        case 1:
            if !isFull { window?.toggleFullScreen(nil) }
            say("Screen mode: full screen (F for full screen without panels)")
        case 2:
            setPanelsHidden(true)
            say("Screen mode: full screen without panels (F for standard)")
        default:
            setPanelsHidden(false)
            if isFull { window?.toggleFullScreen(nil) }
            say("Screen mode: standard")
        }
    }

    /// Leaving document mode restores the panels.
    func didLeaveDocumentMode() {
        if panelsHidden { setPanelsHidden(false) }
        spaceHeld = false
    }

    // MARK: Snapshots

    func promptSnapshot() {
        guard let doc = current, let window = self.window else { return }
        let alert = NSAlert()
        alert.messageText = "New Snapshot"
        alert.informativeText = "Names the current state of “\(doc.title)” so you can return to it."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        field.stringValue = "Snapshot \(doc.snapshots.count + 1)"
        alert.accessoryView = field
        alert.addButton(withTitle: "Save")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { response in
            let name = field.stringValue
            MainActor.assumeIsolated {
                if response == .alertFirstButtonReturn { doc.snapshot(named: name) }
            }
        }
    }
}

/// File ▸ New Document… values.
struct NewDocumentSettings: Equatable {
    var width = 2400
    var height = 1600
    var depth: DocBitDepth = .u8
    var profile = "sRGB IEC61966-2.1"

    static let profiles = ["sRGB IEC61966-2.1", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"]
    static let presets: [(String, Int, Int)] = [
        ("Default (2400 × 1600)", 2400, 1600), ("HD (1920 × 1080)", 1920, 1080), ("4K UHD (3840 × 2160)", 3840, 2160),
        ("Square (2048 × 2048)", 2048, 2048), ("A4 at 300 ppi (2480 × 3508)", 2480, 3508),
    ]
    var isValid: Bool { (1...30_000).contains(width) && (1...30_000).contains(height) }
}

/// File ▸ Export Flat… values (the export sheet's format and colour vocabulary).
struct ExportFlatSettings: Equatable {
    var format: ExportSettings.FileFormat = .png
    var quality = 90
    var color: ExportSettings.ColorSpace = .srgb
}

extension ExportSettings.FileFormat {
    var documentFormat: DocExportFormat {
        switch self {
        case .jpeg: .jpeg
        case .png: .png
        case .tiff: .tiff
        }
    }
    var utType: UTType {
        switch self {
        case .jpeg: .jpeg
        case .png: .png
        case .tiff: .tiff
        }
    }
}

extension ExportSettings.ColorSpace {
    var documentColor: DocExportColor { DocExportColor(rawValue: rawValue) ?? .srgb }
}

// MARK: - B5-15 background Export Flat state and bar

/// One background Export Flat (DocumentWorkspace.startExportFlat).
@MainActor
final class FlatExportTask: Identifiable {
    enum Outcome: Equatable { case exported, cancelled, failed(String) }

    let id = UUID()
    let fileName: String
    let documentTitle: String
    weak var progressHost: NSView?
    private(set) var fraction: Double = 0
    private(set) var phase = "Preparing"
    private(set) var cancelling = false
    private let cancelAction: @Sendable () -> Void
    // Bound by the native row. A delivery changes layers only, never observable UI state.
    var progressChanged: (@MainActor () -> Void)?

    init(fileName: String, documentTitle: String, cancel: @escaping @Sendable () -> Void) {
        self.fileName = fileName
        self.documentTitle = documentTitle
        cancelAction = cancel
    }

    func update(_ fraction: Double, _ phase: String) {
        guard !cancelling else { return }
        self.fraction = max(self.fraction, fraction)
        self.phase = phase
        progressChanged?()
    }

    func cancelNow() {
        guard !cancelling else { return }
        cancelling = true
        phase = "Cancelling"
        progressChanged?()
        cancelAction()
    }
}

/// A thread-safe flag (the stub backend's cancel).
private final class CancelBox: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false
    var isSet: Bool { lock.withLock { value } }
    func set() { lock.withLock { value = true } }
}

/// One pending main-queue delivery per export. Tile callbacks replace the pending
/// value instead of queuing main-actor tasks. Both phase and percentage changes
/// obey the 100 ms interval, and identical displayed values never publish again.
final class FlatExportProgressPublisher: @unchecked Sendable {
    private let lock = NSLock()
    private let publish: @MainActor @Sendable (Double, String) -> Void
    private var latest: (Double, String)?
    private var displayed: (Int, String)?
    private var scheduled = false
    private var finished = false
    private var lastDeliveryAttempt: TimeInterval = -.infinity

    private let now: @Sendable () -> TimeInterval
    private let schedule: @Sendable (TimeInterval, @escaping @MainActor @Sendable () -> Void) -> Void

    init(now: @escaping @Sendable () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
         schedule: @escaping @Sendable (TimeInterval, @escaping @MainActor @Sendable () -> Void) -> Void = { delay, action in
             DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
                 MainActor.assumeIsolated { action() }
             }
         }, publish: @escaping @MainActor @Sendable (Double, String) -> Void) {
        self.now = now
        self.schedule = schedule
        self.publish = publish
    }

    func receive(_ fraction: Double, _ phase: String) {
        let delay: TimeInterval? = lock.withLock {
            guard !finished else { return nil }
            latest = (fraction, phase)
            guard !scheduled else { return nil }
            scheduled = true
            return max(0, 0.1 - (now() - lastDeliveryAttempt))
        }
        guard let delay else { return }
        schedule(delay) { [self] in deliver() }
    }

    @MainActor private func deliver() {
        let value: (Double, String)? = lock.withLock {
            scheduled = false
            guard !finished, let value = latest else { return nil }
            latest = nil
            let key = (Int((value.0 * 100).rounded()), value.1)
            // Even a skipped duplicate consumes the interval: otherwise every
            // tile after the last publication's deadline wakes the main queue.
            lastDeliveryAttempt = now()
            if let displayed, displayed == key { return nil }
            displayed = key
            return value
        }
        if let value { publish(value.0, value.1) }
    }

    func finish() {
        lock.withLock {
            finished = true
            latest = nil
        }
    }
}

/// Native lower-right viewport HUD, above the zoom chip. Manual layout deliberately
/// cannot invalidate the hosting view's size or re-evaluate the document's SwiftUI graph.
@MainActor
final class FlatExportProgressView: NSView {
    typealias AccessibilityPost = (Any, NSAccessibility.Notification) -> Void
    private let post: AccessibilityPost
    static let rowHeight: CGFloat = 64
    private weak var workspace: DocumentWorkspace?
    private var rows: [UUID: Row] = [:]
    private var reusableRow: Row?
    private var order: [UUID] = []
    override var isFlipped: Bool { true }
    // Consume events on the HUD background instead of forwarding edits to the viewport.
    override func mouseDown(with event: NSEvent) {}
    override func mouseUp(with event: NSEvent) {}
    override func mouseDragged(with event: NSEvent) {}
    override func rightMouseDown(with event: NSEvent) {}
    override func otherMouseDown(with event: NSEvent) {}
    override func scrollWheel(with event: NSEvent) {}
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    init(workspace: DocumentWorkspace, post: @escaping AccessibilityPost = {
        NSAccessibility.post(element: $0, notification: $1)
    }) {
        self.post = post
        self.workspace = workspace
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Document exports")
        setAccessibilityIdentifier("document-export-progress")
        reusableRow = Row(workspace: workspace, post: post)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    func update(_ tasks: [FlatExportTask]) {
        let nextOrder = tasks.map(\.id)
        let changedRows = order != nextOrder
        let ids = Set(tasks.map(\.id))
        for id in Array(rows.keys) where !ids.contains(id) {
            if let row = rows.removeValue(forKey: id) {
                row.removeFromSuperview()
                if reusableRow == nil { reusableRow = row }
            }
        }
        order = nextOrder
        for task in tasks {
            let row: Row
            if let existing = rows[task.id] {
                row = existing
            } else if let reusable = reusableRow {
                row = reusable
                reusableRow = nil
            } else {
                let trace = workspace?.exportTrace
                let span = trace?.begin("export_flat_row_create")
                row = Row(workspace: workspace, post: post)
                trace?.end(span)
            }
            if row.superview == nil { addSubview(row) }
            rows[task.id] = row
            row.update(task)
        }
        if changedRows { arrangeRows() }
    }

    override func resize(withOldSuperviewSize oldSize: NSSize) {
        super.resize(withOldSuperviewSize: oldSize)
        placeInViewport()
    }

    override func accessibilityHitTest(_ point: NSPoint) -> Any? {
        // AX invokes AppKit views synchronously on main, but this SDK entry point
        // lacks actor isolation. Keep the non-Sendable AX result in this call.
        nonisolated(unsafe) var result: Any?
        MainActor.assumeIsolated {
            for id in order {
                if let row = rows[id], row.accessibilityFrame().contains(point) {
                    result = row.accessibilityHitTest(point)
                    break
                }
            }
        }
        return result ?? super.accessibilityHitTest(point)
    }

    func placeInViewport() {
        guard let parent = superview else { return }
        let inset = Theme.Space.m
        let width = min(400, max(0, parent.bounds.width - 2 * inset))
        let height = Self.rowHeight * CGFloat(order.count)
        // Leave the transient zoom chip's bottom-center lane clear.
        let bottom = Theme.Space.l + Theme.Height.large + Theme.Space.m
        autoresizingMask = [.minXMargin, parent.isFlipped ? .minYMargin : .maxYMargin]
        let placement = NSRect(x: parent.bounds.maxX - inset - width,
                               y: parent.isFlipped ? parent.bounds.maxY - bottom - height : parent.bounds.minY + bottom,
                               width: width, height: height)
        if frame != placement { frame = placement }
    }

    override func setFrameSize(_ newSize: NSSize) {
        guard newSize != frame.size else { return }
        super.setFrameSize(newSize)
        arrangeRows()
    }

    private func arrangeRows() {
        for (i, id) in order.enumerated() {
            rows[id]?.frame = NSRect(x: 0, y: CGFloat(i) * Self.rowHeight,
                                     width: bounds.width, height: Self.rowHeight)
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: Theme.Space.hairline, dy: Theme.Space.hairline),
                                 xRadius: Theme.Radius.card, yRadius: Theme.Radius.card)
        Theme.Palette.hud.setFill()
        shape.fill()
        Theme.Palette.hairline.setStroke()
        shape.lineWidth = Theme.Space.hairline
        shape.stroke()
    }

    /// A custom cell keeps NSButton's tracking, keyboard and AX behavior, while
    /// drawing the small HUD bezel without macOS 26's hosted AppKitButton graph.
    private final class CancelCell: NSButtonCell {
        override func drawBezel(withFrame frame: NSRect, in controlView: NSView) {
            let path = NSBezierPath(roundedRect: frame.insetBy(dx: Theme.Space.hairline, dy: Theme.Space.hairline),
                                    xRadius: Theme.Radius.control, yRadius: Theme.Radius.control)
            Theme.Palette.raised.setFill()
            path.fill()
            if isHighlighted {
                Theme.Palette.pressed.setFill()
                path.fill()
            }
            Theme.Palette.hairlineStrong.setStroke()
            path.lineWidth = Theme.Space.hairline
            path.stroke()
        }

        override func drawInterior(withFrame frame: NSRect, in controlView: NSView) {
            // NSButtonCell's default titleRectForBounds also consults a hosted
            // sizing view on macOS 26, even when the bezel is custom drawn.
            let font = Theme.NSFonts.label
            let paragraph = NSMutableParagraphStyle()
            paragraph.alignment = .center
            paragraph.lineBreakMode = .byTruncatingTail
            let height = ceil(font.ascender - font.descender)
            let rect = NSRect(x: frame.minX + Theme.Space.xs, y: frame.midY - height / 2,
                              width: max(0, frame.width - 2 * Theme.Space.xs), height: height)
            (title as NSString).draw(in: rect, withAttributes: [
                .font: font, .paragraphStyle: paragraph,
                .foregroundColor: isEnabled ? Theme.Palette.textPrimary : Theme.Palette.textTertiary,
            ])
        }
    }

    /// Virtual AX children supply the same text/progress semantics without native controls.
    private final class LayerAccessibility: NSAccessibilityElement {
        weak var owner: NSView?
        let content: CALayer

        init(owner: NSView, content: CALayer, role: NSAccessibility.Role) {
            self.owner = owner
            self.content = content
            super.init()
            setAccessibilityRole(role)
            setAccessibilityParent(owner)
            setAccessibilityElement(true)
        }

        override func accessibilityFrame() -> NSRect {
            guard let owner else { return .zero }
            let frame = content.frame
            // AppKit queries these virtual children on main, just like NSView AX methods.
            // Capture the actor-isolated view and value, not the non-Sendable AX element.
            return MainActor.assumeIsolated {
                guard let window = owner.window else { return .zero }
                return window.convertToScreen(owner.convert(frame, to: nil))
            }
        }
    }

    private final class Row: NSView {
        private let post: AccessibilityPost
        private let name = CATextLayer()
        private let phase = CATextLayer()
        private let progress = CALayer()
        private let fill = CALayer()
        private let cancel = NSButton(title: "Cancel", target: nil, action: nil)
        private lazy var nameAX = LayerAccessibility(owner: self, content: name, role: .staticText)
        private lazy var phaseAX = LayerAccessibility(owner: self, content: phase, role: .staticText)
        private lazy var progressAX = LayerAccessibility(owner: self, content: progress, role: .progressIndicator)
        private weak var task: FlatExportTask?
        private weak var workspace: DocumentWorkspace?
        override var isFlipped: Bool { true }

        init(workspace: DocumentWorkspace?, post: @escaping AccessibilityPost) {
            self.post = post
            self.workspace = workspace
            super.init(frame: .zero)
            wantsLayer = true
            setAccessibilityElement(true)
            setAccessibilityRole(.group)
            // CATextLayer draws in its own coordinates; match the flipped row so
            // glyphs remain upright while frames are measured from the top.
            name.isGeometryFlipped = true
            phase.isGeometryFlipped = true
            name.font = Theme.NSFonts.body
            name.fontSize = Theme.NSFonts.body.pointSize
            name.truncationMode = .middle
            phase.font = Theme.NSFonts.labelNumeric
            phase.fontSize = Theme.NSFonts.labelNumeric.pointSize
            phase.truncationMode = .end
            for content in [name, phase, progress] { layer?.addSublayer(content) }
            progress.addSublayer(fill)
            progress.cornerRadius = Theme.Radius.control
            progress.masksToBounds = true
            progressAX.setAccessibilityMinValue(0)
            progressAX.setAccessibilityMaxValue(1)
            cancel.cell = CancelCell(textCell: "Cancel")
            cancel.setButtonType(.momentaryPushIn)
            cancel.isBordered = true
            cancel.bezelStyle = .rounded
            cancel.controlSize = .small
            cancel.target = self
            cancel.action = #selector(cancelExport)
            addSubview(cancel)
            updateAppearance()
        }
        required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

        override func accessibilityChildren() -> [Any]? { [nameAX, phaseAX, progressAX, cancel] }

        override func accessibilityHitTest(_ point: NSPoint) -> Any? {
            nonisolated(unsafe) var result: Any?
            MainActor.assumeIsolated {
                for child in [nameAX, phaseAX, progressAX] where child.accessibilityFrame().contains(point) {
                    result = child
                    break
                }
                if result == nil, cancel.accessibilityFrame().contains(point) {
                    result = NSAccessibility.unignoredDescendant(of: cancel)
                }
            }
            return result ?? super.accessibilityHitTest(point)
        }

        func update(_ task: FlatExportTask) {
            if self.task !== task {
                self.task?.progressChanged = nil
                self.task = task
                task.progressChanged = { [weak self] in self?.publish() }
                setAccessibilityLabel("Export of \(task.fileName)")
                progressAX.setAccessibilityLabel("Export progress for \(task.fileName)")
                cancel.setAccessibilityLabel("Cancel export of \(task.fileName)")
                cancel.cell?.setAccessibilityLabel("Cancel export of \(task.fileName)")
                CATransaction.begin()
                CATransaction.setDisableActions(true)
                let title = "Exporting \(task.fileName)"
                name.string = title
                nameAX.setAccessibilityValue(title)
                CATransaction.commit()
            }
            publish()
        }

        private func publish() {
            guard let task else { return }
            let trace = workspace?.exportTrace
            let span = trace?.begin("export_flat_hud_update")
            defer { trace?.end(span) }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            let status = "\(task.phase) \(Int((task.fraction * 100).rounded())) %"
            if phase.string as? String != status {
                phase.string = status
                phaseAX.setAccessibilityValue(status)
            }
            fill.frame = CGRect(x: 0, y: 0, width: progress.bounds.width * task.fraction,
                                height: progress.bounds.height)
            progressAX.setAccessibilityValue(task.fraction)
            progressAX.setAccessibilityValueDescription("\(Int((task.fraction * 100).rounded())) %")
            if cancel.isEnabled == task.cancelling { cancel.isEnabled = !task.cancelling }
            CATransaction.commit()
            post(progressAX, .valueChanged)
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            updateAppearance()
        }

        override func viewDidChangeBackingProperties() {
            super.viewDidChangeBackingProperties()
            updateAppearance()
        }

        override func viewDidChangeEffectiveAppearance() {
            super.viewDidChangeEffectiveAppearance()
            updateAppearance()
        }

        private func updateAppearance() {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
            name.contentsScale = scale
            phase.contentsScale = scale
            effectiveAppearance.performAsCurrentDrawingAppearance {
                name.foregroundColor = Theme.Palette.textPrimary.cgColor
                phase.foregroundColor = Theme.Palette.textSecondary.cgColor
                progress.backgroundColor = Theme.Palette.hairlineStrong.cgColor
                fill.backgroundColor = Theme.Palette.accent.cgColor
            }
            CATransaction.commit()
        }

        override func setFrameSize(_ newSize: NSSize) {
            guard newSize != frame.size else { return }
            super.setFrameSize(newSize)
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            let gap = Theme.Space.s, inset = Theme.Space.m
            let available = max(0, bounds.width - 2 * inset)
            let buttonWidth = min(64, available)
            name.frame = NSRect(x: inset, y: 6, width: max(0, available - buttonWidth - gap), height: 20)
            cancel.frame = NSRect(x: bounds.width - inset - buttonWidth, y: 4, width: buttonWidth, height: 24)
            let phaseWidth = min(170, available * 0.55)
            let progressWidth = max(0, available - phaseWidth - gap)
            progress.frame = NSRect(x: inset, y: 43, width: progressWidth, height: Theme.Space.xs)
            phase.frame = NSRect(x: inset + progressWidth + gap, y: 34, width: phaseWidth, height: 20)
            fill.frame = CGRect(x: 0, y: 0, width: progressWidth * (task?.fraction ?? 0), height: progress.bounds.height)
            CATransaction.commit()
        }

        @objc private func cancelExport() {
            guard let task else { return }
            workspace?.cancelExportFlat(task)
        }
    }
}
