import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// In-memory stand-in for the engine's people calls (WP M2-40), with the FFI's semantics:
/// merge keeps the target's name, split and moves reset confirmation, suggestions are read-only.
final class StubPeopleEngine: PeopleEngine {
    struct Face { var person: String; var confirmed: Bool }
    var faces: [PersonFaceRef: Face] = [:]
    var names: [String: String] = [:]
    var candidates: [PeopleNameCandidate] = []
    var refreshResult = PeopleRefresh(assigned: 0, reclustered: false, approximate: false)
    var failNext: String?
    private(set) var calls: [String] = []
    private(set) var nameOptions: [PeopleNameOptions] = []
    private(set) var refreshes = 0
    private(set) var refreshedOnMain: Bool?

    struct Failure: LocalizedError {
        let text: String
        var errorDescription: String? { text }
    }

    private func check(_ call: String) throws {
        calls.append(call)
        if let f = failNext { failNext = nil; throw Failure(text: f) }
    }

    func add(_ person: String, _ faces: [(Int, UInt32)], confirmed: Bool = false) {
        for (item, ordinal) in faces { self.faces[PersonFaceRef(item: item, ordinal: ordinal)] = Face(person: person, confirmed: confirmed) }
    }

    func peopleRefreshJob(force: Bool) throws -> @Sendable () throws -> PeopleRefresh {
        try check("refresh(\(force))")
        refreshes += 1
        let result = refreshResult
        let probe = Probe()
        probeBox = probe
        return {
            probe.onMain = Thread.isMainThread
            return result
        }
    }
    final class Probe: @unchecked Sendable { var onMain: Bool? }
    var probeBox: Probe?

    func people(refresh: Bool) throws -> [PersonSummary] {
        try check("people(\(refresh))")
        let grouped = Dictionary(grouping: faces, by: { $0.value.person })
        return grouped.map { id, members in
            let items = Array(Set(members.map(\.key.item))).sorted()
            let cover = members.map(\.key).min()!
            return PersonSummary(id: id, name: names[id] ?? "Person \(id)", items: items, faces: members.count,
                                 coverItem: cover.item, coverOrdinal: cover.ordinal)
        }.sorted { $0.items.count > $1.items.count }
    }

    func personAssignments(_ item: Int) throws -> [PersonAssignment] {
        faces.filter { $0.key.item == item }.sorted { $0.key < $1.key }.map {
            PersonAssignment(face: $0.key, personID: $0.value.person, name: names[$0.value.person], confirmed: $0.value.confirmed)
        }
    }

    func faceStrip(_ item: Int) throws -> [FaceChip] {
        faces.keys.filter { $0.item == item }.map {
            FaceChip(ordinal: $0.ordinal, rect: CGRect(x: 0.1 * Double($0.ordinal), y: 0.2, width: 0.1, height: 0.15),
                     focus: 0.8, eyesOpen: 0.9, personID: faces[$0]?.person)
        }
    }

    func peopleNameSuggestions() throws -> [PeopleNameCandidate] { candidates }

    func namePerson(_ id: String, name: String?, options: PeopleNamingOptions) throws {
        try check("name(\(id),\(name ?? "nil"))")
        nameOptions.append(options.ffi)
        names[id] = name
    }

    func mergePeople(target: String, source: String) throws {
        try check("merge(\(target),\(source))")
        for (k, v) in faces where v.person == source { faces[k]?.person = target }
        if names[target] == nil { names[target] = names[source] }
        names[source] = nil
    }

    func splitPerson(_ source: String, newID: String, faces moved: [PersonFaceRef]) throws {
        try check("split(\(source),\(moved.count))")
        for f in moved where faces[f]?.person == source { faces[f] = Face(person: newID, confirmed: false) }
    }

    func assignFace(_ face: PersonFaceRef, to person: String) throws {
        try check("assign(\(face.item):\(face.ordinal),\(person))")
        faces[face] = Face(person: person, confirmed: false)
    }

