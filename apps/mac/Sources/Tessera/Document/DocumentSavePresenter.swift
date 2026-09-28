import AppKit
import SwiftUI

/// Child sheetParent metadata may lag native detachment. Membership is read
/// from the captured parent and includes queued sheets, never just the top sheet.
enum DocumentSaveSheetAttachment {
    static func hasDetached(capturedSheet: ObjectIdentifier, parentAttachedSheet: ObjectIdentifier?,
                            parentSheets: [ObjectIdentifier]) -> Bool {
        parentAttachedSheet != capturedSheet && !parentSheets.contains(capturedSheet)
    }
}

struct DocumentSavePresentationToken: Equatable {
    let requestID: UUID
    let presentationID = UUID()
}
struct DocumentSaveHostIdentity: Equatable {
    let bindingID: UUID
    let windowID: ObjectIdentifier
}
enum DocumentSavePresentationContent {
    case form(SaveAsRequest)
    case replacement(SaveAsRequest)
    case test
}
@MainActor
struct DocumentSavePresentationActions {
    var cancel: () -> Void
    var submit: (SaveAsRequest) -> Void
    var chooseFolder: (URL, @escaping (URL?) -> Void) -> Void
    static var inert: Self { .init(cancel: {}, submit: { _ in }, chooseFolder: { _, done in done(nil) }) }
}
enum DocumentSavePresentationEvent {
    case drained(DocumentSavePresentationToken, response: Int)
    case failed(DocumentSavePresentationToken, message: String)
    case hostLost(DocumentSavePresentationToken)
}

/// Native driver contract: completion belongs to THIS begin invocation; changed
/// is only a prompt to re-read membership, never a synthetic completion. A driver
/// must report queued membership as well as the parent's currently attached sheet.
@MainActor
protocol DocumentSaveNativeSession: AnyObject {
    var canBegin: Bool { get }
    var containsSheet: Bool { get }
    var childActive: Bool { get }
    func begin(completed: @escaping (Int) -> Void, changed: @escaping () -> Void,
               parentClosed: @escaping () -> Void)
    func end(response: Int)
    func cancelChild()
    func chooseFolder(_ folder: URL, completion: @escaping (URL?) -> Void)
    func retire()
}

/// Ordinary MainActor state, deliberately not Observable. Native callbacks own
/// the bounded drain context until their exact invocation is complete and clear.
@MainActor
final class DocumentSavePresenter {
    typealias Factory = (DocumentSavePresentationToken, DocumentSavePresentationContent,
                         DocumentSavePresentationActions) -> (any DocumentSaveNativeSession)?
    private final class Entry {
        let token: DocumentSavePresentationToken
        let host: DocumentSaveHostIdentity
        var events: ((DocumentSavePresentationEvent) -> Void)?
        var session: (any DocumentSaveNativeSession)?
        var beginInFlight = false, began = false, endRequested = false, endIssued = false
        var childCancelIssued = false, lost = false
        var response: Int?
        init(_ token: DocumentSavePresentationToken, host: DocumentSaveHostIdentity,
             events: @escaping (DocumentSavePresentationEvent) -> Void) {
            self.token = token; self.host = host; self.events = events
        }
    }
    var onHostLost: ((DocumentSaveHostIdentity) -> Void)?
    private var bindingID: UUID?
    private(set) var host: DocumentSaveHostIdentity?
    private var factory: Factory?
    private var active: Entry?
    var isBusy: Bool { active != nil }

