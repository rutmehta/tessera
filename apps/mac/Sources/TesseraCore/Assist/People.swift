import CoreGraphics
import Foundation
import Observation
import Synchronization
import TesseraFFI

// People view (docs/01 §1.4, WP M2-40): persistent identities from face clusters, naming, merge /
// split / reassign, and the filter bar's Person facet. UI-free so it can be tested against a stub
// engine; the app drives it with `CullController` (engine libraries).

/// One face of one photo, in item ids (the engine's `PersonFace` uses image ids).
public struct PersonFaceRef: Hashable, Sendable, Comparable {
    public var item: Int
    public var ordinal: UInt32
    public init(item: Int, ordinal: UInt32) { self.item = item; self.ordinal = ordinal }
    public static func < (a: PersonFaceRef, b: PersonFaceRef) -> Bool {
        (a.item, a.ordinal) < (b.item, b.ordinal)
    }

    /// Drag payload for face chips ("face:<item>:<ordinal>").
    public var dragToken: String { "face:\(item):\(ordinal)" }
    public init?(dragToken: String) {
        let parts = dragToken.split(separator: ":")
        guard parts.count == 3, parts[0] == "face", let item = Int(parts[1]), let ordinal = UInt32(parts[2]) else { return nil }
        self.init(item: item, ordinal: ordinal)
    }
}

/// A face's persisted assignment (`CullSession.personAssignments`).
public struct PersonAssignment: Sendable, Equatable {
    public var face: PersonFaceRef
    public var personID: String
    /// nil: the identity has no name yet.
    public var name: String?
    public var confirmed: Bool
    public init(face: PersonFaceRef, personID: String, name: String?, confirmed: Bool) {
        self.face = face; self.personID = personID; self.name = name; self.confirmed = confirmed
    }
}

/// "This unnamed cluster looks like <name>" (read-only; accepting it merges).
public struct PeopleNameCandidate: Sendable, Equatable, Hashable {
    public var unnamedID: String
    public var namedID: String
    public var name: String
    /// Cosine similarity, not a calibrated probability.
    public var similarity: Float
    public init(unnamedID: String, namedID: String, name: String, similarity: Float) {
        self.unnamedID = unnamedID; self.namedID = namedID; self.name = name; self.similarity = similarity
    }
}

/// Result of a clustering job (`CullSession.refreshPeople`).
public struct PeopleRefresh: Sendable, Equatable {
    public var assigned: UInt64
    public var reclustered: Bool
    /// Clustered from a reservoir sample (more than `PeopleModel.clusteringSample` eligible faces).
    public var approximate: Bool
    /// Faces the job actually fitted on (`PeopleJobResult.sample_size`); 0 when nothing was pending.
    public var sampleSize: Int
    public init(assigned: UInt64, reclustered: Bool, approximate: Bool, sampleSize: Int = 0) {
        self.assigned = assigned; self.reclustered = reclustered; self.approximate = approximate
        self.sampleSize = sampleSize
    }
}

/// Settings ▸ Library opt-ins for naming (both off by default: no sidecar I/O).
public struct PeopleNamingOptions: Sendable, Equatable {
    public var writeFaceRegions = false
    public var personKeywords = false
    public init(writeFaceRegions: Bool = false, personKeywords: Bool = false) {
        self.writeFaceRegions = writeFaceRegions; self.personKeywords = personKeywords
    }
    /// Keywords are only written with the sidecar regions (the engine appends them to the XMP).
    public var ffi: PeopleNameOptions {
        PeopleNameOptions(writeSidecars: writeFaceRegions, personKeywords: writeFaceRegions && personKeywords)
    }
}

/// What the People view needs from the engine. `CullController` conforms for engine libraries.
public protocol PeopleEngine: AnyObject, Sendable {
    /// A clustering job to run off the main actor (`refresh_people`).
    func peopleRefreshJob(force: Bool) throws -> @Sendable () throws -> PeopleRefresh
    func people(refresh: Bool) throws -> [PersonSummary]
    /// Every member face of one person in one call (`person_members`), in item ids.
    func personMembers(_ person: String) throws -> [PersonFaceRef]
    func personAssignments(_ item: Int) throws -> [PersonAssignment]
    func faceStrip(_ item: Int) throws -> [FaceChip]
    func peopleNameSuggestions() throws -> [PeopleNameCandidate]
    func namePerson(_ id: String, name: String?, options: PeopleNamingOptions) throws
    func mergePeople(target: String, source: String) throws
    func splitPerson(_ source: String, newID: String, faces: [PersonFaceRef]) throws
    func assignFace(_ face: PersonFaceRef, to person: String) throws
    func confirmFace(_ face: PersonFaceRef, confirmed: Bool) throws
    func items(withPerson person: String, eyesClosedBelow: Double?) throws -> [Int]
    /// Session-local people history (`undo_people_edit` / `redo_people_edit`): the applied
    /// edit's description ("Merge people"), nil with an empty history.
    func undoPeopleEdit() throws -> String?
    func redoPeopleEdit() throws -> String?
}