    func confirmFace(_ face: PersonFaceRef, confirmed: Bool) throws {
        try check("confirm(\(face.item):\(face.ordinal),\(confirmed))")
        faces[face]?.confirmed = confirmed
    }

    func items(withPerson person: String, eyesClosedBelow: Double?) throws -> [Int] {
        Array(Set(faces.filter { $0.value.person == person }.map(\.key.item))).sorted()
    }
}

@MainActor
final class PeopleModelTests: XCTestCase {
    private var suite = ""

    private func defaults() -> UserDefaults {
        suite = "tessera.people-tests.\(UUID().uuidString)"
        let d = UserDefaults(suiteName: suite)!
        addTeardownBlock { [suite] in UserDefaults().removePersistentDomain(forName: suite) }
        return d
    }

    /// alice (named): items 0–2; p1 (unnamed): items 0, 3, 4, 5; p2 (unnamed, confirmed): item 6.
    private func fixture() -> (StubPeopleEngine, PeopleModel) {
        let engine = StubPeopleEngine()
        engine.add("alice", [(0, 0), (1, 0), (2, 0)])
        engine.add("p1", [(0, 1), (3, 0), (4, 0), (5, 0)])
        engine.add("p2", [(6, 0)], confirmed: true)
        engine.names["alice"] = "Alice"
        let model = PeopleModel(defaults: defaults())
        model.install(engine)
        model.reload()
        return (engine, model)
    }

    func testTilesNamedFirstThenByFramesWithNamesMembersAndBadges() {
        let (_, model) = fixture()
        XCTAssertEqual(model.tiles.map(\.id), ["alice", "p1", "p2"], "named first although p1 has more frames")
        let alice = model.tiles[0], p1 = model.tiles[1], p2 = model.tiles[2]
        XCTAssertEqual(alice.displayName, "Alice")
        XCTAssertEqual(p1.displayName, "Unnamed", "the engine's “Person <id>” placeholder is not a name")
        XCTAssertNil(p1.name)
        XCTAssertEqual(p1.items, [0, 3, 4, 5])
        XCTAssertEqual(p1.members.map(\.face), [PersonFaceRef(item: 0, ordinal: 1), PersonFaceRef(item: 3, ordinal: 0),
                                                PersonFaceRef(item: 4, ordinal: 0), PersonFaceRef(item: 5, ordinal: 0)])
        XCTAssertTrue(p2.isConfirmed)
        XCTAssertFalse(p1.isConfirmed)
        XCTAssertEqual(alice.cover, PersonFaceRef(item: 0, ordinal: 0))
        XCTAssertEqual(model.named.map(\.id), ["alice"])
        XCTAssertEqual(model.faceRect(PersonFaceRef(item: 0, ordinal: 1))?.minX ?? 0, 0.1, accuracy: 1e-9)
        XCTAssertNil(model.faceRect(PersonFaceRef(item: 0, ordinal: 7)))
        let sorted = PeopleModel.sorted([
            PersonTile(id: "b", name: "Bea", items: [1], faces: 1, coverItem: 1, coverOrdinal: 0, members: []),
            PersonTile(id: "a", name: "Al", items: [1], faces: 1, coverItem: 1, coverOrdinal: 0, members: []),
            PersonTile(id: "c", name: nil, items: [1, 2, 3], faces: 3, coverItem: 1, coverOrdinal: 0, members: []),
        ])
        XCTAssertEqual(sorted.map(\.id), ["a", "b", "c"], "named ties break by name")
    }

