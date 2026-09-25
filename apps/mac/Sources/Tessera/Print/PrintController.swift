import AppKit
import ImageIO
import Observation
import SwiftUI
import TesseraCore
import TesseraFFI
import UniformTypeIdentifiers

/// State of File ▸ Print… (WP M2-20, docs/01 §2.26). The sheet chooses paper, layout, resolution,
/// print sharpening and colour handling over a live preview (grid thumbnails). Output renders each
/// photo with the engine at the print resolution (non-modal, cancellable), then goes to the printer
/// (`NSPrintOperation` over `PrintPageView`), to a PDF (the same operation, saved), or to JPEG pages.
@MainActor @Observable
final class PrintController {
    enum Output: Equatable {
        case printer
        case pdf(URL)
        case jpeg(URL)
        var title: String {
            switch self {
            case .printer: "Printing"
            case .pdf: "Saving PDF"
            case .jpeg: "Saving JPEG pages"
            }
        }
    }

    struct Progress: Equatable {
        var done: Int
        var total: Int
        var current: String
        var output: Output
    }

    var settings = PrintSettings.load() { didSet { if settings != oldValue { settings.save() } } }
    private(set) var items: [PhotoItem] = []
    private(set) var title = ""
    /// Paper: a private copy of the shared print info (page setup changes stay with Tessera).
    private(set) var printInfo: NSPrintInfo = PrintController.makePrintInfo()
    private(set) var profiles: [PrinterProfile] = []
    /// Preview pictures (grid thumbnails) by image index.
    private(set) var thumbnails: [Int: CGImage] = [:]
    var previewPage = 0
    private(set) var progress: Progress?
    var error: String?

    @ObservationIgnored var onMessage: (String, [String]) -> Void = { _, _ in }
    @ObservationIgnored private var cancelFlag: CancelFlag?
    @ObservationIgnored private var requests: [PreviewRequest] = []
    @ObservationIgnored private var cancelled = false

    var isRunning: Bool { progress != nil }

    private static func makePrintInfo() -> NSPrintInfo {
        // swiftlint:disable:next force_cast
        let info = NSPrintInfo.shared.copy() as! NSPrintInfo
        info.topMargin = 0; info.bottomMargin = 0; info.leftMargin = 0; info.rightMargin = 0
        info.horizontalPagination = .clip
        info.verticalPagination = .clip
        info.isHorizontallyCentered = false
        info.isVerticallyCentered = false
        return info
    }

    var pageSize: CGSize { printInfo.paperSize }
    var pageCount: Int { settings.layout.pageCount(images: items.count, page: pageSize) }

    var paperDescription: String {
        let s = pageSize
        let name = printInfo.localizedPaperName ?? printInfo.paperName?.rawValue ?? "Paper"
        return String(format: "%@ · %.2f × %.2f in · %@", name, s.width / 72, s.height / 72,
                      printInfo.orientation == .landscape ? "Landscape" : "Portrait")
    }

    /// Imageable area inside the paper (the printer's hardware margins), in points.
    var hardwareMargins: String {
        let b = printInfo.imageablePageBounds
        let s = pageSize
        let m = [b.minX, s.height - b.maxY, b.minX, s.width - b.maxX].map { $0 / 72 }
        return String(format: "Printer margins ≈ %.2f in top/bottom, %.2f in sides", max(m[0], m[1]), max(m[2], m[3]))
    }

    // MARK: Sheet

    func prepare(items: [PhotoItem], title: String, loader: ThumbnailLoader) {
        guard !isRunning else { return }
        self.items = items
        self.title = title
        error = nil
        previewPage = 0
        thumbnails = [:]
        requests.forEach { $0.cancel() }
        requests = []
        if profiles.isEmpty { profiles = printerProfiles() }
        if settings.profilePath == nil { settings.profilePath = profiles.first?.path }
        for (i, item) in items.enumerated() {
            if let r = loader.request(item, tier: .thumbnail, completion: { [weak self] image in
                self?.thumbnails[i] = image
            }) { requests.append(r) }
        }
    }

    func pageSetup() {
        let layout = NSPageLayout()
        // Modal: the sheet stays up; NSPageLayout edits the print info in place.
        if layout.runModal(with: printInfo) == NSApplication.ModalResponse.OK.rawValue {
            let info = printInfo
            printInfo = info.copy() as! NSPrintInfo  // swiftlint:disable:this force_cast
            previewPage = min(previewPage, max(pageCount - 1, 0))
        }
    }

    func chooseProfileFile(in window: NSWindow?) {
        let panel = NSOpenPanel()
        panel.title = "Printer Profile"
        panel.message = "Choose an ICC output profile for your printer and paper"
        panel.allowedContentTypes = [UTType(filenameExtension: "icc"), UTType(filenameExtension: "icm")].compactMap { $0 }
        let handle: (NSApplication.ModalResponse) -> Void = { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated {
                guard let self else { return }
                do {
                    let p = try describePrinterProfile(path: url.path)
                    if !self.profiles.contains(where: { $0.path == p.path }) { self.profiles.append(p) }
                    self.settings.profilePath = p.path
                } catch { self.error = error.localizedDescription }
            }
        }
        if let window { panel.beginSheetModal(for: window, completionHandler: handle) } else { handle(panel.runModal()) }
    }

