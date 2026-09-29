import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// AppModel-level coverage for failed/retried Develop saves and destinations that arrive
/// while a save reservation is held. These tests use tiny generated JPEGs only.
@MainActor
final class DevelopRecoveryAdmissionBehaviorTests: XCTestCase {
    private struct Fixture {
        let root: URL
        let library: EngineLibrary
        let model: AppModel
        let closePlan: ClosePlan
        let openerCount: () -> Int
    }

    func testFailedSaveKeepsWorkspaceAndExplicitRetryCommitsThenReopensFreshController() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let item = try XCTUnwrap(fixture.library.items.first)
        model.enterPhotoEdit()
        model.openDevelop(for: item)
        guard await waitUntil({ model.develop != nil }) else { return }
        let originalController = try XCTUnwrap(model.develop)
        let originalSelection = model.selection

        fixture.closePlan.failNextClose()
        model.requestLibraryViewMode(.grid)
        guard await waitUntil({
            !model.developRecovery.hasActiveReservations && model.developRecoveries.contains { presentation in
                if case .failed = presentation.phase { return true }
                return false
            }
        }) else { return }

        XCTAssertTrue(model.photoEditing, "A failed save must leave the edit workspace attached")
        XCTAssertEqual(model.viewMode, .loupe)
        XCTAssertTrue(model.develop === originalController)
        XCTAssertTrue(model.engineLibrary === fixture.library)
        XCTAssertEqual(model.selection, originalSelection)
        let sessionID = try XCTUnwrap(model.developRecoveries.first?.id)

        model.retryDevelopRecovery(sessionID)
        guard await waitUntil({ model.viewMode == .grid && model.develop == nil }) else { return }
        XCTAssertFalse(model.photoEditing)
        XCTAssertTrue(model.developRecoveries.isEmpty)

