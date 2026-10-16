import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// CS-2 wiring over a temp directory standing in for the container: a record written on another
/// Mac applies to the models when the store is installed, a local change pushes a record, drives
/// the account knows that are not mounted reach the folder popover's snapshot, the switch
/// persists and stops pushes, an unavailable or isolated store leaves the app alone, and
/// Settings ▸ Sync ▸ Where re-points the store live (CS-2b).
@MainActor
final class AccountSyncControllerTests: XCTestCase {
    private var base: URL!
    private var support: URL!
    private var defaults: UserDefaults!
    private let suiteName = "dev.tessera.tests.sync.controller"

    override func setUpWithError() throws {
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("tessera-sync-ctl-\(UUID().uuidString)", isDirectory: true)
        base = scratch.appendingPathComponent("container", isDirectory: true)
        support = scratch.appendingPathComponent("support", isDirectory: true)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        defaults = try XCTUnwrap(UserDefaults(suiteName: suiteName))
        defaults.removePersistentDomain(forName: suiteName)
    }

    override func tearDownWithError() throws {
        defaults.removePersistentDomain(forName: suiteName)
        try? FileManager.default.removeItem(at: base.deletingLastPathComponent())
    }

    private func makeModel() -> (AppModel, AccountSync, AccountStore) {
        let agent = AgentController(arguments: ["--fake-planner"], supportDirectory: support)
        let sync = AccountSync(supportDirectory: support, defaults: defaults)
        let model = AppModel(agent: agent, sync: sync)
        return (model, sync, AccountStore(baseURL: base, defaults: defaults))
    }

    /// Pushes are written off the main actor: poll briefly.
    private func eventually(_ what: String, timeout: TimeInterval = 5, _ condition: () -> Bool) async {
        let deadline = Date() + timeout
        while !condition() && Date() < deadline { try? await Task.sleep(for: .milliseconds(50)) }
        XCTAssertTrue(condition(), what)
    }

    func testRemotePreferencesApplyOnInstallAndLocalChangesPush() async throws {
        let (model, sync, store) = makeModel()
        // Another Mac chose Ollama and seven rounds. (No appearance in the record: the shared
        // appearance settings belong to the process.)
        var remote = AIPreferences()
        remote.provider = .ollama
        remote.maxIterations = 7
        XCTAssertNotNil(store.write(PreferencesRecord(ai: remote, accent: nil, ground: nil, modified: Date(), machine: "mac-b")))
        XCTAssertEqual(sync.status, .resolving)
        sync.install(store, watch: false)
        XCTAssertEqual(sync.status, .available)
        XCTAssertEqual(model.agent.preferences.provider, .ollama)
        XCTAssertEqual(model.agent.preferences.maxIterations, 7)
        XCTAssertEqual(model.agent.store.load().maxIterations, 7, "applied through savePreferences, so ai-preferences.json follows")
        XCTAssertEqual(store.read(PreferencesRecord.self)?.machine, "mac-b", "applying does not bounce a fresh stamp back")

        // A local change pushes the record with this Mac's stamp.
        model.agent.preferences.maxIterations = 9
        model.agent.savePreferences()
        await eventually("preferences pushed") { store.read(PreferencesRecord.self)?.ai.maxIterations == 9 }
        XCTAssertEqual(store.read(PreferencesRecord.self)?.machine, store.machine)
        // Never a key: the record is the preferences struct, which has none.
        let data = try Data(contentsOf: XCTUnwrap(store.url(for: RecordKey(.preferences))))
        XCTAssertFalse(String(decoding: data, as: UTF8.self).lowercased().contains("apikey"))
    }

    func testDrivesNotMountedHereReachTheFolderPopover() throws {
        let (model, sync, store) = makeModel()
        let uuid = "TEST-\(UUID().uuidString)"
        let root = DriveRecord.Root(path: "2025/Weddings", lastMountPath: "/Volumes/Rut photo/2025/Weddings", photos: 226, lastSeen: Date())
        XCTAssertNotNil(store.write(DriveRecord(volumeUUID: uuid, name: "Rut photo", roots: [root], lastSeenMachine: "mac-b",
                                                lastSeenMachineName: "Studio", lastSeen: Date(), modified: Date(), machine: "mac-b"), id: uuid))
        // The boot volume is mounted: a record for it is not "not attached".
        let boot = try XCTUnwrap(URL(fileURLWithPath: "/").resourceValues(forKeys: [.volumeUUIDStringKey]).volumeUUIDString)
        XCTAssertNotNil(store.write(DriveRecord(volumeUUID: boot, name: "Boot", roots: [], lastSeenMachine: "mac-b", lastSeenMachineName: nil,
                                                lastSeen: Date(), modified: Date(), machine: "mac-b"), id: boot))
        sync.install(store, watch: false)
        XCTAssertEqual(sync.detachedDrives.map(\.volumeUUID), [uuid])
        XCTAssertEqual(sync.detachedDrives.first?.photos, 226)
        let snapshot = SidebarSnapshot(model: model)
        XCTAssertEqual(snapshot.detachedDrives.map(\.name), ["Rut photo"])
    }

