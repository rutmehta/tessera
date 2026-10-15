import AppKit
import Observation
import TesseraCore
import TesseraFFI

/// "The catalog follows the account" (docs/design/cloud-sync/SPEC.md §3, CS-2): what belongs to
/// the photographer rather than to a drive lives in the app's iCloud Drive container through
/// `AccountStore`. Preferences (never API keys), the keyword hierarchy, the style profile and the
/// culling learner (the per-shoot ones merged into one account record), people names and the
/// drives list update the store when they change here, and a record written on another Mac is
/// applied through the same model APIs the user's own edits go through. No container: `status`
/// says why, Settings ▸ Sync shows the one-line fix, and nothing else changes.
@MainActor @Observable
final class AccountSync {
    enum Status: Equatable {
        /// The container is being resolved (it can take a moment after launch).
        case resolving
        case available
        case unavailable(AccountStore.UnavailableReason)
    }

    @ObservationIgnored weak var app: AppModel?
    @ObservationIgnored private(set) var store: AccountStore?
    @ObservationIgnored private var watcher: AccountStore.Watcher?
    @ObservationIgnored private let supportDirectory: URL
    /// Read off the main actor once, to resolve the container (`UserDefaults` is not `Sendable`).
    @ObservationIgnored nonisolated(unsafe) private let defaults: UserDefaults
    /// A remote record is being applied: the model hooks it fires must not push it back.
    @ObservationIgnored private var applyingRemote = false
    /// Cluster ids merged into each person on this Mac (union-find for `PersonRecord.clusters`).
    @ObservationIgnored private var mergedClusters: [String: Set<String>] = [:]
    /// The last person records pushed, so an unchanged person costs no file access.
    @ObservationIgnored private var pushedPeople: [String: PersonRecord] = [:]
    @ObservationIgnored private var learningPush: Task<Void, Never>?
    @ObservationIgnored private var volumeObservers: [NSObjectProtocol] = []

    private(set) var status: Status = .resolving
    private(set) var stats = AccountStore.Stats()
    /// Drives from `drives/` whose volume is not mounted: the folder popover lists them as
    /// "<name> · not attached" (CS-1 opens them on mount).
    private(set) var detachedDrives: [DriveRecord] = []
    private(set) var isEnabled: Bool

    /// No public API names the iCloud account on macOS without CloudKit discoverability, so this
    /// is the fallback the spec allows.
    var accountDescription: String { "this Mac's iCloud account" }

    init(supportDirectory: URL = EngineLibrary.defaultSupportDirectory, defaults: UserDefaults = .standard) {
        self.supportDirectory = supportDirectory
        self.defaults = defaults
        isEnabled = defaults.object(forKey: AccountStore.enabledKey) as? Bool ?? true
    }

    // MARK: Lifecycle

    /// Resolves the container off the main actor (`url(forUbiquityContainerIdentifier:)` may
    /// block while iCloud sets it up), then watches it. Isolated runs (`--app-dir`,
    /// `TESSERA_APP_DIR`) and background audits never touch the real container.
    func start(arguments: [String] = ProcessInfo.processInfo.arguments) {
        guard store == nil else { return }
        if AppDefaultsIsolation.current != nil || arguments.contains("--nonactivating") {
            install(AccountStore(baseURL: nil, reason: .isolatedRun, defaults: defaults))
            return
        }
        Task.detached(priority: .utility) {
            let store = AccountStore.live(defaults: self.defaults)
            await MainActor.run { self.install(store) }
        }
    }