        model.enterPhotoEdit()
        model.openDevelop(for: item)
        guard await waitUntil({ model.develop != nil }) else { return }
        XCTAssertEqual(fixture.openerCount(), 2)
        XCTAssertFalse(model.develop === originalController,
                       "Reopening after successful retry must create a new native session")
        if let close = model.closeDevelop() { _ = await close.value }
    }

    func testSuccessfulRetryClearsRecoveryOwnedBlockedNavigationStatus() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        fixture.closePlan.failNextClose()

        model.requestLibraryViewMode(.grid)
        guard await waitUntil({ model.statusMessage == "Finish saving the photo before leaving this workspace"
            && !model.developRecovery.hasActiveReservations }) else { return }
        let sessionID = try XCTUnwrap(model.developRecoveries.first?.id)
        model.keepEditingDevelopRecovery()

        // A fresh Back request is blocked by the retained failed close and owns this status.
        model.requestLibraryViewMode(.grid)
        guard await waitUntil({ model.statusMessage == "Finish saving the photo before leaving this workspace"
            && !model.developRecovery.hasActiveReservations }) else { return }
        XCTAssertTrue(model.photoEditing)

        model.retryDevelopRecovery(sessionID)
        guard await waitUntil({ model.viewMode == .grid && !model.photoEditing
            && model.develop == nil && model.developRecoveries.isEmpty }) else { return }
        XCTAssertNil(model.statusMessage,
                     "A successful current retry should clear its stale blocked-navigation message")
    }

    func testSuccessfulRetryPreservesNewerUnrelatedStatus() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        fixture.closePlan.failNextClose()

        model.requestLibraryViewMode(.grid)
        guard await waitUntil({ model.statusMessage == "Finish saving the photo before leaving this workspace"
            && !model.developRecovery.hasActiveReservations }) else { return }
        let sessionID = try XCTUnwrap(model.developRecoveries.first?.id)
        model.keepEditingDevelopRecovery()
        model.requestLibraryViewMode(.grid)
        guard await waitUntil({ model.statusMessage == "Finish saving the photo before leaving this workspace"
            && !model.developRecovery.hasActiveReservations }) else { return }

        let entered = expectation(description: "explicit retry close is held")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.retryDevelopRecovery(sessionID)
        await fulfillment(of: [entered], timeout: 5)
        let newerStatus = "A newer import completed"
        model.statusMessage = newerStatus
        hold.release()
        guard await waitUntil({ model.viewMode == .grid && !model.photoEditing
            && model.develop == nil && model.developRecoveries.isEmpty }) else { return }
        XCTAssertEqual(model.statusMessage, newerStatus,
                       "Retry cleanup must not erase status published by another operation")
    }

    func testLatestViewIntentCommitsAfterOneHeldDevelopClose() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }

        let entered = expectation(description: "native close is held")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.requestViewMode(.document)
        await fulfillment(of: [entered], timeout: 5)
        XCTAssertTrue(model.photoEditing)
        XCTAssertNotEqual(model.viewMode, .document,
                          "The requested destination is not published before save settlement")

        model.requestLibraryViewMode(.grid)
        hold.release()
        guard await waitUntil({ model.viewMode == .grid && model.develop == nil }) else { return }
        XCTAssertFalse(model.photoEditing)
        XCTAssertEqual(fixture.closePlan.closeCount, 1,
                       "Replacing a pending destination must share, not restart, the native close")
        XCTAssertEqual(fixture.openerCount(), 1)
    }

    func testHeldCloseCommitsGridDoubleClickSelectionAndLoupeTogether() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let item = try XCTUnwrap(fixture.library.items.first)
        model.enterPhotoEdit()
        model.openDevelop(for: item)
        guard await waitUntil({ model.develop != nil }) else { return }
        let oldSelection = model.selection
        let oldEditing = model.photoEditing

        let entered = expectation(description: "grid double-click waits on held Develop close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.selectAndShowInLoupe(position: 1)
        await fulfillment(of: [entered], timeout: 5)

        XCTAssertTrue(model.photoEditing)
        XCTAssertEqual(model.selection, oldSelection,
                       "The selected item must not change before the save admission completes")
        hold.release()
        guard await waitUntil({ model.photoEditing == oldEditing && model.develop == nil
            && model.selection == IndexSet(integer: 1) && model.viewMode == .loupe }) else { return }

        XCTAssertEqual(fixture.closePlan.closeCount, 1)
        XCTAssertEqual(model.selection, IndexSet(integer: 1))
        XCTAssertEqual(model.viewMode, .loupe)
    }

    func testHeldCloseReturnsToLibraryAndEntersCompareAsOneIntent() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }

        let entered = expectation(description: "Compare waits on held Develop close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.requestLibraryCompare()
        await fulfillment(of: [entered], timeout: 5)

        XCTAssertTrue(model.photoEditing)
        XCTAssertNil(model.compare,
                     "Neither the edit return nor Compare should publish before close succeeds")
        hold.release()
        guard await waitUntil({ !model.photoEditing && model.develop == nil
            && model.viewMode == .compare && model.compare != nil }) else { return }

        XCTAssertEqual(fixture.closePlan.closeCount, 1)
        XCTAssertFalse(model.photoEditing)
        XCTAssertNotNil(model.compare)
        XCTAssertEqual(model.viewMode, .compare)
    }

    func testHeldCloseKeepsTetherAutoAdvanceTargetAndLoupeTogether() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let target = try XCTUnwrap(fixture.library.items.last)
        let targetID = target.id
        let targetPosition = try XCTUnwrap(model.visible.firstIndex(of: targetID))
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        let oldSelection = model.selection
        let oldEditing = model.photoEditing

        let entered = expectation(description: "Tether auto-advance waits on held Develop close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.selectAndAutoAdvance(id: targetID)
        await fulfillment(of: [entered], timeout: 5)

        XCTAssertTrue(model.photoEditing)
        XCTAssertEqual(model.selection, oldSelection)
        hold.release()
        guard await waitUntil({ model.photoEditing == oldEditing && model.develop == nil
            && model.selection == IndexSet(integer: targetPosition) && model.viewMode == .loupe }) else { return }

        XCTAssertEqual(fixture.closePlan.closeCount, 1)
        XCTAssertEqual(model.selection, IndexSet(integer: targetPosition))
        XCTAssertEqual(model.viewMode, .loupe)
    }

    func testHeldGridDestinationResolvesTargetByImageIdentityAfterLibraryRevisionChanges() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let library = fixture.library
        let targetPosition = 1
        let targetID = try XCTUnwrap(model.visible.indices.contains(targetPosition)
            ? library.items[model.visible[targetPosition]].engineImage?.imageID : nil)
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        let sourceRevision = model.libraryRevision

        let entered = expectation(description: "grid destination waits on held Develop close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.selectAndShowInLoupe(position: targetPosition)
        await fulfillment(of: [entered], timeout: 5)

        try Self.writePhoto(to: try XCTUnwrap(library.folder).appendingPathComponent("photo-new.jpg"), shade: 211)
        _ = try library.engine.indexFolder(path: try XCTUnwrap(library.folder).path)
        let synchronized = expectation(description: "new catalog revision applied while close is held")
        model.syncLibrary { synchronized.fulfill() }
        await fulfillment(of: [synchronized], timeout: 10)
        XCTAssertGreaterThan(model.libraryRevision, sourceRevision)
        XCTAssertEqual(library.items.count, 3)
        XCTAssertTrue(model.photoEditing)

        hold.release()
        guard await waitUntil({ model.develop == nil && model.viewMode == .loupe
            && model.focusedItem?.engineImage?.imageID == targetID }) else { return }

        XCTAssertEqual(fixture.closePlan.closeCount, 1)
        XCTAssertEqual(model.focusedItem?.engineImage?.imageID, targetID)
        XCTAssertEqual(model.selection, IndexSet(integer: try XCTUnwrap(model.visible.firstIndex {
            model.library.items[$0].engineImage?.imageID == targetID
        })))
    }

    func testHeldCloseMovesPeoplePhotoToAllPhotosAndLoupeTogether() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.setSource(.people)
        let targetID = try XCTUnwrap(fixture.library.items.last?.id)
        model.viewMode = .loupe
        // The People grid cannot enter Photo Edit. Open the focused photo's Develop session
        // directly to hold the same source-change save gate used by its Loupe command.
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        XCTAssertFalse(model.photoEditing)
        let oldSelection = model.selection

        let entered = expectation(description: "People photo navigation waits on held Develop close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.showInAllPhotosLoupe(targetID)
        await fulfillment(of: [entered], timeout: 5)

        XCTAssertEqual(model.source, .people)
        XCTAssertEqual(model.selection, oldSelection)
        hold.release()
        guard await waitUntil({ model.develop == nil && model.source == .all
            && model.focusedItem?.id == targetID && model.viewMode == .loupe }) else { return }

        XCTAssertEqual(fixture.closePlan.closeCount, 1)
        XCTAssertFalse(model.photoEditing)
        XCTAssertEqual(model.source, .all)
        XCTAssertEqual(model.focusedItem?.id, targetID)
        XCTAssertEqual(model.viewMode, .loupe)
    }

    func testDevelopReuseUsesImageIdentityAcrossDenseIDChanges() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let original = try XCTUnwrap(fixture.library.items.first)
        let target = try XCTUnwrap(fixture.library.items.last)
        let originalImageID = try XCTUnwrap(original.engineImage?.imageID)
        let targetImageID = try XCTUnwrap(target.engineImage?.imageID)
        model.viewMode = .loupe
        model.openDevelop(for: original)
        guard await waitUntil({ model.develop != nil }) else { return }
        let originalController = try XCTUnwrap(model.develop)
        XCTAssertEqual(originalController.imageID, originalImageID)
        XCTAssertNotEqual(originalController.imageID, targetImageID)

        // First verify the same photo's session survives a changed dense id.
        originalController.relink(itemID: target.id)
        model.openDevelop(for: original)
        XCTAssertTrue(model.develop === originalController)
        XCTAssertEqual(fixture.openerCount(), 1)
        XCTAssertEqual(originalController.itemID, original.id)

        // Synthesize a stale dense-index collision: keep A installed, but give it B's
        // numeric id, as an older view can hold across a reorder. Normal AppModel.apply
        // relinks before refreshing focus; this isolates the defensive reuse guard.
        XCTAssertTrue(model.develop === originalController)
        XCTAssertFalse(model.developRecovery.hasActiveReservations)
        // Focus remains A; selection APIs correctly wait for A's save and are not part
        // of this direct stale-view regression.
        originalController.relink(itemID: target.id)
        XCTAssertEqual(originalController.itemID, target.id)
        model.openDevelop(for: target)

        guard await waitUntil({ model.develop == nil
            && model.developRecovery.canOpen(owner: fixture.library, imageID: originalImageID)
            && model.developRecovery.canOpen(owner: fixture.library, imageID: targetImageID) }) else { return }
        XCTAssertNil(model.develop,
                     "A stale request for B cannot leave A attached or publish B while focus remains A")
        XCTAssertEqual(fixture.openerCount(), 2,
                       "The image mismatch must bypass the same-dense-id reuse shortcut")
        XCTAssertEqual(fixture.closePlan.closeCount, 2,
                       "Both the old session and rejected stale-target session are drained")
    }

    func testHeldCloseOpensSavedSmartAlbumAndClearsFilterAsOneIntent() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        var initialFilter = LibraryFilter()
        initialFilter.text = "photo-0"
        model.collections.filter = initialFilter
        guard await waitUntil({ model.visible.count == 1 && model.focusedItem?.name == "photo-0.jpg" }) else { return }
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        guard await waitUntil({ model.develop != nil }) else { return }

        let entered = expectation(description: "smart album navigation waits on held Develop close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.showSavedSmartAlbum(id: 42, name: "Recently Added", clearFilter: true)
        await fulfillment(of: [entered], timeout: 5)

        XCTAssertEqual(model.source, .all)
        XCTAssertEqual(model.collections.filter, initialFilter)
        hold.release()
        guard await waitUntil({ model.develop == nil && !model.photoEditing
            && model.source == .smartAlbum(id: 42, name: "Recently Added")
            && model.collections.filter.isEmpty }) else { return }

        XCTAssertEqual(fixture.closePlan.closeCount, 1)
        XCTAssertEqual(model.source, .smartAlbum(id: 42, name: "Recently Added"))
        XCTAssertTrue(model.collections.filter.isEmpty)
    }

    func testSelectionDuringLayersCloseInvalidatesCopyButStillRequiresFreshSelection() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        let originalSelection = model.selection

        let entered = expectation(description: "Layers recipe read waits on held Develop close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        await fulfillment(of: [entered], timeout: 5)
        XCTAssertTrue(model.developRecovery.hasActiveReservations)

        model.select(position: 1)
        XCTAssertEqual(model.selection, originalSelection,
                       "The selection request is rejected while the Layers read still owns its gate")
        let newerStatus = model.statusMessage
        XCTAssertNotNil(newerStatus)
        hold.release()
        guard await waitUntil({ !model.developRecovery.hasActiveReservations && model.develop == nil }) else { return }

        XCTAssertTrue(model.documents.documents.isEmpty,
                      "The superseded Layers request must not dispatch a backend after close settles")
        XCTAssertNotEqual(model.viewMode, .document)
        XCTAssertEqual(model.statusMessage, newerStatus)
        model.select(position: 1)
        guard await waitUntil({ model.selection == IndexSet(integer: 1) }) else { return }
        XCTAssertTrue(model.documents.documents.isEmpty)
    }

    func testLibraryDestinationDuringLayersCloseInvalidatesCopyUntilReissued() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        let originalMode = model.viewMode

        let entered = expectation(description: "Layers read holds the editor close")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        await fulfillment(of: [entered], timeout: 5)
        XCTAssertTrue(model.developRecovery.hasActiveReservations)

        model.requestLibraryViewMode(.grid)
        XCTAssertEqual(model.viewMode, originalMode,
                       "A Library destination cannot commit while the read gate is active")
        let newerStatus = model.statusMessage
        XCTAssertNotNil(newerStatus)
        hold.release()
        guard await waitUntil({ !model.developRecovery.hasActiveReservations && model.develop == nil }) else { return }

        XCTAssertTrue(model.documents.documents.isEmpty,
                      "The stale Layers destination must not dispatch after a newer Library request")
        XCTAssertEqual(model.viewMode, originalMode)
        XCTAssertEqual(model.statusMessage, newerStatus)
        model.requestLibraryViewMode(.grid)
        guard await waitUntil({ model.viewMode == .grid && !model.photoEditing }) else { return }
    }

    func testFailedClosePreservesSavedSmartAlbumSourceAndFilter() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        var originalFilter = LibraryFilter()
        originalFilter.text = "photo-0"
        model.collections.filter = originalFilter
        guard await waitUntil({ model.visible.count == 1 && model.focusedItem?.name == "photo-0.jpg" }) else { return }
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        guard await waitUntil({ model.develop != nil }) else { return }
        let originalController = try XCTUnwrap(model.develop)
        fixture.closePlan.failNextClose()

        model.showSavedSmartAlbum(id: 42, name: "Recently Added", clearFilter: true)
        guard await waitUntil({ !model.developRecovery.hasActiveReservations
            && model.developRecoveries.contains { if case .failed = $0.phase { return true }; return false } }) else { return }

        XCTAssertEqual(model.source, .all)
        XCTAssertEqual(model.collections.filter, originalFilter)
        XCTAssertTrue(model.photoEditing)
        XCTAssertTrue(model.develop === originalController)
        let sessionID = try XCTUnwrap(model.developRecoveries.first?.id)
        model.retryDevelopRecovery(sessionID)
        guard await waitUntil({ model.develop == nil
            && model.source == .smartAlbum(id: 42, name: "Recently Added")
            && model.collections.filter.isEmpty }) else { return }
    }

    func testSupersededFolderCallbackSettlesOnceAndMayReenterOpenFolder() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let defaults = UserDefaults.standard
        let priorLastFolder = defaults.object(forKey: "LastFolderPath")
        let priorRecentFolders = defaults.object(forKey: "RecentFolderPaths")
        defer {
            if let priorLastFolder { defaults.set(priorLastFolder, forKey: "LastFolderPath") }
            else { defaults.removeObject(forKey: "LastFolderPath") }
            if let priorRecentFolders { defaults.set(priorRecentFolders, forKey: "RecentFolderPaths") }
            else { defaults.removeObject(forKey: "RecentFolderPaths") }
        }
        // AppModel.openFolder opens through EngineLibrary.defaultSupportDirectory. Without an
        // override that is the user's real catalog, shared with the app and every concurrent
        // test process: a writer there holds the index past this test's wait and pollutes it.
        let priorAppDir = ProcessInfo.processInfo.environment["TESSERA_APP_DIR"]
        setenv("TESSERA_APP_DIR", fixture.root.appendingPathComponent("folder-support").path, 1)
        defer {
            if let priorAppDir { setenv("TESSERA_APP_DIR", priorAppDir, 1) }
            else { unsetenv("TESSERA_APP_DIR") }
        }
        XCTAssertEqual(EngineLibrary.defaultSupportDirectory.path,
                       fixture.root.appendingPathComponent("folder-support").path)
        let firstDestination = fixture.root.appendingPathComponent("destination-a")
        let reentrantDestination = fixture.root.appendingPathComponent("destination-c")
        try FileManager.default.createDirectory(at: firstDestination, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: reentrantDestination, withIntermediateDirectories: true)
        try Self.writePhoto(to: firstDestination.appendingPathComponent("a.jpg"), shade: 91)
        try Self.writePhoto(to: reentrantDestination.appendingPathComponent("c.jpg"), shade: 151)

        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        let entered = expectation(description: "folder navigation close is held")
        let hold = fixture.closePlan.holdNext(entered: entered)
        defer { hold.release() }
        var originalCallbacks: [(Bool, ViewMode)] = []
        var reentrantCallbacks: [Bool] = []

        model.openFolder(firstDestination) { model, loaded in
            originalCallbacks.append((loaded, model.viewMode))
            if !loaded {
                model.openFolder(reentrantDestination) { _, nestedLoaded in
                    reentrantCallbacks.append(nestedLoaded)
                }
            }
        }
        await fulfillment(of: [entered], timeout: 5)
        model.requestLibraryViewMode(.grid)

        XCTAssertEqual(originalCallbacks.count, 1)
        XCTAssertFalse(try XCTUnwrap(originalCallbacks.first).0)
        XCTAssertEqual(try XCTUnwrap(originalCallbacks.first).1, .loupe,
                       "The superseded folder callback settles before the pending mode commits")
        hold.release()
        guard await waitUntil({ !model.isLoading && model.library.items.first?.name == "c.jpg" }) else { return }
        XCTAssertEqual(originalCallbacks.count, 1)
        XCTAssertEqual(reentrantCallbacks, [true],
                       "The callback's reentrant folder request should become the latest destination")
        XCTAssertFalse(model.photoEditing)
    }

    func testLayeredCompletionCannotTakeOverAfterLibraryReplacement() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let item = try XCTUnwrap(fixture.library.items.first)
        let initialLibraryView = model.viewMode
        let initialSelection = model.selection
        let replacementFolder = fixture.root.appendingPathComponent("replacement-library")
        try Self.writePhoto(to: replacementFolder.appendingPathComponent("replacement.jpg"), shade: 188)
        let replacement = try EngineLibrary.scan(
            folder: replacementFolder, appSupport: fixture.root.appendingPathComponent("replacement-support"))
        model.enterPhotoEdit()
        model.openDevelop(for: item)
        guard await waitUntil({ model.develop != nil }) else { return }

        let backendReady = expectation(description: "layered backend opened but installation is held")
        let heldLoad = HeldLayerLoad()
        defer { heldLoad.completeIfPending() }
        model.documents.documentLoadExecutor = { engine, body, receive in
            do {
                let backend = try body(engine)
                MainActor.assumeIsolated {
                    heldLoad.backend = backend
                    heldLoad.completion = { result in receive(result) }
                }
                backendReady.fulfill()
            } catch {
                MainActor.assumeIsolated { heldLoad.failure = error }
                backendReady.fulfill()
            }
        }
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        await fulfillment(of: [backendReady], timeout: 8)

        XCTAssertTrue(model.developRecovery.hasActiveReservations,
                      "The host read gate must remain held after native Develop close and until completion")
        XCTAssertEqual(model.viewMode, initialLibraryView,
                       "The document destination stays unpublished until the backend settles")
        model.openDevelop(for: item)
        XCTAssertEqual(fixture.openerCount(), 1,
                       "The source photo remains reserved while layered installation is outstanding")
        model.select(position: 1)
        XCTAssertEqual(model.selection, initialSelection,
                       "Selection navigation is blocked while the source read still owns its gate")
        if let failure = heldLoad.failure { XCTFail("Layered backend fixture failed: \(failure)") }
        model.install(replacement)
        XCTAssertTrue(model.engineLibrary === replacement,
                      "The stale completion scenario requires a real library-owner replacement")
        XCTAssertTrue(model.developRecovery.hasActiveReservations)
        heldLoad.completeIfPending()
        guard await waitUntil({ model.documents.current != nil && !model.developRecovery.hasActiveReservations }) else { return }
        guard model.documents.current != nil else { return }

        XCTAssertTrue(model.engineLibrary === replacement)
        XCTAssertEqual(model.viewMode, initialLibraryView,
                       "A delayed result for the old owner must not switch the replacement library into Layers")
        XCTAssertNotNil(heldLoad.backend)
        if let document = model.documents.current { model.documents.close(document) }
    }

    func testStaleLayeredFailurePreservesReplacementStatusAndReleasesSourceGate() async throws {
        let fixture = try makeFixture(photoCount: 1)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        let owner = fixture.library
        let item = try XCTUnwrap(owner.items.first)
        let imageID = try XCTUnwrap(item.engineImage?.imageID)
        let replacementFolder = fixture.root.appendingPathComponent("replacement-after-layer-failure")
        try Self.writePhoto(to: replacementFolder.appendingPathComponent("replacement.jpg"), shade: 173)
        let replacement = try EngineLibrary.scan(
            folder: replacementFolder, appSupport: fixture.root.appendingPathComponent("replacement-failure-support"))

        model.enterPhotoEdit()
        model.openDevelop(for: item)
        guard await waitUntil({ model.develop != nil }) else { return }

        let backendReady = expectation(description: "failed backend outcome is held")
        let heldLoad = HeldLayerLoad()
        defer { heldLoad.completeIfPending() }
        model.documents.documentLoadExecutor = { engine, body, receive in
            do {
                let backend = try body(engine)
                MainActor.assumeIsolated {
                    heldLoad.backend = backend
                    heldLoad.completion = { result in receive(result) }
                }
                backendReady.fulfill()
            } catch {
                MainActor.assumeIsolated { heldLoad.failure = error }
                backendReady.fulfill()
            }
        }
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        await fulfillment(of: [backendReady], timeout: 8)
        XCTAssertTrue(model.developRecovery.hasActiveReservations)

        model.install(replacement)
        let replacementStatus = "Replacement library is ready"
        model.statusMessage = replacementStatus
        XCTAssertTrue(model.engineLibrary === replacement)
        if let failure = heldLoad.failure { XCTFail("Layered backend fixture failed: \(failure)") }

        heldLoad.failIfPending(InjectedLayerLoadFailure())
        guard await waitUntil({ !model.developRecovery.hasActiveReservations }) else { return }

        XCTAssertEqual(model.statusMessage, replacementStatus,
                       "A stale failure must not replace status owned by the newly installed library")
        XCTAssertTrue(model.developRecovery.canOpen(owner: owner, imageID: imageID),
                      "The old source read gate is released after the delayed failure settles")
    }

    func testSelectionDuringHeldLayerBackendKeepsReservationAndPreventsStaleViewStatus() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let model = fixture.model
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(fixture.library.items.first))
        guard await waitUntil({ model.develop != nil }) else { return }
        let originalSelection = model.selection

        let backendReady = expectation(description: "Layered backend is open with its read gate held")
        let heldLoad = HeldLayerLoad()
        defer { heldLoad.completeIfPending() }
        model.documents.documentLoadExecutor = { engine, body, receive in
            do {
                let backend = try body(engine)
                MainActor.assumeIsolated {
                    heldLoad.backend = backend
                    heldLoad.completion = { result in receive(result) }
                }
                backendReady.fulfill()
            } catch {
                MainActor.assumeIsolated { heldLoad.failure = error }
                backendReady.fulfill()
            }
        }
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        await fulfillment(of: [backendReady], timeout: 8)
        if let failure = heldLoad.failure { XCTFail("Layered backend fixture failed: \(failure)") }
        XCTAssertTrue(model.developRecovery.hasActiveReservations)
        let originalMode = model.viewMode

        model.select(position: 1)
        XCTAssertEqual(model.selection, originalSelection,
                       "A selection cannot commit while the installed backend owns the source read")
        XCTAssertTrue(model.developRecovery.hasActiveReservations,
                      "A newer selection does not cancel the backend's outstanding read")
        let newerStatus = "A newer selection request is waiting"
        model.statusMessage = newerStatus
        heldLoad.completeIfPending()
        guard await waitUntil({ !model.developRecovery.hasActiveReservations && model.documents.current != nil }) else { return }

        XCTAssertNotEqual(model.viewMode, .document,
                          "The stale Layers result must not take over the workspace after selection changed")
        XCTAssertEqual(model.viewMode, originalMode)
        XCTAssertEqual(model.statusMessage, newerStatus,
                       "The backend's stale completion must not overwrite the newer selection status")
        model.select(position: 1)
        guard await waitUntil({ model.selection == IndexSet(integer: 1) }) else { return }
        if let document = model.documents.current { model.documents.close(document) }
    }

    func testPresentingExportSheetDoesNotCloseOrReserveDevelopPixels() async throws {
        let fixture = try makeFixture(photoCount: 2)
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let defaults = UserDefaults.standard
        let priorExportSettings = defaults.object(forKey: "ExportSettings")
        let priorExportPreset = defaults.object(forKey: "ExportPresetName")
        defer {
            if let priorExportSettings { defaults.set(priorExportSettings, forKey: "ExportSettings") }
            else { defaults.removeObject(forKey: "ExportSettings") }
            if let priorExportPreset { defaults.set(priorExportPreset, forKey: "ExportPresetName") }
            else { defaults.removeObject(forKey: "ExportPresetName") }
        }
        let model = fixture.model
        model.setSelectionFromUI([0, 1], clicked: 0)
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        guard await waitUntil({ model.develop != nil }) else { return }
        let controller = try XCTUnwrap(model.develop)
        controller.set(.exposure, 0.25, interactive: true)

        model.presentExport()

        XCTAssertTrue(model.showExport)
        XCTAssertTrue(model.photoEditing)
        XCTAssertTrue(model.develop === controller)
        XCTAssertFalse(model.developRecovery.hasActiveReservations,
                       "Preparing target IDs for the sheet does not read image pixels")
        XCTAssertEqual(fixture.closePlan.closeCount, 0)
        model.showExport = false
        if let close = model.closeDevelop() { _ = await close.value }
    }

    private func makeFixture(photoCount: Int) throws -> Fixture {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("develop-recovery-appmodel-\(UUID().uuidString)")
        let folder = root.appendingPathComponent("photos")
        let support = root.appendingPathComponent("support")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for index in 0..<photoCount {
            try Self.writePhoto(to: folder.appendingPathComponent("photo-\(index).jpg"),
                                shade: UInt8(70 + index * 50))
        }
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let closePlan = ClosePlan()
        var opens = 0
        let model = AppModel(
            agent: AgentController(arguments: ["--fake-planner"], supportDirectory: support),
            developControllerOpener: { reference, itemID in
                opens += 1
                let session = try reference.engine.openDevelopSession(imageId: reference.imageID)
                return try DevelopController(
                    session: ClosePlanSession(session, plan: closePlan),
                    itemID: itemID, imageID: reference.imageID)
            })
        model.install(library)
        return Fixture(root: root, library: library, model: model, closePlan: closePlan,
                       openerCount: { opens })
    }

    private func waitUntil(_ condition: @MainActor () -> Bool,
                           file: StaticString = #filePath, line: UInt = #line) async -> Bool {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(8))
        while clock.now < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(5))
        }
        XCTFail("Condition did not settle", file: file, line: line)
        return false
    }

    private static func writePhoto(to url: URL, shade: UInt8) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        let width = 64, height = 48
        var pixels = [UInt8](repeating: 255, count: width * height * 4)
        for offset in stride(from: 0, to: pixels.count, by: 4) {
            pixels[offset] = shade
            pixels[offset + 1] = shade
            pixels[offset + 2] = shade
        }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }
}

