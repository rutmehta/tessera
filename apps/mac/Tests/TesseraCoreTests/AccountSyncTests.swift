import Foundation
import XCTest
@testable import TesseraCore

/// SPEC §3 (CS-2): the account store over a temp directory standing in for the iCloud container.
/// Merge rules (last writer wins per record, people union their cluster ids, shoots and drive
/// roots union by key), the stamp, the file layout, and an unavailable store that does nothing.
/// CS-2b: the same store over a synced folder (availability, round trip, the persisted location,
/// conflict copies merged and removed, the FSEvents watcher over a real temp folder).
final class AccountSyncTests: XCTestCase {
    private var base: URL!
    private var defaults: UserDefaults!
    private let suiteName = "dev.tessera.tests.sync"

    override func setUpWithError() throws {
        base = FileManager.default.temporaryDirectory.appendingPathComponent("tessera-sync-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        defaults = try XCTUnwrap(UserDefaults(suiteName: suiteName))
        defaults.removePersistentDomain(forName: suiteName)
    }

    override func tearDownWithError() throws {
        defaults.removePersistentDomain(forName: suiteName)
        try? FileManager.default.removeItem(at: base)
    }

    private func store(_ defaults: UserDefaults? = nil) -> AccountStore {
        AccountStore(baseURL: base, defaults: defaults ?? self.defaults)
    }

    private let t0 = Date(timeIntervalSince1970: 1_760_000_000)

    // MARK: Store

    func testFileLayoutAndStamp() throws {
        let s = store()
        XCTAssertEqual(s.availability, .available)
        XCTAssertTrue(s.isEnabled, "on by default")
        let prefs = PreferencesRecord(ai: AIPreferences(), accent: "F0A52A", ground: "black", modified: t0, machine: s.machine)
        XCTAssertNotNil(s.write(prefs))
        XCTAssertNotNil(s.write(PersonRecord(id: "person/1", name: "Rut", clusters: [], modified: t0, machine: s.machine), id: "person/1"))
        XCTAssertNotNil(s.write(DriveRecord(volumeUUID: "8A3C-1", name: "Rut photo", roots: [], lastSeenMachine: s.machine,
                                            lastSeenMachineName: nil, lastSeen: t0, modified: t0, machine: s.machine), id: "8A3C-1"))

        let root = base.appendingPathComponent("Tessera")
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appendingPathComponent("preferences.json").path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appendingPathComponent("people/person%2F1.json").path),
                      "ids are percent-encoded into file names")
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appendingPathComponent("drives/8A3C-1.json").path))

        // The stamp is ISO 8601 and the record never carries a key.
        let text = try String(contentsOf: root.appendingPathComponent("preferences.json"), encoding: .utf8)
        XCTAssertTrue(text.contains("\"modified\" : \"2025-10-09T08:53:20.000Z\""), text)
        XCTAssertTrue(text.contains("\"machine\" : \"\(s.machine)\""))
        XCTAssertFalse(text.lowercased().contains("key"), "no API keys in the container")

        // Paths map back to keys.
        XCTAssertEqual(s.key(for: root.appendingPathComponent("preferences.json")), RecordKey(.preferences))
        XCTAssertEqual(s.key(for: root.appendingPathComponent("people/person%2F1.json")), RecordKey(.person, id: "person/1"))
        XCTAssertEqual(s.key(for: root.appendingPathComponent("drives/8A3C-1.json")), RecordKey(.drive, id: "8A3C-1"))
        XCTAssertNil(s.key(for: root.appendingPathComponent("drives/8A3C-1.txt")))
        XCTAssertNil(s.key(for: base.appendingPathComponent("elsewhere/preferences.json")))

        XCTAssertEqual(s.read(PreferencesRecord.self), prefs)
        XCTAssertEqual(s.list(PersonRecord.self).map(\.name), ["Rut"])
        let stats = s.stats()
        XCTAssertEqual(stats.files, 3)
        XCTAssertGreaterThan(stats.bytes, 0)
        XCTAssertNotNil(stats.lastSync)
    }

    func testMachineIDIsMadeOnceAndKept() {
        let a = store().machine
        XCTAssertFalse(a.isEmpty)
        XCTAssertEqual(store().machine, a, "the same Mac keeps its id")
        let other = UserDefaults(suiteName: suiteName + ".other")!
        other.removePersistentDomain(forName: suiteName + ".other")
        XCTAssertNotEqual(store(other).machine, a)
        other.removePersistentDomain(forName: suiteName + ".other")
    }

    func testUnavailableStoreDoesNothing() {
        let s = AccountStore(baseURL: nil, reason: .noEntitlement, defaults: defaults)
        XCTAssertEqual(s.availability, .unavailable(.noEntitlement))
        XCTAssertEqual(s.unavailableReason?.message, "This build has no iCloud entitlement.")
        XCTAssertNil(s.write(KeywordsRecord(keywords: [], modified: t0, machine: s.machine)))
        XCTAssertNil(s.read(KeywordsRecord.self))
        XCTAssertEqual(s.list(PersonRecord.self), [])
        XCTAssertEqual(s.stats(), AccountStore.Stats())
        XCTAssertNil(s.url(for: RecordKey(.keywords)))
        // Each reason has a one-line fix.
        for reason in [AccountStore.UnavailableReason.noEntitlement, .noAccount, .iCloudDriveOff] {
            XCTAssertFalse(reason.fix.isEmpty)
        }
    }

    func testSwitchedOffStoreDoesNothingButKeepsAvailability() {
        let s = store()
        s.isEnabled = false
        XCTAssertTrue(s.isAvailable)
        XCTAssertFalse(s.isActive)
        XCTAssertNil(s.write(KeywordsRecord(keywords: [], modified: t0, machine: s.machine)))
        XCTAssertEqual(store().isEnabled, false, "the switch persists")
        s.isEnabled = true
        XCTAssertNotNil(s.write(KeywordsRecord(keywords: [], modified: t0, machine: s.machine)))
    }

    // MARK: Last writer wins

    func testLastWriterWinsPerRecord() {
        let s = store()
        let older = KeywordsRecord(keywords: [.init(name: "Travel", parent: nil)], modified: t0, machine: "mac-a")
        let newer = KeywordsRecord(keywords: [.init(name: "Weddings", parent: nil)], modified: t0 + 60, machine: "mac-b")
        XCTAssertEqual(s.write(newer), newer)
        // An older copy arriving later (another Mac's stale write) does not replace the newer one.
        XCTAssertEqual(s.write(older), newer)
        XCTAssertEqual(s.read(KeywordsRecord.self), newer)
        // A newer copy does.
        let newest = KeywordsRecord(keywords: [.init(name: "Family", parent: nil)], modified: t0 + 120, machine: "mac-a")
        XCTAssertEqual(s.write(newest), newest)
        XCTAssertEqual(s.read(KeywordsRecord.self), newest)
    }

    func testTieGoesToTheHigherMachineOnBothMacs() {
        let a = KeywordsRecord(keywords: [.init(name: "A", parent: nil)], modified: t0, machine: "mac-a")
        let b = KeywordsRecord(keywords: [.init(name: "B", parent: nil)], modified: t0, machine: "mac-b")
        XCTAssertEqual(KeywordsRecord.merge(a, b), b)
        XCTAssertEqual(KeywordsRecord.merge(b, a), b)
    }

    func testSameContentIsNotRewrittenWithAFreshStamp() throws {
        let s = store()
        let first = PreferencesRecord(ai: AIPreferences(), accent: "F0A52A", ground: "black", modified: t0, machine: "mac-a")
        XCTAssertEqual(s.write(first), first)
        // The same content as applied on this Mac, re-pushed with a new stamp: nothing changes.
        let echo = PreferencesRecord(ai: AIPreferences(), accent: "F0A52A", ground: "black", modified: t0 + 5, machine: s.machine)
        XCTAssertEqual(s.write(echo), first)
        XCTAssertEqual(s.read(PreferencesRecord.self)?.machine, "mac-a")
    }

    func testPreferencesRecordTakesDefaultsForMissingMembers() throws {
        // An older Mac's file lacks members a newer build added; they take their defaults.
        let json = """
        { "ai": { "provider": "ollama" }, "modified": "2025-10-09T08:53:20Z", "machine": "mac-a" }
        """
        let rec = try AccountStore.decoder().decode(PreferencesRecord.self, from: Data(json.utf8))
        XCTAssertEqual(rec.ai.provider, .ollama)
        XCTAssertEqual(rec.ai.maxIterations, AIPreferences().maxIterations)
        XCTAssertNil(rec.accent)
        XCTAssertEqual(rec.modified, t0, "a stamp without fractional seconds still decodes")
    }

    // MARK: People

    func testPeopleRecordsUnionClusterIDsAndTakeTheLatestName() {
        let s = store()
        // Mac A names cluster p1 "Rut" and merges p2 into it.
        let a = PersonRecord(id: "p1", name: "Rut", clusters: ["p2"], modified: t0, machine: "mac-a")
        XCTAssertEqual(s.write(a, id: "p1"), a)
        // Mac B, later, merges p3 into p1 and spells the name differently.
        let b = PersonRecord(id: "p1", name: "Rut M.", clusters: ["p3"], modified: t0 + 10, machine: "mac-b")
        let merged = s.write(b, id: "p1")
        XCTAssertEqual(merged?.name, "Rut M.")
        XCTAssertEqual(merged?.clusters, ["p2", "p3"], "a union, sorted, without the record's own id")
        XCTAssertEqual(merged?.allIDs, ["p1", "p2", "p3"])
        XCTAssertEqual(merged?.machine, "mac-b")
        // An older write from A with yet another cluster still contributes the cluster, not the name.
        let stale = PersonRecord(id: "p1", name: "Rut", clusters: ["p4"], modified: t0 - 10, machine: "mac-a")
        let again = s.write(stale, id: "p1")
        XCTAssertEqual(again?.name, "Rut M.")
        XCTAssertEqual(again?.clusters, ["p2", "p3", "p4"])
    }

    func testPeopleRecordNeverListsItselfAsACluster() {
        let r = PersonRecord(id: "p1", name: nil, clusters: ["p1", "p2", "p2"], modified: t0, machine: "m")
        XCTAssertEqual(r.clusters, ["p2"])
    }

    // MARK: Shoots (style profile, learning)

    func testStyleProfileShootsUnionByLibraryAndTheQuestionnaireFollowsTheLastWriter() {
        let qa = QuestionnaireRecord(brightness: 0.2)
        let qb = QuestionnaireRecord(warmth: -0.3)
        let a = StyleProfileRecord(questionnaire: qa, shoots: [
            "lib-1": .init(libraryID: "lib-1", samples: 3, questionnaire: qa, modified: t0, machine: "mac-a", model: Data("A1".utf8)),
            "lib-2": .init(libraryID: "lib-2", samples: 1, questionnaire: qa, modified: t0, machine: "mac-a", model: nil),
        ], modified: t0, machine: "mac-a")
        let b = StyleProfileRecord(questionnaire: qb, shoots: [
            "lib-1": .init(libraryID: "lib-1", samples: 5, questionnaire: qb, modified: t0 + 30, machine: "mac-b", model: Data("B1".utf8)),
            "lib-3": .init(libraryID: "lib-3", samples: 2, questionnaire: qb, modified: t0 + 30, machine: "mac-b", model: nil),
        ], modified: t0 + 30, machine: "mac-b")
        let m = StyleProfileRecord.merge(a, b)
        XCTAssertEqual(m.questionnaire, qb)
        XCTAssertEqual(Set(m.shoots.keys), ["lib-1", "lib-2", "lib-3"])
        XCTAssertEqual(m.shoots["lib-1"]?.model, Data("B1".utf8), "the newer copy of a shoot wins")
        XCTAssertEqual(m.shoots["lib-1"]?.samples, 5)
        XCTAssertEqual(m.samples, 8, "samples over every shoot")
        XCTAssertEqual(StyleProfileRecord.merge(b, a), m, "symmetric")
    }

    func testLearningShootsUnionByLibrary() {
        let a = LearningRecord(shoots: ["lib-1": .init(libraryID: "lib-1", labels: 10, modified: t0, machine: "mac-a", model: nil)],
                               modified: t0, machine: "mac-a")
        let b = LearningRecord(shoots: ["lib-2": .init(libraryID: "lib-2", labels: 4, modified: t0 - 5, machine: "mac-b", model: nil)],
                               modified: t0 - 5, machine: "mac-b")
        let m = LearningRecord.merge(a, b)
        XCTAssertEqual(m.labels, 14)
        XCTAssertEqual(m.machine, "mac-a")
        XCTAssertEqual(Set(m.shoots.keys), ["lib-1", "lib-2"])
    }

    // MARK: Drives

    func testDriveRootsUnionByPathAndKeyByVolumeUUID() {
        let s = store()
        let a = DriveRecord(volumeUUID: "VOL-1", name: "Rut photo",
                            roots: [.init(path: "2025/Weddings", lastMountPath: "/Volumes/Rut photo/2025/Weddings", photos: 226, lastSeen: t0)],
                            lastSeenMachine: "mac-a", lastSeenMachineName: "Studio", lastSeen: t0, modified: t0, machine: "mac-a")
        XCTAssertEqual(s.write(a, id: a.volumeUUID), a)
        let b = DriveRecord(volumeUUID: "VOL-1", name: "Rut photo",
                            roots: [.init(path: "2025/Travel", lastMountPath: "/Volumes/Rut photo-1/2025/Travel", photos: 40, lastSeen: t0 + 9)],
                            lastSeenMachine: "mac-b", lastSeenMachineName: "Laptop", lastSeen: t0 + 9, modified: t0 + 9, machine: "mac-b")
        let m = s.write(b, id: b.volumeUUID)
        XCTAssertEqual(m?.roots.map(\.path), ["2025/Travel", "2025/Weddings"])
        XCTAssertEqual(m?.photos, 266)
        XCTAssertEqual(m?.lastSeenMachineName, "Laptop")
        XCTAssertEqual(s.list(DriveRecord.self).map(\.volumeUUID), ["VOL-1"])
    }

    // MARK: Keywords

    func testKeywordsInCreationOrderPutParentsFirst() {
        let r = KeywordsRecord(keywords: [
            .init(name: "Ceremony", parent: "Weddings"), .init(name: "Weddings", parent: "Events"),
            .init(name: "Events", parent: nil), .init(name: "Animals", parent: nil),
        ], modified: t0, machine: "m")
        XCTAssertEqual(r.inCreationOrder.map(\.name), ["Animals", "Events", "Weddings", "Ceremony"])
    }

    // MARK: Change detection (what the watcher feeds)

    func testOwnWritesAreNotReportedAsChangesButRemoteOnesAre() throws {
        let s = store()
        let url = try XCTUnwrap(s.url(for: RecordKey(.keywords)))
        s.write(KeywordsRecord(keywords: [], modified: t0, machine: s.machine))
        let stamp = try XCTUnwrap(url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate)
        XCTAssertEqual(s.changed([(url, stamp)]), [], "our own write")
        // Another Mac's file lands (newer modification date).
        XCTAssertEqual(s.changed([(url, stamp + 2)]), [RecordKey(.keywords)])
        XCTAssertEqual(s.changed([(url, stamp + 2)]), [], "reported once")
        // Something that is not a record.
        XCTAssertEqual(s.changed([(base.appendingPathComponent("Tessera/notes.txt"), stamp + 3)]), [])
    }

    // MARK: Synced folder (CS-2b)

    func testFolderStoreIsAvailableWhenTheFolderIsThereAndWritable() throws {
        let s = AccountStore(folder: base, defaults: defaults)
        XCTAssertEqual(s.availability, .available)
        XCTAssertEqual(s.location, .folder(base))
        XCTAssertEqual(s.root, base.appendingPathComponent("Tessera", isDirectory: true))
        var isDirectory: ObjCBool = false
        XCTAssertTrue(FileManager.default.fileExists(atPath: s.root!.path, isDirectory: &isDirectory) && isDirectory.boolValue,
                      "Tessera/ is created under the folder")

        // Write / read round trip through the same coordinated, atomic path.
        let prefs = PreferencesRecord(ai: AIPreferences(), accent: "F0A52A", ground: "black", modified: t0, machine: s.machine)
        XCTAssertEqual(s.write(prefs), prefs)
        XCTAssertEqual(AccountStore(folder: base, defaults: defaults).read(PreferencesRecord.self), prefs)
        XCTAssertEqual(s.stats().files, 1)

        // Not there (the sync app is not running): unavailable, with a one-line fix.
        let gone = AccountStore(folder: base.appendingPathComponent("Nextcloud"), defaults: defaults)
        XCTAssertEqual(gone.availability, .unavailable(.folderMissing))
        XCTAssertNil(gone.root)
        XCTAssertNil(gone.write(prefs))
        XCTAssertTrue(gone.unavailableReason!.message.hasPrefix("Folder not found"))
        XCTAssertFalse(gone.unavailableReason!.fix.isEmpty)
        // A file where the folder should be is not a folder.
        let file = base.appendingPathComponent("notes.txt")
        try Data().write(to: file)
        XCTAssertEqual(AccountStore(folder: file, defaults: defaults).availability, .unavailable(.folderMissing))
        // No folder chosen yet.
        XCTAssertEqual(AccountStore.live(location: nil, defaults: defaults).availability, .unavailable(.noFolder))
        for reason in [AccountStore.UnavailableReason.noFolder, .folderMissing, .folderReadOnly] { XCTAssertFalse(reason.fix.isEmpty) }
    }

    func testFolderStoreIsUnavailableWhenTheFolderIsReadOnly() throws {
        let locked = base.appendingPathComponent("locked", isDirectory: true)
        try FileManager.default.createDirectory(at: locked, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: locked.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: locked.path) }
        XCTAssertEqual(AccountStore(folder: locked, defaults: defaults).availability, .unavailable(.folderReadOnly))
    }

    func testSyncLocationPersistsWithABookmark() throws {
        XCTAssertEqual(SyncLocation.stored(in: defaults), .iCloudDrive, "iCloud Drive by default")
        SyncLocation.folder(base).save(in: defaults)
        XCTAssertNotNil(defaults.data(forKey: SyncLocation.bookmarkKey))
        XCTAssertEqual(SyncLocation.stored(in: defaults)?.folderURL?.standardizedFileURL.resolvingSymlinksInPath(),
                       base.standardizedFileURL.resolvingSymlinksInPath())
        // Back to iCloud: the folder is still remembered for the Settings row.
        SyncLocation.iCloudDrive.save(in: defaults)
        XCTAssertEqual(SyncLocation.stored(in: defaults), .iCloudDrive)
        XCTAssertNotNil(SyncLocation.storedFolder(in: defaults))
        // "A synced folder" with none chosen.
        defaults.removeObject(forKey: SyncLocation.bookmarkKey)
        defaults.removeObject(forKey: SyncLocation.pathKey)
        SyncLocation.saveFolderPending(in: defaults)
        XCTAssertNil(SyncLocation.stored(in: defaults))
    }

    func testConflictCopyNamesMapToTheirRecord() {
        let s = AccountStore(folder: base, defaults: defaults)
        let people = s.root!.appendingPathComponent("people")
        let cases = [
            "keywords (conflicted copy 2026-10-15).json", "keywords (Rut's conflicted copy 2026-10-15).json",
            "keywords (conflict 2026-10-15 120000).json", "keywords.sync-conflict-20261015-120000-ABCDEFG.json",
            "keywords_conflict-20261015-120000.json",
        ]
        for name in cases {
            XCTAssertEqual(s.canonicalURL(forConflictCopy: people.appendingPathComponent(name))?.lastPathComponent, "keywords.json", name)
        }
        XCTAssertNil(s.canonicalURL(forConflictCopy: people.appendingPathComponent("keywords.json")))
        XCTAssertNil(s.canonicalURL(forConflictCopy: people.appendingPathComponent("person%2F1.json")))
    }

    func testConflictCopiesAreMergedByTheRecordRulesAndRemoved() throws {
        let s = AccountStore(folder: base, defaults: defaults)
        // This Mac named p1 "Rut" with cluster p2; the other Mac's copy came back as a conflict.
        let mine = PersonRecord(id: "p1", name: "Rut", clusters: ["p2"], modified: t0, machine: "mac-a")
        XCTAssertEqual(s.write(mine, id: "p1"), mine)
        let theirs = PersonRecord(id: "p1", name: "Rut M.", clusters: ["p3"], modified: t0 + 10, machine: "mac-b")
        let copy = try XCTUnwrap(s.url(for: RecordKey(.person, id: "p1"))).deletingLastPathComponent()
            .appendingPathComponent("p1 (conflicted copy 2026-10-15).json")
        try AccountStore.encoder().encode(theirs).write(to: copy)
        // And a stale keywords copy from Syncthing that must not replace the newer record.
        let newer = KeywordsRecord(keywords: [.init(name: "Weddings", parent: nil)], modified: t0 + 60, machine: "mac-b")
        XCTAssertEqual(s.write(newer), newer)
        let stale = KeywordsRecord(keywords: [.init(name: "Travel", parent: nil)], modified: t0, machine: "mac-a")
        let staleCopy = s.root!.appendingPathComponent("keywords.sync-conflict-20261015-120000-ABCDEFG.json")
        try AccountStore.encoder().encode(stale).write(to: staleCopy)

        XCTAssertEqual(Set(s.absorbConflictCopies()), [RecordKey(.person, id: "p1"), RecordKey(.keywords)])
        let merged = try XCTUnwrap(s.read(PersonRecord.self, id: "p1"))
        XCTAssertEqual(merged.name, "Rut M.")
        XCTAssertEqual(merged.clusters, ["p2", "p3"], "a union, as if the other Mac had written it")
        XCTAssertEqual(s.read(KeywordsRecord.self), newer, "last writer wins")
        XCTAssertFalse(FileManager.default.fileExists(atPath: copy.path), "the copy is removed once merged")
        XCTAssertFalse(FileManager.default.fileExists(atPath: staleCopy.path))
        XCTAssertEqual(s.absorbConflictCopies(), [], "nothing left")
        XCTAssertEqual(s.list(PersonRecord.self).count, 1)
    }

    @MainActor
    func testFolderWatcherReportsAWriteFromAnotherMac() throws {
        let s = AccountStore(folder: base, defaults: defaults)
        let watcher = AccountStore.Watcher(store: s)
        defer { watcher.stop() }
        var seen: [RecordKey] = []
        let reported = expectation(description: "the remote write is reported")
        watcher.onChange = { key in
            seen.append(key)
            if key == RecordKey(.drive, id: "VOL-9") { reported.fulfill() }
        }
        watcher.start()
        XCTAssertEqual(seen, [], "an empty folder reports nothing")
        // Another Mac's file lands (the sync app writes it; here, a plain write from this process).
        let url = try XCTUnwrap(s.url(for: RecordKey(.drive, id: "VOL-9")))
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let rec = DriveRecord(volumeUUID: "VOL-9", name: "Rut photo", roots: [], lastSeenMachine: "mac-b", lastSeenMachineName: nil,
                              lastSeen: t0, modified: t0, machine: "mac-b")
        try AccountStore.encoder().encode(rec).write(to: url, options: .atomic)
        // FSEvents coalesces for 1 s; allow 2 s.
        wait(for: [reported], timeout: 2)
        XCTAssertEqual(seen, [RecordKey(.drive, id: "VOL-9")])
        XCTAssertEqual(s.read(DriveRecord.self, id: "VOL-9"), rec)
    }
}