    func testNamingCommitsTrimmedNameWithOptInsAndReloads() {
        let (engine, model) = fixture()
        var changes = 0
        model.onPeopleChange = { changes += 1 }
        XCTAssertTrue(model.name("p1", as: "  Bob \n"))
        XCTAssertEqual(engine.calls.suffix(2), ["name(p1,Bob)", "people(true)"], "then reloads from people(refresh: true)")
        XCTAssertEqual(engine.nameOptions.last?.writeSidecars, false, "default: no sidecar writes")
        XCTAssertEqual(engine.nameOptions.last?.personKeywords, false)
        XCTAssertEqual(model.person("p1")?.displayName, "Bob")
        XCTAssertEqual(model.tiles.map(\.id), ["p1", "alice", "p2"], "Bob now named and has more frames")
        XCTAssertEqual(model.message, "Named Bob")
        XCTAssertEqual(changes, 1)

        XCTAssertFalse(model.name("p1", as: "Bob"), "unchanged name: no engine call")
        XCTAssertEqual(engine.calls.filter { $0.hasPrefix("name") }.count, 1)

        // Settings ▸ Library opt-ins; keywords only travel with face regions. Persisted.
        model.naming.personKeywords = true
        XCTAssertEqual(model.naming.ffi.personKeywords, false)
        model.naming.writeFaceRegions = true
        XCTAssertTrue(model.name("p2", as: "Cy"))
        XCTAssertEqual(engine.nameOptions.last?.writeSidecars, true)
        XCTAssertEqual(engine.nameOptions.last?.personKeywords, true)
        XCTAssertEqual(model.message, "Named Cy · face regions written to XMP")
        let again = PeopleModel(defaults: UserDefaults(suiteName: suite)!)
        XCTAssertEqual(again.naming, PeopleNamingOptions(writeFaceRegions: true, personKeywords: true))

        XCTAssertTrue(model.name("alice", as: " "), "empty clears the name")
        XCTAssertEqual(engine.calls.last { $0.hasPrefix("name") }, "name(alice,nil)")
        XCTAssertNil(model.person("alice")?.name)
    }

    func testSuggestionsAreFilteredSortedAndAcceptingMerges() {
        let (engine, model) = fixture()
        engine.candidates = [
            PeopleNameCandidate(unnamedID: "p1", namedID: "alice", name: "Alice", similarity: 0.61),
            PeopleNameCandidate(unnamedID: "p1", namedID: "zed", name: "Zed", similarity: 0.9),     // unknown person
            PeopleNameCandidate(unnamedID: "p2", namedID: "alice", name: "Alice", similarity: 0.7),
            PeopleNameCandidate(unnamedID: "alice", namedID: "alice", name: "Alice", similarity: 1), // not unnamed
        ]
        model.reload()
        XCTAssertEqual(model.suggestions["p1"]?.map(\.namedID), ["alice"])
        XCTAssertEqual(model.suggestions["p2"]?.first?.similarity, 0.7)
        XCTAssertNil(model.suggestions["alice"])
        XCTAssertTrue(model.accept(model.suggestions["p2"]![0]))
        XCTAssertTrue(engine.calls.contains("merge(alice,p2)"), "a suggestion is accepted by merging into the named person")
        XCTAssertNil(model.person("p2"))
        XCTAssertEqual(model.person("alice")?.items, [0, 1, 2, 6])
        XCTAssertEqual(model.message, "Merged into Alice")
    }

