import AppKit
import ImageIO
import IOSurface
import UniformTypeIdentifiers
import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor
final class AgentReviewNavigationTests: XCTestCase {
    /// The legacy Review Show path clears filters and selects a different Library row.
    /// The new Review→Edit boundary must leave that return destination intact.
    func testOpeningReviewedPhotoOutsideFilterPreservesLibraryPlace() {
        let model = AppModel()
        model.loadStubItems(count: 10)
        model.autoAdvance = false
        model.setSelectionFromUI([0, 1], clicked: 1)
        model.perform(.keep)
        model.setSource(.decision(.keep))
        model.setSelectionFromUI([0, 1], clicked: 1)
        XCTAssertEqual(model.visibleIDs, [0, 1])
        model.enterReview()
        model.openReviewPhotoForEditing(5)
        XCTAssertEqual(model.source, .decision(.keep), "Review Edit must not redefine the Library source")
        XCTAssertEqual(model.visibleIDs, [0, 1])
        XCTAssertEqual(model.selection, [0, 1])
        XCTAssertEqual(model.focus, 1)
        XCTAssertEqual(model.focusedItem?.id, 5, "The explicit edit target is independent of Library selection")
    }

    func testLegacyPeopleAndTetherInspectionKeepsLibraryLoupe() {
        let model = AppModel()
        model.loadStubItems(count: 8)
        model.showInLoupe(5)
        XCTAssertEqual(model.viewMode, .loupe)
        XCTAssertFalse(model.isPhotoEditing)
        XCTAssertEqual(model.focusedItem?.id, 5)
        model.autoAdvance = false
        model.perform(.keep)
        XCTAssertEqual(model.state(id: 5).decision, .keep)
    }

    func testNewFolderRestoresLibraryInspectorPreferenceWhenLeavingReview() {
        let model = AppModel()
        model.loadStubItems(count: 8)
        model.showInspector = false
        model.enterReview()
        XCTAssertTrue(model.showInspector)
        model.loadStubItems(count: 3)
        XCTAssertFalse(model.showInspector)
        XCTAssertFalse(model.isReviewing)
        XCTAssertFalse(model.isPhotoEditing)
    }

    func testEmptyReviewIsReachableAndDoesNotConsumeLibraryUndoOrSelection() {
        let model = AppModel()
        model.loadStubItems(count: 8)
        model.autoAdvance = false
        model.setSelectionFromUI([1, 2], clicked: 2)
        model.perform(.keep)
        model.showInspector = false
        model.enterReview()
        XCTAssertTrue(model.isReviewing)
        XCTAssertTrue(model.showInspector)
        XCTAssertTrue(model.targetIDs.isEmpty)
        model.perform(.reject)
        model.selectAll()
        model.undo()
        model.redo()
        model.requestLayeredCopy()
        XCTAssertNil(model.layeredCopyRequest)
        XCTAssertEqual(model.selection, [1, 2])
        XCTAssertEqual(model.state(id: 2).decision, .keep)
        XCTAssertTrue(model.undoMenuTitle.contains("Edit photo"))
        model.returnToLibrary()
        XCTAssertEqual(model.viewMode, .grid)
        XCTAssertFalse(model.showInspector)
        XCTAssertEqual(model.selection, [1, 2])
        model.undo()
        XCTAssertEqual(model.state(id: 2).decision, .undecided)
    }

    func testExplicitReviewEditBackUnwindsBeforeLibraryAndSourceChangeClearsIt() {
        let model = AppModel()
        model.loadStubItems(count: 8)
        model.setSelectionFromUI([1, 2], clicked: 2)
        model.enterReview()
        model.openReviewPhotoForEditing(5)
        XCTAssertTrue(model.isReviewEditing)
        XCTAssertEqual(model.targetIDs, [5])
        model.returnFromPhotoEdit()
        XCTAssertTrue(model.isReviewing)
        XCTAssertEqual(model.selection, [1, 2])
        model.returnToLibrary()
        XCTAssertEqual(model.focus, 2)
        model.enterReview()
        model.openReviewPhotoForEditing(5)
        model.setSource(.decision(.keep))
        XCTAssertFalse(model.isReviewEditing)
        XCTAssertFalse(model.isReviewing)
        XCTAssertEqual(model.source, .decision(.keep))
    }

