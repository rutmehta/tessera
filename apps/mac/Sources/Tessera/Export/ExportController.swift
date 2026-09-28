import AppKit
import Observation
import TesseraCore
import TesseraFFI
import UniformTypeIdentifiers

/// State of File ▸ Export… (WP M2-20, docs/01 §2.22). The sheet edits `settings` (starting from a
/// preset) for a target (the selection or the current album); the export itself runs without a
/// sheet, with progress and Cancel in the main window, and ends in a results toast.
@MainActor @Observable
final class ExportController {
    /// What gets exported.
    struct Target: Identifiable, Equatable {
        enum Kind: Equatable { case selection, view }
        let kind: Kind
        let title: String
        let target: ExportTarget
        let count: Int
        let firstName: String
        let firstDate: Date
        var id: String { "\(kind)-\(title)" }
        static func == (a: Target, b: Target) -> Bool { a.id == b.id && a.count == b.count }
    }

    var settings: ExportSettings { didSet { if settings != oldValue { settingsChanged() } } }
    /// Preset the settings came from; nil once edited ("Custom").
    private(set) var presetName: String?
    private(set) var presets: [ExportPresetEntry] = []
    private(set) var targets: [Target] = []
    var targetID: String? {
        didSet { if targetID != oldValue { previewGeneration = UUID() } }
    }
    var error: String?

    /// The last watermark and JPEG size limit, so None ↔ Text ↔ Graphic and the limit's checkbox
    /// (or a format that drops them) never lose what the user typed.
    @ObservationIgnored var watermarkDraft = ExportWatermark()
    @ObservationIgnored var sizeLimitDraftKB = 500

    /// Watermark preview rendered by the engine (a small PNG export of the first photo).
    private(set) var enginePreview: NSImage?
    /// The watermark the engine preview was rendered with (stale once the settings differ).
    private(set) var enginePreviewMark: ExportWatermark?
    private(set) var isRenderingPreview = false
    private(set) var previewError: String?
    @ObservationIgnored private var previewGeneration = UUID()

    /// Non-nil while an export runs (the sheet is closed meanwhile).
    private(set) var progress: ExportProgress?
    private(set) var starting = false
    private(set) var lastReport: ExportReport?
    /// Warnings of the last finished run (M2-51), shown in the completion toast.
    private(set) var lastWarnings = ExportWarnings()
    /// The last run's report with its warnings, for `ExportReport.toastLines`.
    static var warningsByReport: (report: ExportReport, warnings: ExportWarnings)?
    private(set) var runningTitle = ""

    /// Called with the report when a run ends (toast, statuses, Finder).
    @ObservationIgnored var onFinish: (ExportReport, ExportSettings) -> Void = { _, _ in }
    /// Synchronous host reservation; the caller owns the captured library and target set.
    @ObservationIgnored var acquireSaveGate: ((Set<String>) -> DevelopRecoveryCoordinator.Gate?)?
    @ObservationIgnored private var engine: Engine?
    @ObservationIgnored private var cancelFlag: CancelFlag?
    @ObservationIgnored private var cancelledBeforeRun = false
    @ObservationIgnored private var applyingPreset = false
    /// Observation's generated accessors run `didSet` during init too; persist only afterwards.
    @ObservationIgnored private var ready = false

    private static let settingsKey = "ExportSettings"
    private static let presetKey = "ExportPresetName"

    var isRunning: Bool { progress != nil || starting }
    var target: Target? { targets.first { $0.id == targetID } ?? targets.first }

    private func savedGate(for target: ExportTarget) -> DevelopRecoveryCoordinator.Gate? {
        guard case .images(let ids) = target else { return nil }
        return acquireSaveGate?(Set(ids))
    }

    init() {
        let saved = UserDefaults.standard.string(forKey: Self.settingsKey).flatMap { try? ExportSettings(json: $0) }
        var initial = saved ?? ExportSettings()
        if initial.destination.isEmpty { initial.destination = Self.defaultDestination.path }
        settings = initial
        presetName = saved == nil ? nil : UserDefaults.standard.string(forKey: Self.presetKey)
        ready = true
    }

    static var defaultDestination: URL {
        FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Tessera Export", isDirectory: true)
    }

    private func settingsChanged() {
        previewGeneration = UUID()
        guard ready else { return }
        if let mark = settings.watermark { watermarkDraft = mark }
        if let kb = settings.maxFileKilobytes { sizeLimitDraftKB = kb }
        if !applyingPreset { presetName = nil }
        UserDefaults.standard.set(settings.json, forKey: Self.settingsKey)
        UserDefaults.standard.set(presetName, forKey: Self.presetKey)
    }

    // MARK: Sheet

    /// Refreshes presets and targets before the sheet opens.
    func prepare(engine: Engine, targets: [Target], preferred: Target.Kind) {
        previewGeneration = UUID()
        enginePreview = nil
        enginePreviewMark = nil
        self.engine = engine
        self.targets = targets
        targetID = (targets.first { $0.kind == preferred } ?? targets.first)?.id
        error = nil
        reloadPresets()
        // First export ever: start from the first shipped preset (Web).
        if UserDefaults.standard.string(forKey: Self.settingsKey) == nil, let first = presets.first { apply(first) }
    }

