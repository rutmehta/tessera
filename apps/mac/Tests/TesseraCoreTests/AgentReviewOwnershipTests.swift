import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Uses the existing controller/engine path with the deterministic scripted
/// planner. Two folders share an engine catalog, as real folder switching does.
@MainActor
final class AgentReviewOwnershipTests: XCTestCase {
    private struct Fixture {
        let a: EngineLibrary
        let b: EngineLibrary
        let model: AppModel
        let agent: AgentController
    }

    private func fixture(photosPerFolder: Int = 1, useModelAgent: Bool = false) throws -> Fixture {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("review-owner-\(UUID().uuidString)")
        let support = root.appendingPathComponent("support")
        let folders = [root.appendingPathComponent("shoot-a"), root.appendingPathComponent("shoot-b")]
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        for (index, folder) in folders.enumerated() {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let width = 160, height = 120
            var pixels = [UInt8](repeating: 255, count: width * height * 4)
            for y in 0..<height {
                for x in 0..<width {
                    let offset = (y * width + x) * 4
                    let value = UInt8(60 + ((x / 8 + y / 8 + index) % 2) * 100)
                    pixels[offset] = value
                    pixels[offset + 1] = value
                    pixels[offset + 2] = value
                }
            }
            let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
            let image = try XCTUnwrap(CGImage(width: width, height: height, bitsPerComponent: 8,
                bitsPerPixel: 32, bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
                folder.appendingPathComponent("photo-\(index).jpg") as CFURL,
                UTType.jpeg.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(destination, image, nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
            for extra in 1..<photosPerFolder {
                try FileManager.default.copyItem(at: folder.appendingPathComponent("photo-\(index).jpg"),
                    to: folder.appendingPathComponent("photo-\(index)-extra-\(extra).jpg"))
            }
        }
        let a = try EngineLibrary.scan(folder: folders[0], appSupport: support)
        let b = try EngineLibrary.scan(folder: folders[1], appSupport: support)
        XCTAssertEqual(a.items.count, photosPerFolder)
        XCTAssertEqual(b.items.count, photosPerFolder)
        XCTAssertNotEqual(a.imageIDs[0], b.imageIDs[0])
        let model = AppModel()
        model.install(a)
        let agent = useModelAgent ? model.agent : AgentController(arguments: ["--fake-planner"])
        agent.provider = .scripted
        agent.preferences = AIPreferences()
        agent.preferences.sceneConsistency = false
        agent.preferences.personConsistency = false
        agent.app = model
        return Fixture(a: a, b: b, model: model, agent: agent)
    }

    private func settle(_ predicate: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Agent operation did not settle", file: file, line: line)
    }

    private func run(_ f: Fixture) async throws -> AgentReviewEntry {
        f.agent.start(itemIDs: [0], provider: .scripted)
        XCTAssertTrue(f.agent.isRunning)
        try await settle { !f.agent.isRunning }
        let entry = try XCTUnwrap(f.agent.queue.entries.first)
        XCTAssertEqual(entry.imageID, f.a.imageIDs[0])
        XCTAssertNil(entry.error)
        XCTAssertNotNil(entry.groupID)
        return entry
    }

    func testRetainedQueueAcceptCannotTrainNewLibraryProfile() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        f.model.install(f.b)
        let before = try f.b.engine.styleProfileStatus(libraryFolder: f.b.folder!.path)
        f.agent.accept(target)
        try await settle { f.agent.busy.isEmpty }
        let after = try f.b.engine.styleProfileStatus(libraryFolder: f.b.folder!.path)
        XCTAssertEqual(after.samples, before.samples, "Old review must not teach the newly opened folder")
        let provenance = try XCTUnwrap(f.a.engine.agentProvenance(imageId: entry.imageID))
        XCTAssertEqual(provenance.item.reviewStatus, "needs review", "Foreign queue action must be rejected")
    }

    func testRetainedQueueRevertCannotRefreshNewLibraryOrMutateOldRecipe() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        let recipe = try f.a.engine.getRecipe(imageId: entry.imageID)
        f.model.install(f.b)
        let revision = f.model.agentRevision
        f.agent.revert(target)
        try await settle { f.agent.busy.isEmpty }
        XCTAssertEqual(f.model.agentRevision, revision, "Old review cannot refresh the new library's item0")
        XCTAssertEqual(try f.a.engine.getRecipe(imageId: entry.imageID), recipe)
    }

    func testAcceptAlreadyInFlightCannotPublishStatusInNewLibrary() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        var completed: Bool?
        f.agent.accept(target) { completed = $0 }
        XCTAssertTrue(f.agent.busy.contains(entry.imageID))
        // The accepted action may finish for A, but its main-actor completion
        // cannot overwrite B's current operation status after this switch.
        f.model.install(f.b)
        f.model.statusMessage = "Current library B"
        try await settle { f.agent.busy.isEmpty }
        XCTAssertEqual(f.model.statusMessage, "Current library B")
        XCTAssertEqual(completed, false)
        let bProfile = try f.b.engine.styleProfileStatus(libraryFolder: f.b.folder!.path)
        XCTAssertEqual(bProfile.samples, 0)
    }