    func previewComposer() -> PrintComposer {
        let names = items.map(\.name)
        let thumbs = thumbnails
        return PrintComposer(layout: settings.layout, pageSize: pageSize, picture: { thumbs[$0] },
                             caption: { names[$0] }, fallbackAspect: { _ in 1.5 }, count: items.count)
    }

    /// Asks where to save, then runs. Sheet presentation belongs to the caller (it closes first).
    func chooseDestination(_ kind: String, in window: NSWindow?, then run: @escaping @MainActor (URL) -> Void) {
        let panel = NSSavePanel()
        panel.canCreateDirectories = true
        let base = items.count == 1 ? (items[0].name as NSString).deletingPathExtension
            : settings.layout.style == .contactSheet ? "Contact Sheet" : "Prints"
        if kind == "pdf" {
            panel.title = "Save as PDF"
            panel.allowedContentTypes = [.pdf]
            panel.nameFieldStringValue = "\(base).pdf"
        } else {
            panel.title = "Save Pages as JPEG"
            panel.allowedContentTypes = [.jpeg]
            panel.nameFieldStringValue = "\(base).jpg"
            panel.message = String(format: "One JPEG per page at %.0f dpi. Several pages get -1, -2, … names.", settings.fileDPI)
        }
        let handle: (NSApplication.ModalResponse) -> Void = { response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { run(url) }
        }
        if let window { panel.beginSheetModal(for: window, completionHandler: handle) } else { handle(panel.runModal()) }
    }

    // MARK: Run

    /// Renders every photo, then produces `output`. Returns immediately; progress in the window.
    func run(_ output: Output, engine: Engine, window: NSWindow?, completion: (@MainActor (Bool) -> Void)? = nil) {
        guard !isRunning, !items.isEmpty else { return }
        let settings = settings
        let page = pageSize
        guard let cell = settings.layout.cells(page: page).first else {
            error = "The margins leave no room on the page"
            return
        }
        if settings.colorHandling == .application, settings.profilePath == nil {
            error = "Choose a printer profile, or let the printer manage colour"
            return
        }
        let box = settings.renderBox(for: settings.layout.imageArea(in: cell))
        let requests = items.compactMap { item in item.engineImage.map { (item.name, $0.imageID) } }
        let profile: PrintProfile? = settings.colorHandling == .application ? settings.profilePath.map {
            PrintProfile(path: $0, intent: settings.intent == .perceptual ? .perceptual : .relativeColorimetric,
                         blackPointCompensation: settings.blackPointCompensation)
        } : nil
        let sharpening: PrintSharpening = switch settings.sharpening {
        case .none: .none
        case .matte: .matte
        case .glossy: .glossy
        }
        let cancel = CancelFlag()
        cancelFlag = cancel
        error = nil
        progress = Progress(done: 0, total: requests.count, current: "", output: output)
        let names = items.map(\.name)
        Task.detached(priority: .userInitiated) {
            var pictures: [Int: CGImage] = [:]
            var failures: [String] = []
            for (i, (name, id)) in requests.enumerated() {
                if cancel.isCancelled() { break }
                await MainActor.run { self.progress?.current = name; self.progress?.done = i }
                do {
                    let image = try engine.renderForPrint(
                        request: PrintRenderRequest(imageId: id, maxWidth: box.width, maxHeight: box.height,
                                                    sharpening: sharpening, profile: profile),
                        cancel: cancel)
                    if let cg = Self.cgImage(image) { pictures[i] = cg } else { failures.append("\(name): unusable pixels") }
                } catch {
                    if cancel.isCancelled() { break }
                    failures.append("\(name): \(error.localizedDescription)")
                }
            }
            let wasCancelled = cancel.isCancelled()
            let rendered = pictures
            let failed = failures
            await MainActor.run {
                self.progress?.done = requests.count
                self.cancelFlag = nil
                guard !wasCancelled else {
                    self.progress = nil
                    self.onMessage("Printing cancelled", [])
                    completion?(false)
                    return
                }
                let composer = PrintComposer(layout: settings.layout, pageSize: page, picture: { rendered[$0] },
                                             caption: { names[$0] }, fallbackAspect: { _ in 1.5 }, count: names.count)
                let ok = self.produce(output, composer: composer, settings: settings, window: window, failures: failed)
                self.progress = nil
                completion?(ok)
            }
        }
    }

    func cancel() { cancelFlag?.cancel() }

