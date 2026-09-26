import CoreGraphics
import Foundation
import Observation
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
    public init(assigned: UInt64, reclustered: Bool, approximate: Bool) {
        self.assigned = assigned; self.reclustered = reclustered; self.approximate = approximate
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
public protocol PeopleEngine: AnyObject {
    /// A clustering job to run off the main actor (`refresh_people`).
    func peopleRefreshJob(force: Bool) throws -> @Sendable () throws -> PeopleRefresh
    func people(refresh: Bool) throws -> [PersonSummary]
    func personAssignments(_ item: Int) throws -> [PersonAssignment]
    func faceStrip(_ item: Int) throws -> [FaceChip]
    func peopleNameSuggestions() throws -> [PeopleNameCandidate]
    func namePerson(_ id: String, name: String?, options: PeopleNamingOptions) throws
    func mergePeople(target: String, source: String) throws
    func splitPerson(_ source: String, newID: String, faces: [PersonFaceRef]) throws
    func assignFace(_ face: PersonFaceRef, to person: String) throws
    func confirmFace(_ face: PersonFaceRef, confirmed: Bool) throws
    func items(withPerson person: String, eyesClosedBelow: Double?) throws -> [Int]
}

/// A person tile: identity, name, frames and member faces.
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
    /// The representative face (the sharpest member; the engine's cover).
    public var coverItem: Int?
    public var coverOrdinal: UInt32
    public var members: [Member]

    public var displayName: String { name ?? "Unnamed" }
    public var isNamed: Bool { name != nil }
    public var confirmedCount: Int { members.filter(\.confirmed).count }
    /// Every member face confirmed (protected from automatic refits).
    public var isConfirmed: Bool { !members.isEmpty && confirmedCount == members.count }
    public var cover: PersonFaceRef? { coverItem.map { PersonFaceRef(item: $0, ordinal: coverOrdinal) } }
}

@MainActor @Observable
public final class PeopleModel {
    /// Reservoir size of library-scale clustering (`ml_faces::clustering`, 1024 eligible faces);
    /// the FFI reports only the `approximate` flag, not the sample size.
    public static let clusteringSample = 1024
    static let writeRegionsKey = "People.WriteFaceRegions"
    static let personKeywordsKey = "People.PersonKeywords"

