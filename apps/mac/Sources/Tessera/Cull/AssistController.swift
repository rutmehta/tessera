import AppKit
import Observation
import TesseraCore
import enum TesseraFFI.AssistMode

/// Assisted culling (docs/06 §3, WP M3-11): real signals computed in the background, the
/// library's learner (assisted: predictions and a confidence order; automated: pre-filled
/// decisions to confirm), the face strip, people and the per-person filter. Suggestions never
/// change a decision until the photographer confirms them (Y) or dismisses them (N).
@MainActor @Observable
final class AssistController {
    @ObservationIgnored weak var app: AppModel?

    /// Toolbar ▸ Assist.
    private(set) var enabled = false
    /// Learner labels for this library (shown in the inspector).
    private(set) var labels: UInt64 = 0
    /// Sort the grid by keep confidence (likely keepers first, likely rejects last).
    var sortByConfidence = true { didSet { if sortByConfidence != oldValue { app?.refreshVisible() } } }
    /// Predictions by item id, and the review order (item ids).
    @ObservationIgnored private(set) var predictions: [Int: AssistPrediction] = [:]
    @ObservationIgnored private(set) var order: [Int] = []
    /// Pending pre-filled decisions (automated mode), by item id.
    private(set) var suggestionCount = 0
    /// Background analysis progress.
    private(set) var progress: (done: Int, total: Int, current: String, title: String)?
    @ObservationIgnored private var cancelAnalysis = false
    @ObservationIgnored private var analysisGeneration = 0
    private(set) var people: [PersonSummary] = []
    /// Per-person filter: the grid shows only these frames.
    private(set) var personFilter: (person: PersonSummary, eyesClosed: Bool, items: Set<Int>)?
    /// Faces of the focused photo (loupe face strip).
    private(set) var faces: [FaceChip] = []
    @ObservationIgnored private var facesItem: Int?

    var isRunning: Bool { progress != nil }

    // MARK: Library lifecycle

    /// A folder opened: forget the old state and measure what is missing (quality only; faces
    /// on request, since their weights download on first use).
    func libraryDidLoad(seedFaces: Bool) {
        enabled = false
        predictions = [:]
        order = []
        suggestionCount = 0
        people = []
        personFilter = nil
        faces = []
        facesItem = nil
        labels = 0
        guard let app, let lib = app.engineLibrary else { return }
        if seedFaces {
            do { try lib.seedSyntheticFaces() } catch { app.statusMessage = "Synthetic faces: \(error.localizedDescription)" }
        }
        analyze(faces: false, force: false, title: "Analyzing", announce: false)
        refreshPeople()
    }

    /// Cull ▸ Analyze Shoot / Analyze Faces: computes real signals off the main actor.
    /// `announce`: report the result in the status bar (off for the automatic pass on open, so
    /// the "Opened …" summary stays).
    func analyze(faces: Bool, force: Bool, title: String, announce: Bool = true) {
        guard let app, let lib = app.engineLibrary, !isRunning else { return }
        analysisGeneration += 1
        let generation = analysisGeneration
        cancelAnalysis = false
        let ids = Array(lib.items.indices)
        progress = (0, ids.count, "", title)
        Task.detached(priority: .utility) { [weak self] in
            let result = lib.analyze(ids, faces: faces, force: force) { done, total, name in
                let keepGoing = DispatchQueue.main.sync {
                    MainActor.assumeIsolated { () -> Bool in
                        guard let self, generation == self.analysisGeneration, !self.cancelAnalysis else { return false }
                        self.progress = (done, total, name, title)
                        return true
                    }
                }
                return keepGoing
            }
            await MainActor.run { self?.analysisDidFinish(result, faces: faces, generation: generation, announce: announce) }
        }
    }

    func cancel() { cancelAnalysis = true }

    private func analysisDidFinish(_ result: (analyzed: Int, errors: [String]), faces: Bool, generation: Int,
                                   announce: Bool) {
        guard generation == analysisGeneration, let app else { return }
        progress = nil
        if let first = result.errors.first {
            app.statusMessage = faces && first.contains("face models")
                ? "Face analysis unavailable: \(first)" : "Analysis: \(result.errors.count) failed (\(first))"
        } else if announce {
            app.statusMessage = "Analyzed \(result.analyzed) photo\(result.analyzed == 1 ? "" : "s")"
                + (faces ? " for faces" : "") + (cancelAnalysis ? " (cancelled)" : "")
        }
        if faces { refreshPeople(refresh: true) }
        refreshFaces(force: true)
        if enabled { refresh() }
    }

    // MARK: Assist mode

    func setEnabled(_ on: Bool) {
        guard let app else { return }
        guard app.isEngineBacked else {
            app.statusMessage = "Assisted culling needs a folder opened on the engine"
            return
        }
        let mode: AssistMode = on ? app.agent.preferences.assistMode : .off
        do {
            labels = try app.cull.setAssistMode(mode).labels
            enabled = on
        } catch {
            app.statusMessage = "Assist: \(error.localizedDescription)"
            return
        }
        if on {
            refresh()
            let automated = app.agent.preferences.assistAutomated
            app.statusMessage = "Assist on: " + (automated
                ? "\(suggestionCount) suggested decision\(suggestionCount == 1 ? "" : "s") · Y confirms all · N dismisses"
                : "sorted by keep confidence · decisions teach the learner")
        } else {
            predictions = [:]
            order = []
            suggestionCount = 0
            app.assistDidChange()
            app.statusMessage = "Assist off"
        }
    }