    func testRealQueueFilteredEditRoundTripDraftAndSuccessfulAcceptNext() async throws {
        let model = try await reviewedModel()
        let loupe = LoupeController(model: model)
        defer { withExtendedLifetime(loupe) {} }
        let selected = try XCTUnwrap(model.agent.queue.entries.first)
        let next = try XCTUnwrap(model.agent.queue.next(after: selected.imageID))
        model.autoAdvance = false
        model.setSelectionFromUI([0], clicked: 0)
        model.perform(.keep)
        model.setSource(.decision(.keep))
        let original = model.focusedItem?.engineImage?.imageID
        XCTAssertEqual(model.visibleCount, 1)
        model.enterReview()
        model.selectReviewPhoto(selected.imageID)
        model.reviewNavigation.beginRedo()
        model.reviewNavigation.instruction = "Keep the sky"
        model.editReviewedPhoto()
        XCTAssertTrue(model.isReviewEditing)
        XCTAssertEqual(model.editTarget?.engineImage?.imageID, selected.imageID)
        XCTAssertEqual(model.source, .decision(.keep))
        XCTAssertEqual(model.visibleCount, 1)
        try await settle { model.developStatus == .ready }
        let firstController = try XCTUnwrap(model.develop)
        XCTAssertEqual(firstController.imageID, selected.imageID)
        firstController.onNeedsFlush = {}
        firstController.set(.exposure, 1.25, interactive: true)
        model.returnFromPhotoEdit()
        try await settle { model.isReviewing && model.develop == nil }
        let previewBarrier = model.pendingDevelopSaveBarrier(imageID: selected.imageID, library: try XCTUnwrap(model.engineLibrary))
        guard case .saved = await previewBarrier.result() else {
            XCTFail("The settled photo edit should admit a preview read")
            previewBarrier.finish()
            return
        }
        previewBarrier.finish()
        model.editReviewedPhoto() // Same native loupe/shown ID must reopen the closed session.
        try await settle { model.developStatus == .ready }
        XCTAssertEqual(model.develop?.imageID, selected.imageID)
        XCTAssertFalse(model.develop === firstController)
        XCTAssertEqual(model.develop?.value(.exposure), 1.25, "Preview/edit return must await the pending manual save")
        model.returnFromPhotoEdit()
        try await settle { model.isReviewing && model.develop == nil }
        XCTAssertTrue(model.isReviewing)
        XCTAssertEqual(model.reviewNavigation.instruction, "Keep the sky")
        XCTAssertEqual(model.reviewNavigation.selectedID, selected.imageID)
        model.acceptReviewedPhoto(advance: true)
        try await settle { model.agent.busy.isEmpty }
        XCTAssertEqual(model.agent.queue.entry(selected.imageID)?.status, .accepted)
        XCTAssertEqual(model.reviewNavigation.selectedID, next.imageID)
        XCTAssertFalse(model.reviewNavigation.isDrafting)
        model.returnToLibrary()
        try await settle { !model.isReviewing && !model.isPhotoEditing && model.viewMode == .grid }
        XCTAssertEqual(model.focusedItem?.engineImage?.imageID, original)
        XCTAssertEqual(model.selection, [0])
        XCTAssertEqual(model.source, .decision(.keep))
        XCTAssertEqual(model.viewMode, .grid)
    }

