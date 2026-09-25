import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// File ▸ Export… (⇧⌘E; docs/01 §2.22): a preset picker over editable settings — format and
/// quality, colour space, sizing, output sharpening, metadata, a naming template with a live
/// example, 2×/4× upscale, destination and "Show in Finder" — for the selection or the current
/// album. Export runs non-modally (progress in the main window).
struct ExportSheet: View {
    @Bindable var exporter: ExportController
    @Environment(\.dismiss) private var dismiss
    @State private var newPresetName = ""
    @State private var namingPreset = false

    var body: some View {
        SheetScaffold(title: "Export", subtitle: nil) {
            Picker("Photos", selection: Binding(get: { exporter.target?.id ?? "" }, set: { exporter.targetID = $0 })) {
                ForEach(exporter.targets) { t in Text("\(t.title) (\(t.count))").tag(t.id) }
            }
            .labelsHidden()
            .frame(width: 240)
            .accessibilityIdentifier("export-target")
        } content: {
            VStack(spacing: 0) {
                HStack(spacing: Theme.Space.s) {
                    Text("Preset").font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
                    Picker("Preset", selection: Binding(get: { exporter.presetName ?? "" }, set: { name in
                        if let p = exporter.presets.first(where: { $0.name == name }) { exporter.apply(p) }
                    })) {
                        Text("Custom").tag("")
                        Divider()
                        ForEach(exporter.presets) { p in Text(p.name).tag(p.name) }
                    }
                    .labelsHidden()
                    .frame(maxWidth: 260)
                    .accessibilityIdentifier("export-preset")
                    Menu {
                        Button("Save as Preset…") { newPresetName = exporter.presetName ?? ""; namingPreset = true }
                        Button("Delete “\(exporter.presetName ?? "")”") { exporter.presetName.map(exporter.deletePreset) }
                            .disabled(exporter.presetName == nil)
                        Divider()
                        Button("Restore Default Presets") { exporter.restoreDefaultPresets() }
                    } label: { Image(systemName: "ellipsis") }
                        .menuStyle(IconMenuStyle())
                        .help("Preset actions")
                    Spacer()
                    Text(exporter.settings.summary).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                        .accessibilityIdentifier("export-summary")
                }
                .padding(.horizontal, Theme.Space.l)
                .frame(height: Theme.Height.large + Theme.Space.l)
                Hairline()
                Form {
                    locationSection
                    namingSection
                    fileSection
                    sizingSection
                    Section("Output") {
                        Picker("Sharpen for", selection: $exporter.settings.sharpening) {
                            ForEach(ExportSettings.Sharpening.allCases) { Text($0.title).tag($0) }
                        }
                        Picker("Metadata", selection: $exporter.settings.metadata) {
                            ForEach(ExportSettings.MetadataPolicy.allCases) { Text($0.title).tag($0) }
                        }
                    }
                }
                .formStyle(.grouped)
                .scrollContentBackground(.hidden)
                if let error = exporter.error {
                    Hairline()
                    StatusLine(text: error, kind: .error)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, Theme.Space.l).padding(.vertical, Theme.Space.s)
                        .accessibilityIdentifier("export-error")
                }
            }
        } leading: {
            if exporter.isRunning { Text("An export is running") }
        } actions: {
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).sheetButton()
            Button(exportTitle) {
                if let problem = exporter.validate() {
                    exporter.error = problem
                } else {
                    dismiss()
                    exporter.start()
                }
            }
            .keyboardShortcut(.defaultAction)
            .sheetButton(primary: true)
            .disabled(exporter.target == nil || exporter.isRunning || !exporter.namingIsValid)
            .accessibilityIdentifier("export-start")
        }
        .frame(width: 620, height: 720)
        .alert("Save Export Preset", isPresented: $namingPreset) {
            TextField("Name", text: $newPresetName)
            Button("Save") { exporter.savePreset(named: newPresetName) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Saves these settings (not the folder) under this name.")
        }
    }

    private var locationSection: some View {
        Section("Location") {
            HStack {
                Text(exporter.settings.destination.isEmpty ? "No folder chosen" : exporter.settings.destination)
                    .lineLimit(1).truncationMode(.middle)
                    .foregroundStyle(exporter.settings.destination.isEmpty ? Theme.textSecondary : Theme.textPrimary)
                    .accessibilityIdentifier("export-destination")
                Spacer()
                Button("Choose…") { exporter.chooseDestination(in: NSApp.keyWindow) }.buttonStyle(.themeBordered)
            }
            Picker("If a file exists", selection: $exporter.settings.onConflict) {
                ForEach(ExportSettings.OnConflict.allCases) { Text($0.title).tag($0) }
            }
            Toggle("After export: show in Finder", isOn: $exporter.settings.openInFinder)
        }
    }

    private var namingSection: some View {
        Section("File Naming") {
            HStack {
                TextField("Template", text: $exporter.settings.naming)
                    .font(Theme.Fonts.labelMono)
                    .accessibilityIdentifier("export-naming")
                ForEach(ExportNaming.tokens, id: \.token) { t in
                    Button(t.token) { exporter.settings.naming += t.token }
                        .help("Insert \(t.title)")
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                }
            }
            Text("Example: \(exporter.namingExample)")
                .font(Theme.Fonts.caption)
                .foregroundStyle(exporter.namingIsValid ? Theme.textSecondary : Theme.reject)
                .accessibilityIdentifier("export-naming-example")
        }
    }

    private var fileSection: some View {
        Section("File Settings") {
            LabeledContent("Format") {
                SegmentedPicker(selection: Binding(get: { exporter.settings.format }, set: {
                    exporter.settings.format = $0
                    exporter.settings.normalizeForFormat()
                }), segments: ExportSettings.FileFormat.allCases.map { .init(value: $0, title: $0.title) }, fill: false)
                .fixedSize()
            }
            if exporter.settings.format == .jpeg {
                LabeledContent("Quality") {
                    HStack {
                        Slider(value: Binding(get: { Double(exporter.settings.quality) },
                                              set: { exporter.settings.quality = Int($0.rounded()) }), in: 1...100)
                        Text("\(exporter.settings.quality)").font(Theme.Fonts.labelNumeric).frame(width: 28, alignment: .trailing)
                    }
                }
            }
            if exporter.settings.format == .tiff {
                LabeledContent("Bit depth") {
                    SegmentedPicker(selection: $exporter.settings.bitDepth,
                                    segments: [.init(value: 8, title: "8-bit"), .init(value: 16, title: "16-bit")], fill: false)
                    .fixedSize()
                }
            }
            Picker("Colour space", selection: $exporter.settings.colorSpace) {
                ForEach(ExportSettings.ColorSpace.allCases) { Text($0.title).tag($0) }
            }
        }
    }

    private var sizingSection: some View {
        Section("Image Sizing") {
            Picker("Resize", selection: $exporter.settings.resize.mode) {
                ForEach(ExportSettings.ResizeMode.allCases) { Text($0.title).tag($0) }
            }
            switch exporter.settings.resize.mode {
            case .none:
                EmptyView()
            case .longEdge:
                HStack {
                    TextField("Long edge", value: $exporter.settings.resize.longEdge, format: .number)
                        .accessibilityIdentifier("export-long-edge")
                    unitPicker
                }
            case .fit:
                HStack {
                    TextField("Width", value: $exporter.settings.resize.width, format: .number)
                    Text("×")
                    TextField("Height", value: $exporter.settings.resize.height, format: .number)
                    unitPicker
                }
            case .percent:
                TextField("Percent", value: $exporter.settings.resize.percent, format: .number)
            }
            TextField("Resolution (dpi)", value: $exporter.settings.dpi, format: .number)
                .help("Recorded in the file; converts inch and centimetre sizes to pixels")
            LabeledContent("Upscale (Real-ESRGAN)") {
                SegmentedPicker(selection: $exporter.settings.upscale,
                                segments: [.init(value: 1, title: "Off"), .init(value: 2, title: "2×"), .init(value: 4, title: "4×")],
                                fill: false)
                .fixedSize()
            }
            if exporter.settings.upscale > 1 {
                Hint("Upscales before resizing. Downloads the pinned model into the app folder on first use; slow on large photos.")
            }
        }
    }

    private var unitPicker: some View {
        Picker("", selection: $exporter.settings.resize.unit) {
            ForEach(ExportSettings.SizeUnit.allCases) { Text($0.title).tag($0) }
        }
        .labelsHidden()
        .frame(width: 90)
    }

    private var exportTitle: String {
        let n = exporter.target?.count ?? 0
        return "Export \(n) Photo\(n == 1 ? "" : "s")"
    }
}

/// Non-modal export progress above the status bar, with Cancel.
struct ExportProgressBar: View {
    let exporter: ExportController
    var body: some View {
        if let p = exporter.progress {
            ProgressStrip(title: "Exporting · \(exporter.runningTitle)", done: Int(p.done), total: Int(p.total),
                          detail: p.failed > 0 ? " · \(p.failed) failed" : "", current: p.current) {
                Button("Cancel Export") { exporter.cancel() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("export-cancel")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("export-progress")
        }
    }
}