    /// Printer / PDF / JPEG from finished renders.
    private func produce(_ output: Output, composer: PrintComposer, settings: PrintSettings, window: NSWindow?,
                         failures: [String]) -> Bool {
        let pages = composer.pages.count
        switch output {
        case .printer, .pdf:
            let view = PrintPageView(composer: composer)
            let info = printInfo.copy() as! NSPrintInfo  // swiftlint:disable:this force_cast
            let op: NSPrintOperation
            if case .pdf(let url) = output {
                try? FileManager.default.removeItem(at: url)
                info.jobDisposition = .save
                info.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = url
                op = NSPrintOperation(view: view, printInfo: info)
                op.showsPrintPanel = false
                op.showsProgressPanel = false
            } else {
                op = NSPrintOperation(view: view, printInfo: info)
                op.showsPrintPanel = true
                op.printPanel.options.insert([.showsPaperSize, .showsOrientation, .showsPreview])
            }
            op.jobTitle = title
            if case .pdf(let url) = output {
                let ok = op.run()
                let written = ok && FileManager.default.fileExists(atPath: url.path)
                onMessage(written ? "Saved \(pages) page\(pages == 1 ? "" : "s") to \(url.lastPathComponent)"
                                  : "Could not save the PDF", failures)
                return written
            }
            if let window {
                op.runModal(for: window, delegate: nil, didRun: nil, contextInfo: nil)
            } else {
                op.run()
            }
            if !failures.isEmpty { onMessage("Some photos could not be rendered", failures) }
            return true
        case .jpeg(let url):
            do {
                let written = try Self.writeJPEGPages(composer: composer, dpi: settings.fileDPI, to: url,
                                                      settings: settings, profile: settings.colorHandling == .application ? settings.profilePath : nil)
                onMessage("Saved \(written.count) JPEG page\(written.count == 1 ? "" : "s") at \(Int(settings.fileDPI)) dpi", failures)
                return true
            } catch {
                onMessage("Could not save JPEG pages: \(error.localizedDescription)", failures)
                return false
            }
        }
    }

    // MARK: Pixels

    nonisolated static func cgImage(_ image: PrintImage) -> CGImage? {
        guard let space = CGColorSpace(iccData: image.icc as CFData),
              space.numberOfComponents == Int(image.channels),
              let provider = CGDataProvider(data: image.data as CFData) else { return nil }
        let bpp = 8 * Int(image.channels)
        return CGImage(width: Int(image.width), height: Int(image.height), bitsPerComponent: 8, bitsPerPixel: bpp,
                       bytesPerRow: Int(image.width) * Int(image.channels), space: space,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue), provider: provider,
                       decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }

    /// One JPEG per page at `dpi`, tagged with that resolution. RGB output profiles are kept as
    /// the file's colour space; otherwise (printer-managed, CMYK or gray profiles) Display P3.
    static func writeJPEGPages(composer: PrintComposer, dpi: Double, to url: URL, settings: PrintSettings,
                               profile: String?) throws -> [URL] {
        let space: CGColorSpace = profile.flatMap { path in
            (try? Data(contentsOf: URL(fileURLWithPath: path))).flatMap { CGColorSpace(iccData: $0 as CFData) }
        }.flatMap { $0.model == .rgb ? $0 : nil } ?? CGColorSpace(name: CGColorSpace.displayP3)!
        let scale = dpi / 72
        let width = Int((composer.pageSize.width * scale).rounded()), height = Int((composer.pageSize.height * scale).rounded())
        guard width > 0, height > 0, width * height <= 400_000_000 else {
            throw NSError(domain: "Tessera", code: 1, userInfo: [NSLocalizedDescriptionKey: "page too large at this resolution"])
        }
        var urls: [URL] = []
        let pages = composer.pages.count
        let stem = url.deletingPathExtension().lastPathComponent
        for page in 0..<pages {
            guard let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                                      space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
                throw NSError(domain: "Tessera", code: 2, userInfo: [NSLocalizedDescriptionKey: "no bitmap context"])
            }
            // Flipped points, like the print view.
            ctx.translateBy(x: 0, y: CGFloat(height))
            ctx.scaleBy(x: scale, y: -scale)
            composer.draw(page: page, in: ctx)
            guard let image = ctx.makeImage() else { continue }
            let target = pages == 1 ? url : url.deletingLastPathComponent().appendingPathComponent("\(stem)-\(page + 1).jpg")
            guard let dest = CGImageDestinationCreateWithURL(target as CFURL, UTType.jpeg.identifier as CFString, 1, nil) else {
                throw NSError(domain: "Tessera", code: 3, userInfo: [NSLocalizedDescriptionKey: "cannot write \(target.path)"])
            }
            CGImageDestinationAddImage(dest, image, [
                kCGImagePropertyDPIWidth: dpi, kCGImagePropertyDPIHeight: dpi,
                kCGImageDestinationLossyCompressionQuality: 0.95,
            ] as CFDictionary)
            guard CGImageDestinationFinalize(dest) else {
                throw NSError(domain: "Tessera", code: 4, userInfo: [NSLocalizedDescriptionKey: "cannot write \(target.path)"])
            }
            urls.append(target)
        }
        return urls
    }
}

/// Non-modal print rendering progress above the status bar, with Cancel.
struct PrintProgressBar: View {
    let printing: PrintController
    var body: some View {
        if let p = printing.progress {
            ProgressStrip(title: "\(p.output.title) · rendering for print", done: Int(p.done), total: Int(p.total), current: p.current) {
                Button("Cancel") { printing.cancel() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("print-cancel")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("print-progress")
        }
    }
}

