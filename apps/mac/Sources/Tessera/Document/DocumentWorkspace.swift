import AppKit
import Observation
import SwiftUI
import TesseraCore
import TesseraFFI
import UniformTypeIdentifiers

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
    case failed(String)
}

/// At the captured parent's didEndSheet notification, the old child's
/// sheetParent may still contain stale metadata. The parent's attachment owns
/// the decision; a newer attached sheet is never modified by this predicate.
enum DocumentSaveSheetAttachment {
    static func hasDetached(capturedSheet: ObjectIdentifier, parentAttachedSheet: ObjectIdentifier?) -> Bool {
        parentAttachedSheet != capturedSheet
    }
}

/// Notification tokens also clean up if their workspace disappears while a sheet
/// is draining. NotificationCenter removal is safe from deinitialization's thread.
private final class DocumentSaveNativeObservers {
    let tokens: [NSObjectProtocol]
    init(_ tokens: [NSObjectProtocol]) { self.tokens = tokens }
    func remove() { for token in tokens { NotificationCenter.default.removeObserver(token) } }
    deinit { remove() }
}

@MainActor
private final class DocumentSaveNativeDismissal {
    var swiftDismissed = false
    var nativeDetached = false
    var observedAttachment = false
    var activeProbes: Set<UUID> = []
    var parent: NSWindow?
    var sheet: NSWindow?
    var removeObservers: (() -> Void)?
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
    var windowObserver: NSObjectProtocol?
    weak var presentingWindow: NSWindow?
    weak var saveSheetWindow: NSWindow?
    var replaceAlert: NSAlert?
    var pendingReplacement: SaveAsRequest?
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
    var resolvedEngine: any DocumentEngine {
        Self.selectEngine(library: app?.library, policy: policy) { [weak self] in
            if let e = self?.standaloneEngine { return e.engine }
            do {
                let e = try Engine.open(appSupportDir: EngineLibrary.defaultSupportDirectory.path)
                self?.standaloneEngine = EngineDocumentEngine.for(e)
                return e
            } catch {
                self?.say("Documents: the engine did not open (\(error.localizedDescription)); using the stub backend")
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

    func install(_ backend: any DocumentBackend) throws {
        if let existing = documents.first(where: { $0.backend === backend }) {
            select(existing)
            return
        }
        let doc = try DocumentController(backend: backend)
        doc.report = { [weak self] in self?.say($0) }
        documents.append(doc)
        select(doc)
    }

    func select(_ doc: DocumentController) {
        // B5-10 begin: switching documents applies the text being edited (Esc first to discard it).
        if current !== doc, let c = current, DocumentText.shared.isEditing(c) { DocumentText.shared.documentWillChange() }
        // B5-10 end
        current = doc
        app?.viewMode = .document
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

    // Tests hold backend completion without native decode, disk or GPU work.
    @ObservationIgnored var documentLoadExecutor: ((any DocumentEngine,
        @escaping @Sendable (any DocumentEngine) throws -> any DocumentBackend,
        @escaping @MainActor (Result<any DocumentBackend, Error>) -> Void) -> Void)?
    @ObservationIgnored private var openingToken: UUID?

    /// Completion retains the captured engine until actual backend return and
    /// installation/orphan cleanup, even if the workspace no longer exists.
    private func load(_ what: String, engine: any DocumentEngine, done: String,
                      completion: @escaping @MainActor (DocumentLoadOutcome) -> Void = { _ in },
                      _ body: @escaping @Sendable (any DocumentEngine) throws -> any DocumentBackend) {
        let token = UUID()
        openingToken = token
        opening = what
        say("\(what)…")
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
                    try self.install(backend)
                    self.say(done)
                    settlement.finish(.installed)
                } catch {
                    backend.close()
                    self.say("\(what): \(error.localizedDescription)")
                    settlement.finish(.failed(error.localizedDescription))
                }
            case .failure(let error):
                self.say("\(what): \(error.localizedDescription)")
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

    /// Library ▸ Edit in Layers (⌘E): the focused image, developed, as a new document. Engine images
    /// open on their own engine by image id (`open_document_from_image(id, developed: true)`); the stub
    /// takes the file.
    /// Release saved-pixel reservations from completion, never on method return.
    /// Rejections/stub loads may settle synchronously. There is no early cancellation
    /// signal: completion follows backend return, including when the UI disappears.
    func editInLayers(_ item: PhotoItem?,
                      completion: @escaping @MainActor (DocumentLoadOutcome) -> Void = { _ in }) {
        guard let item else {
            let message = "Edit in Layers: select a photo first"
            say(message); completion(.rejected(message)); return
        }
        let what = "Edit \(item.name) in Layers", done = "Editing \(item.name) in layers"
        if !(engineOverride is StubDocumentEngine), let ref = item.engineImage {
            let id = ref.imageID
            load(what, engine: EngineDocumentEngine.for(ref.engine), done: done, completion: completion) {
                try $0.openDocumentFromImage(imageId: id, developed: true)
            }
            return
        }
        let engine = self.engine
        guard let url = item.url else {
            let message = "Edit in Layers needs a photo file (stub items have none)"
            say(message); completion(.rejected(message)); return
        }
        if engine is StubDocumentEngine {
            let id = "file:\(url.path)"
            load(what, engine: engine, done: done, completion: completion) { try $0.openDocumentFromImage(imageId: id, developed: true) }
        } else if item.kind == .raw {
            let message = "Edit in Layers: open the photo's folder to edit a RAW on the engine"
            say(message); completion(.rejected(message))
        } else {
            let path = url.path
            load(what, engine: engine, done: done, completion: completion) { try $0.openDocument(path: path) }
        }
    }

    // MARK: Saving

    @ObservationIgnored private var saveOperations: [UUID: DocumentSaveOperation] = [:]
    @ObservationIgnored private var activeSavePrompt: UUID?
    @ObservationIgnored private var latestSaveRequest: UUID?
    private var presentedSaveAs: SaveAsRequest?
    @ObservationIgnored private(set) var saveAsPresentationID: UUID?
    @ObservationIgnored private var queuedSaveAs: SaveAsRequest?
    @ObservationIgnored private var nativeSaveDismissals: [UUID: DocumentSaveNativeDismissal] = [:]
    @ObservationIgnored var saveSheetParentIsClear: (() -> Bool)?
    @ObservationIgnored var saveSheetDetachmentObserver: ((UUID, @escaping @MainActor () -> Void) -> (() -> Void))?
    // The Shell binding's nil setter carries no request identity. Dismissal is
    // settled by SaveAsSheet.onDisappear with its captured ID, never this setter.
    var saveAsRequest: SaveAsRequest? {
        get { presentedSaveAs }
        set { /* Identity-bearing sheet callbacks own settlement. */ }
    }
    @ObservationIgnored var lastSaveFolder: URL?

    // Injected only by deterministic tests; production uses native prompts/backend.
    @ObservationIgnored var saveHasWindow: (() -> Bool)?
    @ObservationIgnored var saveFileExists: ((URL) -> Bool)?
    @ObservationIgnored var saveReplacePrompt: ((URL, @escaping @MainActor (Bool) -> Void) -> Void)?
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
        traceSaveLifecycle("operation.begin", id)
        guard !doc.isClosed else { completion(.failed("Document is closed")); return id }
        guard !saveOperations.values.contains(where: { $0.document === doc && $0.phase == .writing }) else {
            completion(.failed("A save for this document is still running")); return id
        }
        guard !saveOperations.values.contains(where: { $0.replaceAlert != nil }) else {
            completion(.failed("A replacement prompt is still open")); return id
        }
        let old = activeSavePrompt
        let operation = DocumentSaveOperation(id: id, document: doc, requiresWindow: requiresWindow, completion: completion)
        saveOperations[id] = operation
        latestSaveRequest = id
        activeSavePrompt = id
        if let old { cancelDocumentSave(old) }
        // Cancelling the old prompt may synchronously start another request.
        guard saveOperations[id] === operation, activeSavePrompt == id else { return id }
        let hasWindow = saveHasWindow?() ?? (window != nil)
        guard !requiresWindow || hasWindow else {
            settleDocumentSave(id, .failed("Save requires a document window")); return id
        }
        if let window {
            operation.presentingWindow = window
            operation.windowObserver = NotificationCenter.default.addObserver(
                forName: NSWindow.willCloseNotification, object: window, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.documentSaveWindowLost(id) }
            }
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

    private func traceSaveLifecycle(_ event: String, _ id: UUID) {
        guard DocumentSaveLifecycleTrace.enabled else { return }
        let state = nativeSaveDismissals[id]
        let probes = state?.activeProbes.map { $0.uuidString }.sorted().joined(separator: ",") ?? "nil"
        DocumentSaveLifecycleTrace.emit(event, id,
            "workspace=\(ObjectIdentifier(self)) claim=\(saveAsPresentationID?.uuidString ?? "nil") presented=\(presentedSaveAs?.id.uuidString ?? "nil") queued=\(queuedSaveAs?.id.uuidString ?? "nil") active=\(activeSavePrompt?.uuidString ?? "nil") phase=\(saveOperations[id].map { String(describing: $0.phase) } ?? "nil") state=\(state != nil) swift=\(state?.swiftDismissed ?? false) native=\(state?.nativeDetached ?? false) observed=\(state?.observedAttachment ?? false) probes=[\(probes)] parent={\(DocumentSaveLifecycleTrace.window(state?.parent))} sheet={\(DocumentSaveLifecycleTrace.window(state?.sheet))}")
    }

    func cancelDocumentSave(_ id: UUID) {
        traceSaveLifecycle("cancel.enter", id)
        defer { traceSaveLifecycle("cancel.exit", id) }
        guard let operation = saveOperations[id] else { return }
        if operation.phase == .writing {
            operation.continuationCancelled = true
        } else { settleDocumentSave(id, .cancelled) }
    }

    func saveAsSheetDidDisappear(_ id: UUID) {
        traceSaveLifecycle("swift.contentDisappear.enter", id)
        defer { traceSaveLifecycle("swift.contentDisappear.exit", id) }
        // Choosing -> replacing/writing hides the sheet deliberately.
        guard saveOperations[id]?.phase == .choosing else { return }
        cancelDocumentSave(id)
    }

    func documentSaveWindowLost(_ id: UUID) {
        traceSaveLifecycle("native.windowLost.enter", id)
        defer { traceSaveLifecycle("native.windowLost.exit", id) }
        if let operation = saveOperations[id] {
            if operation.phase == .writing { operation.continuationCancelled = true }
            else { settleDocumentSave(id, .failed("Document window closed before save")) }
        }
        if let state = nativeSaveDismissals.removeValue(forKey: id) {
            state.removeObservers?(); state.removeObservers = nil
            // A queued request on the closing parent cannot become a new sheet.
            if let next = queuedSaveAs, let operation = saveOperations[next.id],
               (state.parent != nil && operation.presentingWindow === state.parent)
                || saveHasWindow?() == false {
                settleDocumentSave(next.id, .failed("Document window closed before save"))
            }
            completeSaveAsPresentationDismissal(id)
        }
    }

    func captureSaveAsSheetWindow(_ id: UUID, window: NSWindow) {
        traceSaveLifecycle("probe.capture.enter", id)
        defer { traceSaveLifecycle("probe.capture.exit", id) }
        DocumentSaveLifecycleTrace.emit("probe.capture.window", id, DocumentSaveLifecycleTrace.window(window))
        // The logical operation may already have settled while this claimed
        // presentation is still appearing. Native ownership outlives that operation.
        guard saveAsPresentationID == id, let state = nativeSaveDismissals[id],
              !state.nativeDetached else { return }
        if let captured = state.sheet, captured !== window { return }
        state.sheet = window
        if state.parent?.attachedSheet === window { state.observedAttachment = true }
        saveOperations[id]?.saveSheetWindow = window
    }

    @discardableResult
    func saveAsProbeBegan(_ id: UUID, probe: UUID) -> Bool {
        traceSaveLifecycle("probe.leaseBegin.enter", id)
        defer { traceSaveLifecycle("probe.leaseBegin.exit", id) }
        DocumentSaveLifecycleTrace.emit("probe.leaseIdentity", id, "probe=\(probe.uuidString)")
        guard saveAsPresentationID == id, let state = nativeSaveDismissals[id],
              !state.swiftDismissed else { return false }
        state.activeProbes.insert(probe)
        return true
    }

    func saveAsProbeEnded(_ id: UUID, probe: UUID) {
        traceSaveLifecycle("probe.leaseEnd.enter", id)
        defer { traceSaveLifecycle("probe.leaseEnd.exit", id) }
        DocumentSaveLifecycleTrace.emit("probe.leaseIdentity", id, "probe=\(probe.uuidString)")
        guard let state = nativeSaveDismissals[id], state.activeProbes.remove(probe) != nil else { return }
        finishTerminalSavePresentationIfReady(id, state)
    }

    private func finishTerminalSavePresentationIfReady(_ id: UUID, _ state: DocumentSaveNativeDismissal) {
        traceSaveLifecycle("terminal.check.enter", id)
        defer { traceSaveLifecycle("terminal.check.exit", id) }
        // Dismantle is view teardown, not proof of native detachment. Join it
        // with dismissal of this generation AND the actual parent's clear slot.
        // A content claim that never materialized a probe has no view lease to drain.
        guard nativeSaveDismissals[id] === state, state.swiftDismissed,
              state.activeProbes.isEmpty else { return }
        let parentClear = saveSheetParentIsClear?()
            ?? state.parent.map { $0.attachedSheet == nil } ?? false
        guard parentClear else { return }
        state.nativeDetached = true
        finishNativeSaveDismissalIfReady(id, state)
    }

    func saveAsParentSheetDidEnd(_ id: UUID) {
        traceSaveLifecycle("native.didEnd.enter", id)
        defer { traceSaveLifecycle("native.didEnd.exit", id) }
        guard let state = nativeSaveDismissals[id] else { return }
        if state.observedAttachment, let parent = state.parent, let sheet = state.sheet,
           DocumentSaveSheetAttachment.hasDetached(capturedSheet: ObjectIdentifier(sheet),
                parentAttachedSheet: parent.attachedSheet.map { ObjectIdentifier($0) }) {
            state.nativeDetached = true
            finishNativeSaveDismissalIfReady(id, state)
        } else {
            // Also handles end-before-probe ordering. Teardown/dismissal may
            // arrive later and re-evaluate the parent without needing another event.
            finishTerminalSavePresentationIfReady(id, state)
        }
    }

    private func trackNativeSavePresentation(_ id: UUID) {
        traceSaveLifecycle("native.track.enter", id)
        defer { traceSaveLifecycle("native.track.exit", id) }
        guard nativeSaveDismissals[id] == nil else { return }
        let state = DocumentSaveNativeDismissal()
        state.parent = saveOperations[id]?.presentingWindow ?? window
        nativeSaveDismissals[id] = state
        let detached: @MainActor () -> Void = { [weak self, weak state] in
            guard let self, let state, self.nativeSaveDismissals[id] === state else { return }
            state.nativeDetached = true
            self.finishNativeSaveDismissalIfReady(id, state)
        }
        if let saveSheetDetachmentObserver {
            state.removeObservers = saveSheetDetachmentObserver(id, detached)
            return
        }
        // Missing parent/sheet is not proof of detachment. A later probe may
        // capture the appearing sheet, or explicit window loss releases the claim.
        guard let parent = state.parent else { return }
        // AppKit offers willBeginSheet, not a post-attachment begin event.
        // The sheet probe records attachment once the parent actually owns it;
        // didEnd and the terminal parent-clear check settle its release.
        let ended = NotificationCenter.default.addObserver(forName: NSWindow.didEndSheetNotification,
                                                           object: parent, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.saveAsParentSheetDidEnd(id) }
        }
        let closed = NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification,
                                                            object: parent, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.documentSaveWindowLost(id) }
        }
        let observers = DocumentSaveNativeObservers([ended, closed])
        state.removeObservers = { observers.remove() }
    }

    private func observeNativeSaveSheetDismissal(_ operation: DocumentSaveOperation) -> Bool {
        // Tracking starts at claim, before any cancellation can clear the item.
        // Reuse that ownership rather than replacing it at Save submission.
        guard let state = nativeSaveDismissals[operation.id] else { return false }
        if saveSheetDetachmentObserver != nil { return true }
        guard let parent = state.parent, let sheet = state.sheet,
              parent.attachedSheet === sheet else {
            settleDocumentSave(operation.id, .failed("Could not identify the active Save As sheet"))
            return false
        }
        state.observedAttachment = true
        return true
    }

    private func finishNativeSaveDismissalIfReady(_ id: UUID, _ state: DocumentSaveNativeDismissal) {
        traceSaveLifecycle("join.check.enter", id)
        defer { traceSaveLifecycle("join.check.exit", id) }
        guard nativeSaveDismissals[id] === state, state.nativeDetached, state.swiftDismissed else { return }
        nativeSaveDismissals.removeValue(forKey: id)
        state.removeObservers?(); state.removeObservers = nil
        completeSaveAsPresentationDismissal(id, nativeParent: state.parent)
    }

    private func presentDocumentSaveSheet(_ request: SaveAsRequest) {
        traceSaveLifecycle("successor.request.enter", request.id)
        defer { traceSaveLifecycle("successor.request.exit", request.id) }
        if saveAsPresentationID != nil {
            queuedSaveAs = request
            presentedSaveAs = nil
        } else {
            // A requested item is not yet a native presentation. Only the sheet
            // content boundary may claim the dismissal barrier below.
            presentedSaveAs = request
        }
    }

    /// Called when SwiftUI consumes a captured sheet item, before onAppear.
    /// Idempotent content evaluations share one presentation claim. A late claim
    /// for a cancelled item must drain before a newer requested sheet can appear.
    @discardableResult
    func saveAsPresentationWillPresent(_ id: UUID) -> Bool {
        traceSaveLifecycle("swift.claim.enter", id)
        defer { traceSaveLifecycle("swift.claim.exit", id) }
        if let current = saveAsPresentationID { return current == id }
        saveAsPresentationID = id
        trackNativeSavePresentation(id)
        if presentedSaveAs?.id != id {
            if let requested = presentedSaveAs { queuedSaveAs = requested }
            presentedSaveAs = nil
        }
        return true
    }

    /// Native SwiftUI sheet completion, not content onDisappear. ID remains owned
    /// until this boundary so stale nil binding writes cannot dismiss a successor.
    func saveAsPresentationDidDismiss(_ id: UUID) {
        traceSaveLifecycle("swift.dismiss.enter", id)
        defer { traceSaveLifecycle("swift.dismiss.exit", id) }
        guard saveAsPresentationID == id else { return }
        if let state = nativeSaveDismissals[id] {
            state.swiftDismissed = true // Seals this generation against new view leases.
            finishTerminalSavePresentationIfReady(id, state)
            finishNativeSaveDismissalIfReady(id, state)
            return
        }
        completeSaveAsPresentationDismissal(id)
    }

    private func completeSaveAsPresentationDismissal(_ id: UUID, nativeParent: NSWindow? = nil) {
        traceSaveLifecycle("presentation.complete.enter", id)
        defer { traceSaveLifecycle("presentation.complete.exit", id) }
        guard saveAsPresentationID == id else { return }
        saveAsPresentationID = nil
        if presentedSaveAs?.id == id { presentedSaveAs = nil }
        if let operation = saveOperations[id] {
            if operation.phase == .waitingForDismissal, let request = operation.pendingReplacement {
                operation.pendingReplacement = nil
                beginReplacementPrompt(operation, request: request)
            } else if operation.phase == .choosing { cancelDocumentSave(id) }
        }
        if let request = queuedSaveAs {
            queuedSaveAs = nil
            if saveOperations[request.id]?.phase == .choosing, activeSavePrompt == request.id {
                guard nativeParent?.attachedSheet == nil else {
                    settleDocumentSave(request.id, .failed("Another sheet is still attached to the document window"))
                    return
                }
                presentDocumentSaveSheet(request)
            }
        }
    }

    func finishSaveAs(_ request: SaveAsRequest) {
        guard let operation = saveOperations[request.id], operation.document === request.doc,
              operation.phase == .choosing, activeSavePrompt == request.id else { return }
        guard request.isValid else { settleDocumentSave(request.id, .failed("Invalid file name")); return }
        guard saveAsPresentationID == request.id else {
            settleDocumentSave(request.id, .failed("Save As presentation is not active")); return
        }
        let exists = saveFileExists?(request.url) ?? FileManager.default.fileExists(atPath: request.url.path)
        if exists {
            operation.phase = .waitingForDismissal
            operation.pendingReplacement = request
            guard observeNativeSaveSheetDismissal(operation) else { return }
            if presentedSaveAs?.id == request.id { presentedSaveAs = nil }
            // Only saveAsPresentationDidDismiss may admit a replacement prompt.
        } else { admitDocumentWrite(operation, url: request.url, folder: request.folder) }
    }

    private func beginReplacementPrompt(_ operation: DocumentSaveOperation, request: SaveAsRequest) {
        guard saveOperations[request.id] === operation, operation.document === request.doc,
              operation.phase == .waitingForDismissal, activeSavePrompt == request.id else { return }
        operation.phase = .replacing
        let proceed: @MainActor (Bool) -> Void = { [weak self] accepted in
            guard let self, self.saveOperations[request.id] === operation,
                  operation.phase == .replacing else { return }
            operation.replaceAlert = nil
            guard accepted else { self.settleDocumentSave(request.id, .cancelled); return }
            self.admitDocumentWrite(operation, url: request.url, folder: request.folder)
        }
        if let saveReplacePrompt { saveReplacePrompt(request.url, proceed); return }
        guard let window = operation.presentingWindow else {
            settleDocumentSave(request.id, .failed("Document window closed before save")); return
        }
        guard window.attachedSheet == nil else {
            settleDocumentSave(request.id, .failed("Another sheet is still attached to the document window")); return
        }
        let alert = NSAlert()
        operation.replaceAlert = alert
        alert.messageText = "Replace “\(request.url.lastPathComponent)”?"
        alert.informativeText = "A file already exists at this destination. Replacing it will overwrite its contents."
        alert.addButton(withTitle: "Replace")
        alert.addButton(withTitle: "Cancel")
        alert.beginSheetModal(for: window) { response in
            MainActor.assumeIsolated { proceed(response == .alertFirstButtonReturn) }
        }
    }

    private func admitDocumentWrite(_ operation: DocumentSaveOperation, url: URL?, folder: URL?) {
        guard saveOperations[operation.id] === operation, operation.phase != .writing else { return }
        guard !operation.document.isClosed else { settleDocumentSave(operation.id, .failed("Document is closed")); return }
        if operation.requiresWindow, !(saveHasWindow?() ?? (window != nil)) {
            settleDocumentSave(operation.id, .failed("Document window closed before save")); return
        }
        operation.phase = .writing
        if activeSavePrompt == operation.id { activeSavePrompt = nil }
        if presentedSaveAs?.id == operation.id { presentedSaveAs = nil }
        // Strong self/operation ownership lasts until an admitted writer settles.
        let done: @MainActor (Result<Void, Error>) -> Void = { [self, operation] result in
            guard saveOperations[operation.id] === operation, operation.phase == .writing else { return }
            switch result {
            case .success:
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
        if let saveWriter { saveWriter(operation.document, url, done) }
        else {
            done(Result {
                if let url { try operation.document.backend.saveAs(path: url.path) }
                else { try operation.document.backend.save() }
            })
        }
    }

    private func settleDocumentSave(_ id: UUID, _ outcome: DocumentSaveOutcome) {
        traceSaveLifecycle("operation.settle.enter", id)
        defer { traceSaveLifecycle("operation.settle.exit", id) }
        guard let operation = saveOperations.removeValue(forKey: id) else { return }
        if let observer = operation.windowObserver { NotificationCenter.default.removeObserver(observer) }
        if let alert = operation.replaceAlert, let parent = alert.window.sheetParent {
            parent.endSheet(alert.window, returnCode: .cancel)
        }
        operation.replaceAlert = nil
        operation.pendingReplacement = nil
        if queuedSaveAs?.id == id { queuedSaveAs = nil }
        if activeSavePrompt == id { activeSavePrompt = nil }
        if presentedSaveAs?.id == id { presentedSaveAs = nil }
        let completion = operation.completion
        operation.completion = nil // Latch before reentrant observers.
        if latestSaveRequest == id, case .failed(let message) = outcome { say("Save: \(message)") }
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

    /// Export Flat of `doc` to `url` with `s`.
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
            self?.exportFlat(doc, s, to: url)
        }
        if let window = self.window {
            panel.beginSheetModal(for: window) { r in MainActor.assumeIsolated { handle(r) } }
        } else {
            handle(panel.runModal())
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