@MainActor
private final class HeldLayerLoad {
    var backend: (any DocumentBackend)?
    var completion: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
    var failure: Error?

    func completeIfPending() {
        let callback = completion
        completion = nil
        if let backend { callback?(.success(backend)) }
    }

    func failIfPending(_ error: Error) {
        let callback = completion
        completion = nil
        backend?.close()
        backend = nil
        callback?(.failure(error))
    }
}

private struct InjectedLayerLoadFailure: LocalizedError {
    var errorDescription: String? { "injected layered backend failure" }
}

private final class ClosePlan: @unchecked Sendable {
    struct Hold {
        let semaphore: DispatchSemaphore
        let entered: XCTestExpectation
        func release() { semaphore.signal() }
    }

    private let lock = NSLock()
    private var queuedFailures = 0
    private var failureCountStorage = 0
    private var nextHold: Hold?
    private var closeCountStorage = 0
    var failureCount: Int { lock.withLock { failureCountStorage } }
    var closeCount: Int { lock.withLock { closeCountStorage } }

    func failNextClose() { lock.withLock { queuedFailures += 1 } }

    func holdNext(entered: XCTestExpectation) -> Hold {
        let hold = Hold(semaphore: DispatchSemaphore(value: 0), entered: entered)
        lock.withLock { nextHold = hold }
        return hold
    }

