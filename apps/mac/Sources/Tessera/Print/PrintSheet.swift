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
        VStack(spacing: 0) {
            HStack(alignment: .firstTextBaseline) {
                Text("Print").font(.system(size: 15, weight: .semibold))
                Text("\(printing.title) · \(printing.items.count) photo\(printing.items.count == 1 ? "" : "s")")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
                Spacer()
            }
            .padding(16)
            Divider()
            HStack(spacing: 0) {
                preview
                    .frame(width: 400)
                    .background(Color(nsColor: Theme.gridBackground))
                Divider()
                Form {
                    paperSection
                    layoutSection
                    qualitySection
                    colorSection
                }
                .formStyle(.grouped)
            }
            if let error = printing.error {
                Divider()
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 11)).foregroundStyle(Color(nsColor: Theme.reject))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16).padding(.vertical, 6)
                    .accessibilityIdentifier("print-error")
            }
            Divider()
            footer
        }
        .frame(width: 900, height: 660)
    }

    // MARK: Preview

    private var preview: some View {
        VStack(spacing: 10) {
            PrintPreview(composer: printing.previewComposer(), page: printing.previewPage)
                .aspectRatio(printing.pageSize.width / max(printing.pageSize.height, 1), contentMode: .fit)
                .shadow(color: .black.opacity(0.4), radius: 6, y: 2)
                .padding(20)
            HStack {
                Button { printing.previewPage = max(printing.previewPage - 1, 0) } label: { Image(systemName: "chevron.left") }
                    .disabled(printing.previewPage == 0)
                Text("Page \(min(printing.previewPage + 1, max(printing.pageCount, 1))) of \(printing.pageCount)")
                    .font(.system(size: 11).monospacedDigit())
                    .accessibilityIdentifier("print-page-count")
                Button { printing.previewPage = min(printing.previewPage + 1, max(printing.pageCount - 1, 0)) } label: {
                    Image(systemName: "chevron.right")
                }
                .disabled(printing.previewPage >= printing.pageCount - 1)
            }
            .buttonStyle(.borderless)
            .padding(.bottom, 12)
        }
    }

    // MARK: Controls

    private var paperSection: some View {
        Section("Paper") {
            HStack {
                Text(printing.paperDescription).font(.system(size: 12)).accessibilityIdentifier("print-paper")
                Spacer()
                Button("Page Setup…") { printing.pageSetup() }
            }
            Text(printing.hardwareMargins).font(.system(size: 11)).foregroundStyle(.secondary)
        }
    }

    private var layoutSection: some View {
        Section("Layout") {
            Picker("Layout", selection: $printing.settings.layout.style) {
                ForEach(PrintLayout.Style.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier("print-layout")
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
            HStack {
                inches("Margins: top", \.layout.marginTop)
                inches("bottom", \.layout.marginBottom)
            }
            HStack {
                inches("left", \.layout.marginLeft)
                inches("right", \.layout.marginRight)
            }
            Toggle("Rotate to fit", isOn: $printing.settings.layout.rotateToFit)
            Text("\(printing.settings.layout.cellsPerPage(page: printing.pageSize)) per page · \(printing.pageCount) page\(printing.pageCount == 1 ? "" : "s")")
                .font(.system(size: 11)).foregroundStyle(.secondary)
        }
    }

    private var qualitySection: some View {
        Section("Print Job") {
            Picker("Resolution", selection: $printing.settings.dpi) {
                ForEach(PrintSettings.resolutions, id: \.self) { Text("\(Int($0)) dpi").tag($0) }
            }
            Picker("Print sharpening", selection: $printing.settings.sharpening) {
                ForEach(PrintSettings.Sharpening.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
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
                    Button("Other…") { printing.chooseProfileFile(in: NSApp.keyWindow) }
                }
                Picker("Intent", selection: $printing.settings.intent) {
                    ForEach(PrintSettings.Intent.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                Toggle("Black point compensation", isOn: $printing.settings.blackPointCompensation)
                Text("Turn off colour management in the printer driver's dialog: the pixels are already in the printer's space.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            } else {
                Text("Sends Display P3 pixels; the printer driver converts them for the paper you choose there.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
        }
    }

    private func inches(_ label: String, _ path: WritableKeyPath<PrintSettings, Double>) -> some View {
        TextField(label, value: Binding(get: { (printing.settings[keyPath: path] / 72 * 100).rounded() / 100 },
                                        set: { printing.settings[keyPath: path] = max($0, 0) * 72 }),
                  format: .number.precision(.fractionLength(0...2)))
            .help("inches")
    }

    // MARK: Footer

    private var footer: some View {
        HStack {
            Text("Photos are rendered with the engine at \(Int(printing.settings.dpi)) dpi before printing.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
            Spacer()
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            Button("Save as JPEG…") {
                printing.chooseDestination("jpeg", in: NSApp.keyWindow) { url in start(.jpeg(url)) }
            }
            .disabled(printing.pageCount == 0)
            Button("Save as PDF…") {
                printing.chooseDestination("pdf", in: NSApp.keyWindow) { url in start(.pdf(url)) }
            }
            .disabled(printing.pageCount == 0)
            .accessibilityIdentifier("print-save-pdf")
            Button("Print…") { start(.printer) }
                .keyboardShortcut(.defaultAction)
                .disabled(printing.pageCount == 0)
        }
        .padding(16)
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
            ctx.setStrokeColor(CGColor(red: 0.3, green: 0.55, blue: 1, alpha: 0.5))
            ctx.setLineWidth(0.5 / scale)
            ctx.stroke(composer.layout.contentRect(page: s))
            ctx.restoreGState()
        }
    }
}
