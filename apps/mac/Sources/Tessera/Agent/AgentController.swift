import AppKit
import Observation
import TesseraCore
import TesseraFFI

/// Auto Edit (docs/10, WP M3-11): the agent makes a non-generative base edit with the engine's
/// own tools; every step is an ordinary recipe step in an "Agent base edit" history group. This
/// holds Settings ▸ AI (preferences under the app directory, keys in the Keychain), the sheet's
/// choices, the non-modal run with progress and Cancel, and the review queue.
@MainActor @Observable
final class AgentController {
    enum Scope: String, CaseIterable, Identifiable {
        case selection, view, shoot
        var id: String { rawValue }
    }

    @ObservationIgnored weak var app: AppModel?
    @ObservationIgnored let store: AISettingsStore
    var preferences: AIPreferences
    /// Hidden `--fake-planner` test aid: offers (and preselects) the scripted FakePlanner.
    let scriptedAvailable: Bool

    // Sheet state.
    var provider: AIProviderKind
    var scope: Scope = .selection
    var error: String?

    // Run state.
    private(set) var progress: AgentRunProgress?
    private(set) var runningTitle = ""
    private(set) var queue = AgentReviewQueue()
    var showReview = false
    /// Photos whose accept / revert / redo is in flight (row spinners).
    private(set) var busy: Set<String> = []
    private(set) var profile: StyleProfileStatus?
    private(set) var training: AgentRunProgress?
    @ObservationIgnored private var cancelFlag: CancelFlag?

    var isRunning: Bool { progress != nil }

    init(arguments: [String] = ProcessInfo.processInfo.arguments) {
        store = AISettingsStore(directory: EngineLibrary.defaultSupportDirectory)
        let prefs = store.load()
        preferences = prefs
        scriptedAvailable = arguments.contains("--fake-planner")
        provider = scriptedAvailable ? .scripted : prefs.provider
    }

    var providers: [AIProviderKind] { AIProviderKind.visible + (scriptedAvailable ? [.scripted] : []) }

    func savePreferences() {
        do { try store.save(preferences) } catch { app?.statusMessage = "AI settings not saved: \(error.localizedDescription)" }
    }

    /// Key state for a provider, for the sheet and Settings (masked; never the key).
    func keyStatus(_ kind: AIProviderKind) -> String? {
        guard kind.keyAccount != nil else { return nil }
        return store.apiKey(for: kind).map { "Key in Keychain: \(AISettingsStore.masked($0))" }
    }