    /// Adopts a store (tests pass one over a temp directory; `watch` starts the metadata query,
    /// which only makes sense on the real container).
    func install(_ store: AccountStore, watch: Bool = true) {
        watcher?.stop()
        self.store = store
        status = store.unavailableReason.map { .unavailable($0) } ?? .available
        guard store.isAvailable else { return }
        if watch {
            let w = AccountStore.Watcher(store: store)
            w.onChange = { [weak self] key in self?.apply(key) }
            w.start()
            watcher = w
        }
        if volumeObservers.isEmpty {
            let center = NSWorkspace.shared.notificationCenter
            for name in [NSWorkspace.didMountNotification, NSWorkspace.didUnmountNotification] {
                volumeObservers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.refreshDrives() }
                })
            }
        }
        applyAll()
        refreshStats()
    }

    /// Settings ▸ Sync's switch. Turning it on applies what the account has and pushes what this
    /// Mac has, so both sides converge without waiting for the next edit.
    func setEnabled(_ on: Bool) {
        guard isEnabled != on else { return }
        isEnabled = on
        store?.isEnabled = on
        if on {
            applyAll()
            pushAll()
        } else {
            detachedDrives = []
        }
        refreshStats()
    }

    func refreshStats() {
        guard let store else { return }
        Task.detached(priority: .utility) { [store] in
            let stats = store.stats()
            await MainActor.run { self.stats = stats }
        }
    }

    private var canPush: Bool { store?.isActive == true && !applyingRemote }
    private var machine: String { store?.machine ?? "" }

    /// Merges a record into the container off the main actor; stats follow.
    private func push<R: SyncRecord>(_ record: R, id: String? = nil) {
        guard let store, canPush else { return }
        Task.detached(priority: .utility) { [store] in
            store.write(record, id: id)
            await MainActor.run { self.refreshStats() }
        }
    }

    private func pushAll() {
        preferencesDidChange()
        if let keywords = app?.collections.keywords, !keywords.isEmpty { keywordsDidChange(keywords) }
        pushedPeople = [:]
        peopleDidChange()
        styleProfileDidChange()
        learningDidChange()
        if let app, let folder = app.library.folder { driveDidChange(folder: folder, photos: app.library.items.count) }
    }

    // MARK: Preferences

    /// Settings ▸ AI saved (`AgentController.savePreferences`) or Settings ▸ Appearance changed.
    func preferencesDidChange() {
        guard let app, canPush else { return }
        let appearance = AppearanceSettings.shared
        push(PreferencesRecord(ai: app.agent.preferences, accent: String(format: "%06X", appearance.accent),
                               ground: appearance.ground.rawValue, modified: Date(), machine: machine))
    }

    private func applyPreferences() {
        guard let app, let rec = store?.read(PreferencesRecord.self) else { return }
        applyingRemote = true
        defer { applyingRemote = false }
        if app.agent.preferences != rec.ai {
            app.agent.preferences = rec.ai
            app.agent.savePreferences()
            if app.assist.enabled { app.assist.setEnabled(true) }
        }
        let appearance = AppearanceSettings.shared
        if let accent = rec.accent.flatMap({ UInt32($0, radix: 16) }), accent != appearance.accent { appearance.setAccent(accent) }
        if let ground = rec.ground.flatMap(AppearanceSettings.Ground.init(rawValue:)), ground != appearance.ground {
            appearance.setGround(ground)
        }
    }

    // MARK: Keywords

    /// The hierarchy was reloaded after an edit (`LibraryModel.reloadKeywords`). Only the tree
    /// syncs (tags that exist on photos alone stay with the photos), and never an empty one: a
    /// folder with no keywords yet must not clear the account's list.
    func keywordsDidChange(_ keywords: [KeywordInfo]) {
        guard canPush else { return }
        let tree = keywords.filter(\.inTree).map { KeywordsRecord.Keyword(name: $0.name, parent: $0.parent) }
        guard !tree.isEmpty else { return }
        push(KeywordsRecord(keywords: tree, modified: Date(), machine: machine))
    }

    /// Additive: keywords the account has and this index lacks are created (parents first) and
    /// moved under their account parent. Removals are not propagated (a keyword deleted on one
    /// Mac stays on the others; photos keep their tags either way).
    private func applyKeywords() {
        guard let app, let catalog = app.collections.catalog, let rec = store?.read(KeywordsRecord.self) else { return }
        let local = Dictionary(app.collections.keywords.filter(\.inTree).map { ($0.name, $0.parent) }, uniquingKeysWith: { a, _ in a })
        var changed = false
        for keyword in rec.inCreationOrder {
            do {
                if let parent = local[keyword.name] {
                    if parent != keyword.parent { try catalog.store.moveKeyword(name: keyword.name, parent: keyword.parent); changed = true }
                } else {
                    try catalog.store.addKeyword(name: keyword.name, parent: keyword.parent)
                    changed = true
                }
            } catch {
                app.statusMessage = "Sync: keyword “\(keyword.name)”: \(error.localizedDescription)"
            }
        }
        if changed { app.collections.reloadKeywords() }
    }

    // MARK: People

    /// Identities changed (`PeopleModel.onPeopleChange`): named people are pushed by person id
    /// with the cluster ids merged into them; a cleared name is pushed as nil.
    func peopleDidChange() {
        guard let app, canPush else { return }
        let now = Date()
        var records: [PersonRecord] = []
        for tile in app.people.tiles where tile.isNamed {
            records.append(PersonRecord(id: tile.id, name: tile.name, clusters: mergedClusters[tile.id] ?? [], modified: now, machine: machine))
        }
        let named = Set(records.map(\.id))
        for (id, previous) in pushedPeople where !named.contains(id) && previous.name != nil {
            // Named before, unnamed now (or gone): keep the clusters, clear the name.
            guard app.people.person(id) != nil else { continue }
            records.append(PersonRecord(id: id, name: nil, clusters: previous.clusters, modified: now, machine: machine))
        }
        for rec in records where pushedPeople[rec.id].map({ rec.sameContent(as: $0) }) != true {
            pushedPeople[rec.id] = rec
            push(rec, id: rec.id)
        }
    }

    /// `PeopleModel.onMerge`: the sources are now the target (and so is whatever they absorbed).
    func peopleDidMerge(target: String, sources: [String]) {
        var set = mergedClusters[target] ?? []
        for s in sources {
            set.insert(s)
            if let absorbed = mergedClusters.removeValue(forKey: s) { set.formUnion(absorbed) }
        }
        mergedClusters[target] = set
    }

    /// For every account person present on this Mac (by its id or any merged cluster id): the
    /// clusters are merged through the model (one undoable step each) and the name applied.
    private func applyPeople() {
        guard let app, let store, app.people.engine != nil else { return }
        let records = store.list(PersonRecord.self)
        guard !records.isEmpty else { return }
        applyingRemote = true
        defer { applyingRemote = false }
        for rec in records {
            let local = Dictionary(app.people.tiles.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
            let present = rec.allIDs.filter { local[$0] != nil }.sorted()
            guard let target = local[rec.id] != nil ? rec.id : present.first else { continue }
            for source in present where source != target {
                guard let tile = local[source] else { continue }
                let candidate = PeopleNameCandidate(unnamedID: source, namedID: target, name: rec.name ?? local[target]?.displayName ?? tile.displayName,
                                                    similarity: 1)
                app.people.accept(candidate)
                peopleDidMerge(target: target, sources: [source])
            }
            if let tile = app.people.person(target), tile.name != rec.name {
                app.people.name(target, as: rec.name ?? "")
            }
            mergedClusters[target, default: []].formUnion(rec.allIDs.subtracting([target]))
        }
    }

    // MARK: Style profile and learning

    /// The engine stores one file per library in app support, named by the library id in hex.
    private func shootFile(_ folder: String, libraryID: String) -> URL {
        let key = libraryID.utf8.map { String(format: "%02x", $0) }.joined()
        return supportDirectory.appendingPathComponent(folder, isDirectory: true).appendingPathComponent("\(key).json")
    }

    /// The questionnaire was saved, a training run finished or an agent edit was accepted: the
    /// open shoot's profile (status plus the engine's stored model) merges into the account record.
    func styleProfileDidChange() {
        guard let app, let store, canPush, let lib = app.engineLibrary, !lib.isReadOnly, let folder = lib.folder else { return }
        let support = supportDirectory
        let machine = machine
        Task.detached(priority: .utility) { [store] in
            guard let status = try? lib.engine.styleProfileStatus(libraryFolder: folder.path), status.stored else { return }
            let key = status.libraryId.utf8.map { String(format: "%02x", $0) }.joined()
            let model = try? Data(contentsOf: support.appendingPathComponent("style-profile").appendingPathComponent("\(key).json"))
            let q = status.questionnaire
            let questionnaire = QuestionnaireRecord(brightness: q.brightness, contrast: q.contrast, warmth: q.warmth,
                                                    saturation: q.saturation, skinTonePriority: q.skinTonePriority)
            let now = Date()
            let shoot = StyleProfileRecord.Shoot(libraryID: status.libraryId, samples: Int(status.samples), questionnaire: questionnaire,
                                                 modified: now, machine: machine, model: model)
            store.write(StyleProfileRecord(questionnaire: questionnaire, shoots: [status.libraryId: shoot], modified: now, machine: machine))
            await MainActor.run { self.refreshStats() }
        }
    }

    /// Labels changed (`AssistController.labels`): coalesced, since decisions come in bursts.
    func learningDidChange() {
        guard let app, let store, canPush, app.isEngineBacked, !app.cull.isReadOnly else { return }
        learningPush?.cancel()
        let cull = app.cull
        let support = supportDirectory
        let machine = machine
        learningPush = Task { [store] in
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled, let status = try? cull.assistStatus(), status.labels > 0 else { return }
            let key = status.libraryId.utf8.map { String(format: "%02x", $0) }.joined()
            let model = try? Data(contentsOf: support.appendingPathComponent("cull-learning").appendingPathComponent("\(key).json"))
            let now = Date()
            let shoot = LearningRecord.Shoot(libraryID: status.libraryId, labels: Int(status.labels), modified: now, machine: machine, model: model)
            await Task.detached(priority: .utility) {
                store.write(LearningRecord(shoots: [status.libraryId: shoot], modified: now, machine: machine))
            }.value
            refreshStats()
        }
    }

    /// Shoots learned on another Mac land in app support, where the engine opens them with the
    /// library. The open library's own files are the engine's (it writes them); they are skipped
    /// and picked up when the folder is next opened.
    private func applyShoots() {
        guard let store else { return }
        var openIDs: Set<String> = []
        if let app, let lib = app.engineLibrary, !lib.isReadOnly, let folder = lib.folder {
            if let s = try? lib.engine.styleProfileStatus(libraryFolder: folder.path) { openIDs.insert(s.libraryId) }
            if let s = try? app.cull.assistStatus() { openIDs.insert(s.libraryId) }
        }
        var files: [(URL, Data, Date)] = []
        if let rec = store.read(StyleProfileRecord.self) {
            for shoot in rec.shoots.values where !openIDs.contains(shoot.libraryID) {
                if let model = shoot.model { files.append((shootFile("style-profile", libraryID: shoot.libraryID), model, shoot.modified)) }
            }
        }
        if let rec = store.read(LearningRecord.self) {
            for shoot in rec.shoots.values where !openIDs.contains(shoot.libraryID) {
                if let model = shoot.model { files.append((shootFile("cull-learning", libraryID: shoot.libraryID), model, shoot.modified)) }
            }
        }
        for (url, data, modified) in files {
            let local = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
            if let local, local >= modified { continue }
            do {
                try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
                try data.write(to: url, options: .atomic)
            } catch {
                NSLog("Sync: could not restore %@: %@", url.lastPathComponent, error.localizedDescription)
            }
        }
    }

    // MARK: Drives

    /// A folder on an external or removable volume was opened (`AppModel.rememberOpenedFolder`):
    /// the drive's record gains (or updates) that root. The boot volume stays local (SPEC §1).
    func driveDidChange(folder: URL, photos: Int) {
        guard let store, canPush else { return }
        let keys: Set<URLResourceKey> = [.volumeUUIDStringKey, .volumeNameKey, .volumeURLKey, .volumeIsInternalKey,
                                         .volumeIsRemovableKey, .volumeIsEjectableKey]
        guard let v = try? folder.resourceValues(forKeys: keys), let uuid = v.volumeUUIDString, let volume = v.volume else { return }
        let external = v.volumeIsInternal == false || v.volumeIsRemovable == true || v.volumeIsEjectable == true
        guard external else { return }
        let volumePath = volume.standardizedFileURL.path
        var relative = folder.standardizedFileURL.path
        if relative.hasPrefix(volumePath) { relative = String(relative.dropFirst(volumePath.count)) }
        relative = relative.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let now = Date()
        let root = DriveRecord.Root(path: relative, lastMountPath: folder.path, photos: photos, lastSeen: now)
        push(DriveRecord(volumeUUID: uuid, name: v.volumeName ?? volume.lastPathComponent, roots: [root],
                         lastSeenMachine: store.machine, lastSeenMachineName: Host.current().localizedName,
                         lastSeen: now, modified: now, machine: store.machine), id: uuid)
    }

    /// Drives the account knows that are not mounted here.
    func refreshDrives() {
        guard let store, store.isActive else { detachedDrives = []; return }
        let volumes = FileManager.default.mountedVolumeURLs(includingResourceValuesForKeys: [.volumeUUIDStringKey], options: [.skipHiddenVolumes]) ?? []
        let mounted = Set(volumes.compactMap { (try? $0.resourceValues(forKeys: [.volumeUUIDStringKey]))?.volumeUUIDString })
        let detached = store.list(DriveRecord.self).filter { !mounted.contains($0.volumeUUID) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        if detached != detachedDrives { detachedDrives = detached }
    }

    // MARK: Applying

    /// A folder was opened: the account's keywords, people and shoots apply to it.
    func libraryInstalled() {
        guard store?.isActive == true else { return }
        applyKeywords()
        applyPeople()
        applyShoots()
    }

    private func applyAll() {
        guard store?.isActive == true else { return }
        applyPreferences()
        applyKeywords()
        applyPeople()
        applyShoots()
        refreshDrives()
    }

    /// The watcher reported a record written elsewhere.
    private func apply(_ key: RecordKey) {
        guard store?.isActive == true else { return }
        switch key.kind {
        case .preferences: applyPreferences()
        case .keywords: applyKeywords()
        case .person: applyPeople()
        case .styleProfile, .learning: applyShoots()
        case .drive: refreshDrives()
        }
        refreshStats()
    }
}