    func closeWillFailOrWait() throws {
        let (hold, shouldFail) = lock.withLock { () -> (Hold?, Bool) in
            closeCountStorage += 1
            let hold = nextHold
            nextHold = nil
            let shouldFail = queuedFailures > 0
            if shouldFail { queuedFailures -= 1; failureCountStorage += 1 }
            return (hold, shouldFail)
        }
        if let hold {
            hold.entered.fulfill()
            guard hold.semaphore.wait(timeout: .now() + 10) == .success else {
                throw CloseFixtureTimedOut()
            }
        }
        if shouldFail { throw InjectedDevelopCloseFailure() }
    }
}

private final class ClosePlanSession: DevelopSession, @unchecked Sendable {
    private let wrapped: DevelopSession
    private let plan: ClosePlan

    init(_ wrapped: DevelopSession, plan: ClosePlan) {
        self.wrapped = wrapped
        self.plan = plan
        super.init(noHandle: .init())
    }

    required init(unsafeFromHandle: UInt64) { fatalError("Use wrapped session initializer") }
    override func info() -> DevelopInfo { wrapped.info() }
    override func historyState() throws -> HistoryState { try wrapped.historyState() }
    override func getSettingsJson() throws -> String { try wrapped.getSettingsJson() }
    override func getHistogram() throws -> Histogram { try wrapped.getHistogram() }
    override func ignoredSettings() throws -> [String] { try wrapped.ignoredSettings() }
    override func setListener(listener: DevelopListener?) { wrapped.setListener(listener: listener) }
    override func setMaskListener(listener: MaskListener?) { wrapped.setMaskListener(listener: listener) }
    override func setSettings(jsonPatch: String, interactive: Bool) throws {
        try wrapped.setSettings(jsonPatch: jsonPatch, interactive: interactive)
    }
    override func close() throws {
        try plan.closeWillFailOrWait()
        try wrapped.close()
    }
}

private struct InjectedDevelopCloseFailure: LocalizedError {
    var errorDescription: String? { "injected Develop close failure" }
}

private struct CloseFixtureTimedOut: LocalizedError {
    var errorDescription: String? { "test close gate did not receive release" }
}