    func testMergeSelectionKeepsTheNamedPersonAndItsName() {
        let (engine, model) = fixture()
        model.click("p2")
        XCTAssertFalse(model.canMerge)
        XCTAssertFalse(model.mergeSelection())
        XCTAssertEqual(model.message, "Select two or more people to merge")
        model.click("p1", command: true)
        model.click("alice", command: true)
        XCTAssertEqual(model.selection, ["alice", "p1", "p2"])
        XCTAssertTrue(model.canMerge)
        XCTAssertEqual(PeopleModel.mergeTarget(model.tiles.filter { model.selection.contains($0.id) })?.id, "alice")
        XCTAssertTrue(model.mergeSelection())
        XCTAssertEqual(engine.calls.filter { $0.hasPrefix("merge") }.sorted(), ["merge(alice,p1)", "merge(alice,p2)"])
        XCTAssertEqual(model.tiles.map(\.id), ["alice"])
        XCTAssertEqual(model.tiles[0].displayName, "Alice")
        XCTAssertEqual(model.tiles[0].items, [0, 1, 2, 3, 4, 5, 6])
        XCTAssertEqual(model.selection, ["alice"])
        XCTAssertEqual(model.message, "Merged 3 people into Alice")

        // ⇧-click extends in grid order; plain click replaces.
        let (_, other) = fixture()
        other.click("alice")
        other.click("p2", shift: true)
        XCTAssertEqual(other.selection, ["alice", "p1", "p2"])
        other.click("p1")
        XCTAssertEqual(other.selection, ["p1"])
        other.click("p1", command: true)
        XCTAssertEqual(other.selection, [])
    }

    func testSplitMovesSelectedFacesToANewUnconfirmedPerson() {
        let (engine, model) = fixture()
        model.openDetail("p1")
        XCTAssertNil(model.splitSelection(newID: "new"), "nothing selected")
        XCTAssertEqual(model.message, "Select faces to split off")
        for m in model.detail!.members { model.toggleFace(m.face) }
        XCTAssertNil(model.splitSelection(newID: "new"), "a split must leave a face behind")
        XCTAssertTrue(model.message?.hasPrefix("Leave at least one face") == true)
        model.faceSelection = []
        model.toggleFace(PersonFaceRef(item: 4, ordinal: 0))
        model.toggleFace(PersonFaceRef(item: 5, ordinal: 0))
        model.toggleFace(PersonFaceRef(item: 5, ordinal: 0))
        model.toggleFace(PersonFaceRef(item: 5, ordinal: 0))
        XCTAssertEqual(model.faceSelection.count, 2)
        XCTAssertEqual(model.splitSelection(newID: "new"), "new")
        XCTAssertTrue(engine.calls.contains("split(p1,2)"))
        XCTAssertEqual(model.person("new")?.items, [4, 5])
        XCTAssertEqual(model.person("new")?.confirmedCount, 0)
        XCTAssertEqual(model.person("p1")?.items, [0, 3])
        XCTAssertEqual(model.detailID, "p1", "the detail view stays on the source")
        XCTAssertTrue(model.faceSelection.isEmpty)
        XCTAssertEqual(model.message, "Split 2 faces into a new person")
    }

    func testReassignAndConfirmGoThroughTheEngine() {
        let (engine, model) = fixture()
        let face = PersonFaceRef(item: 3, ordinal: 0)
        XCTAssertFalse(model.reassign(face, to: "p1"), "already there")
        XCTAssertFalse(model.reassign(face, to: "nobody"))
        XCTAssertTrue(model.setConfirmed(face, true))
        XCTAssertEqual(model.person("p1")?.confirmedCount, 1)
        XCTAssertTrue(model.reassign(face, to: "alice"))
        XCTAssertEqual(engine.calls.suffix(2), ["assign(3:0,alice)", "people(true)"])
        XCTAssertEqual(model.person("alice")?.items, [0, 1, 2, 3])
        XCTAssertEqual(model.person("alice")?.members.first { $0.face == face }?.confirmed, false, "moves reset confirmation")
        model.openDetail("alice")
        XCTAssertTrue(model.confirmAll())
        XCTAssertTrue(model.detail!.isConfirmed)
        XCTAssertEqual(model.message, "Confirmed 4 faces of Alice")
        XCTAssertFalse(model.confirmAll(), "nothing left to confirm")
        XCTAssertTrue(model.setConfirmed(face, false))
        XCTAssertFalse(model.detail!.isConfirmed)
        XCTAssertEqual(model.detail!.confirmedCount, 3)
        XCTAssertEqual(PersonFaceRef(dragToken: face.dragToken), face)
        XCTAssertNil(PersonFaceRef(dragToken: "face:x:1"))
        XCTAssertNil(PersonFaceRef(dragToken: "album:1:2"))
    }

