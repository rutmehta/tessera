import AppKit
import Observation
import TesseraCore
import enum TesseraFFI.AssistMode
import struct TesseraFFI.KeepPrediction

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
    /// UX-14: the agent's triage of the shoot (SPEC §4), refreshed as the analysis pass fills in
    /// signals and again when it finishes. Observed by the Cull column; the signals and scores
    /// behind it are read per frame by the burst view.
    private(set) var triage = CullTriage()
    /// Replaced wholesale a few times a minute while the pass runs, so views read them directly.
    private(set) var signals: [Int: FrameSignals] = [:]
    /// The engine's technical score and keep likelihood per frame (`review`, Assist on or off).
    private(set) var scores: [Int: (technical: Double, pKeep: Double)] = [:]
    /// Preview dHashes (near-duplicates are within 6 bits of their burst's pick).
    @ObservationIgnored private(set) var hashes: [Int: UInt64] = [:]
    @ObservationIgnored private var triageRefreshing = false
    @ObservationIgnored private var triageStale = false
    @ObservationIgnored private var lastTriageRefresh: CFAbsoluteTime = 0

    var isRunning: Bool { progress != nil }

    /// The library was renumbered in place (M2-28): follow the photos, drop those that left.
    /// New frames get predictions on the next refresh.
    func libraryDidUpdate(_ remap: (Int) -> Int?) {
        predictions = Dictionary(predictions.compactMap { id, p -> (Int, AssistPrediction)? in
            guard let new = remap(id) else { return nil }
            var p = p
            p.itemID = new
            return (new, p)
        }, uniquingKeysWith: { a, _ in a })
        order = order.compactMap(remap)
        suggestionCount = predictions.values.filter { $0.suggested != nil }.count
        func relink(_ person: PersonSummary) -> PersonSummary {
            var person = person
            person.items = person.items.compactMap(remap)
            person.coverItem = person.coverItem.flatMap(remap)
            return person
        }
        people = people.map(relink)
        if let filter = personFilter {
            personFilter = (relink(filter.person), filter.eyesClosed, Set(filter.items.compactMap(remap)))
        }
        facesItem = facesItem.flatMap(remap)
        signals = Dictionary(signals.compactMap { id, s in remap(id).map { ($0, s) } }, uniquingKeysWith: { a, _ in a })
        scores = Dictionary(scores.compactMap { id, s in remap(id).map { ($0, s) } }, uniquingKeysWith: { a, _ in a })
        hashes = Dictionary(hashes.compactMap { id, h in remap(id).map { ($0, h) } }, uniquingKeysWith: { a, _ in a })
        triage = triage.remapped(remap)
        // New frames (a tether, an import) get measured and bucketed by the next refresh.
        refreshTriage()
    }

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
        signals = [:]
        scores = [:]
        hashes = [:]
        triage = CullTriage()
        triageStale = false
        guard let app, let lib = app.engineLibrary, !lib.isReadOnly else {
            // The stub library has groups and picks but no signals: the column still counts bursts.
            if let app { triage = CullTriage.compute(groups: app.cull.groups, best: app.cull.bestOfGroup, signals: [:]) }
            return
        }
        // Frames measured on an earlier open show up before the pass starts.
        refreshTriage()
        if seedFaces {
            do { try lib.seedSyntheticFaces() } catch { app.statusMessage = "Synthetic faces: \(error.localizedDescription)" }
        }
        analyze(faces: false, force: false, title: "Measuring sharpness and exposure for culling", announce: false)
        refreshPeople()
    }

    /// Cull ▸ Analyze Shoot / Analyze Faces: computes real signals off the main actor.
    /// `announce`: report the result in the status bar (off for the automatic pass on open, so
    /// the "Opened …" summary stays).
    func analyze(faces: Bool, force: Bool, title: String, announce: Bool = true) {
        guard let app, let lib = app.engineLibrary, !lib.isReadOnly, !isRunning else { return }
        analysisGeneration += 1
        let generation = analysisGeneration
        cancelAnalysis = false
        let targets = lib.analysisTargets(Array(lib.items.indices))
        progress = (0, targets.count, "", title)
        // A Sendable weak box, not a `[weak self]` capture: Swift 6.2/6.3 treat a weak capture as a
        // task-isolated variable and reject sending it to the main actor.
        let box = WeakAssist(self)
        Task.detached(priority: .utility) {
            let result = lib.analyze(images: targets, faces: faces, force: force) { done, total, name in
                let keepGoing = DispatchQueue.main.sync {
                    MainActor.assumeIsolated { () -> Bool in
                        guard let owner = box.value, generation == owner.analysisGeneration, !owner.cancelAnalysis else { return false }
                        owner.progress = (done, total, name, title)
                        // The triage column fills in while the pass runs (SPEC §4), a few times a minute.
                        owner.refreshTriage(throttle: 3)
                        return true
                    }
                }
                return keepGoing
            }
            await MainActor.run { box.value?.analysisDidFinish(result, faces: faces, generation: generation, announce: announce) }
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
        if faces {
            refreshPeople(refresh: true)
            app.peopleAfterFaceAnalysis()
        }
        refreshFaces(force: true)
        // The picks follow the new signals; cells of groups whose pick moved redraw.
        let moved = app.cull.refreshBests()
        if !moved.isEmpty { app.assistItemsChanged(moved.flatMap { Array(app.cull.groups[$0]) }) }
        refreshTriage()
        if enabled { refresh() }
    }

    // MARK: Triage (UX-14)

    /// Recomputes the triage from the session's signals and scores, off the main actor. With
    /// `throttle`, at most once per that many seconds (the running pass); a refresh asked for
    /// while one is in flight runs once more afterwards.
    func refreshTriage(throttle: TimeInterval = 0) {
        guard let app, let lib = app.engineLibrary, !lib.isReadOnly else { return }
        let now = CFAbsoluteTimeGetCurrent()
        guard throttle == 0 || now - lastTriageRefresh >= throttle else { return }
        guard !triageRefreshing else { triageStale = true; return }
        triageRefreshing = true
        triageStale = false
        lastTriageRefresh = now
        let box = WeakAssist(self)
        Task.detached(priority: .utility) {
            let raw = try? lib.rawFrameSignals()
            let review = try? lib.session.review()
            let hashes = try? lib.rawPreviewHashes()
            await MainActor.run { box.value?.triageDidRefresh(lib, raw: raw, review: review, hashes: hashes) }
        }
    }

    private func triageDidRefresh(_ lib: EngineLibrary, raw: [String: FrameSignals]?, review: [KeepPrediction]?,
                                  hashes rawHashes: [String: UInt64]?) {
        triageRefreshing = false
        guard let app, app.engineLibrary === lib else { return }
        if let raw {
            signals = Dictionary(raw.compactMap { imageID, s in lib.itemOfImage[imageID].map { ($0, s) } }, uniquingKeysWith: { a, _ in a })
        }
        if let rawHashes {
            hashes = Dictionary(rawHashes.compactMap { imageID, h in lib.itemOfImage[imageID].map { ($0, h) } }, uniquingKeysWith: { a, _ in a })
        }
        if let review {
            scores = Dictionary(review.compactMap { p in lib.itemOfImage[p.imageId].map { ($0, (p.technicalScore, p.pKeep)) } },
                                uniquingKeysWith: { a, _ in a })
        }
        let fresh = CullTriage.compute(groups: app.cull.groups, best: app.cull.bestOfGroup, signals: signals, hashes: hashes)
        if fresh != triage {
            triage = fresh
            app.triageDidChange()
        }
        if triageStale { refreshTriage() }
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

/// Main-actor object held weakly across a detached task (see `AssistController.analyze`).
private final class WeakAssist: Sendable {
    nonisolated(unsafe) weak var value: AssistController?
    init(_ value: AssistController) { self.value = value }
}