    /// Switches between assisted and automated (Cull ▸ Assist Mode); keeps Assist on.
    func setAutomated(_ automated: Bool) {
        guard let app else { return }
        app.agent.preferences.assistAutomated = automated
        app.agent.savePreferences()
        if enabled { setEnabled(true) }
    }

    /// Re-predicts every frame (after new signals, decisions or confirmations).
    func refresh() {
        guard enabled, let app else { return }
        do {
            let review = try app.cull.review()
            predictions = Dictionary(review.map { ($0.itemID, $0) }, uniquingKeysWith: { a, _ in a })
            order = review.map(\.itemID)
            suggestionCount = review.filter { $0.suggested != nil }.count
            labels = (try? app.cull.assistStatus().labels) ?? labels
        } catch {
            app.statusMessage = "Assist: \(error.localizedDescription)"
        }
        app.assistDidChange()
    }

    func suggestion(for id: Int) -> Decision? {
        guard enabled, let app, app.cull[id].decision == .undecided else { return nil }
        return predictions[id]?.suggested
    }

    /// Decisions were made by hand: their suggestions are gone (the engine learned from them).
    func itemsDecided(_ ids: [Int]) {
        guard enabled else { return }
        var changed = false
        for id in ids where predictions[id]?.suggested != nil && app?.cull[id].decision != .undecided {
            predictions[id]?.suggested = nil
            changed = true
        }
        if changed { suggestionCount = predictions.values.filter { $0.suggested != nil }.count }
        labels = (try? app?.cull.assistStatus().labels) ?? labels
    }

    /// Y: confirm every visible suggestion as one undoable step.
    func confirmAll() {
        guard let app else { return }
        guard enabled else { app.statusMessage = "Turn on Assist (toolbar) to get suggested decisions"; return }
        let ids = app.visibleIDs.filter { suggestion(for: $0) != nil }
        guard !ids.isEmpty else { app.statusMessage = "No suggested decisions to confirm"; return }
        let keeps = ids.filter { predictions[$0]?.suggested == .keep }.count
        guard app.runCull({ try app.cull.confirmSuggestions(ids) }) else { refresh(); return }
        app.showToast("Confirmed \(ids.count) suggestion\(ids.count == 1 ? "" : "s"): \(keeps) keep, \(ids.count - keeps) reject",
                      undoable: true)
        refresh()
    }

    /// N: reject (dismiss) the suggestion on the focused photo or the selection. Nothing is decided.
    func dismiss(_ ids: [Int]) {
        guard let app else { return }
        let withSuggestion = ids.filter { suggestion(for: $0) != nil }
        guard !withSuggestion.isEmpty else { app.statusMessage = "No suggestion here to dismiss"; return }
        do {
            try app.cull.dismissSuggestions(withSuggestion)
            for id in withSuggestion { predictions[id]?.suggested = nil }
            suggestionCount = predictions.values.filter { $0.suggested != nil }.count
            app.assistItemsChanged(withSuggestion)
            app.statusMessage = "Dismissed \(withSuggestion.count) suggestion\(withSuggestion.count == 1 ? "" : "s")"
        } catch {
            app.statusMessage = "Dismiss failed: \(error.localizedDescription)"
        }
    }

    // MARK: Faces and people

    func refreshPeople(refresh: Bool = false) {
        guard let app else { return }
        people = (try? app.cull.people(refresh: refresh)) ?? []
    }

    /// Loads the focused photo's face strip (cheap: an index read).
    func refreshFaces(force: Bool = false) {
        guard let app, let item = app.focusedItem else { faces = []; facesItem = nil; return }
        guard force || facesItem != item.id else { return }
        facesItem = item.id
        faces = app.isEngineBacked ? ((try? app.cull.faceStrip(item.id)) ?? []) : []
    }

    func person(_ id: String?) -> PersonSummary? { id.flatMap { pid in people.first { $0.id == pid } } }

    /// Shows only frames with this person (optionally only where their eyes read closed).
    func filter(person: PersonSummary, eyesClosed: Bool) {
        guard let app else { return }
        do {
            let items = try app.cull.items(withPerson: person.id, eyesClosedBelow: eyesClosed ? 0.3 : nil)
            personFilter = (person, eyesClosed, Set(items))
            app.refreshVisible()
            app.statusMessage = "\(person.name)\(eyesClosed ? " with eyes closed" : ""): \(items.count) frame\(items.count == 1 ? "" : "s")"
        } catch {
            app.statusMessage = "Person filter: \(error.localizedDescription)"
        }
    }

    func clearPersonFilter() {
        guard personFilter != nil else { return }
        personFilter = nil
        app?.refreshVisible()
    }

    var personFilterTitle: String? {
        personFilter.map { "\($0.person.name)\($0.eyesClosed ? " · eyes closed" : "")" }
    }
}