    func testPersonFacetIsAUnionWithinAndIntersectsOtherFilters() {
        let (engine, model) = fixture()
        XCTAssertNil(model.facetItems)
        XCTAssertEqual(model.facetTitle, "Person")
        model.toggleFacet("alice")
        XCTAssertEqual(model.facetItems, [0, 1, 2])
        XCTAssertEqual(model.facetTitle, "Person · Alice")
        model.toggleFacet("p2")
        XCTAssertEqual(model.facetItems, [0, 1, 2, 6], "any of the chosen people")
        XCTAssertEqual(model.facetTitle, "Person · 2")

        // Other filters (the engine search) matched items 1, 2, 3 and 6.
        let matches: Set<Int> = [1, 2, 3, 6]
        XCTAssertEqual(PeopleModel.visible(base: Array(0..<8), matches: matches, people: model.facetItems), [1, 2, 6])
        XCTAssertEqual(PeopleModel.visible(base: [6, 2, 1, 0], matches: nil, people: model.facetItems), [6, 2, 1, 0],
                       "keeps the base order (album order, confidence order)")
        XCTAssertEqual(PeopleModel.visible(base: Array(0..<4), matches: nil, people: nil), [0, 1, 2, 3])
        XCTAssertEqual(PeopleModel.visible(base: Array(0..<8), matches: [7], people: model.facetItems), [])
        XCTAssertEqual(model.facetCount("alice", within: matches), 2)
        XCTAssertEqual(model.facetCount("alice", within: nil), 3)
        XCTAssertEqual(model.facetCount("nobody", within: nil), 0)

        // Identities change under the facet: it follows (merged-away people drop out).
        XCTAssertTrue(model.accept(PeopleNameCandidate(unnamedID: "p2", namedID: "alice", name: "Alice", similarity: 0.8)))
        XCTAssertEqual(model.facet, ["alice"])
        XCTAssertEqual(model.facetItems, [0, 1, 2, 6])
        _ = engine
        model.clearFacet()
        XCTAssertNil(model.facetItems)
        XCTAssertEqual(model.facetTitle, "Person")
    }

    func testRefreshRunsTheJobOffTheMainActorAndReportsApproximation() async {
        let (engine, model) = fixture()
        XCTAssertNil(model.approximateNote)
        engine.refreshResult = PeopleRefresh(assigned: 12, reclustered: true, approximate: true)
        engine.faces[PersonFaceRef(item: 7, ordinal: 0)] = .init(person: "p3", confirmed: false)
        await model.refresh()
        XCTAssertEqual(engine.calls.first, "people(false)", "fixture reload")
        XCTAssertTrue(engine.calls.contains("refresh(false)"), "refresh_people(force: false) when the view opens")
        XCTAssertEqual(engine.probeBox?.onMain, false, "the clustering job ran off the main thread")
        XCTAssertFalse(model.isRefreshing)
        XCTAssertNotNil(model.person("p3"), "reloaded after the job")
        XCTAssertEqual(model.approximateNote, "Clustered from a sample of 1,024 faces")
        // An incremental pass without a refit keeps the last clustering's flag.
        engine.refreshResult = PeopleRefresh(assigned: 1, reclustered: false, approximate: false)
        await model.refresh()
        XCTAssertNotNil(model.approximateNote)
        engine.refreshResult = PeopleRefresh(assigned: 0, reclustered: true, approximate: false)
        await model.refresh(force: true)
        XCTAssertTrue(engine.calls.contains("refresh(true)"))
        XCTAssertNil(model.approximateNote)
    }