/// A person tile: identity, name, frames and counts. Member faces are loaded only for the
/// person open in the detail view (`person_members`, one call).
public struct PersonTile: Sendable, Equatable, Identifiable {
    public struct Member: Sendable, Equatable, Hashable, Identifiable {
        public var face: PersonFaceRef
        public var confirmed: Bool
        public var id: PersonFaceRef { face }
    }

    public var id: String
    /// nil: unnamed cluster.
    public var name: String?
    /// Frames (item ids) in queue order.
    public var items: [Int]
    public var faces: Int
    /// The sharpest member (the engine's cover): the crop when there is no medoid.
    public var coverItem: Int?
    public var coverOrdinal: UInt32
    /// Member faces: filled for the detail person only (empty on grid tiles).
    public var members: [Member]
    /// Confirmed faces (`PersonInfo.confirmed_count`, same scope as `faces`).
    public var confirmedCount: Int = 0
    /// The most central member (`PersonInfo.medoid_face`); nil when invalidated or outside the queue.
    public var medoid: PersonFaceRef? = nil

    public var displayName: String { name ?? "Unnamed" }
    public var isNamed: Bool { name != nil }
    /// Every member face confirmed (protected from automatic refits).
    public var isConfirmed: Bool { faces > 0 && confirmedCount >= faces }
    /// The face shown for this person: the medoid, else the sharpest member.
    public var cover: PersonFaceRef? {
        medoid ?? coverItem.map { PersonFaceRef(item: $0, ordinal: coverOrdinal) }
    }
}

@MainActor @Observable
public final class PeopleModel {
    /// Reservoir size of library-scale clustering (`ml_faces::clustering`, 1024 eligible faces):
    /// the footnote's fallback when a job did not report its `sample_size`.
    public static let clusteringSample = 1024
    /// Engine bound on the session-local people history.
    public static let historyLimit = 32
    static let writeRegionsKey = "People.WriteFaceRegions"
    static let personKeywordsKey = "People.PersonKeywords"

    @ObservationIgnored public var engine: (any PeopleEngine)?
    @ObservationIgnored private let defaults: UserDefaults
    /// Called after identities changed (names, merges, moves): the face strip and filters follow.
    @ObservationIgnored public var onPeopleChange: (() -> Void)?
    /// Called after `sources` were merged into `target` (CS-2: the account record unions them).
    @ObservationIgnored public var onMerge: ((_ target: String, _ sources: [String]) -> Void)?