    func testSwitchPersistsAndOffStopsPushing() async throws {
        let (model, sync, store) = makeModel()
        sync.install(store, watch: false)
        XCTAssertTrue(sync.isEnabled)
        sync.setEnabled(false)
        XCTAssertFalse(sync.isEnabled)
        XCTAssertEqual(defaults.object(forKey: AccountStore.enabledKey) as? Bool, false)
        XCTAssertFalse(AccountSync(supportDirectory: support, defaults: defaults).isEnabled, "the switch persists")
        model.agent.preferences.maxIterations = 4
        model.agent.savePreferences()
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertNil(store.url(for: RecordKey(.preferences)).flatMap { FileManager.default.fileExists(atPath: $0.path) ? $0 : nil },
                     "off: nothing written")
        // Back on: what this Mac has is pushed.
        sync.setEnabled(true)
        await eventually("pushed when switched on") { store.read(PreferencesRecord.self)?.ai.maxIterations == 4 }
    }

    func testUnavailableAndIsolatedStoresLeaveTheAppAlone() async throws {
        let (model, sync, _) = makeModel()
        sync.install(AccountStore(baseURL: nil, reason: .noEntitlement, defaults: defaults))
        XCTAssertEqual(sync.status, .unavailable(.noEntitlement))
        model.agent.preferences.maxIterations = 5
        model.agent.savePreferences()
        sync.refreshDrives()
        XCTAssertEqual(sync.detachedDrives, [])
        XCTAssertEqual(model.agent.store.load().maxIterations, 5, "local save still happens")

        // An isolated run resolves synchronously, without touching any container.
        let isolated = AccountSync(supportDirectory: support, defaults: defaults)
        isolated.start(arguments: ["--nonactivating"])
        XCTAssertEqual(isolated.status, .unavailable(.isolatedRun))
        XCTAssertEqual(isolated.store?.availability, .unavailable(.isolatedRun))
    }

    // MARK: Where (CS-2b)

    func testChoosingASyncedFolderRePointsTheStoreLiveAndPersists() async throws {
        let (model, sync, _) = makeModel()
        XCTAssertFalse(sync.usesFolder, "iCloud Drive by default")
        XCTAssertEqual(sync.location, .iCloudDrive)
        // The friend's Nextcloud folder, with a record another Mac already put there.
        let nextcloud = base.deletingLastPathComponent().appendingPathComponent("Nextcloud", isDirectory: true)
        try FileManager.default.createDirectory(at: nextcloud, withIntermediateDirectories: true)
        let remote = AccountStore(folder: nextcloud, defaults: defaults)
        var prefs = AIPreferences()
        prefs.maxIterations = 11
        XCTAssertNotNil(remote.write(PreferencesRecord(ai: prefs, accent: nil, ground: nil, modified: Date(), machine: "mac-b")))

        sync.setFolder(nextcloud)
        XCTAssertTrue(sync.usesFolder)
        XCTAssertEqual(sync.folder, nextcloud)
        await eventually("the folder store is installed") { sync.status == .available }
        XCTAssertEqual(sync.store?.location, .folder(nextcloud))
        XCTAssertEqual(model.agent.preferences.maxIterations, 11, "the folder's records apply")
        // The choice and the folder persist (as a bookmark) for the next launch.
        XCTAssertEqual(defaults.string(forKey: SyncLocation.kindKey), "folder")
        let again = AccountSync(supportDirectory: support, defaults: defaults)
        XCTAssertTrue(again.usesFolder)
        XCTAssertEqual(again.folder?.standardizedFileURL.resolvingSymlinksInPath(), nextcloud.standardizedFileURL.resolvingSymlinksInPath())

        // A local change now lands in the folder.
        model.agent.preferences.maxIterations = 12
        model.agent.savePreferences()
        await eventually("pushed to the folder") { remote.read(PreferencesRecord.self)?.ai.maxIterations == 12 }

        // Back to iCloud Drive: the folder is remembered, the store re-points (no entitlement
        // under swift test, so it is unavailable with that reason), nothing more is written.
        sync.setUsesFolder(false)
        XCTAssertFalse(sync.usesFolder)
        XCTAssertEqual(sync.folder, nextcloud)
        await eventually("the iCloud store is installed") { sync.status != .resolving }
        XCTAssertEqual(sync.status, .unavailable(.noEntitlement))
        XCTAssertEqual(defaults.string(forKey: SyncLocation.kindKey), "icloud")
        model.agent.preferences.maxIterations = 13
        model.agent.savePreferences()
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(remote.read(PreferencesRecord.self)?.ai.maxIterations, 12)

        // "A synced folder" chosen but the folder gone: the reason says so.
        try FileManager.default.removeItem(at: nextcloud)
        sync.setUsesFolder(true)
        await eventually("the missing folder is reported") { sync.status == .unavailable(.folderMissing) }
    }
}