    func testEngineErrorsBecomeMessagesAndNothingIsHalfApplied() {
        let (engine, model) = fixture()
        var changes = 0
        model.onPeopleChange = { changes += 1 }
        engine.failNext = "index is locked"
        XCTAssertFalse(model.name("p1", as: "Bob"))
        XCTAssertEqual(model.message, "Name failed: index is locked")
        XCTAssertNil(model.person("p1")?.name)
        XCTAssertEqual(changes, 0)
        let empty = PeopleModel(defaults: defaults())
        XCTAssertFalse(empty.name("p1", as: "Bob"))
        XCTAssertEqual(empty.message, "Name: open a folder on the engine first")
        empty.reload()
        XCTAssertTrue(empty.tiles.isEmpty)
    }

    func testInstallResetsAndLibraryUpdatesRemapItems() {
        let (_, model) = fixture()
        model.toggleFacet("alice")
        model.openDetail("p1")
        model.toggleFace(PersonFaceRef(item: 3, ordinal: 0))
        // Item 0 left the library; everything else moved up by one.
        model.libraryDidUpdate { $0 == 0 ? nil : $0 - 1 }
        XCTAssertEqual(model.person("alice")?.items, [0, 1])
        XCTAssertEqual(model.person("p1")?.members.map(\.face.item), [2, 3, 4])
        XCTAssertEqual(model.person("p1")?.coverItem, nil, "its cover (item 0) left")
        XCTAssertEqual(model.faceSelection, [PersonFaceRef(item: 2, ordinal: 0)])
        XCTAssertEqual(model.facetItems, [0, 1])
        model.install(nil)
        XCTAssertTrue(model.tiles.isEmpty)
        XCTAssertNil(model.facetItems)
        XCTAssertNil(model.detailID)
        XCTAssertTrue(model.facet.isEmpty)
    }
}