    /// Named first, then by frame count, then faces, then id.
    public private(set) var tiles: [PersonTile] = []
    /// Selected tiles (merge).
    public var selection: Set<String> = []
    /// The person shown in the detail view.
    public var detailID: String?
    /// Faces selected in the detail view (split).
    public var faceSelection: Set<PersonFaceRef> = []
    /// Name suggestions per unnamed person, most similar first.
    public private(set) var suggestions: [String: [PeopleNameCandidate]] = [:]
    public private(set) var isRefreshing = false
    /// The last clustering job was approximate (sampled).
    public private(set) var approximate = false
    /// Faces the last clustering job fitted on (0: not reported).
    public private(set) var sampleSize = 0
    /// Mirror of the engine's people history (descriptions, oldest first) for the Edit menu
    /// titles; the engine stays the source of truth and reports what it actually replayed.
    public private(set) var undoHistory: [String] = []
    public private(set) var redoHistory: [String] = []
    /// Last result or error, for the status bar.
    public private(set) var message: String?
    public var naming: PeopleNamingOptions {
        didSet {
            defaults.set(naming.writeFaceRegions, forKey: Self.writeRegionsKey)
            defaults.set(naming.personKeywords, forKey: Self.personKeywordsKey)
        }
    }
    /// Filter bar ▸ Person: selected person ids and the frames with any of them.
    public private(set) var facet: Set<String> = []
    public private(set) var facetItems: Set<Int>?
    @ObservationIgnored private var faceRects: [Int: [UInt32: CGRect]] = [:]
    @ObservationIgnored private var refreshGeneration = 0

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        naming = PeopleNamingOptions(writeFaceRegions: defaults.bool(forKey: Self.writeRegionsKey),
                                     personKeywords: defaults.bool(forKey: Self.personKeywordsKey))
    }

    // MARK: Lifecycle

    /// A new library: forget everything (the facet, the detail view, cached rectangles).
    public func install(_ engine: (any PeopleEngine)?) {
        self.engine = engine
        refreshGeneration += 1
        installGeneration += 1
        tiles = []; selection = []; detailID = nil; faceSelection = []; suggestions = [:]
        isRefreshing = false; approximate = false; sampleSize = 0; message = nil
        undoHistory = []; redoHistory = []
        facet = []; facetItems = nil; faceRects = [:]
    }

    /// The library was renumbered in place: follow the photos, drop those that left.
    public func libraryDidUpdate(_ remap: (Int) -> Int?) {
        faceRects = [:]
        func move(_ f: PersonFaceRef) -> PersonFaceRef? { remap(f.item).map { PersonFaceRef(item: $0, ordinal: f.ordinal) } }
        tiles = tiles.map { tile in
            var t = tile
            t.items = t.items.compactMap(remap)
            t.coverItem = t.coverItem.flatMap(remap)
            t.medoid = t.medoid.flatMap(move)
            t.members = t.members.compactMap { m in move(m.face).map { PersonTile.Member(face: $0, confirmed: m.confirmed) } }
            return t
        }
        faceSelection = Set(faceSelection.compactMap(move))
        if let items = facetItems { facetItems = Set(items.compactMap(remap)) }
    }

    public func person(_ id: String?) -> PersonTile? { id.flatMap { pid in tiles.first { $0.id == pid } } }
    public var named: [PersonTile] { tiles.filter(\.isNamed) }
    public var detail: PersonTile? { person(detailID) }

    /// Named first, then by frames, faces and id (stable).
    public static func sorted(_ tiles: [PersonTile]) -> [PersonTile] {
        tiles.sorted { a, b in
            if a.isNamed != b.isNamed { return a.isNamed }
            if a.items.count != b.items.count { return a.items.count > b.items.count }
            if a.faces != b.faces { return a.faces > b.faces }
            if a.isNamed, let an = a.name, let bn = b.name, an != bn {
                return an.localizedStandardCompare(bn) == .orderedAscending
            }
            return a.id < b.id
        }
    }

    // MARK: Engine access (ST-2: every FFI call runs off the main actor)

    /// Operations run one after another (an edit, then its reload), each on a worker, so the
    /// engine's people history and the mirror here stay in step. The view keeps its last state
    /// until a result lands; a `Busy` refusal (ST-1) is retried by `EngineCall`.
    @ObservationIgnored private var chain: Task<Void, Never>?
    /// Bumped by `install`: results of operations started on the previous library are dropped.
    @ObservationIgnored private var installGeneration = 0

    private func serialized<T: Sendable>(_ body: @escaping @MainActor () async -> T) async -> T {
        let previous = chain
        let task = Task { @MainActor () -> T in
            await previous?.value
            return await body()
        }
        chain = Task { _ = await task.value }
        return await task.value
    }

    private struct NoEngine: LocalizedError {
        var errorDescription: String? { "open a folder on the engine first" }
    }

    /// One engine call on a worker (`EngineCall.detached`: `Busy` retried, other errors thrown).
    private func call<T: Sendable>(_ body: @escaping @Sendable (any PeopleEngine) throws -> T) async throws -> T {
        guard let engine else { throw NoEngine() }
        return try await EngineCall.detached { try body(engine) }
    }

    /// What one reload reads: the people, the detail person's members, the suggestions and the
    /// facet's frames, in one worker pass (`people` first, as the engine's history expects).
    private struct Snapshot: Sendable {
        var people: [PersonSummary]
        var members: [PersonTile.Member]?
        var suggestions: [PeopleNameCandidate]
        var facetItems: Set<Int>?
    }

    private nonisolated static func snapshot(_ engine: any PeopleEngine, refresh: Bool, detail: String?,
                                             facet: Set<String>) throws -> Snapshot {
        let people = try engine.people(refresh: refresh)
        var members: [PersonTile.Member]?
        if let detail, let summary = people.first(where: { $0.id == detail }) {
            members = try Self.members(of: detail, confirmedCount: summary.confirmedCount, engine: engine)
        }
        let suggestions = (try? engine.peopleNameSuggestions()) ?? []
        var facetItems: Set<Int>?
        if !facet.isEmpty {
            var items = Set<Int>()
            for id in facet.sorted() where people.contains(where: { $0.id == id }) {
                items.formUnion(try engine.items(withPerson: id, eyesClosedBelow: nil))
            }
            facetItems = items
        }
        return Snapshot(people: people, members: members, suggestions: suggestions, facetItems: facetItems)
    }

    // MARK: Loading

    /// The People view opened, or faces were analysed: run the incremental job off the main
    /// actor (`refresh_people(force: false)`), then reload. `force` refits everything.
    public func refresh(force: Bool = false) async {
        guard let engine else { return }
        let job: @Sendable () throws -> PeopleRefresh
        do { job = try engine.peopleRefreshJob(force: force) } catch { fail("People", error); return }
        refreshGeneration += 1
        let generation = refreshGeneration
        isRefreshing = true
        let result = await Task.detached(priority: .utility) { Result { try job() } }.value
        guard generation == refreshGeneration else { return }
        isRefreshing = false
        switch result {
        case .success(let r):
            if r.reclustered || r.approximate {
                approximate = r.approximate
                sampleSize = r.sampleSize
            }
            await reload()
        case .failure(let error):
            fail("People", error)
            await reload()
        }
    }

    /// Re-reads identities, names, counts and suggestions from `people()` alone (names, counts
    /// and the medoid come with each person), then the detail person's members. `refresh` also
    /// ingests new faces (`people(refresh: true)`, the incremental job) — used after every edit.
    public func reload(refresh: Bool = false) async {
        guard engine != nil else { tiles = []; return }
        await serialized { await self.reloadNow(refresh: refresh) }
    }

    /// The reload itself, inside the chain (an edit calls it after its own engine step).
    private func reloadNow(refresh: Bool) async {
        let generation = installGeneration
        let detail = detailID
        let facet = self.facet
        do {
            let snapshot = try await call { try Self.snapshot($0, refresh: refresh, detail: detail, facet: facet) }
            guard generation == installGeneration else { return }
            apply(snapshot)
        } catch {
            guard generation == installGeneration else { return }
            fail("People", error)
            suggestions = [:]
        }
    }

    private func apply(_ snapshot: Snapshot) {
        tiles = Self.sorted(snapshot.people.map { p in
            let name = p.named ? p.name.trimmingCharacters(in: .whitespaces) : ""
            return PersonTile(id: p.id, name: name.isEmpty ? nil : name, items: p.items, faces: p.faces,
                              coverItem: p.coverItem, coverOrdinal: p.coverOrdinal, members: [],
                              confirmedCount: p.confirmedCount, medoid: p.medoid)
        })
        let ids = Set(tiles.map(\.id))
        selection.formIntersection(ids)
        if let d = detailID, !ids.contains(d) { detailID = nil }
        if let d = detailID, let i = tiles.firstIndex(where: { $0.id == d }), let members = snapshot.members {
            tiles[i].members = members
            faceSelection.formIntersection(members.map(\.face))
        } else {
            faceSelection = []
        }
        faceRects = [:]
        let unnamed = Set(tiles.filter { !$0.isNamed }.map(\.id))
        let named = Set(tiles.filter(\.isNamed).map(\.id))
        suggestions = Dictionary(grouping: snapshot.suggestions
            .filter { unnamed.contains($0.unnamedID) && named.contains($0.namedID) }, by: \.unnamedID)
            .mapValues { $0.sorted { $0.similarity > $1.similarity } }
        facet.formIntersection(ids)
        facetItems = facet.isEmpty ? nil : snapshot.facetItems
    }

    /// Every member of one person (one `person_members` call), in item order. Confirmation comes
    /// from the person's counts; only a partly confirmed person reads its members' assignments.
    public func members(_ id: String) async throws -> [PersonTile.Member] {
        guard engine != nil else { return [] }
        let confirmed = person(id)?.confirmedCount ?? 0
        return try await call { try Self.members(of: id, confirmedCount: confirmed, engine: $0) }
    }

    private nonisolated static func members(of id: String, confirmedCount confirmed: Int,
                                            engine: any PeopleEngine) throws -> [PersonTile.Member] {
        let faces = try engine.personMembers(id).sorted()
        if confirmed == 0 { return faces.map { .init(face: $0, confirmed: false) } }
        if confirmed >= faces.count { return faces.map { .init(face: $0, confirmed: true) } }
        var state: [PersonFaceRef: Bool] = [:]
        for item in Set(faces.map(\.item)).sorted() {
            for a in try engine.personAssignments(item) where a.personID == id { state[a.face] = a.confirmed }
        }
        return faces.map { .init(face: $0, confirmed: state[$0] ?? false) }
    }

    /// Fills the detail person's members; the face selection keeps only its faces.
    private func loadDetailMembers() async {
        guard let d = detailID, tiles.contains(where: { $0.id == d }) else {
            faceSelection = []
            return
        }
        let generation = installGeneration
        do {
            let members = try await members(d)
            guard generation == installGeneration, detailID == d, let i = tiles.firstIndex(where: { $0.id == d }) else { return }
            tiles[i].members = members
            faceSelection.formIntersection(members.map(\.face))
        } catch {
            guard generation == installGeneration else { return }
            fail("People", error)
        }
    }

    /// Footnote under the grid when the clustering was sampled (the job's reported sample size).
    public var approximateNote: String? {
        let n = sampleSize > 0 ? sampleSize : Self.clusteringSample
        return approximate ? "Clustered from a sample of \(n.formatted()) faces" : nil
    }

    /// Normalized face rectangle (origin top-left) in the item's preview, from the face strip.
    /// `face_strip` is UI-safe (ST-1): a `Busy` answer leaves the cache empty so the next draw asks again.
    public func faceRect(_ face: PersonFaceRef) -> CGRect? {
        if let hit = faceRects[face.item] { return hit[face.ordinal] }
        guard let strip = try? engine?.faceStrip(face.item) else { return nil }
        let rects = Dictionary(strip.map { ($0.ordinal, $0.rect) }, uniquingKeysWith: { a, _ in a })
        faceRects[face.item] = rects
        return rects[face.ordinal]
    }

    // MARK: Edits (each goes through the FFI, then reloads from `people(refresh: true)`)

    private func fail(_ verb: String, _ error: Error) {
        message = "\(verb) failed: \(error.localizedDescription)"
    }

    /// One engine edit = one step of the engine's people history (mirrored for the menu titles).
    private func recordStep(_ description: String) {
        redoHistory = []
        undoHistory.append(description)
        if undoHistory.count > Self.historyLimit { undoHistory.removeFirst(undoHistory.count - Self.historyLimit) }
    }

    private struct Step: Sendable {
        let description: String
        let call: @Sendable (any PeopleEngine) throws -> Void
    }

    /// Runs `steps` in order on a worker (each one engine history step), mirrors the ones that
    /// applied, reloads from `people(refresh: true)` and reports `message` on success.
    private func edit(_ verb: String, _ steps: [Step], message: String) async -> Bool {
        guard engine != nil else { self.message = "\(verb): open a folder on the engine first"; return false }
        return await serialized {
            let generation = self.installGeneration
            // How many steps applied before a failure, so the mirror matches the engine's history.
            let applied = Mutex(0)
            let result = await Task { try await self.call { engine in
                for step in steps {
                    try step.call(engine)
                    applied.withLock { $0 += 1 }
                }
            } }.result
            guard generation == self.installGeneration else { return false }
            for step in steps.prefix(applied.withLock { $0 }) { self.recordStep(step.description) }
            if case .failure(let error) = result {
                self.fail(verb, error)
                await self.reloadNow(refresh: true)
                return false
            }
            self.message = message
            await self.reloadNow(refresh: true)
            self.onPeopleChange?()
            return true
        }
    }

    /// Return in a tile's name field: `name_person` with the Settings opt-ins. Empty clears.
    @discardableResult
    public func name(_ id: String, as text: String) async -> Bool {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let old = person(id)?.name
        guard trimmed != (old ?? "") else { return false }
        let options = naming
        let written = options.writeFaceRegions ? " · face regions written to XMP" : ""
        return await edit("Name", [Step(description: "Name person") {
            try $0.namePerson(id, name: trimmed.isEmpty ? nil : trimmed, options: options)
        }], message: trimmed.isEmpty ? "Cleared the name of \(old ?? "person")" : "Named \(trimmed)\(written)")
    }

    /// A suggestion from the name menu: the unnamed cluster is the named person (merge, name wins).
    @discardableResult
    public func accept(_ suggestion: PeopleNameCandidate) async -> Bool {
        let ok = await edit("Merge", [Step(description: "Merge people") {
            try $0.mergePeople(target: suggestion.namedID, source: suggestion.unnamedID)
        }], message: "Merged into \(suggestion.name)")
        if ok { onMerge?(suggestion.namedID, [suggestion.unnamedID]) }
        return ok
    }

    /// The person that survives a merge: a named one first, then the most frames.
    public static func mergeTarget(_ people: [PersonTile]) -> PersonTile? { sorted(people).first }

    public var canMerge: Bool { selection.count >= 2 }

    /// Toolbar ▸ Merge: every selected person into the target (the target's name wins).
    @discardableResult
    public func mergeSelection() async -> Bool {
        let people = tiles.filter { selection.contains($0.id) }
        guard people.count >= 2, let target = Self.mergeTarget(people) else {
            message = "Select two or more people to merge"
            return false
        }
        let sources = people.filter { $0.id != target.id }
        let steps = sources.map { source in
            Step(description: "Merge people") { try $0.mergePeople(target: target.id, source: source.id) }
        }
        let ok = await edit("Merge", steps, message: "Merged \(sources.count + 1) people into \(target.displayName)")
        if ok {
            selection = [target.id]
            onMerge?(target.id, sources.map(\.id))
        }
        return ok
    }

    /// Detail ▸ Split: the selected faces become a new unnamed person. Returns its id.
    @discardableResult
    public func splitSelection(newID: String = "person-split-\(UUID().uuidString.lowercased())") async -> String? {
        guard let person = detail else { return nil }
        let faces = person.members.map(\.face).filter(faceSelection.contains)
        guard !faces.isEmpty else { message = "Select faces to split off"; return nil }
        guard faces.count < person.members.count else {
            message = "Leave at least one face in \(person.displayName)"
            return nil
        }
        let ok = await edit("Split", [Step(description: "Split person") {
            try $0.splitPerson(person.id, newID: newID, faces: faces)
        }], message: "Split \(faces.count) face\(faces.count == 1 ? "" : "s") into a new person")
        guard ok else { return nil }
        faceSelection = []
        return newID
    }

    /// A face chip dropped on another person's tile (or Move To in its menu).
    @discardableResult
    public func reassign(_ face: PersonFaceRef, to target: String) async -> Bool {
        guard person(target) != nil else { return false }
        // The face's current owner, from its photo's assignments (tiles carry no members).
        let current = try? await call { try $0.personAssignments(face.item) }.first { $0.face == face }
        if current?.personID == target { return false }
        let name = person(target)?.displayName ?? "person"
        let ok = await edit("Move face", [Step(description: "Assign face") { try $0.assignFace(face, to: target) }],
                            message: "Moved the face to \(name) (unconfirmed)")
        if ok { faceSelection.remove(face) }
        return ok
    }

    /// Confirm protects an assignment from automatic refits.
    @discardableResult
    public func setConfirmed(_ face: PersonFaceRef, _ confirmed: Bool) async -> Bool {
        await edit(confirmed ? "Confirm" : "Unconfirm",
                   [Step(description: confirmed ? "Confirm face" : "Unconfirm face") { try $0.confirmFace(face, confirmed: confirmed) }],
                   message: confirmed ? "Confirmed the face" : "Unconfirmed the face")
    }

    /// Confirms every face of the detail person.
    @discardableResult
    public func confirmAll() async -> Bool {
        guard let person = detail else { return false }
        let pending = person.members.filter { !$0.confirmed }
        guard !pending.isEmpty else { return false }
        let steps = pending.map { m in Step(description: "Confirm face") { try $0.confirmFace(m.face, confirmed: true) } }
        return await edit("Confirm", steps,
                          message: "Confirmed \(pending.count) face\(pending.count == 1 ? "" : "s") of \(person.displayName)")
    }

    // MARK: Undo / Redo (Edit menu while the People view is frontmost)

    /// "Merge people" → "Merge People" (menu title case).
    public static func menuTitle(_ description: String) -> String {
        description.split(separator: " ").map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ")
    }

    /// Edit ▸ Undo's title: "Undo Merge People", or plain "Undo" with nothing recorded.
    public var undoTitle: String { undoHistory.last.map { "Undo \(Self.menuTitle($0))" } ?? "Undo" }
    public var redoTitle: String { redoHistory.last.map { "Redo \(Self.menuTitle($0))" } ?? "Redo" }

    /// Undo the last people edit (`undo_people_edit`), then reload from `people(refresh: false)`.
    @discardableResult
    public func undo() async -> Bool { await replay(redo: false) }

    /// Redo the last undone people edit (`redo_people_edit`), then reload from `people(refresh: false)`.
    @discardableResult
    public func redo() async -> Bool { await replay(redo: true) }

    private func replay(redo: Bool) async -> Bool {
        let verb = redo ? "Redo" : "Undo"
        guard engine != nil else { message = "Nothing to \(verb.lowercased())"; return false }
        return await serialized {
            let generation = self.installGeneration
            let applied: String?
            do {
                applied = try await self.call { redo ? try $0.redoPeopleEdit() : try $0.undoPeopleEdit() }
            } catch {
                guard generation == self.installGeneration else { return false }
                // The engine keeps its history on a conflict: so does the mirror.
                self.fail(verb, error)
                return false
            }
            guard generation == self.installGeneration else { return false }
            guard let description = applied else {
                if redo { self.redoHistory = [] } else { self.undoHistory = [] }
                self.message = "Nothing to \(verb.lowercased())"
                return false
            }
            if redo {
                if !self.redoHistory.isEmpty { self.redoHistory.removeLast() }
                self.undoHistory.append(description)
            } else {
                if !self.undoHistory.isEmpty { self.undoHistory.removeLast() }
                self.redoHistory.append(description)
            }
            self.message = "\(verb) \(Self.menuTitle(description))"
            await self.reloadNow(refresh: false)
            self.onPeopleChange?()
            return true
        }
    }

    // MARK: Selection

    /// Click (replace), ⌘-click (toggle), ⇧-click (extend in grid order).
    public func click(_ id: String, command: Bool = false, shift: Bool = false) {
        if command {
            if selection.contains(id) { selection.remove(id) } else { selection.insert(id) }
        } else if shift, let anchor = tiles.firstIndex(where: { selection.contains($0.id) }),
                  let to = tiles.firstIndex(where: { $0.id == id }) {
            selection.formUnion(tiles[min(anchor, to)...max(anchor, to)].map(\.id))
        } else {
            selection = [id]
        }
    }

    public func toggleFace(_ face: PersonFaceRef) {
        if faceSelection.contains(face) { faceSelection.remove(face) } else { faceSelection.insert(face) }
    }

    /// Opens the detail view at once; its members arrive from the worker.
    public func openDetail(_ id: String) async {
        clearDetailMembers()
        detailID = id
        faceSelection = []
        selection = [id]
        await serialized { await self.loadDetailMembers() }
    }

    public func closeDetail() {
        clearDetailMembers()
        detailID = nil
        faceSelection = []
    }

    private func clearDetailMembers() {
        guard let d = detailID, let i = tiles.firstIndex(where: { $0.id == d }) else { return }
        tiles[i].members = []
    }

    // MARK: Filter bar ▸ Person

    public func toggleFacet(_ id: String) async {
        if facet.contains(id) { facet.remove(id) } else { facet.insert(id) }
        await refreshFacetItems()
    }

    public func setFacet(_ ids: Set<String>) async {
        facet = ids
        await refreshFacetItems()
    }

    public func clearFacet() async { await setFacet([]) }

    /// Frames with any of the selected people (`frames_with_person`), nil with no selection.
    private func refreshFacetItems() async {
        facet.formIntersection(tiles.map(\.id))
        guard !facet.isEmpty, engine != nil else { facetItems = nil; return }
        let ids = facet
        let generation = installGeneration
        do {
            let items = try await call { engine in
                var items = Set<Int>()
                for id in ids.sorted() { items.formUnion(try engine.items(withPerson: id, eyesClosedBelow: nil)) }
                return items
            }
            guard generation == installGeneration, facet == ids else { return }
            facetItems = items
        } catch {
            guard generation == installGeneration else { return }
            fail("Person filter", error)
            facetItems = []
        }
    }

    /// Facet label: "Person", "Person · Alice", "Person · 2".
    public var facetTitle: String {
        switch facet.count {
        case 0: "Person"
        case 1: "Person · \(person(facet.first)?.displayName ?? "1")"
        default: "Person · \(facet.count)"
        }
    }

    /// Frames of `id` within the other filters' matches (nil: no other filter).
    public func facetCount(_ id: String, within matches: Set<Int>?) -> Int {
        guard let tile = person(id) else { return 0 }
        guard let matches else { return tile.items.count }
        return tile.items.filter(matches.contains).count
    }


    /// The grid rule: items that pass the source and the filter bar (`matches`, nil = all) and,
    /// with a Person facet, show one of the selected people. Order follows `base`.
    public static func visible(base: [Int], matches: Set<Int>?, people: Set<Int>?) -> [Int] {
        base.filter { matches?.contains($0) != false && people?.contains($0) != false }
    }
}

