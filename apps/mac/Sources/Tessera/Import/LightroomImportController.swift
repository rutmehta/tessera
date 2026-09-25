import AppKit
import Observation
import TesseraCore
import TesseraFFI

/// State of File ▸ Import Lightroom Catalog… (WP M2-13b). The sheet walks choose → summary →
/// mapping → fidelity; the import itself runs without a sheet (progress in the main window,
/// cancellable) and the sheet comes back with the report when it finishes.
@MainActor @Observable
final class LightroomImportController {
    enum Step: Int, CaseIterable, Comparable {
        case choose, summary, mapping, fidelity, report
        static func < (a: Step, b: Step) -> Bool { a.rawValue < b.rawValue }
        var title: String {
            switch self {
            case .choose: "Catalog"
            case .summary: "Summary"
            case .mapping: "Mapping"
            case .fidelity: "Fidelity"
            case .report: "Report"
            }
        }
    }

    var step: Step = .choose
    private(set) var catalogURL: URL?
    /// Blocking engine work in flight (reading the catalog, planning, rendering samples).
    private(set) var busy: String?
    var error: String?

    private(set) var summary: LrcatSummary?
    var folders: FolderMappingTable? { didSet { if folders != oldValue { schedulePlan() } } }
    var marks: MarkMappingTable? { didSet { if marks != oldValue { schedulePlan() } } }
    var overwriteExistingEdits = false { didSet { if overwriteExistingEdits != oldValue { schedulePlan() } } }
    private(set) var preview: LrcatPlanPreview?

    var fidelity: FidelityGrid?
    private(set) var fidelityResult: LrcatFidelity?
    private(set) var fidelityOptions: LrcatOptions?

    /// Non-nil while an import runs (the sheet is closed meanwhile).
    private(set) var progress: LrcatProgress?
    private(set) var report: LrcatReport?
    private(set) var reportURL: URL?
    private(set) var reportMarkdown: String?
    private(set) var reportOptions: LrcatOptions?

    /// Asked to show the sheet again (the report) and to open the imported library.
    @ObservationIgnored var presentSheet: () -> Void = {}
    @ObservationIgnored var openLibrary: (URL, String) -> Void = { _, _ in }

    @ObservationIgnored private var engine: Engine?
    @ObservationIgnored private var importer: LrcatImport?
    @ObservationIgnored private var planGeneration = 0

    var isRunning: Bool { progress != nil }

    var options: LrcatOptions? {
        guard let folders, let marks else { return nil }
        return folders.options(marks: marks.mappings, overwrite: overwriteExistingEdits)
    }

    // MARK: Steps

    /// Starts over (keeps nothing from a previous catalog). Not while an import runs.
    func reset() {
        guard !isRunning else { return }
        importer = nil
        catalogURL = nil; summary = nil; folders = nil; marks = nil; preview = nil
        fidelity = nil; fidelityResult = nil; fidelityOptions = nil
        report = nil; reportURL = nil; reportMarkdown = nil; reportOptions = nil
        overwriteExistingEdits = false
        error = nil; busy = nil
        step = .choose
    }