    func testSamePathLibraryReopenRehydratesAllReviewedQueueAndRejectsStaleTarget() async throws {
        let model = try await reviewedModel()
        let oldEntry = try XCTUnwrap(model.agent.queue.entries.first)
        let staleTarget = try XCTUnwrap(model.agent.queueTarget(oldEntry))
        model.enterReview()
        for _ in 0..<model.agent.queue.count {
            model.acceptReviewedPhoto(advance: true)
            try await settle { model.agent.busy.isEmpty }
        }
        XCTAssertEqual(model.agent.queue.pendingCount, 0)
        model.returnToLibrary()
        model.enterReview()
        XCTAssertTrue(model.isReviewing)
        XCTAssertNotNil(model.selectedReviewEntry)
        let owner = try XCTUnwrap(model.engineLibrary)
        let folder = try XCTUnwrap(owner.folder)
        let support = folder.deletingLastPathComponent().appendingPathComponent("support")
        let reopened = try EngineLibrary.scan(folder: folder, appSupport: support)
        model.install(reopened)

        XCTAssertTrue(model.agent.reviewLibrary === reopened)
        XCTAssertEqual(model.agent.queue.count, 2)
        XCTAssertTrue(model.agent.queue.entries.allSatisfy { $0.status == .accepted })
        XCTAssertEqual(model.agent.queue.pendingCount, 0)
        XCTAssertEqual(model.viewMode, .grid, "a library reopen does not force Review navigation")
        XCTAssertNil(model.agent.currentItem(for: staleTarget), "rebind invalidates captured runtime targets")
        model.enterReview()
        XCTAssertTrue(model.isReviewing)
        XCTAssertNotNil(model.selectedReviewEntry, "all-reviewed queues remain reachable after restoration")
        XCTAssertNotNil(model.reviewTargetItem)
        var acceptedStaleTarget: Bool?
        model.agent.accept(staleTarget) { acceptedStaleTarget = $0 }
        model.agent.revert(staleTarget)
        model.agent.redo(staleTarget, instruction: "warmer")
        XCTAssertEqual(acceptedStaleTarget, false)
        XCTAssertFalse(model.agent.isRunning)
        XCTAssertTrue(model.agent.busy.isEmpty)
    }

    func testForeignFolderRejectsCapturedReviewTarget() async throws {
        let model = try await reviewedModel()
        let entry = try XCTUnwrap(model.agent.queue.entries.first)
        let staleTarget = try XCTUnwrap(model.agent.queueTarget(entry))
        let owner = try XCTUnwrap(model.engineLibrary)
        let originalFolder = try XCTUnwrap(owner.folder)
        let root = originalFolder.deletingLastPathComponent()
        let foreignFolder = root.appendingPathComponent("foreign-photos")
        try FileManager.default.createDirectory(at: foreignFolder, withIntermediateDirectories: true)
        let source = try XCTUnwrap(owner.items.first?.url)
        try FileManager.default.copyItem(at: source, to: foreignFolder.appendingPathComponent("photo-0.jpg"))
        let foreign = try EngineLibrary.scan(folder: foreignFolder,
                                             appSupport: root.appendingPathComponent("support"))
        model.install(foreign)

        XCTAssertNil(model.agent.currentItem(for: staleTarget))
        XCTAssertTrue(model.agent.queue.isEmpty)
        var acceptedStaleTarget: Bool?
        model.agent.accept(staleTarget) { acceptedStaleTarget = $0 }
        model.agent.revert(staleTarget)
        model.agent.redo(staleTarget, instruction: "warmer")
        XCTAssertEqual(acceptedStaleTarget, false)
        XCTAssertFalse(model.agent.isRunning)
        XCTAssertTrue(model.agent.busy.isEmpty)
        XCTAssertNil(try foreign.engine.agentProvenance(imageId: foreign.imageIDs[0]))
    }

