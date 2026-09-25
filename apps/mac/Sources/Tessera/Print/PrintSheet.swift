import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// File ▸ Print… (⌘P; docs/01 §2.26): page setup, layout (single, contact sheet, custom cells),
/// margins, rotate-to-fit, resolution, print sharpening and colour handling over a page preview,
/// with three outputs: Print…, Save as PDF… and Save as JPEG….
struct PrintSheet: View {
    @Bindable var printing: PrintController
    let model: AppModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        SheetScaffold(title: "Print",
                      subtitle: "\(printing.title) · \(printing.items.count) photo\(printing.items.count == 1 ? "" : "s")") {
            EmptyView()
        } content: {
            VStack(spacing: 0) {
                HStack(spacing: 0) {
                    preview
                        .frame(width: 400)
                        .background(Theme.canvas)
                    Hairline(vertical: true)
                    Form {
                        paperSection
                        layoutSection
                        qualitySection
                        colorSection
                    }
                    .formStyle(.grouped)
                    .scrollContentBackground(.hidden)
                }
                if let error = printing.error {
                    Hairline()
                    StatusLine(text: error, kind: .error)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, Theme.Space.l).padding(.vertical, Theme.Space.s)
                        .accessibilityIdentifier("print-error")
                }
            }
        } leading: {
            Text("Photos are rendered with the engine at \(Int(printing.settings.dpi)) dpi before printing.")
        } actions: {
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).sheetButton()
            Button("Save as JPEG…") {
                printing.chooseDestination("jpeg", in: NSApp.keyWindow) { url in start(.jpeg(url)) }
            }
            .sheetButton()
            .disabled(printing.pageCount == 0)
            Button("Save as PDF…") {
                printing.chooseDestination("pdf", in: NSApp.keyWindow) { url in start(.pdf(url)) }
            }
            .sheetButton()
            .disabled(printing.pageCount == 0)
            .accessibilityIdentifier("print-save-pdf")
            Button("Print…") { start(.printer) }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .disabled(printing.pageCount == 0)
        }
        .frame(width: 900, height: 660)
    }

    // MARK: Preview

    private var preview: some View {
        VStack(spacing: Theme.Space.s) {
            PrintPreview(composer: printing.previewComposer(), page: printing.previewPage)
                .aspectRatio(printing.pageSize.width / max(printing.pageSize.height, 1), contentMode: .fit)
                .shadow(color: Theme.shadow.opacity(0.65), radius: Theme.Space.s, y: Theme.Space.xxs)
                .padding(Theme.Space.xl)
            HStack(spacing: Theme.Space.xs) {
                IconButton(symbol: "chevron.left", help: "Previous page") {
                    printing.previewPage = max(printing.previewPage - 1, 0)
                }
                .disabled(printing.previewPage == 0)
                Text("Page \(min(printing.previewPage + 1, max(printing.pageCount, 1))) of \(printing.pageCount)")
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                    .accessibilityIdentifier("print-page-count")
                IconButton(symbol: "chevron.right", help: "Next page") {
                    printing.previewPage = min(printing.previewPage + 1, max(printing.pageCount - 1, 0))
                }
                .disabled(printing.previewPage >= printing.pageCount - 1)
            }
            .padding(.bottom, Theme.Space.m)
        }
    }

    // MARK: Controls

    private var paperSection: some View {
        Section("Paper") {
            HStack {
                Text(printing.paperDescription).font(Theme.Fonts.label).accessibilityIdentifier("print-paper")
                Spacer()
                Button("Page Setup…") { printing.pageSetup() }.buttonStyle(.themeBordered)
            }
            Hint(printing.hardwareMargins)
        }
    }

    private var layoutSection: some View {
        Section("Layout") {
            LabeledContent("Layout") {
                SegmentedPicker(selection: $printing.settings.layout.style,
                                segments: PrintLayout.Style.allCases.map { .init(value: $0, title: $0.title) }, fill: false)
                .fixedSize()
                .accessibilityIdentifier("print-layout")
            }
            switch printing.settings.layout.style {
            case .single:
                EmptyView()
            case .contactSheet:
                Stepper("Rows: \(printing.settings.layout.rows)", value: $printing.settings.layout.rows, in: 1...20)
                Stepper("Columns: \(printing.settings.layout.columns)", value: $printing.settings.layout.columns, in: 1...20)
                Toggle("File names under pictures", isOn: $printing.settings.layout.captions)
            case .custom:
                HStack {
                    inches("Cell width", \.layout.cellWidth)
                    inches("height", \.layout.cellHeight)
                }
            }
            if printing.settings.layout.style != .single { inches("Spacing", \.layout.spacing) }
            LabeledContent("Margins (in)") {
                Grid(horizontalSpacing: Theme.Space.s, verticalSpacing: Theme.Space.xs) {
                    GridRow {
                        inches("Top", \.layout.marginTop)
                        inches("Bottom", \.layout.marginBottom)
                    }
                    GridRow {
                        inches("Left", \.layout.marginLeft)
                        inches("Right", \.layout.marginRight)
                    }
                }
            }
            Toggle("Rotate to fit", isOn: $printing.settings.layout.rotateToFit)
            Text("\(printing.settings.layout.cellsPerPage(page: printing.pageSize)) per page · \(printing.pageCount) page\(printing.pageCount == 1 ? "" : "s")")
                .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
        }
    }

    private var qualitySection: some View {
        Section("Print Job") {
            Picker("Resolution", selection: $printing.settings.dpi) {
                ForEach(PrintSettings.resolutions, id: \.self) { Text("\(Int($0)) dpi").tag($0) }
            }
            LabeledContent("Print sharpening") {
                SegmentedPicker(selection: $printing.settings.sharpening,
                                segments: PrintSettings.Sharpening.allCases.map { .init(value: $0, title: $0.title) }, fill: false)
                .fixedSize()
            }
            Picker("JPEG pages at", selection: $printing.settings.fileDPI) {
                ForEach(PrintSettings.resolutions, id: \.self) { Text("\(Int($0)) dpi").tag($0) }
            }
        }
    }

    private var colorSection: some View {
        Section("Colour Management") {
            Picker("Colour handling", selection: $printing.settings.colorHandling) {
                ForEach(PrintSettings.ColorHandling.allCases) { Text($0.title).tag($0) }
            }
            if printing.settings.colorHandling == .application {
                HStack {
                    Picker("Profile", selection: Binding(get: { printing.settings.profilePath ?? "" },
                                                         set: { printing.settings.profilePath = $0.isEmpty ? nil : $0 })) {
                        if printing.profiles.isEmpty { Text("No printer profiles installed").tag("") }
                        ForEach(printing.profiles, id: \.path) { p in Text("\(p.name) (\(p.colorSpace))").tag(p.path) }
                    }
                    Button("Other…") { printing.chooseProfileFile(in: NSApp.keyWindow) }.buttonStyle(.themeBordered)
                }
                LabeledContent("Intent") {
                    SegmentedPicker(selection: $printing.settings.intent,
                                    segments: PrintSettings.Intent.allCases.map { .init(value: $0, title: $0.title) }, fill: false)
                    .fixedSize()
                }
                Toggle("Black point compensation", isOn: $printing.settings.blackPointCompensation)
                Hint("Turn off colour management in the printer driver's dialog: the pixels are already in the printer's space.")
            } else {
                Hint("Sends Display P3 pixels; the printer driver converts them for the paper you choose there.")
            }
        }
    }

    private func inches(_ label: String, _ path: WritableKeyPath<PrintSettings, Double>) -> some View {
        TextField(label, value: Binding(get: { (printing.settings[keyPath: path] / 72 * 100).rounded() / 100 },
                                        set: { printing.settings[keyPath: path] = max($0, 0) * 72 }),
                  format: .number.precision(.fractionLength(0...2)))
            .help("inches")
    }

    private func start(_ output: PrintController.Output) {
        guard let engine = model.engineLibrary?.engine else { return }
        if printing.settings.colorHandling == .application, printing.settings.profilePath == nil {
            printing.error = "Choose a printer profile, or let the printer manage colour"
            return
        }
        dismiss()
        let window = model.mainWindow
        // After the sheet has gone: the print panel attaches to the main window.
        DispatchQueue.main.async {
            MainActor.assumeIsolated { printing.run(output, engine: engine, window: window) }
        }
    }
}

