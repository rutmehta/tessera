import Foundation
import XCTest
@testable import TesseraCore

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
}