// MARK: - Engine conformance

/// The people calls read the library's id maps (mutated on the main actor only) and the session
/// (locked by the engine), so the adapter can be handed to a worker (ST-2).
extension CullController: @unchecked Sendable {}

extension CullController: PeopleEngine {
    private func peopleLibrary() throws -> EngineLibrary {
        guard case .engine(let lib) = backend else { throw CullError.unavailable("People need a folder opened on the engine") }
        try lib.accessMode.requireCatalogMutation()
        return lib
    }

    private func personFace(_ f: PersonFaceRef, _ lib: EngineLibrary) throws -> PersonFace {
        guard lib.imageIDs.indices.contains(f.item) else { throw CullError.unavailable("That photo left the library") }
        return PersonFace(imageId: lib.imageIDs[f.item], ordinal: f.ordinal)
    }

    public func peopleRefreshJob(force: Bool) throws -> @Sendable () throws -> PeopleRefresh {
        let session = try peopleLibrary().session
        return {
            let r = try session.refreshPeople(force: force)
            return PeopleRefresh(assigned: r.assigned, reclustered: r.reclustered, approximate: r.approximate,
                                 sampleSize: Int(r.sampleSize))
        }
    }

    public func personMembers(_ person: String) throws -> [PersonFaceRef] {
        let lib = try peopleLibrary()
        return try lib.session.personMembers(personId: person).compactMap { f in
            lib.itemOfImage[f.imageId].map { PersonFaceRef(item: $0, ordinal: f.ordinal) }
        }
    }

