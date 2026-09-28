import AppKit
import SwiftUI
import TesseraCore

/// File ▸ New Document…: size, depth and colour profile.
struct NewDocumentSheet: View {
    @Bindable var workspace: DocumentWorkspace
    @Environment(\.dismiss) private var dismiss
    @State private var settings = NewDocumentSettings()

    var body: some View {
        SheetScaffold(title: "New Document", subtitle: "A layered document") {
            EmptyView()
        } content: {
            Form {
                Picker("Preset", selection: Binding(get: { "" }, set: { name in
                    if let p = NewDocumentSettings.presets.first(where: { $0.0 == name }) { settings.width = p.1; settings.height = p.2 }
                })) {
                    Text("Custom").tag("")
                    ForEach(NewDocumentSettings.presets, id: \.0) { p in Text(p.0).tag(p.0) }
                }
                .accessibilityIdentifier("document.new.preset")
                TextField("Width (px)", value: $settings.width, format: .number)
                    .accessibilityIdentifier("document.new.width")
                TextField("Height (px)", value: $settings.height, format: .number)
                    .accessibilityIdentifier("document.new.height")
                LabeledContent("Bit depth") {
                    SegmentedPicker(selection: $settings.depth, segments: DocBitDepth.allCases.map { .init(value: $0, title: $0.title) },
                                    fill: false)
                    .fixedSize()
                    .accessibilityIdentifier("document.new.depth")
                }
                Picker("Colour profile", selection: $settings.profile) {
                    ForEach(NewDocumentSettings.profiles, id: \.self) { Text($0).tag($0) }
                }
                .accessibilityIdentifier("document.new.profile")
                if !settings.isValid {
                    StatusLine(text: "Width and height must be 1 to 30,000 pixels", kind: .error)
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        } leading: {
            Text(workspace.engine is StubDocumentEngine ? "Stub backend: the document opens with sample layers" : "")
        } actions: {
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).sheetButton()
            Button("Create") {
                workspace.newSettings = settings
                dismiss()
                workspace.newDocument(settings)
            }
            .keyboardShortcut(.defaultAction)
            .sheetButton(primary: true)
            .disabled(!settings.isValid)
            .accessibilityIdentifier("document.new.create")
        }
        .frame(width: 460, height: 400)
        .onAppear { settings = workspace.newSettings }
    }
}

/// File ▸ Export Flat…: format, quality and colour space (the export sheet's controls).
struct ExportFlatSheet: View {
    @Bindable var workspace: DocumentWorkspace
    @Environment(\.dismiss) private var dismiss
    @State private var settings = ExportFlatSettings()

    var body: some View {
        SheetScaffold(title: "Export Flat", subtitle: workspace.current?.title) {
            EmptyView()
        } content: {
            Form {
                LabeledContent("Format") {
                    SegmentedPicker(selection: $settings.format,
                                    segments: ExportSettings.FileFormat.allCases.map { .init(value: $0, title: $0.title) }, fill: false)
                    .fixedSize()
                    .accessibilityIdentifier("document.export.format")
                }
                if settings.format == .jpeg {
                    LabeledContent("Quality") {
                        HStack {
                            Slider(value: Binding(get: { Double(settings.quality) }, set: { settings.quality = Int($0.rounded()) }),
                                   in: 1...100)
                            Text("\(settings.quality)").font(Theme.Fonts.labelNumeric)
                                .frame(width: Theme.Height.large, alignment: .trailing)
                        }
                    }
                    .accessibilityIdentifier("document.export.quality")
                }
                Picker("Colour space", selection: $settings.color) {
                    ForEach(ExportSettings.ColorSpace.allCases) { Text($0.title).tag($0) }
                }
                .accessibilityIdentifier("document.export.color")
                Hint("The visible layers are flattened. JPEG has no transparency: transparent areas become white.")
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).sheetButton()
            Button("Export…") {
                dismiss()
                let s = settings
                DispatchQueue.main.async { MainActor.assumeIsolated { workspace.exportFlat(s) } }
            }
            .keyboardShortcut(.defaultAction)
            .sheetButton(primary: true)
            .disabled(workspace.current == nil)
            .accessibilityIdentifier("document.export.start")
        }
        .frame(width: 460, height: 320)
        .onAppear { settings = workspace.exportSettings }
    }
}

/// One Save As in progress: the document, the file name and folder the sheet edits.
struct SaveAsRequest: Identifiable {
    enum Format: String, CaseIterable, Identifiable {
        case tessera = "tessera-doc", psd, psb
        var id: String { rawValue }
        var title: String {
            switch self {
            case .tessera: "Tessera Document"
            case .psd: "Photoshop (PSD)"
            case .psb: "Large Document (PSB)"
            }
        }
    }