/// The same calls through a real engine session (`CullController`), on the `--seed-faces` people.
final class PeopleBridgeTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    private func folder() throws -> (URL, URL) {
        let temp = root.appendingPathComponent("build/people-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        for (n, name) in ["a.jpg", "b.jpg", "c.jpg", "d.jpg"].enumerated() {
            let w = 160, h = 120
            var pixels = [UInt8](repeating: 0, count: w * h * 4)
            for i in stride(from: 0, to: pixels.count, by: 4) {
                pixels[i] = UInt8(40 * n); pixels[i + 1] = 90; pixels[i + 2] = UInt8(200 - 40 * n); pixels[i + 3] = 255
            }
            let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
            let image = try XCTUnwrap(CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                              space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                              bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                              provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
            let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(folder.appendingPathComponent(name) as CFURL,
                                                                     UTType.jpeg.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(dest, image, nil)
            XCTAssertTrue(CGImageDestinationFinalize(dest))
        }
        return (folder, temp.appendingPathComponent("support"))
    }

    @MainActor
    func testNameMergeSplitConfirmAndFilterThroughTheEngine() async throws {
        let (folder, support) = try folder()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let cull = library.makeCullController()
        try library.seedSyntheticFaces()
        let model = PeopleModel(defaults: UserDefaults(suiteName: "tessera.people-bridge.\(UUID().uuidString)")!)
        model.install(cull)
        await model.refresh()
        XCTAssertNil(model.message)
        XCTAssertGreaterThanOrEqual(model.tiles.count, 1)
        let a = try XCTUnwrap(model.tiles.first { $0.items.count == library.items.count }, "person A is in every frame")
        let summary = try XCTUnwrap(library.session.people(refresh: false).first { $0.id == a.id })
        XCTAssertFalse(summary.named)
        XCTAssertEqual(summary.faceCount, summary.faces)
        XCTAssertEqual(summary.confirmedCount, 0)
        XCTAssertNotNil(summary.medoidFace)
        let members = try library.session.personMembers(personId: a.id)
        XCTAssertEqual(members.count, a.members.count)
        XCTAssertNil(try library.session.undoPeopleEdit())
        let member = try XCTUnwrap(members.first)
        try library.session.confirmPersonFace(face: member, confirmed: true)
        XCTAssertEqual(try library.session.people(refresh: false).first { $0.id == a.id }?.confirmedCount, 1)
        XCTAssertEqual(try library.session.undoPeopleEdit(), "Confirm face")
        XCTAssertEqual(try library.session.redoPeopleEdit(), "Confirm face")
        XCTAssertEqual(try library.session.undoPeopleEdit(), "Confirm face")
        XCTAssertEqual(try library.session.refreshPeople(force: false).sampleSize, 0)
        // Old source-level initializers remain available through UniFFI defaults.
        let legacy = PersonInfo(id: "legacy", name: "Person legacy", images: [], faces: 0, coverImage: "", coverOrdinal: 0)
        XCTAssertFalse(legacy.named)
        XCTAssertNil(legacy.medoidFace)
        XCTAssertEqual(PeopleJobResult(assigned: 0, reclustered: false, approximate: false).sampleSize, 0)
        XCTAssertNil(a.name)
        XCTAssertEqual(a.members.count, library.items.count)
        XCTAssertNotNil(model.faceRect(try XCTUnwrap(a.cover)))

        XCTAssertTrue(model.name(a.id, as: "Ada"), model.message ?? "")
        XCTAssertEqual(model.person(a.id)?.name, "Ada")
        XCTAssertEqual(model.tiles.first?.id, a.id)
        let xmp = try FileManager.default.contentsOfDirectory(atPath: folder.path).filter { $0.hasSuffix(".xmp") }
        XCTAssertFalse(try xmp.contains { try String(contentsOf: folder.appendingPathComponent($0), encoding: .utf8).contains("Ada") },
                       "default naming writes no face regions")

        // Split two faces off, confirm one, move it back, merge the rest back.
        model.openDetail(a.id)
        let faces = model.detail!.members.map(\.face)
        model.toggleFace(faces[0]); model.toggleFace(faces[1])
        let split = try XCTUnwrap(model.splitSelection(newID: "person-split-test"), model.message ?? "")
        XCTAssertEqual(model.person(split)?.members.map(\.face), [faces[0], faces[1]])
        XCTAssertNil(model.person(split)?.name)
        XCTAssertEqual(model.person(a.id)?.members.count, library.items.count - 2)
        XCTAssertTrue(model.setConfirmed(faces[0], true), model.message ?? "")
        XCTAssertEqual(model.person(split)?.confirmedCount, 1)
        XCTAssertTrue(model.reassign(faces[0], to: a.id), model.message ?? "")
        XCTAssertEqual(model.person(a.id)?.members.count, library.items.count - 1)
        model.selection = [a.id, split]
        XCTAssertTrue(model.mergeSelection(), model.message ?? "")
        XCTAssertNil(model.person(split))
        XCTAssertEqual(model.person(a.id)?.name, "Ada", "the named target's name wins")
        XCTAssertEqual(model.person(a.id)?.members.count, library.items.count)

        // Person facet: frames_with_person.
        model.toggleFacet(a.id)
        XCTAssertEqual(model.facetItems, Set(library.items.indices))

        // Settings ▸ Library opt-in: face regions (and the name as a keyword) go to every photo's XMP.
        model.naming = PeopleNamingOptions(writeFaceRegions: true, personKeywords: true)
        XCTAssertTrue(model.name(a.id, as: "Ada Lovelace"), model.message ?? "")
        let sidecars = try FileManager.default.contentsOfDirectory(atPath: folder.path).filter { $0.hasSuffix(".xmp") }
        let named = try sidecars.filter {
            let xml = try String(contentsOf: folder.appendingPathComponent($0), encoding: .utf8)
            return xml.contains("Ada Lovelace") && xml.contains("mwg-rs")
        }
        XCTAssertEqual(named.count, library.items.count, "\(sidecars)")
    }
}