    func reloadPresets() {
        guard let engine else { return }
        do { presets = try ExportPresetStore(engine: engine).list() } catch { self.error = "Presets: \(error.localizedDescription)" }
    }

    /// Takes a preset's settings; an empty preset destination keeps the current folder.
    func apply(_ preset: ExportPresetEntry) {
        var s = preset.settings
        if s.destination.isEmpty { s.destination = settings.destination.isEmpty ? Self.defaultDestination.path : settings.destination }
        applyingPreset = true
        presetName = preset.name
        settings = s
        applyingPreset = false
        settingsChanged()
    }

    func savePreset(named name: String) {
        guard let engine else { return }
        do {
            try ExportPresetStore(engine: engine).save(name, settings)
            reloadPresets()
            presetName = presets.first { $0.name == name.trimmingCharacters(in: .whitespaces) }?.name
            settingsChanged()
        } catch { self.error = error.localizedDescription }
    }

    func deletePreset(_ name: String) {
        guard let engine else { return }
        do {
            try ExportPresetStore(engine: engine).delete(name)
            if presetName == name { presetName = nil }
            reloadPresets()
        } catch { self.error = error.localizedDescription }
    }

    func restoreDefaultPresets() {
        guard let engine else { return }
        do { try ExportPresetStore(engine: engine).restoreDefaults(); reloadPresets() } catch { self.error = error.localizedDescription }
    }

    func chooseDestination(in window: NSWindow?) {
        let panel = NSOpenPanel()
        panel.title = "Export To"
        panel.prompt = "Choose"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = URL(fileURLWithPath: settings.destination)
        let handle: (NSApplication.ModalResponse) -> Void = { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { self?.settings.destination = url.path }
        }
        if let window { panel.beginSheetModal(for: window, completionHandler: handle) } else { handle(panel.runModal()) }
    }

    /// The live file-name example under the naming field.
    var namingExample: String {
        guard let t = target else { return "" }
        return ExportNaming.example(template: settings.naming, firstName: (t.firstName as NSString).deletingPathExtension,
                                    date: t.firstDate, format: settings.format, count: t.count)
    }

    /// Switches the format, keeping every other field the engine accepts for it.
    func setFormat(_ format: ExportSettings.OutputFormat) {
        var s = settings
        s.format = format
        s.normalizeForFormat()
        settings = s
    }

    /// None (nil), Text or Graphic, restoring the last-used fields of that kind.
    func setWatermarkKind(_ kind: ExportWatermark.Kind?) {
        guard let kind else { settings.watermark = nil; return }
        var mark = watermarkDraft
        mark.kind = kind
        if kind == .text, mark.font.isEmpty { mark.font = ExportWatermark.defaultFontPath }
        settings.watermark = mark
    }

    func setSizeLimit(_ on: Bool) {
        settings.maxFileKilobytes = on ? max(sizeLimitDraftKB, 1) : nil
    }

    /// Opens a panel for a PNG (graphic) or a .ttf / .otf font file (text).
    func chooseWatermarkFile(font: Bool, in window: NSWindow?) {
        let panel = NSOpenPanel()
        panel.title = font ? "Choose a Font File" : "Choose a Watermark Graphic"
        panel.prompt = "Choose"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = font ? [UTType("public.truetype-ttf-font"), UTType("public.opentype-font")].compactMap { $0 } : [.png]
        let handle: (NSApplication.ModalResponse) -> Void = { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated {
                guard let self, var mark = self.settings.watermark else { return }
                if font { mark.font = url.path } else { mark.path = url.path }
                self.settings.watermark = mark
            }
        }
        if let window { panel.beginSheetModal(for: window, completionHandler: handle) } else { handle(panel.runModal()) }
    }

    /// The first photo of a selection target (album targets resolve in the engine, so they have
    /// no photo id here); the watermark preview renders it.
    var previewImageID: String? {
        let order = [target].compactMap { $0 } + targets
        for t in order { if case .images(let ids) = t.target, let first = ids.first { return first } }
        return nil
    }