    var id = UUID()
    let doc: DocumentController
    var name: String
    var folder: URL

    /// The format the name's extension asks for (`.tessera-doc` when it has none).
    var format: Format { Self.format(of: name) }
    /// The file name with an extension: one of ours is kept, anything else gets `.tessera-doc`.
    var fileName: String { Self.fileName(name) }
    var url: URL { folder.appendingPathComponent(fileName) }
    var isValid: Bool {
        let stem = (fileName as NSString).deletingPathExtension.trimmingCharacters(in: .whitespaces)
        return !stem.isEmpty && !fileName.contains("/") && !fileName.hasPrefix(".")
    }

    static func format(of name: String) -> Format {
        Format(rawValue: (name as NSString).pathExtension.lowercased()) ?? .tessera
    }

    static func fileName(_ name: String) -> String {
        let n = name.trimmingCharacters(in: .whitespaces)
        return Format(rawValue: (n as NSString).pathExtension.lowercased()) != nil ? n : n + ".tessera-doc"
    }

    /// `<title stem>.<ext>`: the current file's extension for a PSD / PSB, else `.tessera-doc`.
    static func defaultName(_ title: String, path: String?) -> String {
        let ext = path.map { ($0 as NSString).pathExtension.lowercased() } ?? ""
        let format = Format(rawValue: ext) ?? .tessera
        return (title as NSString).deletingPathExtension + "." + format.rawValue
    }

    /// `name` with its extension switched to `format`.
    static func renamed(_ name: String, to format: Format) -> String {
        let n = name.trimmingCharacters(in: .whitespaces)
        let stem = Format(rawValue: (n as NSString).pathExtension.lowercased()) != nil ? (n as NSString).deletingPathExtension : n
        return stem + "." + format.rawValue
    }
}

/// Captures the actual window hosting this Save As content, not an arbitrary
/// attached sheet discovered later from the application's current main window.
struct DocumentSaveSheetWindowProbe: NSViewRepresentable {
    let workspace: DocumentWorkspace
    let requestID: UUID
    // Internal reporting seam for a reused, already-attached view regression.
    var reportWindow: @MainActor (DocumentWorkspace, UUID, NSWindow) -> Void = { owner, id, window in
        owner.captureSaveAsSheetWindow(id, window: window)
    }
    final class ProbeView: NSView {
        var capture: ((NSWindow) -> Void)?
        var ownerIdentity: ObjectIdentifier?
        var requestIdentity: UUID?
        var finishOwnership: (() -> Void)?
        func endOwnership() {
            capture = nil
            let finish = finishOwnership
            finishOwnership = nil
            ownerIdentity = nil; requestIdentity = nil
            finish?()
        }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { capture?(window) }
        }
    }
    func makeNSView(context: Context) -> ProbeView {
        let view = ProbeView()
        refreshCapture(on: view)
        return view
    }
    func updateNSView(_ view: ProbeView, context: Context) {
        DocumentSaveAsTrace.emit("probe.update", requestID, DocumentSaveAsTrace.window(view.window))
        refreshCapture(on: view)
    }
    static func dismantleNSView(_ view: ProbeView, coordinator: ()) {
        if let window = view.window { view.capture?(window) }
        view.endOwnership()
    }
    func refreshCapture(on view: ProbeView) {
        DocumentSaveAsTrace.emit("probe.refresh", requestID, DocumentSaveAsTrace.window(view.window))
        let id = requestID
        if view.ownerIdentity != ObjectIdentifier(workspace) || view.requestIdentity != id {
            view.endOwnership()
            view.ownerIdentity = ObjectIdentifier(workspace)
            view.requestIdentity = id
            let probe = UUID()
            if workspace.saveAsProbeBegan(id, probe: probe) {
                view.finishOwnership = { [weak workspace] in workspace?.saveAsProbeEnded(id, probe: probe) }
            }
        }
        let report = reportWindow
        view.capture = { [weak workspace] window in
            DocumentSaveAsTrace.emit("probe.windowCallback", id, "owner=\(workspace != nil) \(DocumentSaveAsTrace.window(window))")
            guard let workspace else { return }
            report(workspace, id, window)
        }
        // Reuse may not produce another viewDidMoveToWindow callback.
        if let window = view.window { view.capture?(window) }
    }
}