    func registerBinding(_ id: UUID) {
        guard bindingID != id else { return }
        let oldHost = host, old = active
        bindingID = id; host = nil; factory = nil
        if let oldHost { onHostLost?(oldHost) }
        if let old { lose(old) }
    }
    func updateBinding(_ id: UUID, windowID: ObjectIdentifier, factory: @escaping Factory) {
        guard bindingID == id else { return }
        let newHost = DocumentSaveHostIdentity(bindingID: id, windowID: windowID)
        if let oldHost = host, oldHost != newHost {
            clearWindow(id)
        }
        guard bindingID == id else { return }
        host = newHost; self.factory = factory
    }
    func clearWindow(_ id: UUID) {
        guard bindingID == id else { return }
        let oldHost = host
        host = nil; factory = nil
        if let oldHost { onHostLost?(oldHost) }
        if let active, active.host.bindingID == id { lose(active) }
    }
    func removeBinding(_ id: UUID, windowID: ObjectIdentifier? = nil) {
        guard bindingID == id else { return }
        if let windowID, host?.windowID != windowID { return }
        let oldHost = host
        bindingID = nil; host = nil; factory = nil
        if let oldHost { onHostLost?(oldHost) }
        if let active, active.host.bindingID == id { lose(active) }
    }
    func present(_ token: DocumentSavePresentationToken, content: DocumentSavePresentationContent,
                 actions: DocumentSavePresentationActions,
                 events: @escaping (DocumentSavePresentationEvent) -> Void) {
        guard active == nil, let host, let factory else {
            events(.failed(token, message: "Save requires an available document window")); return
        }
        let entry = Entry(token, host: host, events: events)
        active = entry // Latch before factory or AppKit can reenter.
        let session = factory(token, content, actions)
        entry.session = session
        guard active === entry else { session?.retire(); return }
        if entry.endRequested { retire(entry, event: .drained(token, response: NSApplication.ModalResponse.cancel.rawValue)); return }
        guard let session, session.canBegin else {
            retire(entry, event: .failed(token, message: "Another sheet is still attached to the document window")); return
        }
        entry.beginInFlight = true; entry.began = true
        // Strong drain ownership is intentional. retire clears native callbacks,
        // hosted action owners are weak, and the bridge explicitly shuts down.
        session.begin(completed: { [self, entry] response in
            guard active === entry, entry.response == nil else { return }
            entry.response = response; advance(entry)
        }, changed: { [self, entry] in advance(entry) }, parentClosed: { [self, entry] in
            guard active === entry else { return }
            if self.host == entry.host { self.removeBinding(entry.host.bindingID) }
            lose(entry)
        })
        entry.beginInFlight = false
        advance(entry)
    }
    func end(_ token: DocumentSavePresentationToken) {
        guard let active, active.token == token else { return }
        active.endRequested = true
        advance(active)
    }
    func chooseFolder(_ token: DocumentSavePresentationToken, folder: URL,
                      completion: @escaping (URL?) -> Void) {
        guard let entry = active, entry.token == token, !entry.endRequested,
              entry.response == nil, let session = entry.session, !session.childActive else { return }
        session.chooseFolder(folder) { [weak self, weak entry] url in
            guard let self, let entry, self.active === entry,
                  !entry.endRequested, entry.response == nil else { return }
            completion(url)
        }
        advance(entry)
    }
    private func lose(_ entry: Entry) {
        guard active === entry else { return }
        entry.endRequested = true
        if !entry.lost {
            entry.lost = true
            entry.events?(.hostLost(entry.token))
        }
        advance(entry)
    }
    private func advance(_ entry: Entry) {
        guard active === entry, !entry.beginInFlight, let session = entry.session else { return }
        if entry.endRequested, entry.began {
            if session.childActive {
                if !entry.childCancelIssued {
                    entry.childCancelIssued = true
                    session.cancelChild()
                }
                return // Child return emits changed; nested runModal must unwind first.
            }
            if entry.response == nil, !entry.endIssued, session.containsSheet {
                entry.endIssued = true
                session.end(response: NSApplication.ModalResponse.cancel.rawValue)
            }
        }
        guard active === entry, let response = entry.response,
              !session.childActive, !session.containsSheet else { return }
        retire(entry, event: .drained(entry.token, response: response))
    }
    private func retire(_ entry: Entry, event: DocumentSavePresentationEvent) {
        guard active === entry else { return }
        let events = entry.events
        entry.events = nil; active = nil
        let session = entry.session; entry.session = nil
        session?.retire() // Release native windows/observers before outward reentry.
        events?(event)
    }
}

