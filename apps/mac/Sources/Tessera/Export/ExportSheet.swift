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
        VStack(spacing: 0) {
            header
            Divider()
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
            if let error = exporter.error {
                Divider()
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 11)).foregroundStyle(Color(nsColor: Theme.reject))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16).padding(.vertical, 6)
                    .accessibilityIdentifier("export-error")
            }
            Divider()
            footer
        }
        .frame(width: 620, height: 680)
        .alert("Save Export Preset", isPresented: $namingPreset) {
            TextField("Name", text: $newPresetName)
            Button("Save") { exporter.savePreset(named: newPresetName) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Saves these settings (not the folder) under this name.")
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Text("Export").font(.system(size: 15, weight: .semibold))
                Spacer()
                Picker("Photos", selection: Binding(get: { exporter.target?.id ?? "" }, set: { exporter.targetID = $0 })) {
                    ForEach(exporter.targets) { t in Text("\(t.title) (\(t.count))").tag(t.id) }
                }
                .frame(width: 300)
                .accessibilityIdentifier("export-target")
            }
            HStack {
                Picker("Preset", selection: Binding(get: { exporter.presetName ?? "" }, set: { name in
                    if let p = exporter.presets.first(where: { $0.name == name }) { exporter.apply(p) }
                })) {
                    Text("Custom").tag("")
                    Divider()
                    ForEach(exporter.presets) { p in Text(p.name).tag(p.name) }
                }
                .accessibilityIdentifier("export-preset")
                Menu {
                    Button("Save as Preset…") { newPresetName = exporter.presetName ?? ""; namingPreset = true }
                    Button("Delete “\(exporter.presetName ?? "")”") { exporter.presetName.map(exporter.deletePreset) }
                        .disabled(exporter.presetName == nil)
                    Divider()
                    Button("Restore Default Presets") { exporter.restoreDefaultPresets() }
                } label: { Image(systemName: "ellipsis.circle") }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
            }
            Text(exporter.settings.summary)
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .accessibilityIdentifier("export-summary")
        }
        .padding(16)
    }

    private var locationSection: some View {
        Section("Location") {
            HStack {
                Text(exporter.settings.destination.isEmpty ? "No folder chosen" : exporter.settings.destination)
                    .lineLimit(1).truncationMode(.middle)
                    .foregroundStyle(exporter.settings.destination.isEmpty ? .secondary : .primary)
                    .accessibilityIdentifier("export-destination")
                Spacer()
                Button("Choose…") { exporter.chooseDestination(in: NSApp.keyWindow) }
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
                    .font(.system(size: 12, design: .monospaced))
                    .accessibilityIdentifier("export-naming")
                ForEach(ExportNaming.tokens, id: \.token) { t in
                    Button(t.token) { exporter.settings.naming += t.token }
                        .help("Insert \(t.title)")
                        .controlSize(.small)
                }
            }
            Text("Example: \(exporter.namingExample)")
                .font(.system(size: 11))
                .foregroundStyle(exporter.namingIsValid ? Color.secondary : Color(nsColor: Theme.reject))
                .accessibilityIdentifier("export-naming-example")
        }
    }

    private var fileSection: some View {
        Section("File Settings") {
            Picker("Format", selection: Binding(get: { exporter.settings.format }, set: {
                exporter.settings.format = $0
                exporter.settings.normalizeForFormat()
            })) {
                ForEach(ExportSettings.FileFormat.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            if exporter.settings.format == .jpeg {
                LabeledContent("Quality") {
                    HStack {
                        Slider(value: Binding(get: { Double(exporter.settings.quality) },
                                              set: { exporter.settings.quality = Int($0.rounded()) }), in: 1...100)
                        Text("\(exporter.settings.quality)").monospacedDigit().frame(width: 28, alignment: .trailing)
                    }
                }
            }
            if exporter.settings.format == .tiff {
                Picker("Bit depth", selection: $exporter.settings.bitDepth) {
                    Text("8-bit").tag(8)
                    Text("16-bit").tag(16)
                }
                .pickerStyle(.segmented)
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
            Picker("Upscale (Real-ESRGAN)", selection: $exporter.settings.upscale) {
                Text("Off").tag(1)
                Text("2×").tag(2)
                Text("4×").tag(4)
            }
            .pickerStyle(.segmented)
            if exporter.settings.upscale > 1 {
                Text("Upscales before resizing. Downloads the pinned model into the app folder on first use; slow on large photos.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
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

    private var footer: some View {
        HStack {
            if exporter.isRunning {
                Text("An export is running").font(.system(size: 11)).foregroundStyle(.secondary)
            }
            Spacer()
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            Button(exportTitle) {
                if let problem = exporter.validate() {
                    exporter.error = problem
                } else {
                    dismiss()
                    exporter.start()
                }
            }
            .keyboardShortcut(.defaultAction)
            .disabled(exporter.target == nil || exporter.isRunning || !exporter.namingIsValid)
            .accessibilityIdentifier("export-start")
        }
        .padding(16)
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
            HStack(spacing: 10) {
                Text("Exporting · \(exporter.runningTitle)").font(.system(size: 11, weight: .medium))
                ProgressView(value: Double(min(p.done, p.total)), total: Double(max(p.total, 1)))
                    .frame(width: 180)
                Text("\(p.done) / \(p.total)" + (p.failed > 0 ? " · \(p.failed) failed" : ""))
                    .font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
                Text(p.current).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                Spacer()
                Button("Cancel Export") { exporter.cancel() }
                    .controlSize(.small)
                    .accessibilityIdentifier("export-cancel")
            }
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Color(nsColor: Theme.cellBackground))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("export-progress")
        }
    }
}
