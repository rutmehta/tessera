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
    @ObservationIgnored private let resumeStore: ReviewResumeStore
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
    private(set) var resumeMessage: String?
    private var resumeRecord: ReviewResumeRecord?
    private var queueOwner: EngineLibrary?
    private var queueGeneration = UUID()
    @ObservationIgnored private var runID: UUID?
    private var runningLibrary: EngineLibrary?
    private var runningImages: Set<String> = []
    private var mutationOwners: [String: EngineLibrary] = [:]
    private enum SourceKey: Hashable {
        case resource(device: UInt64, inode: UInt64)
        case path(URL)
    }
    private var runningSources: Set<SourceKey> = []
    private var mutationSources: [String: SourceKey] = [:]
    // Scoped to active mutations: SwiftUI availability queries reuse the first
    // resolution, and later mutations do not inherit stale filesystem identities.
    @ObservationIgnored private var sourceKeys: [URL: SourceKey] = [:]
    var showReview = false
    var reviewLibrary: EngineLibrary? { queueOwner }
    var reviewGeneration: UUID { queueGeneration }

    /// A row's owner and stable photo identity, captured before any suspension.
    struct ReviewTarget: Sendable {
        let entry: AgentReviewEntry
        let library: EngineLibrary
        fileprivate let generation: UUID
    }

    func queueTarget(_ entry: AgentReviewEntry) -> ReviewTarget? {
        guard entry.canBeTargeted, let queueOwner, let stored = queue.entry(entry.imageID),
              stored.groupID == entry.groupID, stored.unavailableReason == nil,
              queueOwner.itemOfImage[entry.imageID] != nil else { return nil }
        return ReviewTarget(entry: entry, library: queueOwner, generation: queueGeneration)
    }

    func reviewTarget(_ entry: AgentReviewEntry, library: EngineLibrary) -> ReviewTarget {
        ReviewTarget(entry: entry, library: library, generation: queueGeneration)
    }

    /// Never use a stored item index after a library update or folder switch.
    func currentItem(for target: ReviewTarget) -> Int? {
        guard app?.engineLibrary === target.library,
              target.generation == queueGeneration else { return nil }
        return target.library.itemOfImage[target.entry.imageID]
    }

    private func setStatus(_ status: AgentReviewEntry.Status, for target: ReviewTarget) {
        guard queueOwner === target.library,
              target.generation == queueGeneration,
              queue.entry(target.entry.imageID)?.groupID == target.entry.groupID else { return }
        queue.setStatus(status, for: target.entry.imageID)
    }
    /// Photos whose accept / revert / redo is in flight (row spinners).
    private(set) var busy: Set<String> = []
    private(set) var profile: StyleProfileStatus?
    private(set) var training: AgentRunProgress?
    @ObservationIgnored private var cancelFlag: CancelFlag?

    var isRunning: Bool { progress != nil }

    /// Session creation must not race an agent's recipe write. Other owners and
    /// unrelated photos remain available while a captured run finishes offscreen.
    func isMutating(imageID: String, library: EngineLibrary) -> Bool {
        if (runningLibrary === library && runningImages.contains(imageID)) || mutationOwners[imageID] === library {
            return true
        }
        guard !runningSources.isEmpty || !mutationSources.isEmpty,
              let key = sourceKey(imageID: imageID, library: library) else { return false }
        return runningSources.contains(key) || mutationSources.values.contains(key)
    }

    private func sourceKey(imageID: String, library: EngineLibrary) -> SourceKey? {
        guard let item = library.itemOfImage[imageID], library.items.indices.contains(item),
              let url = library.items[item].url else { return nil }
        if let cached = sourceKeys[url] { return cached }
        let canonical = url.standardizedFileURL.resolvingSymlinksInPath()
        let attributes = try? FileManager.default.attributesOfItem(atPath: canonical.path)
        let key: SourceKey
        if let device = attributes?[.systemNumber] as? NSNumber,
           let inode = attributes?[.systemFileNumber] as? NSNumber {
            key = .resource(device: device.uint64Value, inode: inode.uint64Value)
        } else { key = .path(canonical) }
        sourceKeys[url] = key
        return key
    }

    private func clearSourceKeysIfIdle() {
        if runningImages.isEmpty && mutationOwners.isEmpty { sourceKeys.removeAll() }
    }

    private func finishMutation(_ imageID: String) {
        mutationOwners.removeValue(forKey: imageID)
        mutationSources.removeValue(forKey: imageID)
        busy.remove(imageID)
        clearSourceKeysIfIdle()
    }

    init(arguments: [String] = ProcessInfo.processInfo.arguments, supportDirectory: URL? = nil) {
        let support = supportDirectory ?? EngineLibrary.supportDirectory(arguments: arguments)
        store = AISettingsStore(directory: support)
        resumeStore = ReviewResumeStore(directory: support)
        let prefs = store.load()
        preferences = prefs
        scriptedAvailable = arguments.contains("--fake-planner")
        provider = scriptedAvailable ? .scripted : prefs.provider
    }

    var providers: [AIProviderKind] { AIProviderKind.visible + (scriptedAvailable ? [.scripted] : []) }

    func savePreferences() {
        do { try store.save(preferences) } catch { app?.statusMessage = "AI settings not saved: \(error.localizedDescription)" }
    }

    /// Rehydrates only the manifest captured for this exact canonical library path.
    /// Recipe provenance supplies the current per-photo detail and remains authoritative.
    func libraryInstalled(_ installedLibrary: any PhotoLibrary) {
        guard let library = installedLibrary as? EngineLibrary else { clearReviewQueue(); return }
        guard let folder = library.folder else { clearReviewQueue(); return }
        if isRunning, let runningFolder = runningLibrary?.folder,
           ReviewResumeStore.canonicalPath(runningFolder) == ReviewResumeStore.canonicalPath(folder) {
            resumeMessage = "Auto Edit is still running for this library. Its results will appear when it finishes."
            return
        }
        do {
            guard let record = try resumeStore.restore(libraryFolder: folder) else {
                clearReviewQueue()
                return
            }
            let entries = record.targets.sorted { $0.ordinal < $1.ordinal }.map { target -> AgentReviewEntry in
                let itemID = library.itemOfImage[target.imageID]
                guard let expectedGroup = target.expectedGroupID else {
                    if let error = target.error {
                        return AgentReviewEntry(imageID: target.imageID, itemID: itemID, name: target.name,
                                                confidence: 0, error: error)
                    }
                    return unavailableEntry(target, itemID: itemID,
                                            reason: record.state == .interrupted
                                                ? "This target may not have completed before Tessera closed."
                                                : "This target has no saved review result.")
                }
                guard let itemID,
                      let provenance = try? library.engine.agentProvenance(imageId: target.imageID) else {
                    return unavailableEntry(target, itemID: itemID,
                                            reason: "This photo or its saved review result is unavailable.")
                }
                guard provenance.item.groupId == expectedGroup else {
                    return unavailableEntry(target, itemID: itemID,
                                            reason: "A newer agent edit replaced this review result.")
                }
                var entry = AgentReviewEntry(provenance.item, itemID: itemID)
                if let error = target.error { entry.error = error }
                return entry
            }
            queue = AgentReviewQueue(entries: entries, provider: record.provider)
            queueOwner = library
            queueGeneration = UUID()
            resumeRecord = record
            resumeMessage = nil
            showReview = false
            app?.reviewNavigation.restoreCursor(selectedID: record.selectedImageID,
                                                anchorID: record.anchorImageID, queue: queue)
            app?.reconcileReviewNavigation()
        } catch {
            clearReviewQueue()
            resumeMessage = "Review history unavailable: \(error.localizedDescription)"
            app?.statusMessage = resumeMessage
        }
    }

    private func clearReviewQueue() {
        queue = AgentReviewQueue()
        queueOwner = nil
        queueGeneration = UUID()
        resumeRecord = nil
        resumeMessage = nil
        showReview = false
    }

    private func unavailableEntry(_ target: ReviewResumeRecord.Target, itemID: Int?, reason: String) -> AgentReviewEntry {
        AgentReviewEntry(imageID: target.imageID, itemID: itemID, name: target.name,
                         groupID: target.expectedGroupID, confidence: 0, unavailableReason: reason)
    }

    private func writeResumeRecord(_ record: ReviewResumeRecord, for library: EngineLibrary) {
        do {
            try resumeStore.save(record)
            if queueOwner === library || app?.engineLibrary === library {
                resumeRecord = record
                resumeMessage = nil
            }
        } catch {
            if queueOwner === library || app?.engineLibrary === library {
                resumeMessage = "Review history could not be saved: \(error.localizedDescription)"
                if app?.engineLibrary === library, let app { app.statusMessage = resumeMessage }
            }
        }
    }

    func persistReviewCursor() {
        guard let record = resumeRecord, let library = queueOwner,
              app?.engineLibrary === library,
              library.folder.map(ReviewResumeStore.canonicalPath) == record.libraryPath else { return }
        do {
            let updated = try record.updatingCursor(selectedID: app?.reviewNavigation.selectedID,
                                                    anchorID: app?.reviewNavigation.anchorID)
            writeResumeRecord(updated, for: library)
        } catch {
            resumeMessage = "Review history could not be saved: \(error.localizedDescription)"
            if let app { app.statusMessage = resumeMessage }
        }
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
        guard busy.isEmpty else { app.statusMessage = "Wait for the current review action to finish"; return }
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
        if !busy.isEmpty { return "Wait for the current review action to finish" }
        if provider.keyAccount != nil, !store.hasKey(for: provider) { return AISettingsError.missingKey(provider).errorDescription }
        if count(scope) == 0 { return "Nothing to edit in this scope" }
        return nil
    }

    // MARK: Run

    /// Starts the base edit (or, with `instruction`, a scoped redo) for item ids.
    func start(itemIDs: [Int], instruction: String? = nil, provider kind: AIProviderKind? = nil) {
        guard let app, let lib = app.engineLibrary, let folder = lib.folder, !isRunning, !itemIDs.isEmpty else { return }
        // A run replaces the queue generation; let existing review completions
        // publish their result and refresh their photo before any run starts.
        guard busy.isEmpty, itemIDs.allSatisfy(lib.imageIDs.indices.contains) else { return }
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
        let images = inputs.map(\.imageId)
        let canonicalPath = ReviewResumeStore.canonicalPath(folder)
        let displayProvider = Self.providerName(kind, preferences: prefs)
        let shouldMerge = queueOwner === lib && !queue.isEmpty
            && (instruction != nil || queue.provider == displayProvider)
        let storedBeforeRun: ReviewResumeRecord?
        do {
            storedBeforeRun = resumeRecord?.libraryPath == canonicalPath
                ? resumeRecord : try resumeStore.load(libraryFolder: folder)
        } catch {
            storedBeforeRun = nil
            resumeMessage = "Review history could not be read: \(error.localizedDescription)"
        }
        var capturedTargets: [ReviewResumeRecord.Target] = shouldMerge
            ? (storedBeforeRun?.targets ?? queue.entries.enumerated().map {
                ReviewResumeRecord.Target(imageID: $0.element.imageID, name: $0.element.name,
                                          ordinal: $0.offset, expectedGroupID: $0.element.groupID,
                                          error: $0.element.error == nil ? nil : "Auto edit did not produce a review result.")
            }) : []
        capturedTargets = ReviewResumeRecord.reindexedTargets(capturedTargets)
        var nextOrdinal = capturedTargets.count
        for imageID in images {
            let name = lib.itemOfImage[imageID].flatMap { lib.items.indices.contains($0) ? lib.items[$0].name : nil } ?? imageID
            if let existing = capturedTargets.firstIndex(where: { $0.imageID == imageID }) {
                capturedTargets[existing].name = name
                capturedTargets[existing].error = nil
            } else {
                capturedTargets.append(.init(imageID: imageID, name: name,
                                             ordinal: nextOrdinal))
                nextOrdinal += 1
            }
        }
        let now = Date()
        let priorRevision = storedBeforeRun?.recordRevision ?? 0
        let intent = ReviewResumeRecord(
            recordRevision: priorRevision < UInt64.max ? priorRevision + 1 : priorRevision,
            libraryPath: canonicalPath,
            queueID: shouldMerge ? storedBeforeRun?.queueID ?? UUID() : UUID(),
            provider: shouldMerge ? storedBeforeRun?.provider ?? displayProvider : displayProvider,
            scope: shouldMerge ? storedBeforeRun?.scope ?? scope.rawValue : scope.rawValue,
            sourceDescription: shouldMerge ? storedBeforeRun?.sourceDescription
                ?? "\(title(scope)) · \(images.count) photo\(images.count == 1 ? "" : "s")"
                : "\(title(scope)) · \(images.count) photo\(images.count == 1 ? "" : "s")",
            state: .running,
            startedAt: shouldMerge ? storedBeforeRun?.startedAt ?? now : now,
            updatedAt: now,
            targets: capturedTargets,
            selectedImageID: app.reviewNavigation.selectedID,
            anchorImageID: app.reviewNavigation.anchorID)
        let cancel = CancelFlag()
        cancelFlag = cancel
        error = nil
        runningTitle = instruction.map { "Redo “\($0)”" } ?? "Auto edit · \(kind.title)"
        progress = AgentRunProgress(done: 0, total: UInt32(itemIDs.count), current: "", phase: "Preparing")
        let job = UUID()
        runID = job
        let relay = AgentRelay { [weak self] p in
            Task { @MainActor in if self?.runID == job { self?.progress = p } }
        }
        runningLibrary = lib
        runningImages = Set(images)
        runningSources = Set(images.compactMap { sourceKey(imageID: $0, library: lib) })
        let closeBarrier = app.prepareForAgent(imageIDs: Set(images), library: lib)
        Task {
            var gateFinished = false
            defer { if !gateFinished { closeBarrier.finish() } }
            // The captured owner can finish offscreen, but all of its prior
            // Develop saves must land before the agent reads the recipes.
            guard await closeBarrier.result().isSaved, self.runID == job else {
                if self.runID == job {
                    self.progress = nil
                    self.cancelFlag = nil
                    self.runID = nil
                    self.runningLibrary = nil
                    self.runningImages.removeAll()
                    self.runningSources.removeAll()
                    self.clearSourceKeysIfIdle()
                    if app.engineLibrary === lib {
                        self.error = "Finish saving the photo before Auto edit"
                        app.statusMessage = self.error
                    }
                }
                return
            }
            self.writeResumeRecord(intent, for: lib)
            let result = await Task.detached(priority: .userInitiated) {
                Result { try lib.engine.runAgent(request: request, cancel: cancel, listener: relay) }
            }.value
            // The engine has finished reading and writing captured recipes. Release
            // admission before notifying the UI, which may reopen Develop.
            closeBarrier.finish()
            gateFinished = true
            self.progress = nil
            self.cancelFlag = nil
            self.runID = nil
            self.runningLibrary = nil
            self.runningImages.removeAll()
            self.runningSources.removeAll()
            self.clearSourceKeysIfIdle()
            switch result {
            case .success(let report):
                let completed = self.recording(report: report, base: intent, runImageIDs: images,
                                               redo: instruction != nil, library: lib)
                self.writeResumeRecord(completed, for: lib)
                self.didFinish(report, library: lib, redo: instruction != nil, resumeRecord: completed,
                               runImageIDs: images)
            case .failure(let e):
                self.writeResumeRecord(self.recordingFailure(base: intent, runImageIDs: images,
                                                             redo: instruction != nil, library: lib), for: lib)
                if app.engineLibrary === lib {
                    self.error = e.localizedDescription
                    app.statusMessage = "Auto edit failed: \(e.localizedDescription)"
                    app.showToast("Auto edit failed: \(e.localizedDescription)", undoable: false)
                }
            }
            // Item ids may have moved while the run was going (frames arriving, an import).
            if app.engineLibrary === lib {
                app.agentDidEdit(images.compactMap { lib.itemOfImage[$0] })
            }
            if let current = app.engineLibrary, current !== lib,
               let currentFolder = current.folder,
               ReviewResumeStore.canonicalPath(currentFolder) == ReviewResumeStore.canonicalPath(folder) {
                self.libraryInstalled(current)
            }
        }
    }

    func cancel() { cancelFlag?.cancel() }

    /// The library changed in place: review rows follow their photos.
    func libraryDidUpdate(_ lib: EngineLibrary) {
        guard queueOwner === lib, !queue.isEmpty else { return }
        queue.relink { lib.itemOfImage[$0] }
    }

    private static func providerName(_ kind: AIProviderKind, preferences: AIPreferences) -> String {
        switch kind {
        case .styleProfile: "style profile"
        case .anthropic: "Anthropic \(preferences.anthropicModel)"
        case .openAI: "OpenAI \(preferences.openAIModel)"
        case .ollama: "Ollama \(preferences.ollamaModel)"
        case .scripted: "scripted planner"
        }
    }

    private func recording(report: AgentRunReport, base: ReviewResumeRecord,
                           runImageIDs: [String], redo: Bool, library: EngineLibrary) -> ReviewResumeRecord {
        let returned = Dictionary(report.items.map { ($0.imageId, $0) }, uniquingKeysWith: { _, last in last })
        let current = latestRecord(matching: base, library: library)
        var targets = current.targets
        for imageID in runImageIDs {
            guard let index = targets.firstIndex(where: { $0.imageID == imageID }) else { continue }
            if let item = returned[imageID] {
                targets[index].expectedGroupID = item.groupId
                targets[index].error = item.error == nil ? nil : "Auto edit did not produce a review result."
            } else {
                targets[index].expectedGroupID = nil
                targets[index].error = nil // Cancelled/unreported targets stay explicitly unknown.
            }
        }
        let state: ReviewResumeRecord.RunState = report.cancelled ? .cancelled
            : targets.contains(where: { runImageIDs.contains($0.imageID) && $0.error != nil }) ? .partial : .completed
        guard var result = try? current.advanced(state: state, targets: targets) else { return current }
        if !redo { result.provider = report.provider }
        return result
    }

    private func latestRecord(matching base: ReviewResumeRecord, library: EngineLibrary) -> ReviewResumeRecord {
        if let latest = resumeRecord,
           latest.libraryPath == base.libraryPath, latest.queueID == base.queueID { return latest }
        if let folder = library.folder,
           let latest = try? resumeStore.load(libraryFolder: folder),
           latest.libraryPath == base.libraryPath, latest.queueID == base.queueID { return latest }
        return base
    }

    private func recordingFailure(base: ReviewResumeRecord, runImageIDs: [String], redo: Bool,
                                  library: EngineLibrary) -> ReviewResumeRecord {
        let current = latestRecord(matching: base, library: library)
        let targets = current.targets.map { target -> ReviewResumeRecord.Target in
            var target = target
            if runImageIDs.contains(target.imageID), !redo || target.expectedGroupID == nil {
                target.error = "Auto edit did not produce a review result."
            }
            return target
        }
        return (try? current.advanced(state: .failed, targets: targets)) ?? current
    }

    private func didFinish(_ report: AgentRunReport, library lib: EngineLibrary, redo: Bool,
                           resumeRecord: ReviewResumeRecord, runImageIDs: [String]) {
        guard app?.engineLibrary === lib else { return }
        var entries = report.items.map { AgentReviewEntry($0, itemID: lib.itemOfImage[$0.imageId]) }
        let returned = Set(report.items.map(\.imageId))
        for target in resumeRecord.targets where runImageIDs.contains(target.imageID) && !returned.contains(target.imageID) {
            entries.append(unavailableEntry(target, itemID: lib.itemOfImage[target.imageID],
                                            reason: "The run stopped before saving a review result for this photo."))
        }
        if queueOwner === lib && (redo || !queue.isEmpty && queue.provider == report.provider) {
            queue.merge(entries)
        } else {
            queue = AgentReviewQueue(entries: entries, provider: report.provider)
        }
        queueOwner = lib
        queueGeneration = UUID()
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

    func accept(_ target: ReviewTarget, completion: @escaping @MainActor (Bool) -> Void = { _ in }) {
        let entry = target.entry
        guard let app, currentItem(for: target) != nil,
              let folder = target.library.folder, entry.groupID != nil,
              entry.error == nil, entry.unavailableReason == nil,
              !isRunning, !busy.contains(entry.imageID) else { completion(false); return }
        let lib = target.library
        busy.insert(entry.imageID)
        mutationOwners[entry.imageID] = lib
        mutationSources[entry.imageID] = sourceKey(imageID: entry.imageID, library: lib)
        let imageID = entry.imageID
        let closeBarrier = app.prepareForAgent(imageIDs: [imageID], library: lib)
        Task {
            var gateFinished = false
            defer { if !gateFinished { closeBarrier.finish() } }
            guard await closeBarrier.result().isSaved else {
                self.finishMutation(imageID)
                completion(false)
                if app.engineLibrary === lib { app.statusMessage = "Finish saving the photo before accepting" }
                return
            }
            guard self.currentItem(for: target) != nil else {
                self.finishMutation(imageID)
                completion(false)
                return
            }
            let result = await Task.detached(priority: .userInitiated) {
                Result { try lib.engine.acceptAgentEdit(imageId: imageID, libraryFolder: folder.path) }
            }.value
            closeBarrier.finish()
            gateFinished = true
            self.finishMutation(imageID)
            switch result {
            case .success(let r):
                self.setStatus(.accepted, for: target)
                if let item = self.currentItem(for: target) {
                    app.agentDidEdit([item])
                    app.statusMessage = "Accepted \(entry.name)" + (r.feedbackRecorded
                        ? " · style profile now has \(r.samples) sample\(r.samples == 1 ? "" : "s")"
                        : " · not learned: \(r.note ?? "")")
                    completion(true)
                } else { completion(false) }
            case .failure(let e):
                if self.currentItem(for: target) != nil { app.statusMessage = "Accept failed: \(e.localizedDescription)" }
                completion(false)
            }
        }
    }

    func revert(_ target: ReviewTarget) {
        let entry = target.entry
        guard let app, currentItem(for: target) != nil, let group = entry.groupID,
              entry.unavailableReason == nil, !isRunning, !busy.contains(entry.imageID) else { return }
        let lib = target.library
        busy.insert(entry.imageID)
        mutationOwners[entry.imageID] = lib
        mutationSources[entry.imageID] = sourceKey(imageID: entry.imageID, library: lib)
        let imageID = entry.imageID
        let closeBarrier = app.prepareForAgent(imageIDs: [imageID], library: lib)
        Task {
            var gateFinished = false
            defer { if !gateFinished { closeBarrier.finish() } }
            guard await closeBarrier.result().isSaved else {
                self.finishMutation(imageID)
                if app.engineLibrary === lib { app.statusMessage = "Finish saving the photo before reverting" }
                return
            }
            guard self.currentItem(for: target) != nil else { self.finishMutation(imageID); return }
            let result = await Task.detached { Result { try lib.engine.revertAgentEdit(imageId: imageID, groupId: group) } }.value
            closeBarrier.finish()
            gateFinished = true
            self.finishMutation(imageID)
            switch result {
            case .success:
                self.setStatus(.reverted, for: target)
                if self.currentItem(for: target) != nil {
                    app.statusMessage = "Reverted the agent's edit of \(entry.name) (one step in its history)"
                }
            case .failure(let e):
                if self.currentItem(for: target) != nil { app.statusMessage = "Revert failed: \(e.localizedDescription)" }
            }
            if let item = self.currentItem(for: target) { app.agentDidEdit([item]) }
        }
    }

    /// Natural-language redo ("warmer, keep the sky"): a new, named group on that photo.
    func redo(_ target: ReviewTarget, instruction: String) {
        guard let item = currentItem(for: target) else { return }
        let text = instruction.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        start(itemIDs: [item], instruction: text)
    }
}

/// Engine progress (running thread) → main actor.
final class AgentRelay: AgentRunListener, @unchecked Sendable {
    let handler: @Sendable (AgentRunProgress) -> Void
    init(_ handler: @escaping @Sendable (AgentRunProgress) -> Void) { self.handler = handler }
    func onProgress(progress: AgentRunProgress) { handler(progress) }
}