    public func undoPeopleEdit() throws -> String? { try peopleLibrary().session.undoPeopleEdit() }

    public func redoPeopleEdit() throws -> String? { try peopleLibrary().session.redoPeopleEdit() }

    public func personAssignments(_ item: Int) throws -> [PersonAssignment] {
        let lib = try peopleLibrary()
        guard lib.imageIDs.indices.contains(item) else { return [] }
        return try lib.session.personAssignments(imageId: lib.imageIDs[item]).compactMap { a in
            guard let owner = lib.itemOfImage[a.face.imageId] else { return nil }
            return PersonAssignment(face: PersonFaceRef(item: owner, ordinal: a.face.ordinal), personID: a.personId,
                                    name: a.name, confirmed: a.confirmed)
        }
    }

    public func peopleNameSuggestions() throws -> [PeopleNameCandidate] {
        try peopleLibrary().session.peopleNameSuggestions(threshold: nil).map {
            PeopleNameCandidate(unnamedID: $0.unnamedId, namedID: $0.namedId, name: $0.name, similarity: $0.similarity)
        }
    }

    public func namePerson(_ id: String, name: String?, options: PeopleNamingOptions) throws {
        try peopleLibrary().session.namePerson(personId: id, name: name, options: options.ffi)
    }

    public func mergePeople(target: String, source: String) throws {
        try peopleLibrary().session.mergePeople(targetId: target, sourceId: source)
    }

    public func splitPerson(_ source: String, newID: String, faces: [PersonFaceRef]) throws {
        let lib = try peopleLibrary()
        try lib.session.splitPerson(sourceId: source, newId: newID, faces: faces.map { try personFace($0, lib) })
    }

    public func assignFace(_ face: PersonFaceRef, to person: String) throws {
        let lib = try peopleLibrary()
        try lib.session.assignPersonFace(face: try personFace(face, lib), personId: person)
    }

    public func confirmFace(_ face: PersonFaceRef, confirmed: Bool) throws {
        let lib = try peopleLibrary()
        try lib.session.confirmPersonFace(face: try personFace(face, lib), confirmed: confirmed)
    }
}