/// Non-observable identity latch: capturing sheet content must not rely on an
/// onAppear state update, which may never occur after a synchronous cancellation.
@MainActor
private final class DocumentSavePresentationCapture {
    var id: UUID?
    func claim(_ request: SaveAsRequest, in workspace: DocumentWorkspace) -> Bool {
        DocumentSaveAsTrace.emit("swift.contentClaim", request.id, "captureExists=\(id != nil) matching=\(id == request.id)")
        if let id, id != request.id { return false }
        guard workspace.saveAsPresentationWillPresent(request.id) else { return false }
        id = request.id
        return true
    }
}

/// Keeps native dismissal paired with the item SwiftUI actually consumed.
struct DocumentSaveAsPresentation: ViewModifier {
    @Bindable var workspace: DocumentWorkspace
    @State private var capture = DocumentSavePresentationCapture()

    func body(content: Content) -> some View {
        content.sheet(item: Binding(get: { workspace.saveAsRequest }, set: { _ in }), onDismiss: {
            DocumentSaveAsTrace.emit("swift.onDismiss.callback", capture.id)
            guard let id = capture.id else { return }
            capture.id = nil
            workspace.saveAsPresentationDidDismiss(id)
        }) { request in
            // Capture synchronously at the content boundary, including the
            // appearing-but-not-yet-onAppear interval. Merely setting the item
            // in the workspace does not claim this presentation identity.
            if capture.claim(request, in: workspace) {
                SaveAsSheet(workspace: workspace, request: request)
                    .background(DocumentSaveSheetWindowProbe(workspace: workspace, requestID: request.id)
                        .frame(width: 0, height: 0))
            }
        }
    }
}

/// File ▸ Save As…: name (focused on open, `document.saveAs.name`), format and folder.
struct SaveAsSheet: View {
    @Bindable var workspace: DocumentWorkspace
    @State var request: SaveAsRequest
    @FocusState private var nameFocused: Bool

    var body: some View {
        SheetScaffold(title: "Save As", subtitle: request.doc.title) {
            EmptyView()
        } content: {
            Form {
                TextField("Name", text: $request.name)
                    .focused($nameFocused)
                    .onSubmit { if request.isValid { save() } }
                    .accessibilityIdentifier("document.saveAs.name")
                Picker("Format", selection: Binding(get: { request.format },
                                                    set: { request.name = SaveAsRequest.renamed(request.name, to: $0) })) {
                    ForEach(SaveAsRequest.Format.allCases) { Text($0.title).tag($0) }
                }
                .accessibilityIdentifier("document.saveAs.format")
                LabeledContent("Where") {
                    HStack(spacing: Theme.Space.s) {
                        Text(request.folder.path)
                            .font(Theme.Fonts.caption)
                            .foregroundStyle(Theme.textSecondary)
                            .lineLimit(1).truncationMode(.head)
                            .help(request.folder.path)
                            .accessibilityIdentifier("document.saveAs.folder")
                        Button("Choose…") { chooseFolder() }
                            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                            .accessibilityIdentifier("document.saveAs.choose")
                    }
                }
                if request.format != .tessera {
                    Hint("PSD and PSB keep layers, masks and adjustment layers; fill layers and native-only adjustments need a Tessera document.")
                }
                if !request.isValid {
                    StatusLine(text: "Enter a file name", kind: .error)
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { workspace.cancelDocumentSave(request.id) }
                .keyboardShortcut(.cancelAction).sheetButton()
                .accessibilityIdentifier("document.saveAs.cancel")
            Button("Save") { save() }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .disabled(!request.isValid)
                .accessibilityIdentifier("document.saveAs.save")
        }
        .frame(width: 520, height: 330)
        .onDisappear { workspace.saveAsSheetDidDisappear(request.id) }
        .onAppear {
            // The field takes the keyboard as the sheet opens (after SwiftUI installs it).
            DispatchQueue.main.async { MainActor.assumeIsolated { nameFocused = true } }
        }
    }

    private func save() {
        guard request.isValid else { return }
        workspace.finishSaveAs(request)
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.title = "Choose a Folder"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = request.folder
        panel.prompt = "Choose"
        if panel.runModal() == .OK, let url = panel.url { request.folder = url }
    }
}
