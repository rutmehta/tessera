import AppKit
import ImageIO
import TesseraCore
import TesseraFFI

/// A finished Deliver export: the status row's "Done · n files · Reveal".
struct DeliverResult: Equatable {
    let files: [URL]
    let folder: URL
    let failed: Int
}

/// Deliver (redesign SPEC §5, WP UX-15): What (Keeps · Selection · Whole shoot) · How (Web JPEG ·
/// Instagram · Full size · Print) · Where (the export folder and naming, shared with the Export
/// sheet). One Export button runs `ExportController` with the chosen settings; "More…" opens the
/// full sheet on the same photos; Print opens the print flow. The agent field maps "export the
/// keeps for Instagram" to the same rows and runs it.
extension AppModel {
    static let deliverWhatKey = "Deliver.what"
    static let deliverHowKey = "Deliver.how"

    /// The photos a What row stands for: engine-backed ones only (those have pixels to write).
    func deliverItems(_ what: DeliverWhat) -> [PhotoItem] {
        switch what {
        case .keeps: library.items.filter { $0.engineImage != nil && cull[$0.id].decision == .keep }
        case .selection: selectedItems.filter { $0.engineImage != nil }
        case .shoot: library.items.filter { $0.engineImage != nil }
        }
    }

    /// The files the Export button will write.
    var deliverItems: [PhotoItem] { deliverItems(deliverWhat) }

    /// The chosen How over the person's own Where: folder, naming, conflicts and "show in Finder"
    /// stay what the sheet last used (`exporter.settings` persists them).
    var deliverSettings: ExportSettings {
        var s = deliverHow.settings(presets: exporter.presets)
        s.destination = exporter.settings.destination.isEmpty ? ExportController.defaultDestination.path : exporter.settings.destination
        s.naming = exporter.settings.naming
        s.onConflict = exporter.settings.onConflict
        s.openInFinder = exporter.settings.openInFinder
        return s
    }

    /// "4 photos · Web JPEG · 2048 px · sRGB", the preview's header.
    var deliverHeadline: String {
        let n = deliverItems.count
        return "\(n.formatted()) photo\(n == 1 ? "" : "s") · \(deliverHow.title) · \(deliverHow.subtitle)"
    }

    /// The photo's oriented pixel size: the library's when it knows it, else the file header's
    /// (ImageIO reads RAW headers too), cached by item; nil when neither knows.
    func deliverPixelSize(of item: PhotoItem) -> (width: Int, height: Int)? {
        if item.pixelWidth > 0, item.pixelHeight > 0 { return (item.pixelWidth, item.pixelHeight) }
        if let cached = deliverPixelSizes[item.id] { return cached.width > 0 ? cached : nil }
        var size = (width: 0, height: 0)
        if let url = item.url, let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
           let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
           let w = props[kCGImagePropertyPixelWidth] as? Int, let h = props[kCGImagePropertyPixelHeight] as? Int, w > 0, h > 0 {
            let rotated = ((props[kCGImagePropertyOrientation] as? UInt32) ?? 1) >= 5
            size = rotated ? (h, w) : (w, h)
        }
        deliverPixelSizes[item.id] = size
        return size.width > 0 ? size : nil
    }

    /// The total of the per-file estimates ("about 3.1 MB"); nil for Print and for no photos.
    var deliverEstimate: String? {
        guard deliverHow != .print else { return nil }
        let settings = deliverSettings
        let bytes = deliverItems.reduce(0) { total, item in
            guard let size = deliverPixelSize(of: item) else { return total }
            return total + settings.estimatedBytes(width: size.width, height: size.height)
        }
        guard bytes > 0 else { return nil }
        return "about " + ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }

    /// The output pixel size under a preview cell ("2048 × 1365"), or nil when the photo's size
    /// is unknown or the row is Print.
    func deliverOutputSize(of item: PhotoItem) -> String? {
        guard deliverHow != .print, let pixels = deliverPixelSize(of: item),
              let size = deliverSettings.outputSize(width: pixels.width, height: pixels.height) else { return nil }
        return "\(size.width.formatted()) × \(size.height.formatted())"
    }

    /// The live file-name example of the Where section (the first file to be written).
    var deliverNamingExample: String {
        guard let first = deliverItems.first else { return "" }
        let settings = deliverSettings
        return ExportNaming.example(template: settings.naming, firstName: (first.name as NSString).deletingPathExtension,
                                    date: first.captureDate, format: settings.format, count: deliverItems.count)
    }

