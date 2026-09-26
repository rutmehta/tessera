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