    func setKey(_ key: String?, for kind: AIProviderKind) -> String? {
        do {
            try store.setAPIKey(key, for: kind)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    // MARK: Style profile

    func refreshProfile() {
        guard let lib = app?.engineLibrary, let folder = lib.folder else { profile = nil; return }
        profile = try? lib.engine.styleProfileStatus(libraryFolder: folder.path)
    }

    func saveQuestionnaire(_ q: StyleQuestionnaire) {
        guard let lib = app?.engineLibrary, let folder = lib.folder else { return }
        do { profile = try lib.engine.setStyleQuestionnaire(libraryFolder: folder.path, answers: q) }
        catch { app?.statusMessage = "Style profile: \(error.localizedDescription)" }
    }

    /// Settings ▸ AI ▸ Learn from My Edits (non-modal, progress in Settings).
    func trainProfile() {
        guard let lib = app?.engineLibrary, let folder = lib.folder, training == nil else { return }
        let cancel = CancelFlag()
        training = AgentRunProgress(done: 0, total: 0, current: "", phase: "Learning your edits")
        let relay = AgentRelay { [weak self] p in Task { @MainActor in if self?.training != nil { self?.training = p } } }
        Task.detached(priority: .userInitiated) {
            let result = Result { try lib.engine.trainStyleProfile(libraryFolder: folder.path, cancel: cancel, listener: relay) }
            await MainActor.run {
                self.training = nil
                switch result {
                case .success(let status):
                    self.profile = status
                    self.app?.statusMessage = "Style profile learned from \(status.samples) edited photo\(status.samples == 1 ? "" : "s")"
                case .failure(let e):
                    self.app?.statusMessage = "Style profile: \(e.localizedDescription)"
                }
            }
        }
    }

    // MARK: Sheet

    /// ⌘⇧A / toolbar: prepare the sheet for the current selection.
    func present() {
        guard let app else { return }
        guard app.isEngineBacked else { app.statusMessage = "Auto Edit needs a folder opened on the engine"; return }
        guard !isRunning else { app.statusMessage = "An auto edit is already running"; return }
        error = nil
        if !providers.contains(provider) { provider = preferences.provider }
        scope = app.selectionCount > 1 ? .selection : (app.source == .all ? .shoot : .view)
        refreshProfile()
        app.showAutoEdit = true
    }

    func count(_ scope: Scope) -> Int { itemIDs(for: scope).count }

    func title(_ scope: Scope) -> String {
        switch scope {
        case .selection: "Selection"
        case .view: app?.source == .all ? "Current view" : (app?.source.title ?? "Current view")
        case .shoot: "Whole shoot"
        }
    }

    func itemIDs(for scope: Scope) -> [Int] {
        guard let app else { return [] }
        return switch scope {
        case .selection: app.targetIDs
        case .view: app.visibleIDs
        case .shoot: Array(app.library.items.indices)
        }
    }

    /// Problem that blocks the run, if any (shown in the sheet).
    var blocker: String? {
        if provider.keyAccount != nil, !store.hasKey(for: provider) { return AISettingsError.missingKey(provider).errorDescription }
        if count(scope) == 0 { return "Nothing to edit in this scope" }
        return nil
    }

    // MARK: Run

    /// Starts the base edit (or, with `instruction`, a scoped redo) for item ids.
    func start(itemIDs: [Int], instruction: String? = nil, provider kind: AIProviderKind? = nil) {
        guard let app, let lib = app.engineLibrary, let folder = lib.folder, !isRunning, !itemIDs.isEmpty else { return }
        let kind = kind ?? provider
        let provider: AgentProvider
        do { provider = try store.provider(kind, preferences) } catch {
            self.error = error.localizedDescription
            app.statusMessage = error.localizedDescription
            return
        }
        let prefs = preferences
        var personsOf: [Int: [String]] = [:]
        if prefs.personConsistency, itemIDs.count > 1 {
            for person in app.assist.people { for id in person.items { personsOf[id, default: []].append(person.id) } }
        }
        let inputs = itemIDs.map { id in
            AgentImageInput(imageId: lib.imageIDs[id],
                            burst: prefs.sceneConsistency && itemIDs.count > 1 && lib.groups[lib.items[id].groupID].count > 1
                                ? "G\(lib.items[id].groupID + 1)" : nil,
                            people: personsOf[id] ?? [])
        }
        let request = AgentRunRequest(images: inputs, libraryFolder: folder.path, provider: provider,
                                      guardrails: prefs.guardrails, instruction: instruction)
        let cancel = CancelFlag()
        cancelFlag = cancel
        error = nil
        runningTitle = instruction.map { "Redo “\($0)”" } ?? "Auto edit · \(kind.title)"
        progress = AgentRunProgress(done: 0, total: UInt32(itemIDs.count), current: "", phase: "Preparing")
        let relay = AgentRelay { [weak self] p in Task { @MainActor in if self?.progress != nil { self?.progress = p } } }
        let ids = Set(itemIDs)
        let images = itemIDs.filter(lib.imageIDs.indices.contains).map { lib.imageIDs[$0] }
        Task {
            // Pending develop saves of these photos land before the agent reads their recipes.
            await app.releaseDevelop(for: ids)
            let result = await Task.detached(priority: .userInitiated) {
                Result { try lib.engine.runAgent(request: request, cancel: cancel, listener: relay) }
            }.value
            self.progress = nil
            self.cancelFlag = nil
            switch result {
            case .success(let report): self.didFinish(report, library: lib, redo: instruction != nil)
            case .failure(let e):
                self.error = e.localizedDescription
                app.statusMessage = "Auto edit failed: \(e.localizedDescription)"
                app.showToast("Auto edit failed: \(e.localizedDescription)", undoable: false)
            }
            // Item ids may have moved while the run was going (frames arriving, an import).
            app.agentDidEdit(images.compactMap { lib.itemOfImage[$0] })
        }
    }

    func cancel() { cancelFlag?.cancel() }

    /// The library changed in place: review rows follow their photos.
    func libraryDidUpdate(_ lib: EngineLibrary) {
        guard !queue.isEmpty else { return }
        queue.relink { lib.itemOfImage[$0] }
    }

    private func didFinish(_ report: AgentRunReport, library lib: EngineLibrary, redo: Bool) {
        let entries = report.items.map { AgentReviewEntry($0, itemID: lib.itemOfImage[$0.imageId]) }
        if redo || !queue.isEmpty && queue.provider == report.provider {
            queue.merge(entries)
        } else {
            queue = AgentReviewQueue(entries: entries, provider: report.provider)
        }
        let failed = entries.filter { $0.error != nil }
        let done = entries.count - failed.count
        var headline = report.cancelled ? "Auto edit cancelled: \(done) edited" : redo
            ? "Redo finished for \(done) photo\(done == 1 ? "" : "s")"
            : "Auto edit finished: \(done) photo\(done == 1 ? "" : "s") to review, least confident first"
        if !failed.isEmpty { headline += "; \(failed.count) failed" }
        app?.statusMessage = headline
        app?.showToast(headline, undoable: false, details: failed.map { "\($0.name): \($0.error ?? "")" })
        if !redo, !entries.isEmpty { showReview = true }
    }

    // MARK: Review actions

    func accept(_ entry: AgentReviewEntry) {
        guard let app, let lib = app.engineLibrary, let folder = lib.folder else { return }
        busy.insert(entry.imageID)
        let imageID = entry.imageID
        Task.detached(priority: .userInitiated) {
            let result = Result { try lib.engine.acceptAgentEdit(imageId: imageID, libraryFolder: folder.path) }
            await MainActor.run {
                self.busy.remove(imageID)
                switch result {
                case .success(let r):
                    self.queue.setStatus(.accepted, for: imageID)
                    app.statusMessage = "Accepted \(entry.name)" + (r.feedbackRecorded
                        ? " · style profile now has \(r.samples) sample\(r.samples == 1 ? "" : "s")"
                        : " · not learned: \(r.note ?? "")")
                case .failure(let e): app.statusMessage = "Accept failed: \(e.localizedDescription)"
                }
            }
        }
    }

    func revert(_ entry: AgentReviewEntry) {
        guard let app, let lib = app.engineLibrary, let group = entry.groupID else { return }
        busy.insert(entry.imageID)
        let imageID = entry.imageID
        let item = entry.itemID
        Task {
            if let item { await app.releaseDevelop(for: [item]) }
            let result = await Task.detached { Result { try lib.engine.revertAgentEdit(imageId: imageID, groupId: group) } }.value
            self.busy.remove(imageID)
            switch result {
            case .success:
                self.queue.setStatus(.reverted, for: imageID)
                app.statusMessage = "Reverted the agent's edit of \(entry.name) (one step in its history)"
            case .failure(let e): app.statusMessage = "Revert failed: \(e.localizedDescription)"
            }
            if let item = lib.itemOfImage[imageID] { app.agentDidEdit([item]) }
        }
    }

    /// Natural-language redo ("warmer, keep the sky"): a new, named group on that photo.
    func redo(_ itemIDs: [Int], instruction: String) {
        let text = instruction.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        // A redo uses the provider that made the queue when possible, else the sheet's choice.
        start(itemIDs: itemIDs, instruction: text)
    }
}

/// Engine progress (running thread) → main actor.
final class AgentRelay: AgentRunListener, @unchecked Sendable {
    let handler: @Sendable (AgentRunProgress) -> Void
    init(_ handler: @escaping @Sendable (AgentRunProgress) -> Void) { self.handler = handler }
    func onProgress(progress: AgentRunProgress) { handler(progress) }
}
