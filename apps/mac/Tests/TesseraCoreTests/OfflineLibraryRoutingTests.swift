import Darwin
import Foundation
import XCTest
@testable import TesseraCore
@testable import Tessera

/// Source-only contract tests. Native cached-session factory and app relaunch are A gates.
final class OfflineLibraryRoutingTests: XCTestCase {
    private let folder = URL(fileURLWithPath: "/Volumes/Offline/photos")
    private enum Failure: Error { case onlineDecode, denied }

    func testUnavailableRelaunchUsesCachedDeclarationsWithoutScanOrListing() throws {
        var probes = 0
        var cachedCalls = 0
        let result: [String] = try LibraryOpenRouter.open(folder: folder, availability: { _ in
            probes += 1; return false
        }, original: { _ in
            XCTFail("Offline path must never index or list originals")
            return ["unexpected scan"]
        }, cached: { path in
            cachedCalls += 1
            XCTAssertEqual(path.path, folder.path)
            return ["declared-raw"] // declaration, not pixel validation
        })
        XCTAssertEqual(result, ["declared-raw"])
        XCTAssertEqual(probes, 1)
        XCTAssertEqual(cachedCalls, 1)
    }

    func testEmptyCachedSessionStaysEmptyAndProvidesReconnectGuidance() throws {
        let result: [String] = try LibraryOpenRouter.open(folder: folder, availability: { _ in false },
            original: { _ in XCTFail("No original scan"); return ["unexpected"] }, cached: { _ in [] })
        XCTAssertTrue(result.isEmpty)
        XCTAssertTrue(LibraryAccessMode.cachedSmartPreviews.emptyMessage.contains("Reconnect"))
        XCTAssertTrue(LibraryAccessMode.cachedSmartPreviews.emptyMessage.contains("build"))
    }

    func testOrdinaryOnlineErrorDoesNotFallBackToCache() {
        var cachedCalls = 0
        XCTAssertThrowsError(try LibraryOpenRouter.open(folder: folder, availability: { _ in true },
            original: { _ -> String in throw Failure.onlineDecode },
            cached: { _ in cachedCalls += 1; return "cached" })) { error in
                guard case Failure.onlineDecode = error else { return XCTFail("Lost online error") }
            }
        XCTAssertEqual(cachedCalls, 0)
    }

    func testAvailabilityPermissionErrorDoesNotBecomeOffline() {
        var opens = 0
        XCTAssertThrowsError(try LibraryOpenRouter.open(folder: folder,
            availability: { _ in throw Failure.denied },
            original: { _ in opens += 1 }, cached: { _ in opens += 1 })) { error in
                guard case Failure.denied = error else { return XCTFail("Lost availability error") }
            }
        XCTAssertEqual(opens, 0)
    }

    func testExplicitReopenObservesOnlineOfflineTransitions() throws {
        var available = false
        var modes: [LibraryAccessMode] = []
        func reopen() throws -> LibraryAccessMode {
            try LibraryOpenRouter.open(folder: folder, availability: { _ in available },
                original: { _ in modes.append(.originalFolder); return .originalFolder },
                cached: { _ in modes.append(.cachedSmartPreviews); return .cachedSmartPreviews })
        }
        XCTAssertEqual(try reopen(), .cachedSmartPreviews)
        available = true
        XCTAssertEqual(try reopen(), .originalFolder)
        available = false
        XCTAssertEqual(try reopen(), .cachedSmartPreviews)
        XCTAssertEqual(modes, [.cachedSmartPreviews, .originalFolder, .cachedSmartPreviews])
    }

    func testCachedModeRefusesCatalogMutationAndRequiresLexicalAbsoluteFolder() throws {
        XCTAssertThrowsError(try LibraryAccessMode.cachedSmartPreviews.requireCatalogMutation())
        XCTAssertNoThrow(try LibraryAccessMode.originalFolder.requireCatalogMutation())
        XCTAssertThrowsError(try LibraryOpenRouter.validateFolderPath("relative/photos"))
        XCTAssertThrowsError(try LibraryOpenRouter.validateFolderPath("/Volumes/a/../b"))
        XCTAssertEqual(try LibraryOpenRouter.validateFolderPath(folder.path), folder.path)
    }

    /// Real native online index, then a removed alias AND original. The offline
    /// closure observes the path supplied by the same persistence adapter as AppModel.
    /// Execution belongs to A; no proxy asset fixture/GUI acceptance is implied.
    @MainActor
    func testOnlineAliasCapturesCanonicalFolderForDisconnectedRelaunch() throws {
        let fm = FileManager.default
        let scratch = fm.temporaryDirectory.appendingPathComponent("offline-alias-\(UUID().uuidString)")
        try fm.createDirectory(at: scratch, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: scratch) }
        let original = scratch.appendingPathComponent("photos", isDirectory: true)
        let alias = scratch.appendingPathComponent("friendly-alias", isDirectory: true)
        try fm.createDirectory(at: original, withIntermediateDirectories: true)
        try fm.createSymbolicLink(at: alias, withDestinationURL: original)
        // Expected canonical identity is captured while the original is ONLINE.
        // Foundation may normalize /private/var back to /var; the native catalog
        // uses POSIX canonical identity. Resolve independently while still online.
        let canonical = try XCTUnwrap(realpath(original.path, nil))
        let expected = String(cString: canonical)
        free(canonical)
        XCTAssertNotEqual(alias.path, expected)
        let library = try EngineLibrary.scan(folder: alias, appSupport: scratch.appendingPathComponent("support"))
        XCTAssertEqual(library.folder?.path, expected)
        XCTAssertEqual(library.title, "friendly-alias")

        let suite = "offline-alias-history-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set([alias.path, expected, "/Volumes/Other/missing"], forKey: "RecentFolderPaths")
        let model = AppModel()
        model.rememberOpenedFolder(library, replacing: alias, defaults: defaults)
        let saved = try XCTUnwrap(defaults.string(forKey: "LastFolderPath"))
        XCTAssertEqual(saved, expected)
        let recent = try XCTUnwrap(defaults.stringArray(forKey: "RecentFolderPaths"))
        XCTAssertEqual(recent.first, expected)
        XCTAssertEqual(recent, [expected, "/Volumes/Other/missing"])
        XCTAssertFalse(recent.contains(alias.path))
        XCTAssertEqual(model.recentFolders.first?.path, expected)

        try fm.removeItem(at: alias)
        try fm.moveItem(at: original, to: scratch.appendingPathComponent("disconnected-photos"))
        // Use real absence detection; only the cached native adapter is replaced.
        var cachedCalls = 0
        let opened: String = try LibraryOpenRouter.open(folder: URL(fileURLWithPath: saved, isDirectory: true),
            original: { _ in XCTFail("Disconnected relaunch must not scan/list originals"); return "wrong" },
            cached: { folder in
                cachedCalls += 1
                XCTAssertEqual(folder.path, expected)
                XCTAssertNotEqual(folder.path, alias.path)
                return folder.path
            })
        XCTAssertEqual(opened, expected)
        XCTAssertEqual(cachedCalls, 1)
        XCTAssertFalse(fm.fileExists(atPath: original.path))
        XCTAssertFalse(fm.fileExists(atPath: alias.path))
    }

}