    /// One sentence for an empty preview (SPEC §7).
    var deliverEmptyMessage: String {
        switch deliverWhat {
        case .keeps: "No keeps yet. Cull first, or pick Selection or Whole shoot."
        case .selection: "Nothing is selected. Pick photos in Cull, or choose Keeps or Whole shoot."
        case .shoot: "No photos to export."
        }
    }

    /// Entering Deliver (the mode control, ⌘3): the presets and the folder are read once so the
    /// rows can show their settings before anything runs.
    func enterDeliver() {
        deliverMode = true
        guard let lib = engineLibrary, !exporter.isRunning else { return }
        prepareDeliverExporter(lib, items: deliverItems)
    }

    func setDeliverWhat(_ what: DeliverWhat) {
        deliverWhat = what
        deliverResult = nil
        exporter.error = nil
    }

    func setDeliverHow(_ how: DeliverHow) {
        deliverHow = how
        deliverResult = nil
        exporter.error = nil
    }

    /// The Export button: Print opens the print flow on the same photos; the other rows run the
    /// export non-modally (progress in the Deliver preview and the strip under the canvas).
    func runDeliver() {
        guard let lib = engineLibrary else { statusMessage = "Export needs a folder opened on the engine"; return }
        let items = deliverItems
        guard !items.isEmpty else { statusMessage = deliverEmptyMessage; return }
        if deliverHow == .print { presentPrint(items: items, title: deliverWhat.title); return }
        guard !exporter.isRunning else { statusMessage = "An export is already running"; return }
        prepareDeliverExporter(lib, items: items)
        exporter.settings = deliverSettings
        if let problem = exporter.validate() {
            exporter.error = problem
            statusMessage = problem
            return
        }
        deliverResult = nil
        exporter.start()
    }

    /// "More…": the full Export sheet on the chosen photos, starting from the row's settings.
    func presentDeliverMore() {
        guard !exporter.isRunning else { statusMessage = "An export is already running"; return }
        guard let lib = engineLibrary else { statusMessage = "Export needs a folder opened on the engine"; return }
        let items = deliverItems
        guard !items.isEmpty else { statusMessage = deliverEmptyMessage; return }
        prepareDeliverExporter(lib, items: items)
        if deliverHow != .print {
            if let name = deliverHow.presetName, let preset = exporter.presets.first(where: { $0.name == name }) {
                exporter.apply(preset)
            } else {
                exporter.settings = deliverSettings
            }
        }
        showExport = true
    }

    /// Reveal: the written files selected in Finder (the folder when none were written).
    func revealDeliverResult() {
        guard let result = deliverResult else { return }
        if result.files.isEmpty {
            NSWorkspace.shared.activateFileViewerSelecting([result.folder])
        } else {
            NSWorkspace.shared.activateFileViewerSelecting(result.files)
        }
    }

    /// Deliver's agent field: a How, a What or both are taken from the phrase and the export runs;
    /// nothing understood is reported like any other text.
    func deliverAgent(_ text: String) -> Bool {
        guard let phrase = DeliverAgentPhrase.parse(text) else { return false }
        if let how = phrase.how { setDeliverHow(how) }
        if let what = phrase.what { setDeliverWhat(what) }
        runDeliver()
        return true
    }

    /// Called when an export finishes while Deliver is up (from `exportDidFinish`).
    func deliverDidFinish(_ report: ExportReport) {
        guard deliverMode else { return }
        let files = report.items.compactMap { $0.outputPath.map { URL(fileURLWithPath: $0) } }
        deliverResult = DeliverResult(files: files, folder: URL(fileURLWithPath: report.destination), failed: Int(report.failed))
    }

    /// The exporter on these photos as its one target (frozen image IDs, so the run gates exactly
    /// them), with the presets read and the save gate installed.
    private func prepareDeliverExporter(_ lib: EngineLibrary, items: [PhotoItem]) {
        exporter.acquireSaveGate = { [weak self, lib] ids in
            guard let self, self.engineLibrary === lib else { return nil }
            return self.prepareForRecipeRead(imageIDs: ids, library: lib)
        }
        let ids = items.compactMap { $0.engineImage?.imageID }
        let targets: [ExportController.Target] = items.first.map { first in
            [ExportController.Target(kind: .selection, title: deliverWhat.title, target: .images(imageIds: ids),
                                     count: ids.count, firstName: first.name, firstDate: first.captureDate,
                                     hasLightroomSmartPreviews: items.contains { $0.engineImage?.lightroomSmartPreview == true })]
        } ?? []
        exporter.prepare(engine: lib.engine, targets: targets, preferred: .selection)
    }
}
