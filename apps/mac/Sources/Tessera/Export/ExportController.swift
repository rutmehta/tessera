import AppKit
import Observation
import TesseraCore
import TesseraFFI

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
    var targetID: String?
    var error: String?

    /// Non-nil while an export runs (the sheet is closed meanwhile).
    private(set) var progress: ExportProgress?
    private(set) var lastReport: ExportReport?
    private(set) var runningTitle = ""

    /// Called with the report when a run ends (toast, statuses, Finder).
    @ObservationIgnored var onFinish: (ExportReport, ExportSettings) -> Void = { _, _ in }
    @ObservationIgnored private var engine: Engine?
    @ObservationIgnored private var cancelFlag: CancelFlag?
    @ObservationIgnored private var applyingPreset = false
    /// Observation's generated accessors run `didSet` during init too; persist only afterwards.
    @ObservationIgnored private var ready = false

    private static let settingsKey = "ExportSettings"
    private static let presetKey = "ExportPresetName"

    var isRunning: Bool { progress != nil }
    var target: Target? { targets.first { $0.id == targetID } ?? targets.first }

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
        guard ready else { return }
        if !applyingPreset { presetName = nil }
        UserDefaults.standard.set(settings.json, forKey: Self.settingsKey)
        UserDefaults.standard.set(presetName, forKey: Self.presetKey)
    }

    // MARK: Sheet

    /// Refreshes presets and targets before the sheet opens.
    func prepare(engine: Engine, targets: [Target], preferred: Target.Kind) {
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

    var namingIsValid: Bool {
        if case .success = ExportNaming.fileName(template: settings.naming, name: "IMG", sequence: 1, date: "2026-01-01",
                                                 extension: settings.format.fileExtension) { return true }
        return false
    }

    /// Validates with the engine (the authority on every rule), or returns the problem.
    func validate() -> String? {
        if settings.destination.isEmpty || !settings.destination.hasPrefix("/") { return "Choose an export folder" }
        do { _ = try normalizeExportSettings(json: settings.json); return nil } catch { return error.localizedDescription }
    }

    // MARK: Run (non-modal)

    func start() {
        guard let engine, let target, !isRunning else { return }
        if let problem = validate() { error = problem; return }
        let settings = settings
        let cancel = CancelFlag()
        cancelFlag = cancel
        lastReport = nil
        runningTitle = target.title
        progress = ExportProgress(done: 0, total: UInt32(target.count), exported: 0, failed: 0, current: "")
        let relay = ExportRelay { [weak self] p in
            Task { @MainActor in if self?.progress != nil { self?.progress = p } }
        }
        let ffiTarget = target.target
        let json = settings.json
        Task.detached(priority: .userInitiated) {
            let result = Result { try engine.exportBatch(target: ffiTarget, settingsJson: json, listener: relay, cancel: cancel) }
            await MainActor.run {
                self.progress = nil
                self.cancelFlag = nil
                switch result {
                case .success(let report):
                    self.lastReport = report
                    self.onFinish(report, settings)
                case .failure(let e):
                    self.error = e.localizedDescription
                    self.onFailure(e.localizedDescription)
                }
            }
        }
    }

    @ObservationIgnored var onFailure: (String) -> Void = { _ in }

    func cancel() { cancelFlag?.cancel() }
}

/// Engine progress (exporting thread) → main actor.
final class ExportRelay: ExportProgressListener, @unchecked Sendable {
    let handler: @Sendable (ExportProgress) -> Void
    init(_ handler: @escaping @Sendable (ExportProgress) -> Void) { self.handler = handler }
    func onProgress(progress: ExportProgress) { handler(progress) }
}

extension ExportReport {
    /// Toast headline and detail lines (failures listed by file).
    var toastLines: (headline: String, details: [String]) {
        let folder = URL(fileURLWithPath: destination).lastPathComponent
        let photos = { (n: UInt32) in "\(n) photo\(n == 1 ? "" : "s")" }
        var headline: String
        if cancelled {
            headline = "Export cancelled: \(photos(exported)) written to \(folder)"
        } else if failed == 0 {
            headline = "Exported \(photos(exported)) to \(folder) in \(String(format: "%.1f", seconds)) s"
        } else {
            headline = "Exported \(photos(exported)) to \(folder); \(failed) failed"
        }
        if exported == 0, failed == 0, !cancelled { headline = "Nothing was exported" }
        let details = items.compactMap { item in item.error.map { "\(item.name): \($0)" } }
        return (headline, details)
    }
}