    func testRunCompletionAfterFolderSwitchDoesNotRefreshNewLibrary() async throws {
        let f = try fixture()
        f.agent.start(itemIDs: [0], provider: .scripted)
        XCTAssertTrue(f.agent.isRunning)
        // No suspension: the run's Task has not resumed when the owner changes.
        f.model.install(f.b)
        let revision = f.model.agentRevision
        try await settle { !f.agent.isRunning }
        XCTAssertEqual(f.model.agentRevision, revision, "Completion must resolve/refresh only in its owner library")
        XCTAssertFalse(f.agent.showReview, "Foreign completion must not present its queue over the new library")
        XCTAssertNotNil(try f.a.engine.agentProvenance(imageId: f.a.imageIDs[0]), "Captured run should finish on A")
        XCTAssertNil(try f.b.engine.agentProvenance(imageId: f.b.imageIDs[0]), "No run was requested for B")
    }
    func testForeignRedoAndShowRejectCapturedQueueTarget() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        f.model.install(f.b)
        f.agent.libraryDidUpdate(f.b)
        XCTAssertEqual(f.agent.queue.entries.first?.itemID, entry.itemID)
        XCTAssertNil(f.agent.currentItem(for: target))
        f.agent.redo(target, instruction: "warmer")
        XCTAssertFalse(f.agent.isRunning)
        XCTAssertNil(try f.b.engine.agentProvenance(imageId: f.b.imageIDs[0]))
    }

    func testCurrentOwnerAcceptCompletesAndPreservesQueuePosition() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        let order = f.agent.queue.entries.map(\.imageID)
        var completed: Bool?
        f.agent.accept(target) { completed = $0 }
        try await settle { completed != nil }
        XCTAssertEqual(completed, true)
        XCTAssertEqual(f.agent.queue.entry(entry.imageID)?.status, .accepted)
        XCTAssertEqual(f.agent.queue.entries.map(\.imageID), order)
        XCTAssertEqual(try f.a.engine.styleProfileStatus(libraryFolder: f.a.folder!.path).samples, 1)
    }

    func testRedoInvalidatesOldTargetAndCurrentRevertStillWorks() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let original = try XCTUnwrap(f.agent.queueTarget(entry))
        f.agent.redo(original, instruction: "warmer")
        XCTAssertTrue(f.agent.isRunning)
        try await settle { !f.agent.isRunning }
        XCTAssertNil(f.agent.currentItem(for: original))
        var staleAccepted: Bool?
        f.agent.accept(original) { staleAccepted = $0 }
        XCTAssertEqual(staleAccepted, false)
        let fresh = try XCTUnwrap(f.agent.queue.entries.first)
        let target = try XCTUnwrap(f.agent.queueTarget(fresh))
        XCTAssertNotEqual(fresh.groupID, original.entry.groupID)
        f.agent.revert(target)
        try await settle { f.agent.busy.isEmpty }
        XCTAssertEqual(f.agent.queue.entry(entry.imageID)?.status, .reverted)
        XCTAssertEqual(try f.a.engine.agentProvenance(imageId: entry.imageID)?.item.reviewStatus, "reverted")
    }

    func testCurrentPhotoContextWorksWhileQueueBelongsToAnotherLibrary() async throws {
        let f = try fixture()
        let old = try await run(f)
        f.model.install(f.b)
        let other = AgentController(arguments: ["--fake-planner"])
        other.preferences = f.agent.preferences
        other.app = f.model
        other.start(itemIDs: [0], provider: .scripted)
        try await settle { !other.isRunning }
        let provenance = try XCTUnwrap(f.b.engine.agentProvenance(imageId: f.b.imageIDs[0]))
        let entry = AgentReviewEntry(provenance.item, itemID: 0)
        let target = f.agent.reviewTarget(entry, library: f.b)
        let revision = f.model.agentRevision
        var completed: Bool?
        f.agent.accept(target) { completed = $0 }
        try await settle { completed != nil }
        XCTAssertEqual(completed, true)
        XCTAssertEqual(f.agent.queue.entry(old.imageID)?.status, .needsReview)
        XCTAssertEqual(f.model.agentRevision, revision + 1, "Inspector provenance must reload even when the queue is foreign")
        XCTAssertEqual(try f.b.engine.styleProfileStatus(libraryFolder: f.b.folder!.path).samples, 1)
    }

    func testRunOnAnotherPhotoWaitsForAcceptAndRevertCompletion() async throws {
        let f = try fixture(photosPerFolder: 2)
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        f.agent.accept(target)
        XCTAssertFalse(f.agent.busy.isEmpty)
        XCTAssertNotNil(f.agent.blocker)
        f.agent.start(itemIDs: [1], provider: .scripted)
        XCTAssertFalse(f.agent.isRunning, "An unrelated run must not invalidate an in-flight review action")
        try await settle { f.agent.busy.isEmpty }
        XCTAssertEqual(f.agent.queue.entry(entry.imageID)?.status, .accepted)
        let revision = f.model.agentRevision
        f.agent.revert(target)
        f.agent.start(itemIDs: [1], provider: .scripted)
        XCTAssertFalse(f.agent.isRunning)
        try await settle { f.agent.busy.isEmpty }
        XCTAssertEqual(f.agent.queue.entry(entry.imageID)?.status, .reverted)
        XCTAssertEqual(f.model.agentRevision, revision + 1)
        XCTAssertNil(try f.a.engine.agentProvenance(imageId: f.a.imageIDs[1]))
    }

    func testDirtyDevelopSavePrecedesCapturedRunAfterFolderSwitch() async throws {
        try await assertDirtyDevelopPrecedesRun(closeBeforeStart: false)
    }

    func testAlreadyClosingDevelopSavePrecedesCapturedRun() async throws {
        try await assertDirtyDevelopPrecedesRun(closeBeforeStart: true)
    }

    private func assertDirtyDevelopPrecedesRun(closeBeforeStart: Bool) async throws {
        let f = try fixture()
        f.model.openDevelop(for: f.a.items[0])
        try await settle { f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        controller.onNeedsFlush = {} // Leave a real patch coalesced on the controller.
        controller.set(.exposure, 1.25, interactive: true)
        if closeBeforeStart { f.model.closeDevelop() }
        f.agent.start(itemIDs: [0], instruction: "warmer", provider: .scripted)
        f.model.install(f.b) // Schedules the old controller close without awaiting it.
        try await settle { !f.agent.isRunning }
        await controller.close()
        let recipe = try XCTUnwrap(try JSONSerialization.jsonObject(with:
            Data(f.a.engine.getRecipe(imageId: f.a.imageIDs[0]).utf8)) as? [String: Any])
        let settings = try XCTUnwrap(recipe["settings"] as? [String: Any])
        let tone = try XCTUnwrap(settings["tone"] as? [String: Any])
        XCTAssertEqual((tone["exposure"] as? NSNumber)?.doubleValue, 1.25,
                       "The manual patch must land before the scoped agent edit reads its recipe")
        XCTAssertNotNil(try f.a.engine.agentProvenance(imageId: f.a.imageIDs[0]),
                        "A late develop save must not overwrite the agent group")
        XCTAssertNil(try f.b.engine.agentProvenance(imageId: f.b.imageIDs[0]))
    }

    func testConcurrentCloseWaitsForOneActualSessionFlush() async throws {
        let f = try fixture()
        let session = BlockingCloseSession(try f.a.engine.openDevelopSession(imageId: f.a.imageIDs[0]))
        defer { session.resume.signal() }
        let controller = try DevelopController(session: session, itemID: 0, imageID: f.a.imageIDs[0])
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        var firstDone = false
        var secondDone = false
        let first = Task { await controller.close(); firstDone = true }
        try await settle { session.closeCount == 1 }
        let second = Task { await controller.close(); secondDone = true }
        // The backend's explicit gate, rather than storage speed, holds close pending.
        try await Task.sleep(for: .milliseconds(40))
        XCTAssertFalse(firstDone)
        XCTAssertFalse(secondDone, "Every caller must await the real close completion")
        session.resume.signal()
        await first.value
        await second.value
        await controller.close()
        XCTAssertEqual(session.closeCount, 1)
        let restored = try await DevelopController.open(try XCTUnwrap(f.a.items[0].engineImage), itemID: 0)
        XCTAssertEqual(restored.value(.exposure), 1.25, "Closing must flush pending controller patches")
        await restored.close()
    }

    func testAcceptWaitsForAlreadyClosingDevelopSave() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        f.model.openDevelop(for: f.a.items[0])
        try await settle { f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        f.model.closeDevelop()
        var completed: Bool?
        f.agent.accept(target) { completed = $0 }
        try await settle { completed != nil }
        await controller.close()
        XCTAssertEqual(completed, true)
        XCTAssertEqual(try f.a.engine.agentProvenance(imageId: entry.imageID)?.item.reviewStatus, "accepted")
        let restored = try await DevelopController.open(try XCTUnwrap(f.a.items[0].engineImage), itemID: 0)
        XCTAssertEqual(restored.value(.exposure), 1.25)
        await restored.close()
    }

    func testRevertWaitsForAlreadyClosingDevelopSave() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        f.model.openDevelop(for: f.a.items[0])
        try await settle { f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        f.model.closeDevelop()
        f.agent.revert(target)
        try await settle { f.agent.busy.isEmpty }
        await controller.close()
        XCTAssertEqual(try f.a.engine.agentProvenance(imageId: entry.imageID)?.item.reviewStatus, "reverted")
        let restored = try await DevelopController.open(try XCTUnwrap(f.a.items[0].engineImage), itemID: 0)
        XCTAssertEqual(restored.value(.exposure), 1.25, "Revert keeps the later manual edit")
        await restored.close()
    }

    func testClosePersistsCoalescedMaskParametersAndAmount() async throws {
        let f = try fixture()
        let ref = try XCTUnwrap(f.a.items[0].engineImage)
        let controller = try await DevelopController.open(ref, itemID: 0)
        let group = try XCTUnwrap(controller.beginBrushStroke(group: nil, radius: 0.05,
            feather: 50, flow: 100, erase: false))
        controller.addBrushSample(x: 0.5, y: 0.5, pressure: 1)
        controller.endBrushStroke()
        controller.onNeedsFlush = {}
        controller.setMaskParam(group, "exposure", 0.75, interactive: true)
        controller.updateMaskGroup(group, MaskGroupPatch(name: nil, enabled: nil, amount: 65, invert: nil),
                                   interactive: true)
        XCTAssertTrue(controller.hasPendingMaskChanges)
        await controller.close()
        let restored = try await DevelopController.open(ref, itemID: 0)
        let mask = try XCTUnwrap(restored.maskGroups().first { $0.id == group })
        XCTAssertEqual(mask.amount, 65)
        XCTAssertEqual(mask.params.first { $0.name == "exposure" }?.value, 0.75)
        await restored.close()
    }

    func testPendingDevelopOpenSettlesBeforeRun() async throws {
        let f = try fixture()
        f.model.openDevelop(for: f.a.items[0])
        XCTAssertEqual(f.model.developStatus, .loading)
        f.agent.start(itemIDs: [0], provider: .scripted)
        try await settle { !f.agent.isRunning }
        XCTAssertNotEqual(f.model.developStatus, .loading)
        XCTAssertNil(f.model.develop)
        XCTAssertNotNil(try f.a.engine.agentProvenance(imageId: f.a.imageIDs[0]))
    }

    func testPendingDevelopOpenSettlesBeforeAcceptAndRevert() async throws {
        let f = try fixture()
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        f.model.openDevelop(for: f.a.items[0])
        XCTAssertEqual(f.model.developStatus, .loading)
        var completed: Bool?
        f.agent.accept(target) { completed = $0 }
        try await settle { completed != nil }
        XCTAssertEqual(completed, true)
        XCTAssertNotEqual(f.model.developStatus, .loading)
        XCTAssertNil(f.model.develop)
        f.model.openDevelop(for: f.a.items[0])
        XCTAssertEqual(f.model.developStatus, .loading)
        f.agent.revert(target)
        try await settle { f.agent.busy.isEmpty }
        XCTAssertNotEqual(f.model.developStatus, .loading)
        XCTAssertNil(f.model.develop)
        XCTAssertEqual(try f.a.engine.agentProvenance(imageId: entry.imageID)?.item.reviewStatus, "reverted")
    }

    func testCapturedOwnerBarrierDoesNotCancelNewLibraryPendingOpen() async throws {
        let f = try fixture()
        f.model.openDevelop(for: f.a.items[0])
        f.model.install(f.b)
        f.model.openDevelop(for: f.b.items[0])
        XCTAssertEqual(f.model.developStatus, .loading)
        let barrier = f.model.prepareForAgent(imageIDs: [f.a.imageIDs[0]], library: f.a)
        XCTAssertEqual(f.model.developStatus, .loading, "A's barrier must preserve B's loading state")
        await barrier.value
        try await settle { f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        XCTAssertEqual(controller.imageID, f.b.imageIDs[0])
        f.model.closeDevelop()
        await controller.close()
    }

    func testRunRejectsNewTargetDevelopOpenAndCompletionOpensFreshRecipe() async throws {
        let f = try fixture(useModelAgent: true)
        f.model.viewMode = .loupe
        f.agent.start(itemIDs: [0], provider: .scripted)
        XCTAssertTrue(f.agent.isRunning)
        f.model.openDevelop(for: f.a.items[0])
        XCTAssertNotEqual(f.model.developStatus, .loading, "A new session cannot open across an active agent write")
        XCTAssertNil(f.model.develop)
        try await settle { !f.agent.isRunning && f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        XCTAssertTrue(controller.history.canUndo, "Completion must reopen the agent's committed recipe")
        XCTAssertNotNil(try f.a.engine.agentProvenance(imageId: f.a.imageIDs[0]))
        f.model.closeDevelop()
        await controller.close()
    }

    func testReviewMutationsRejectNewTargetDevelopOpenAndReleaseOnCompletion() async throws {
        let f = try fixture(useModelAgent: true)
        let entry = try await run(f)
        let target = try XCTUnwrap(f.agent.queueTarget(entry))
        f.model.viewMode = .loupe
        f.agent.accept(target)
        f.model.openDevelop(for: f.a.items[0])
        XCTAssertNotEqual(f.model.developStatus, .loading)
        XCTAssertNil(f.model.develop)
        try await settle { f.agent.busy.isEmpty && f.model.develop != nil }
        XCTAssertEqual(try f.a.engine.agentProvenance(imageId: entry.imageID)?.item.reviewStatus, "accepted")
        f.agent.revert(target)
        f.model.openDevelop(for: f.a.items[0])
        XCTAssertNotEqual(f.model.developStatus, .loading)
        XCTAssertNil(f.model.develop)
        try await settle { f.agent.busy.isEmpty && f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        XCTAssertEqual(try controller.session.historyGroups().first { $0.groupId == entry.groupID }?.amount, 0,
                       "Completion must reopen the recipe with the reverted group")
        XCTAssertEqual(try f.a.engine.agentProvenance(imageId: entry.imageID)?.item.reviewStatus, "reverted")
        f.model.closeDevelop()
        await controller.close()
    }

    func testRunningOwnerDoesNotBlockAnotherLibraryDevelopOpen() async throws {
        let f = try fixture(useModelAgent: true)
        f.agent.start(itemIDs: [0], provider: .scripted)
        f.model.install(f.b)
        f.model.openDevelop(for: f.b.items[0])
        XCTAssertEqual(f.model.developStatus, .loading)
        try await settle { f.model.develop != nil && !f.agent.isRunning }
        let controller = try XCTUnwrap(f.model.develop)
        XCTAssertEqual(controller.imageID, f.b.imageIDs[0])
        f.model.closeDevelop()
        await controller.close()
    }

    func testReopenedOwnerAndFileAliasesCannotOpenAcrossCapturedMutation() async throws {
        let f = try fixture(useModelAgent: true)
        let folder = try XCTUnwrap(f.a.folder)
        let root = folder.deletingLastPathComponent()
        let support = root.appendingPathComponent("support")
        let reopened = try EngineLibrary.scan(folder: folder, appSupport: support)
        let symbolicFolder = root.appendingPathComponent("symbolic-shoot")
        try FileManager.default.createSymbolicLink(at: symbolicFolder, withDestinationURL: folder)
        let symbolic = try EngineLibrary.scan(folder: symbolicFolder, appSupport: support)
        let hardFolder = root.appendingPathComponent("hard-linked-shoot")
        try FileManager.default.createDirectory(at: hardFolder, withIntermediateDirectories: true)
        try FileManager.default.linkItem(at: try XCTUnwrap(f.a.items[0].url),
            to: hardFolder.appendingPathComponent("alias.jpg"))
        let hardLinked = try EngineLibrary.scan(folder: hardFolder, appSupport: support)
        f.agent.start(itemIDs: [0], provider: .scripted)
        for other in [reopened, symbolic, hardLinked] {
            let item = try XCTUnwrap(other.items.first)
            let imageID = try XCTUnwrap(item.engineImage?.imageID)
            f.model.install(other)
            XCTAssertTrue(f.agent.isMutating(imageID: imageID, library: other))
            f.model.openDevelop(for: item)
            XCTAssertNotEqual(f.model.developStatus, .loading)
            XCTAssertNil(f.model.develop)
        }
        try await settle { !f.agent.isRunning }
        f.model.install(reopened)
        XCTAssertFalse(f.agent.isMutating(imageID: reopened.imageIDs[0], library: reopened))
        f.model.openDevelop(for: reopened.items[0])
        try await settle { f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        XCTAssertTrue(controller.history.canUndo)
        f.model.closeDevelop()
        await controller.close()
    }

    func testCancelledRunReleasesMutationExclusion() async throws {
        let f = try fixture(useModelAgent: true)
        f.agent.start(itemIDs: [0], provider: .scripted)
        XCTAssertTrue(f.agent.isMutating(imageID: f.a.imageIDs[0], library: f.a))
        f.agent.cancel()
        try await settle { !f.agent.isRunning }
        XCTAssertFalse(f.agent.isMutating(imageID: f.a.imageIDs[0], library: f.a))
        f.model.openDevelop(for: f.a.items[0])
        try await settle { f.model.develop != nil }
        let controller = try XCTUnwrap(f.model.develop)
        f.model.closeDevelop()
        await controller.close()
    }

}

/// Gates only the real backend close operation; settings/history still use the real session.
private final class BlockingCloseSession: DevelopSession, @unchecked Sendable {
    let wrapped: DevelopSession
    let resume = DispatchSemaphore(value: 0)
    private let lock = NSLock()
    private var closes = 0
    var closeCount: Int { lock.withLock { closes } }

    init(_ wrapped: DevelopSession) {
        self.wrapped = wrapped
        super.init(noHandle: .init())
    }
    required init(unsafeFromHandle: UInt64) { fatalError("Use the wrapped real session initializer") }
    override func info() -> DevelopInfo { wrapped.info() }
    override func historyState() throws -> HistoryState { try wrapped.historyState() }
    override func getSettingsJson() throws -> String { try wrapped.getSettingsJson() }
    override func ignoredSettings() throws -> [String] { try wrapped.ignoredSettings() }
    override func setListener(listener: DevelopListener?) { wrapped.setListener(listener: listener) }
    override func setMaskListener(listener: MaskListener?) { wrapped.setMaskListener(listener: listener) }
    override func setSettings(jsonPatch: String, interactive: Bool) throws {
        try wrapped.setSettings(jsonPatch: jsonPatch, interactive: interactive)
    }
    override func close() throws {
        lock.withLock { closes += 1 }
        resume.wait()
        try wrapped.close()
    }
}