    func chooseCatalog(in window: NSWindow?) {
        let panel = NSOpenPanel()
        panel.title = "Import Lightroom Catalog"
        panel.message = "Choose a Lightroom Classic catalog (.lrcat). It is only read, never changed."
        panel.prompt = "Choose"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.init(filenameExtension: "lrcat") ?? .data]
        let handle: (NSApplication.ModalResponse) -> Void = { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { self?.open(url) }
        }
        if let window { panel.beginSheetModal(for: window, completionHandler: handle) } else { handle(panel.runModal()) }
    }

    func open(_ url: URL) {
        guard !isRunning else { return }
        reset()
        catalogURL = url
        busy = "Reading \(url.lastPathComponent)…"
        let support = EngineLibrary.defaultSupportDirectory.path
        let existing = engine
        Task.detached(priority: .userInitiated) {
            let result = Result { () throws -> (Engine, LrcatImport) in
                let engine = try existing ?? Engine.open(appSupportDir: support)
                return (engine, try engine.openLrcat(path: url.path))
            }
            await MainActor.run {
                guard self.catalogURL == url else { return }
                self.busy = nil
                switch result {
                case .success(let (engine, importer)):
                    self.engine = engine
                    self.importer = importer
                    self.summary = importer.summary()
                    let defaults = importer.defaultOptions()
                    self.folders = FolderMappingTable(options: defaults)
                    self.marks = MarkMappingTable(rows: defaults.marks.map { LrcatMarkRow(label: $0.label, mark: $0.mark, count: 0) })
                    self.step = .summary
                case .failure(let e):
                    self.error = "Could not read the catalog: \(e.localizedDescription)"
                }
            }
        }
    }

    func goTo(_ next: Step) {
        guard !isRunning else { return }
        error = nil
        step = next
        if next == .mapping { schedulePlan() }
        if next == .fidelity, fidelityOptions != options { runFidelity() }
    }

    // MARK: Plan preview

    private func schedulePlan() {
        guard let importer, let options else { return }
        planGeneration += 1
        let generation = planGeneration
        Task.detached(priority: .userInitiated) {
            let result = Result { try importer.plan(options: options) }
            await MainActor.run {
                guard generation == self.planGeneration else { return }
                switch result {
                case .success(let p):
                    self.preview = p
                    // Refresh counts; keep the user's choices.
                    if var table = self.marks {
                        let choices = Dictionary(uniqueKeysWithValues: table.rows.map { ($0.label, $0.choice) })
                        table = MarkMappingTable(rows: p.marks)
                        for (label, choice) in choices { table.set(label, to: choice) }
                        if table != self.marks { self.marks = table }
                    }
                    if var f = self.folders {
                        f.updateCounts(from: p)
                        if f != self.folders { self.folders = f }
                    }
                    self.error = nil
                case .failure(let e):
                    self.error = e.localizedDescription
                }
            }
        }
    }

    func locate(root: FolderMappingTable.Root, in window: NSWindow?) {
        let panel = NSOpenPanel()
        panel.title = "Locate \(URL(fileURLWithPath: root.catalogPath).lastPathComponent)"
        panel.message = "Lightroom had this folder at \(root.catalogPath). Choose where it is now."
        panel.prompt = "Locate"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        let handle: (NSApplication.ModalResponse) -> Void = { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { self?.folders?.relocate(root.catalogPath, to: url.path) }
        }
        if let window { panel.beginSheetModal(for: window, completionHandler: handle) } else { handle(panel.runModal()) }
    }

    func chooseLibraryFolder(in window: NSWindow?) {
        let panel = NSOpenPanel()
        panel.title = "Library Folder"
        panel.message = "library.json and import-report.md go here. Open this folder in Tessera to see the import."
        panel.prompt = "Choose"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        if let current = folders?.libraryFolder { panel.directoryURL = URL(fileURLWithPath: current) }
        let handle: (NSApplication.ModalResponse) -> Void = { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { self?.folders?.libraryFolder = url.path }
        }
        if let window { panel.beginSheetModal(for: window, completionHandler: handle) } else { handle(panel.runModal()) }
    }

    // MARK: Fidelity

    func runFidelity(count: UInt32 = 12) {
        guard let importer, let options else { return }
        busy = "Rendering \(count) sample photos…"
        fidelityOptions = options
        Task.detached(priority: .userInitiated) {
            let result = Result { try importer.fidelitySample(options: options, n: count, thumbPx: 240) }
            await MainActor.run {
                guard self.fidelityOptions == options else { return }
                self.busy = nil
                switch result {
                case .success(let f):
                    self.fidelityResult = f
                    let old = self.fidelity
                    var grid = FidelityGrid(samples: f.samples)
                    grid.sort = old?.sort ?? .largestDifference
                    grid.onlyDifferent = old?.onlyDifferent ?? false
                    self.fidelity = grid
                case .failure(let e):
                    self.error = "Fidelity preview failed: \(e.localizedDescription)"
                }
            }
        }
    }

    func cancelFidelity() { importer?.cancel() }

    // MARK: Import (non-modal)

    /// Closes the sheet and runs the import; progress shows in the main window.
    func startImport() {
        guard let importer, let options, !isRunning else { return }
        progress = LrcatProgress(phase: .preparing, done: 0, total: preview.map { $0.toImport } ?? 0, current: "")
        reportOptions = options
        let summary = summary
        let fidelity = fidelityResult
        let relay = ProgressRelay { [weak self] p in
            Task { @MainActor in if self?.progress != nil { self?.progress = p } }
        }
        Task.detached(priority: .userInitiated) {
            let result = Result { try importer.apply(options: options, listener: relay) }
            let written = result.map { report -> (LrcatReport, String, URL?) in
                let markdown = LightroomImportReport.markdown(report: report, summary: summary, options: options,
                                                              fidelity: fidelity)
                return (report, markdown, try? LightroomImportReport.write(markdown, report: report))
            }
            await MainActor.run {
                self.progress = nil
                switch written {
                case .success(let (report, markdown, url)):
                    self.report = report
                    self.reportMarkdown = markdown
                    self.reportURL = url
                    self.step = .report
                    if !report.cancelled {
                        let folder = URL(fileURLWithPath: report.libraryPath).deletingLastPathComponent()
                        self.openLibrary(folder, "Imported \(report.imported + report.resumed) photos from "
                                         + "\(URL(fileURLWithPath: report.catalogPath).lastPathComponent)")
                    }
                case .failure(let e):
                    self.error = "Import failed: \(e.localizedDescription)"
                    self.step = .fidelity
                }
                self.presentSheet()
            }
        }
    }

    func cancelImport() { importer?.cancel() }

    /// "Resume" after a cancel: same options, finished photos are skipped by the engine.
    func resume() {
        guard report?.cancelled == true else { return }
        report = nil; reportURL = nil; reportMarkdown = nil
        startImport()
    }
}

/// Engine progress (importing thread) → main actor.
final class ProgressRelay: LrcatProgressListener, @unchecked Sendable {
    let handler: @Sendable (LrcatProgress) -> Void
    init(_ handler: @escaping @Sendable (LrcatProgress) -> Void) { self.handler = handler }
    func onProgress(progress: LrcatProgress) { handler(progress) }
}

extension LrcatPhase {
    var title: String {
        switch self {
        case .preparing: "Preparing"
        case .writingEdits: "Writing edits"
        case .library: "Writing library.json"
        case .indexing: "Indexing"
        case .finished: "Finishing"
        }
    }
}
