import AppKit
import ImageIO
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
        model.returnFromPhotoEdit()
        model.editReviewedPhoto() // Same native loupe/shown ID must reopen the closed session.
        try await settle { model.developStatus == .ready }
        XCTAssertEqual(model.develop?.imageID, selected.imageID)
        XCTAssertFalse(model.develop === firstController)
        model.returnFromPhotoEdit()
        XCTAssertTrue(model.isReviewing)
        XCTAssertEqual(model.reviewNavigation.instruction, "Keep the sky")
        XCTAssertEqual(model.reviewNavigation.selectedID, selected.imageID)
        model.acceptReviewedPhoto(advance: true)
        try await settle { model.agent.busy.isEmpty }
        XCTAssertEqual(model.agent.queue.entry(selected.imageID)?.status, .accepted)
        XCTAssertEqual(model.reviewNavigation.selectedID, next.imageID)
        XCTAssertFalse(model.reviewNavigation.isDrafting)
        model.returnToLibrary()
        XCTAssertEqual(model.focusedItem?.engineImage?.imageID, original)
        XCTAssertEqual(model.selection, [0])
        XCTAssertEqual(model.source, .decision(.keep))
        XCTAssertEqual(model.viewMode, .grid)
    }

    func testForeignQueueCannotEditAcceptOrAdvanceAndAllReviewedRemainsReachable() async throws {
        let model = try await reviewedModel()
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
        let original = model.reviewNavigation.selectedID
        let owner = try XCTUnwrap(model.engineLibrary)
        let other = try EngineLibrary.scan(folder: try XCTUnwrap(owner.folder), appSupport: try XCTUnwrap(owner.folder).deletingLastPathComponent().appendingPathComponent("support"))
        model.install(other)
        model.enterReview()
        XCTAssertNotNil(model.reviewUnavailableReason)
        XCTAssertNil(model.reviewTargetItem)
        model.editReviewedPhoto()
        model.acceptReviewedPhoto(advance: true)
        XCTAssertTrue(model.isReviewing)
        XCTAssertEqual(model.reviewNavigation.selectedID, original)
        XCTAssertTrue(model.agent.busy.isEmpty)
        XCTAssertTrue(model.targetIDs.isEmpty)
    }

    func testReviewEditKeysAndMenuCannotOpenTargetDuringRunOrAcceptance() async throws {
        let model = try await reviewedModel()
        model.enterReview()
        let entry = try XCTUnwrap(model.selectedReviewEntry)
        let item = try XCTUnwrap(model.reviewTargetItem)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100),
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
        let model = AppModel()
        model.install(try EngineLibrary.scan(folder: folder, appSupport: root.appendingPathComponent("support")))
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