    /// Renders the first photo through the engine as a 480 px PNG with the current watermark
    /// (the exact compositor `export_batch` uses), into a temporary folder.
    func renderWatermarkPreview() {
        guard let engine, let id = previewImageID, let mark = settings.watermark, !isRenderingPreview else { return }
        let targetID = self.targetID
        let capturedSettings = settings
        if let problem = mark.problem { previewError = problem; return }
        guard let gate = acquireSaveGate?([id]) else {
            previewError = "This preview's photo is no longer available for a saved read"
            return
        }
        var s = settings
        s.format = .png
        s.normalizeForFormat()
        s.watermark = mark
        s.colorSpace = .srgb
        s.resize = .init()
        s.resize.mode = .longEdge
        s.resize.longEdge = 480
        s.upscale = 1
        s.metadata = .none
        s.naming = "preview"
        s.onConflict = .unique
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("tessera-watermark-\(UUID().uuidString)")
        s.destination = folder.path
        let json = s.json
        let generation = UUID()
        previewGeneration = generation
        isRenderingPreview = true
        previewError = nil
        Task {
            defer { gate.finish() }
            let admitted = await gate.result().isSaved
            guard previewGeneration == generation, self.engine === engine,
                  self.targetID == targetID, self.previewImageID == id,
                  self.settings == capturedSettings else {
                isRenderingPreview = false
                return
            }
            guard admitted else {
                isRenderingPreview = false
                previewError = "Finish saving the photo before previewing its watermark"
                return
            }
            let result = await Task.detached(priority: .userInitiated) {
                Result { try engine.exportBatch(target: .images(imageIds: [id]), settingsJson: json,
                                                listener: nil, cancel: nil) }
            }.value
            let image: NSImage?
            var failure: String?
            switch result {
            case .success(let report):
                image = report.items.first?.outputPath.flatMap { NSImage(contentsOfFile: $0) }
                if image == nil { failure = report.items.first?.error ?? "The engine wrote no preview" }
            case .failure(let e):
                image = nil
                failure = e.localizedDescription
            }
            try? FileManager.default.removeItem(at: folder)
            isRenderingPreview = false
            guard previewGeneration == generation, self.engine === engine,
                  self.targetID == targetID, self.previewImageID == id,
                  self.settings == capturedSettings else { return }
            enginePreview = image
            enginePreviewMark = image == nil ? nil : mark
            previewError = failure
        }
    }

    var namingIsValid: Bool {
        if case .success = ExportNaming.fileName(template: settings.naming, name: "IMG", sequence: 1, date: "2026-01-01",
                                                 extension: settings.format.fileExtension) { return true }
        return false
    }

    /// Validates with the engine (the authority on every rule), or returns the problem.
    func validate() -> String? {
        if settings.destination.isEmpty || !settings.destination.hasPrefix("/") { return "Choose an export folder" }
        if let problem = settings.watermark?.problem { return problem }
        do { _ = try normalizeExportSettings(json: settings.json); return nil } catch { return error.localizedDescription }
    }

    // MARK: Run (non-modal)

    func start() {
        guard let engine, let target, !isRunning else { return }
        if let problem = validate() { error = problem; return }
        guard let gate = savedGate(for: target.target) else {
            error = "This export target is no longer available for a saved read"
            return
        }
        let settings = settings
        let ffiTarget = target.target
        let json = settings.json
        cancelledBeforeRun = false
        starting = true
        Task {
            defer { gate.finish() }
            let admitted = await gate.result().isSaved
            guard !cancelledBeforeRun else {
                starting = false
                error = "Export cancelled"
                return
            }
            guard admitted, self.target?.id == target.id,
                  self.settings == settings else {
                starting = false
                error = "Finish saving the photo before Export"
                return
            }
            let cancel = CancelFlag()
            cancelFlag = cancel
            lastReport = nil
            lastWarnings = ExportWarnings()
            runningTitle = target.title
            progress = ExportProgress(done: 0, total: UInt32(target.count), exported: 0, failed: 0, current: "")
            starting = false
            let relay = ExportRelay { [weak self] p in
                Task { @MainActor in if self?.progress != nil { self?.progress = p } }
            }
            let result = await Task.detached(priority: .userInitiated) {
                Result { try engine.exportBatch(target: ffiTarget, settingsJson: json, listener: relay,
                                                cancel: cancel) }
            }.value
            // Recoverable omissions ("Lens Blur skipped: …") the engine wrote beside each file.
            let warnings = (try? result.get()).map { ExportWarnings.read($0) } ?? ExportWarnings()
            progress = nil
            cancelFlag = nil
            switch result {
            case .success(let report):
                lastReport = report
                lastWarnings = warnings
                Self.warningsByReport = (report, warnings)
                onFinish(report, settings)
            case .failure(let e):
                error = e.localizedDescription
                onFailure(e.localizedDescription)
            }
        }
    }

    @ObservationIgnored var onFailure: (String) -> Void = { _ in }

    func cancel() {
        if starting { cancelledBeforeRun = true }
        cancelFlag?.cancel()
    }
}

/// Engine progress (exporting thread) → main actor.
final class ExportRelay: ExportProgressListener, @unchecked Sendable {
    let handler: @Sendable (ExportProgress) -> Void
    init(_ handler: @escaping @Sendable (ExportProgress) -> Void) { self.handler = handler }
    func onProgress(progress: ExportProgress) { handler(progress) }
}

extension ExportReport {
    /// Toast headline and detail lines: failures listed by file, then the export warnings the
    /// engine recorded (e.g. `Lens Blur skipped: depth model is not cached`).
    @MainActor var toastLines: (headline: String, details: [String]) {
        let warnings = ExportController.warningsByReport.flatMap { $0.report == self ? $0.warnings : nil }
            ?? ExportWarnings.read(self)
        return ExportWarnings.toastLines(self, warnings: warnings)
    }
}