/// Captures the exact parent and freshly-created sheet before begin. No native
/// mutation ever uses a lookup of whatever happens to be parent's attachedSheet.
@MainActor
final class AppKitDocumentSaveSession: DocumentSaveNativeSession {
    private var parent: NSWindow?
    private var sheet: NSWindow?
    private var alert: NSAlert?
    private var observers: [NSObjectProtocol] = []
    private var child: NSOpenPanel?
    private var changed: (() -> Void)?
    private var completed: ((Int) -> Void)?
    private var closed: (() -> Void)?
    private var hasBegun = false
    private var isRetired = false
    var childActive: Bool { child != nil }
    var containsSheet: Bool {
        guard let parent, let sheet else { return false }
        return !DocumentSaveSheetAttachment.hasDetached(capturedSheet: ObjectIdentifier(sheet),
            parentAttachedSheet: parent.attachedSheet.map { ObjectIdentifier($0) },
            parentSheets: parent.sheets.map { ObjectIdentifier($0) })
    }
    var canBegin: Bool { parent.map { $0.isVisible && $0.attachedSheet == nil && $0.sheets.isEmpty } ?? false }
    init?(parent: NSWindow, content: DocumentSavePresentationContent, actions: DocumentSavePresentationActions) {
        self.parent = parent
        switch content {
        case .form(let request):
            let controller = NSHostingController(rootView: SaveAsSheet(request: request, actions: actions))
            let window = NSWindow(contentViewController: controller)
            window.styleMask = [.titled]
            window.title = "Save As"
            window.setContentSize(NSSize(width: 520, height: 330))
            window.isReleasedWhenClosed = false
            sheet = window
        case .replacement(let request):
            let alert = NSAlert()
            alert.messageText = "Replace “\(request.url.lastPathComponent)”?"
            alert.informativeText = "A file already exists at this destination. Replacing it will overwrite its contents."
            alert.addButton(withTitle: "Replace")
            alert.addButton(withTitle: "Cancel").keyEquivalent = "\u{1b}"
            self.alert = alert; sheet = alert.window
        case .test: return nil
        }
    }
    func begin(completed: @escaping (Int) -> Void, changed: @escaping () -> Void,
               parentClosed: @escaping () -> Void) {
        guard !hasBegun, !isRetired, let parent, let sheet else { return }
        hasBegun = true; self.completed = completed; self.changed = changed; closed = parentClosed
        observers.append(NotificationCenter.default.addObserver(forName: NSWindow.didEndSheetNotification,
            object: parent, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.changed?() }
            })
        observers.append(NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification,
            object: parent, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.closed?() }
            })
        let finish: (NSApplication.ModalResponse) -> Void = { [self] response in
            // endSheet may invoke before or after didEndSheet. Neither event alone
            // proves both invocation completion and physical membership clearance.
            MainActor.assumeIsolated {
                sheet.orderOut(nil)
                self.completed?(response.rawValue)
                self.changed?()
            }
        }
        if let alert { alert.beginSheetModal(for: parent, completionHandler: finish) }
        else { parent.beginSheet(sheet, completionHandler: finish) }
    }
    func end(response: Int) {
        guard !isRetired, !childActive, let parent, let sheet, containsSheet else { return }
        // sheets includes queued entries. Whether AppKit completes a queued-only
        // endSheet is an explicit native acceptance gate; we never forge completion.
        parent.endSheet(sheet, returnCode: NSApplication.ModalResponse(rawValue: response))
    }
    func cancelChild() { child?.cancel(nil) }
    func chooseFolder(_ folder: URL, completion: @escaping (URL?) -> Void) {
        guard !isRetired, child == nil else { return }
        let panel = NSOpenPanel()
        panel.title = "Choose a Folder"; panel.canChooseDirectories = true
        panel.canChooseFiles = false; panel.canCreateDirectories = true
        panel.directoryURL = folder; panel.prompt = "Choose"
        child = panel
        let response = panel.runModal()
        child = nil // Only the actual nested return permits ending the parent.
        let url = response == .OK ? panel.url : nil
        changed?()
        completion(url)
    }
    func retire() {
        guard !isRetired else { return }
        isRetired = true
        observers.forEach { NotificationCenter.default.removeObserver($0) }; observers.removeAll()
        changed = nil; completed = nil; closed = nil
        sheet?.contentViewController = nil
        alert = nil; sheet = nil; parent = nil
    }
}
