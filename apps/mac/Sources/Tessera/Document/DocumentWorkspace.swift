import AppKit
import Observation
import SwiftUI
import TesseraCore
import UniformTypeIdentifiers

/// Document mode's open documents (WP M5-10): the tab switcher, New / Open / Edit in Layers,
/// Save / Save As / Export Flat, close with a save prompt, panels (Tab) and screen modes (F).
@MainActor @Observable
final class DocumentWorkspace {
    /// The backend factory. M5-10b replaces the stub with the engine's `DocumentEngine` adapter.
    @ObservationIgnored var engine: any DocumentEngine = StubDocumentEngine.shared
    @ObservationIgnored weak var app: AppModel?

    private(set) var documents: [DocumentController] = []
    private(set) var current: DocumentController?
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
        current = doc
        app?.viewMode = .document
    }

    func newDocument(_ s: NewDocumentSettings) {
        do {
            try install(engine.newDocument(width: UInt32(s.width), height: UInt32(s.height), depth: s.depth, profile: s.profile))
            say("New document \(s.width) × \(s.height) px, \(s.depth.title), \(s.profile)"
                + (engine is StubDocumentEngine ? " (stub backend: sample layers)" : ""))
        } catch {
            say("New document: \(error.localizedDescription)")
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
        do {
            try install(engine.openDocument(path: url.path))
            say("Opened \(url.lastPathComponent)")
        } catch {
            say("Open \(url.lastPathComponent): \(error.localizedDescription)")
        }
    }

    /// Library ▸ Edit in Layers (⌘E): the focused image, developed, as a new document.
    func editInLayers(_ item: PhotoItem?) {
        guard let item else { say("Edit in Layers: select a photo first"); return }
        let id: String? = engine is StubDocumentEngine ? item.url.map { "file:\($0.path)" } : item.engineImage?.imageID
        guard let id else { say("Edit in Layers needs a photo file (stub items have none)"); return }
        do {
            try install(engine.openDocumentFromImage(imageId: id, developed: true))
            say("Editing \(item.name) in layers")
        } catch {
            say("Edit in Layers: \(error.localizedDescription)")
        }
    }

    // MARK: Saving

    /// Save; a document without a .tessera-doc path asks where (Save As). `then` runs after a
    /// successful save.
    func save(_ doc: DocumentController? = nil, then: (@MainActor () -> Void)? = nil) {
        guard let doc = doc ?? current else { return }
        if let path = doc.info.path, path.hasSuffix(".tessera-doc") {
            do {
                try doc.backend.save()
                doc.reloadHistory()
                say("Saved \(doc.title)")
                then?()
            } catch { say("Save: \(error.localizedDescription)") }
        } else {
            saveAs(doc, then: then)
        }
    }

    func saveAs(_ doc: DocumentController? = nil, then: (@MainActor () -> Void)? = nil) {
        guard let doc = doc ?? current else { return }
        let panel = NSSavePanel()
        panel.title = "Save As"
        let native = Self.documentTypes[0]
        let psd = UTType(filenameExtension: "psd") ?? .data
        panel.allowedContentTypes = [native, psd, UTType(filenameExtension: "psb") ?? .data]
        panel.allowsOtherFileTypes = false
        let stem = (doc.title as NSString).deletingPathExtension
        panel.nameFieldStringValue = stem + ".tessera-doc"
        let handle: @MainActor (NSApplication.ModalResponse) -> Void = { [weak self] r in
            guard r == .OK, let url = panel.url else { return }
            do {
                try doc.backend.saveAs(path: url.path)
                doc.reloadModel()
                doc.reloadHistory()
                self?.say("Saved \(url.lastPathComponent)")
                then?()
            } catch { self?.say("Save As: \(error.localizedDescription)") }
        }
        if let window = self.window {
            panel.beginSheetModal(for: window) { r in MainActor.assumeIsolated { handle(r) } }
        } else {
            handle(panel.runModal())
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
            do {
                try doc.backend.exportFlat(path: url.path, format: s.format.documentFormat, quality: UInt8(s.quality),
                                           color: s.color.documentColor)
                self?.say("Exported \(url.lastPathComponent) (\(s.format.title), \(s.color.title))")
            } catch { self?.say("Export Flat: \(error.localizedDescription)") }
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

    private func discard(_ doc: DocumentController) {
        guard let i = documents.firstIndex(where: { $0 === doc }) else { return }
        documents.remove(at: i)
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