    func testReviewEditKeysAndMenuCannotOpenTargetDuringRunOrAcceptance() async throws {
        let model = try await reviewedModel()
        model.enterReview()
        let entry = try XCTUnwrap(model.selectedReviewEntry)
        let item = try XCTUnwrap(model.reviewTargetItem)
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100),
                              styleMask: .titled, backing: .buffered, defer: false)
        let router = KeyRouter(model: model)
        func assertEditRejected(file: StaticString = #filePath, line: UInt = #line) {
            XCTAssertFalse(model.canEnterPhotoEdit, file: file, line: line)
            model.enterPhotoEdit() // Toolbar, header and menu entry.
            model.editReviewedPhoto()
            model.openReviewPhotoForEditing(item.id) // Review’s explicit-target entry.
            for (code, character) in [(UInt16(2), "d"), (UInt16(14), "e")] {
                let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, characters: character,
                    charactersIgnoringModifiers: character, isARepeat: false, keyCode: code)!
                XCTAssertTrue(router.handle(event), file: file, line: line)
            }
            XCTAssertTrue(model.isReviewing, file: file, line: line)
            XCTAssertNil(model.develop, file: file, line: line)
        }
        model.agent.start(itemIDs: [item.id], provider: .scripted)
        XCTAssertTrue(model.agent.isRunning)
        assertEditRejected()
        try await settle { !model.agent.isRunning }
        model.reconcileReviewNavigation()
        model.selectReviewPhoto(entry.imageID)
        model.acceptReviewedPhoto(advance: false)
        XCTAssertFalse(model.agent.busy.isEmpty)
        assertEditRejected()
        try await settle { model.agent.busy.isEmpty }
        XCTAssertTrue(model.canEnterPhotoEdit)
    }

    func testFirstLayeredCopyIncludesPendingPhotoAdjustment() async throws {
        let model = try await reviewedModel()
        model.select(position: 0) // This photo was not part of the scripted run.
        let item = try XCTUnwrap(model.focusedItem)
        let owner = try XCTUnwrap(model.engineLibrary)
        let imageID = try XCTUnwrap(item.engineImage?.imageID)
        let baseline = try EngineDocumentEngine.for(owner.engine).openDocumentFromImage(imageId: imageID, developed: false)
        let before = try meanRGB(baseline)
        model.enterPhotoEdit()
        model.openDevelop(for: item)
        try await settle { model.develop != nil }
        let controller = try XCTUnwrap(model.develop)
        controller.onNeedsFlush = {}
        controller.set(.exposure, 1.25, interactive: true)
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        try await settle { model.viewMode == .document }
        let document = try XCTUnwrap(model.documents.current)
        XCTAssertEqual(model.viewMode, .document)
        let firstPixels = try meanRGB(document.backend)
        XCTAssertGreaterThan(firstPixels, before + 10,
                             "The first rendered copy must include the pending brightening, not the previously saved pixels")
        await controller.close()
        model.viewMode = .grid
        model.enterPhotoEdit()
        model.openDevelop(for: item)
        try await settle { model.develop != nil }
        let later = try XCTUnwrap(model.develop)
        later.onNeedsFlush = {}
        later.set(.exposure, -1.0, interactive: true)
        model.requestLayeredCopy()
        model.createRequestedLayeredCopy()
        try await settle { model.viewMode == .document }
        XCTAssertEqual(model.documents.current?.backend.id(), document.backend.id())
        XCTAssertEqual(try meanRGB(try XCTUnwrap(model.documents.current).backend), firstPixels,
                       "The disclosure promises that an existing layered copy reopens without refreshing its pixels")
        await later.close()
    }

    func testLayeredHandoffCancelsWhenSelectionOrOwnerChangesBeforeSaveWaitReturns() async throws {
        for (changeOwner, laterStatus) in [(false, false), (true, false), (false, true)] {
            let model = try await reviewedModel()
            model.select(position: 0)
            let item = try XCTUnwrap(model.focusedItem)
            let owner = try XCTUnwrap(model.engineLibrary)
            model.enterPhotoEdit()
            model.openDevelop(for: item)
            try await settle { model.develop != nil }
            let controller = try XCTUnwrap(model.develop)
            controller.onNeedsFlush = {}
            controller.set(.exposure, 1.25, interactive: true)
            model.requestLayeredCopy()
            model.createRequestedLayeredCopy()
            if changeOwner { model.loadStubItems(count: 2) }
            else { model.select(position: 1) }
            if laterStatus { model.statusMessage = "A newer operation completed" }
            let gate = model.pendingDevelopSaveBarrier(
                imageID: try XCTUnwrap(item.engineImage?.imageID), library: owner)
            _ = await gate.result()
            gate.finish()
            // Drain the handoff continuation after its captured barrier settles.
            try await Task.sleep(for: .milliseconds(40))
            XCTAssertTrue(model.documents.documents.isEmpty)
            XCTAssertNotEqual(model.viewMode, .document)
            if laterStatus { XCTAssertEqual(model.statusMessage, "A newer operation completed") }
            else { XCTAssertNotEqual(model.statusMessage, "Saving photo before opening Layers…") }
            await controller.close()
        }
    }

    private func meanRGB(_ backend: any DocumentBackend) throws -> Double {
        let surface = try XCTUnwrap(IOSurfaceLookup(try backend.compositeThumbnail(maxPx: 120)))
        XCTAssertEqual(IOSurfaceGetBytesPerElement(surface), 4)
        IOSurfaceLock(surface, .readOnly, nil)
        defer { IOSurfaceUnlock(surface, .readOnly, nil) }
        let bytes = try XCTUnwrap(IOSurfaceGetBaseAddress(surface)).assumingMemoryBound(to: UInt8.self)
        let width = IOSurfaceGetWidth(surface), height = IOSurfaceGetHeight(surface), stride = IOSurfaceGetBytesPerRow(surface)
        var sum = 0.0
        for y in 0..<height {
            for x in 0..<width {
                let offset = y * stride + x * 4
                sum += Double(bytes[offset]) + Double(bytes[offset + 1]) + Double(bytes[offset + 2])
            }
        }
        return sum / Double(width * height * 3)
    }

    private func reviewedModel() async throws -> AppModel {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("review-navigation-\(UUID())")
        let folder = root.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let width = 120, height = 80
        for index in 0..<3 {
            var pixels = [UInt8](repeating: 255, count: width * height * 4)
            for y in 0..<height {
                for x in 0..<width {
                    let value = UInt8(60 + ((x / 8 + y / 8 + index) % 2) * 100)
                    for channel in 0..<3 { pixels[(y * width + x) * 4 + channel] = value }
                }
            }
            let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
            let image = try XCTUnwrap(CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider,
                decode: nil, shouldInterpolate: false, intent: .defaultIntent))
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(folder.appendingPathComponent("photo-\(index).jpg") as CFURL,
                UTType.jpeg.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(destination, image, nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
        }
        let support = root.appendingPathComponent("support")
        let agent = AgentController(arguments: ["--fake-planner"], supportDirectory: support)
        let model = AppModel(agent: agent)
        model.install(try EngineLibrary.scan(folder: folder, appSupport: support))
        let saved = model.agent.preferences
        addTeardownBlock { await MainActor.run { model.agent.preferences = saved } }
        model.agent.preferences.sceneConsistency = false
        model.agent.preferences.personConsistency = false
        model.agent.start(itemIDs: [1, 2], provider: .scripted)
        try await settle { !model.agent.isRunning }
        XCTAssertEqual(model.agent.queue.count, 2)
        XCTAssertTrue(model.agent.queue.entries.allSatisfy { $0.error == nil && $0.groupID != nil })
        XCTAssertEqual(model.viewMode, .grid, "Completion must not navigate or open a modal")
        return model
    }

    private func settle(_ predicate: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Operation did not settle", file: file, line: line)
    }
}