    @ObservationIgnored public var engine: (any PeopleEngine)?
    @ObservationIgnored private let defaults: UserDefaults
    /// Called after identities changed (names, merges, moves): the face strip and filters follow.
    @ObservationIgnored public var onPeopleChange: (() -> Void)?

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
        tiles = []; selection = []; detailID = nil; faceSelection = []; suggestions = [:]
        isRefreshing = false; approximate = false; message = nil
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
            if r.reclustered || r.approximate { approximate = r.approximate }
            reload()
        case .failure(let error):
            fail("People", error)
            reload()
        }
    }

    /// Re-reads identities, names, confirmations and suggestions. `refresh` also ingests new
    /// faces (`people(refresh: true)`, the incremental job) — used after every edit.
    public func reload(refresh: Bool = false) {
        guard let engine else { tiles = []; return }
        do {
            let people = try engine.people(refresh: refresh)
            var byPerson: [String: [PersonTile.Member]] = [:]
            var names: [String: String] = [:]
            var seen = Set<Int>()
            for person in people {
                for item in person.items where seen.insert(item).inserted {
                    for a in try engine.personAssignments(item) {
                        byPerson[a.personID, default: []].append(.init(face: a.face, confirmed: a.confirmed))
                        if let n = a.name?.trimmingCharacters(in: .whitespaces), !n.isEmpty { names[a.personID] = n }
                    }
                }
            }
            tiles = Self.sorted(people.map { p in
                PersonTile(id: p.id, name: names[p.id], items: p.items, faces: p.faces, coverItem: p.coverItem,
                           coverOrdinal: p.coverOrdinal, members: (byPerson[p.id] ?? []).sorted { $0.face < $1.face })
            })
            let ids = Set(tiles.map(\.id))
            selection.formIntersection(ids)
            if let d = detailID, !ids.contains(d) { detailID = nil }
            let members = Set(detail?.members.map(\.face) ?? [])
            faceSelection.formIntersection(members)
            faceRects = [:]
        } catch {
            fail("People", error)
        }
        do {
            let unnamed = Set(tiles.filter { !$0.isNamed }.map(\.id))
            let named = Set(tiles.filter(\.isNamed).map(\.id))
            suggestions = Dictionary(grouping: try engine.peopleNameSuggestions()
                .filter { unnamed.contains($0.unnamedID) && named.contains($0.namedID) }, by: \.unnamedID)
                .mapValues { $0.sorted { $0.similarity > $1.similarity } }
        } catch {
            suggestions = [:]
        }
        refreshFacetItems()
    }

    /// Footnote under the grid when the clustering was sampled.
    public var approximateNote: String? {
        approximate ? "Clustered from a sample of \(Self.clusteringSample.formatted()) faces" : nil
    }

    /// Normalized face rectangle (origin top-left) in the item's preview, from the face strip.
    public func faceRect(_ face: PersonFaceRef) -> CGRect? {
        if let hit = faceRects[face.item] { return hit[face.ordinal] }
        let strip = (try? engine?.faceStrip(face.item)) ?? []
        let rects = Dictionary(strip.map { ($0.ordinal, $0.rect) }, uniquingKeysWith: { a, _ in a })
        faceRects[face.item] = rects
        return rects[face.ordinal]
    }

    // MARK: Edits (each goes through the FFI, then reloads from `people(refresh: true)`)

    private func fail(_ verb: String, _ error: Error) {
        message = "\(verb) failed: \(error.localizedDescription)"
    }

    private func edit(_ verb: String, _ body: (any PeopleEngine) throws -> String) -> Bool {
        guard let engine else { message = "\(verb): open a folder on the engine first"; return false }
        do {
            message = try body(engine)
        } catch {
            fail(verb, error)
            reload(refresh: true)
            return false
        }
        reload(refresh: true)
        onPeopleChange?()
        return true
    }

    /// Return in a tile's name field: `name_person` with the Settings opt-ins. Empty clears.
    @discardableResult
    public func name(_ id: String, as text: String) -> Bool {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let old = person(id)?.name
        guard trimmed != (old ?? "") else { return false }
        let options = naming
        return edit("Name") { engine in
            try engine.namePerson(id, name: trimmed.isEmpty ? nil : trimmed, options: options)
            let written = options.writeFaceRegions ? " · face regions written to XMP" : ""
            return trimmed.isEmpty ? "Cleared the name of \(old ?? "person")" : "Named \(trimmed)\(written)"
        }
    }

    /// A suggestion from the name menu: the unnamed cluster is the named person (merge, name wins).
    @discardableResult
    public func accept(_ suggestion: PeopleNameCandidate) -> Bool {
        edit("Merge") { engine in
            try engine.mergePeople(target: suggestion.namedID, source: suggestion.unnamedID)
            return "Merged into \(suggestion.name)"
        }
    }

    /// The person that survives a merge: a named one first, then the most frames.
    public static func mergeTarget(_ people: [PersonTile]) -> PersonTile? { sorted(people).first }

    public var canMerge: Bool { selection.count >= 2 }

    /// Toolbar ▸ Merge: every selected person into the target (the target's name wins).
    @discardableResult
    public func mergeSelection() -> Bool {
        let people = tiles.filter { selection.contains($0.id) }
        guard people.count >= 2, let target = Self.mergeTarget(people) else {
            message = "Select two or more people to merge"
            return false
        }
        let sources = people.filter { $0.id != target.id }
        let ok = edit("Merge") { engine in
            for source in sources { try engine.mergePeople(target: target.id, source: source.id) }
            return "Merged \(sources.count + 1) people into \(target.displayName)"
        }
        if ok { selection = [target.id] }
        return ok
    }

    /// Detail ▸ Split: the selected faces become a new unnamed person. Returns its id.
    @discardableResult
    public func splitSelection(newID: String = "person-split-\(UUID().uuidString.lowercased())") -> String? {
        guard let person = detail else { return nil }
        let faces = person.members.map(\.face).filter(faceSelection.contains)
        guard !faces.isEmpty else { message = "Select faces to split off"; return nil }
        guard faces.count < person.members.count else {
            message = "Leave at least one face in \(person.displayName)"
            return nil
        }
        let ok = edit("Split") { engine in
            try engine.splitPerson(person.id, newID: newID, faces: faces)
            return "Split \(faces.count) face\(faces.count == 1 ? "" : "s") into a new person"
        }
        guard ok else { return nil }
        faceSelection = []
        return newID
    }

    /// A face chip dropped on another person's tile (or Move To in its menu).
    @discardableResult
    public func reassign(_ face: PersonFaceRef, to target: String) -> Bool {
        guard person(target) != nil else { return false }
        if let current = tiles.first(where: { $0.members.contains { $0.face == face } }), current.id == target { return false }
        let name = person(target)?.displayName ?? "person"
        let ok = edit("Move face") { engine in
            try engine.assignFace(face, to: target)
            return "Moved the face to \(name) (unconfirmed)"
        }
        if ok { faceSelection.remove(face) }
        return ok
    }

    /// Confirm protects an assignment from automatic refits.
    @discardableResult
    public func setConfirmed(_ face: PersonFaceRef, _ confirmed: Bool) -> Bool {
        edit(confirmed ? "Confirm" : "Unconfirm") { engine in
            try engine.confirmFace(face, confirmed: confirmed)
            return confirmed ? "Confirmed the face" : "Unconfirmed the face"
        }
    }

    /// Confirms every face of the detail person.
    @discardableResult
    public func confirmAll() -> Bool {
        guard let person = detail else { return false }
        let pending = person.members.filter { !$0.confirmed }
        guard !pending.isEmpty else { return false }
        return edit("Confirm") { engine in
            for m in pending { try engine.confirmFace(m.face, confirmed: true) }
            return "Confirmed \(pending.count) face\(pending.count == 1 ? "" : "s") of \(person.displayName)"
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

    public func openDetail(_ id: String) {
        detailID = id
        faceSelection = []
        selection = [id]
    }

    public func closeDetail() {
        detailID = nil
        faceSelection = []
    }

    // MARK: Filter bar ▸ Person

    public func toggleFacet(_ id: String) {
        if facet.contains(id) { facet.remove(id) } else { facet.insert(id) }
        refreshFacetItems()
    }

    public func setFacet(_ ids: Set<String>) {
        facet = ids
        refreshFacetItems()
    }

    public func clearFacet() { setFacet([]) }

    /// Frames with any of the selected people (`frames_with_person`), nil with no selection.
    private func refreshFacetItems() {
        facet.formIntersection(tiles.map(\.id))
        guard !facet.isEmpty, let engine else { facetItems = nil; return }
        var items = Set<Int>()
        do {
            for id in facet.sorted() { items.formUnion(try engine.items(withPerson: id, eyesClosedBelow: nil)) }
            facetItems = items
        } catch {
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

extension CullController: PeopleEngine {
    private func peopleLibrary() throws -> EngineLibrary {
        guard case .engine(let lib) = backend else { throw CullError.unavailable("People need a folder opened on the engine") }
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
            return PeopleRefresh(assigned: r.assigned, reclustered: r.reclustered, approximate: r.approximate)
        }
    }

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
