import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// File ▸ Export… (⇧⌘E; docs/01 §2.22): a preset picker over editable settings — format (JPEG with
/// an optional size limit, PNG, TIFF, AVIF, lossless JPEG XL, developed DNG), quality and bit depth,
/// colour space, sizing, output sharpening, metadata, a text or graphic watermark (M2-46), a naming
/// template with a live example, 2×/4× upscale, destination and "Show in Finder" — for the
/// selection or the current album. Export runs non-modally (progress in the main window).
struct ExportSheet: View {
    @Bindable var exporter: ExportController
    @Environment(\.dismiss) private var dismiss
    @State private var newPresetName = ""
    @State private var namingPreset = false

    var body: some View {
        SheetScaffold(title: "Export", subtitle: "Pending photo edits are saved before export") {
            if exporter.target?.hasLightroomSmartPreviews == true {
                Text("Exporting from a 2560 px smart preview; original offline")
                    .font(Theme.Fonts.caption)
                    .accessibilityIdentifier("export.smart-preview-note")
            }
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
                    watermarkSection
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
        .frame(width: 640, height: 760)
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
                SegmentedPicker(selection: Binding(get: { exporter.settings.format }, set: { exporter.setFormat($0) }),
                                segments: ExportSettings.OutputFormat.allCases.map { .init(value: $0, title: $0.title) }, fill: false)
                .fixedSize()
                .accessibilityIdentifier("export-format")
            }
            switch exporter.settings.format {
            case .jpeg:
                qualityRow
                sizeLimitRow
            case .png:
                EmptyView()
            case .tiff:
                bitDepthRow
            case .avif:
                qualityRow
                bitDepthRow
                LabeledContent("Speed") {
                    HStack {
                        Slider(value: Binding(get: { Double(exporter.settings.avifSpeed) },
                                              set: { exporter.settings.avifSpeed = Int($0.rounded()) }), in: 1...10, step: 1)
                        Text("\(exporter.settings.avifSpeed)").font(Theme.Fonts.labelNumeric).frame(width: 28, alignment: .trailing)
                    }
                    .help("1 is slowest with the smallest files; 10 is fastest")
                }
                .accessibilityIdentifier("export-avif-speed")
            case .jpegXl:
                LabeledContent("Compression") {
                    Text("Lossless").foregroundStyle(Theme.textPrimary)
                        .accessibilityIdentifier("export-jxl-lossless")
                }
                bitDepthRow
                Hint(ExportSettings.lossyJpegXLReason)
            case .dng:
                LabeledContent("Data") {
                    Text("Linear 32-bit float").foregroundStyle(Theme.textPrimary)
                }
                Hint(ExportSettings.dngExplanation)
                    .accessibilityIdentifier("export-dng-note")
            }
            Picker("Colour space", selection: $exporter.settings.colorSpace) {
                ForEach(ExportSettings.ColorSpace.allCases) { Text($0.title).tag($0) }
            }
            .disabled(exporter.settings.colorSpaceLockedReason != nil)
            .accessibilityIdentifier("export-color-space")
            if let reason = exporter.settings.colorSpaceLockedReason {
                Hint(reason).accessibilityIdentifier("export-color-space-note")
            }
            if exporter.settings.format == .avif || exporter.settings.format == .jpegXl {
                Toggle("HDR output", isOn: .constant(false))
                    .disabled(true)
                    .help(ExportSettings.hdrReason)
                    .accessibilityIdentifier("export-hdr")
                Hint(ExportSettings.hdrReason)
            }
        }
    }

    private var qualityRow: some View {
        LabeledContent("Quality") {
            HStack {
                Slider(value: Binding(get: { Double(exporter.settings.quality) },
                                      set: { exporter.settings.quality = Int($0.rounded()) }), in: 1...100)
                Text("\(exporter.settings.quality)").font(Theme.Fonts.labelNumeric).frame(width: 28, alignment: .trailing)
            }
        }
        .accessibilityIdentifier("export-quality")
    }

    private var bitDepthRow: some View {
        LabeledContent("Bit depth") {
            SegmentedPicker(selection: $exporter.settings.bitDepth,
                            segments: exporter.settings.format.bitDepths.map { .init(value: $0, title: "\($0)-bit") }, fill: false)
            .fixedSize()
            .accessibilityIdentifier("export-bit-depth")
        }
    }

    /// JPEG only: "Limit file size to [ ] KB" (the engine searches the quality that fits).
    private var sizeLimitRow: some View {
        HStack {
            Toggle("Limit file size to", isOn: Binding(get: { exporter.settings.maxFileBytes != nil },
                                                       set: { exporter.setSizeLimit($0) }))
                .accessibilityIdentifier("export-size-limit")
            TextField("Kilobytes", value: Binding(get: { exporter.settings.maxFileKilobytes ?? exporter.sizeLimitDraftKB },
                                                  set: { exporter.settings.maxFileKilobytes = max($0, 1) }), format: .number)
                .labelsHidden()
                .frame(width: Theme.Width.label)
                .disabled(exporter.settings.maxFileBytes == nil)
                .accessibilityIdentifier("export-size-limit-kb")
            Text("KB").foregroundStyle(Theme.textSecondary)
            Spacer()
        }
        .help("Lowers the JPEG quality until the file, with its colour profile and metadata, fits (1 KB = 1,000 bytes)")
    }

    // MARK: Watermark

    private var watermarkSection: some View {
        Section("Watermark") {
            let unavailable = exporter.settings.watermarkUnavailableReason
            LabeledContent("Watermark") {
                SegmentedPicker(selection: Binding<ExportWatermark.Kind?>(get: { exporter.settings.watermark?.kind },
                                                                          set: { exporter.setWatermarkKind($0) }),
                                segments: [.init(value: nil, title: "None"), .init(value: .text, title: "Text"),
                                           .init(value: .graphic, title: "Graphic")], fill: false)
                .fixedSize()
                .disabled(unavailable != nil)
                .accessibilityIdentifier("export-watermark-kind")
            }
            if let unavailable {
                StatusLine(text: unavailable, kind: .warning).accessibilityIdentifier("export-watermark-unavailable")
            }
            if let mark = exporter.settings.watermark {
                switch mark.kind {
                case .text: textWatermarkRows
                case .graphic: graphicWatermarkRows(mark)
                }
                percentRow("Opacity", \.opacity, 0...1, id: "export-watermark-opacity")
                LabeledContent("Position") {
                    AnchorPicker(selection: markBinding(\.anchor))
                }
                .accessibilityIdentifier("export-watermark-anchor")
                percentRow("Inset", \.inset, 0...0.25, id: "export-watermark-inset", help: "Distance from the edges, as a percentage of the short edge")
                LabeledContent("Preview") {
                    VStack(alignment: .trailing, spacing: Theme.Space.s) {
                        WatermarkPlacementPreview(mark: mark,
                                                  engineImage: exporter.enginePreviewMark == mark ? exporter.enginePreview : nil)
                        HStack(spacing: Theme.Space.s) {
                            if exporter.isRenderingPreview { ProgressView().controlSize(.small) }
                            Button("Render with Engine") { exporter.renderWatermarkPreview() }
                                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                                .disabled(exporter.isRenderingPreview || exporter.previewImageID == nil || mark.problem != nil)
                                .help(exporter.previewImageID == nil
                                      ? "Select photos to preview: album exports resolve their photos in the engine"
                                      : "Exports the first photo at 480 px with this watermark, through the engine")
                                .accessibilityIdentifier("export-watermark-render")
                        }
                        if let problem = exporter.previewError ?? mark.problem {
                            StatusLine(text: problem, kind: .error).accessibilityIdentifier("export-watermark-problem")
                        }
                    }
                }
            }
        }
    }

    private var textWatermarkRows: some View {
        Group {
            TextField("Text", text: markBinding(\.text))
                .accessibilityIdentifier("export-watermark-text")
            LabeledContent("Font") {
                HStack {
                    Picker("Font", selection: markBinding(\.font)) {
                        let current = exporter.settings.watermark?.font ?? ""
                        if !current.isEmpty, !ExportWatermark.installedFonts.contains(where: { $0.path == current }) {
                            Text(ExportWatermark.fontName(path: current)).tag(current)
                            Divider()
                        }
                        ForEach(ExportWatermark.installedFonts) { f in Text(f.name).tag(f.path) }
                    }
                    .labelsHidden()
                    .accessibilityIdentifier("export-watermark-font")
                    Button("Other…") { exporter.chooseWatermarkFile(font: true, in: NSApp.keyWindow) }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .help("A TrueType or OpenType font file (.ttf, .otf)")
                }
            }
            percentRow("Size", \.size, 0.01...0.5, id: "export-watermark-size", help: "Text height as a percentage of the short edge")
            LabeledContent("Colour") {
                ColorPicker("Colour", selection: colorBinding, supportsOpacity: false).labelsHidden()
            }
            .help("In the export colour space")
            .accessibilityIdentifier("export-watermark-color")
            LabeledContent("Rotation") {
                HStack {
                    Slider(value: markBinding(\.rotation), in: -180...180, step: 1)
                    Text("\(Int(exporter.settings.watermark?.rotation ?? 0))°").font(Theme.Fonts.labelNumeric)
                        .frame(width: 40, alignment: .trailing)
                }
                .help("Degrees, clockwise")
            }
            .accessibilityIdentifier("export-watermark-rotation")
        }
    }

    private func graphicWatermarkRows(_ mark: ExportWatermark) -> some View {
        Group {
            LabeledContent("Graphic") {
                HStack {
                    Text(mark.path.isEmpty ? "No PNG chosen" : URL(fileURLWithPath: mark.path).lastPathComponent)
                        .lineLimit(1).truncationMode(.middle)
                        .foregroundStyle(mark.path.isEmpty ? Theme.textSecondary : Theme.textPrimary)
                        .help(mark.path)
                        .accessibilityIdentifier("export-watermark-graphic")
                    Button("Choose…") { exporter.chooseWatermarkFile(font: false, in: NSApp.keyWindow) }
                        .buttonStyle(.themeBordered)
                        .accessibilityIdentifier("export-watermark-choose")
                }
            }
            percentRow("Scale", \.scale, 0.01...1, id: "export-watermark-scale", help: "Width as a percentage of the short edge")
        }
    }

    private func markBinding<T>(_ path: WritableKeyPath<ExportWatermark, T>) -> Binding<T> {
        Binding(get: { (exporter.settings.watermark ?? exporter.watermarkDraft)[keyPath: path] },
                set: { value in
                    guard var mark = exporter.settings.watermark else { return }
                    mark[keyPath: path] = value
                    exporter.settings.watermark = mark
                })
    }

    private var colorBinding: Binding<Color> {
        Binding(get: {
            let c = (exporter.settings.watermark ?? exporter.watermarkDraft).color + [1, 1, 1]
            return Color(.sRGB, red: c[0], green: c[1], blue: c[2]) // lint:allow (user watermark colour)
        }, set: { color in
            guard let rgb = NSColor(color).usingColorSpace(.sRGB) else { return }
            let round = { (v: CGFloat) in (min(max(Double(v), 0), 1) * 1000).rounded() / 1000 }
            markBinding(\.color).wrappedValue = [round(rgb.redComponent), round(rgb.greenComponent), round(rgb.blueComponent)]
        })
    }

    /// A 0–1 fraction as a whole-percent slider.
    private func percentRow(_ title: String, _ path: WritableKeyPath<ExportWatermark, Double>, _ range: ClosedRange<Double>,
                            id: String, help: String? = nil) -> some View {
        let value = markBinding(path)
        return LabeledContent(title) {
            HStack {
                Slider(value: Binding(get: { value.wrappedValue * 100 }, set: { value.wrappedValue = ($0.rounded()) / 100 }),
                       in: (range.lowerBound * 100)...(range.upperBound * 100), step: 1)
                Text("\(Int((value.wrappedValue * 100).rounded()))%").font(Theme.Fonts.labelNumeric)
                    .frame(width: 40, alignment: .trailing)
            }
            .help(help ?? title)
        }
        .accessibilityIdentifier(id)
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