/// The sheet's page preview: the same composer as the real output, with grid thumbnails.
struct PrintPreview: NSViewRepresentable {
    let composer: PrintComposer
    let page: Int

    func makeNSView(context: Context) -> PreviewView { PreviewView() }
    func updateNSView(_ view: PreviewView, context: Context) {
        view.composer = composer
        view.page = page
        view.needsDisplay = true
    }

    final class PreviewView: NSView {
        var composer: PrintComposer?
        var page = 0
        override var isFlipped: Bool { true }
        override func draw(_ dirtyRect: NSRect) {
            guard let composer, let ctx = NSGraphicsContext.current?.cgContext else { return }
            let s = composer.pageSize
            guard s.width > 0, s.height > 0 else { return }
            let scale = min(bounds.width / s.width, bounds.height / s.height)
            ctx.saveGState()
            ctx.translateBy(x: (bounds.width - s.width * scale) / 2, y: (bounds.height - s.height * scale) / 2)
            ctx.scaleBy(x: scale, y: scale)
            composer.draw(page: page, in: ctx, placeholders: true)
            // Margins guide.
            ctx.setStrokeColor(Theme.Palette.accent.withAlphaComponent(0.6).cgColor)
            ctx.setLineWidth(0.5 / scale)
            ctx.stroke(composer.layout.contentRect(page: s))
            ctx.restoreGState()
        }
    }
}
