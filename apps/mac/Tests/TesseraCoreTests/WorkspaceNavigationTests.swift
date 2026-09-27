import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

final class WorkspaceSelectionTests: XCTestCase {
    func testRestorationUsesStableKeysAfterInsertionAndRemoval() {
        let saved = WorkspaceSelectionBookmark(order: ["a", "b", "c", "d"], selected: ["b", "c"], focus: "b", anchor: "c")
        let restored = saved.resolve(in: ["new", "a", "c", "d"])
        XCTAssertEqual(restored.selected, [2])
        XCTAssertEqual(restored.focus, 2)
        XCTAssertEqual(restored.anchor, 2)
        XCTAssertTrue(restored.usedFallback)
    }

    func testEmptyDestinationHasNoAccidentalTarget() {
        let saved = WorkspaceSelectionBookmark(order: ["a"], selected: ["a"], focus: "a", anchor: "a")
        XCTAssertNil(saved.resolve(in: []).focus)
        XCTAssertTrue(saved.resolve(in: []).selected.isEmpty)
    }
}

@MainActor
final class WorkspaceNavigationTests: XCTestCase {
    func testUnavailablePhotoExplainsItsActualStateInEdit() throws {
        let model = AppModel()
        model.loadStubItems(count: 2)
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        guard case .unavailable(let reason) = model.developStatus else {
            return XCTFail("A stub is preview-only, never a ready editable photo")
        }
        XCTAssertEqual(model.photoEditAvailabilityHint, reason)
        XCTAssertFalse(model.photoEditAvailabilityHint.contains("(E)"))
    }

    func testEveryExitDisarmsModelOwnedPointerTools() {
        for exit in 0..<4 {
            let model = AppModel()
            model.loadStubItems(count: 12)
            let tools = DevelopTools(model: model)
            let masks = MaskTools(model: model)
            model.enterPhotoEdit()
            tools.hslPicker = .saturation
            tools.detailPicking = true
            masks.tool = .brush
            masks.target = (group: 1, combine: .add)
            switch exit {
            case 0: model.returnToLibrary()
            case 1: model.viewMode = .grid
            case 2: model.viewMode = .document
            default: model.loadStubItems(count: 2)
            }
            XCTAssertNil(tools.hslPicker, "exit \(exit)")
            XCTAssertFalse(tools.detailPicking, "exit \(exit)")
            XCTAssertNil(masks.tool, "exit \(exit)")
            XCTAssertNil(masks.target, "exit \(exit)")
        }
    }

    func testEditTargetsOnePhotoAndReturnsOriginalSelection() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.setSelectionFromUI([2, 3, 4], clicked: 3)
        model.enterPhotoEdit()
        XCTAssertTrue(model.isPhotoEditing)
        XCTAssertEqual(model.editTarget?.id, 3)
        XCTAssertEqual(model.targetIDs, [3])
        XCTAssertEqual(model.workspaceScope, "Preview only · STUB")
        model.select(position: 7)
        model.enterPhotoEdit() // Does not overwrite the return state.
        model.returnToLibrary()
        XCTAssertEqual(model.viewMode, .grid)
        XCTAssertEqual(model.selection, [2, 3, 4])
        XCTAssertEqual(model.focus, 3)
    }

    func testCompareReturnRestoresPairAndActiveSide() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.setSelectionFromUI([2, 4], clicked: 4)
        model.enterCompare()
        let pair = model.compare
        model.enterPhotoEdit()
        model.select(position: 8)
        model.returnToLibrary()
        XCTAssertEqual(model.viewMode, .compare)
        XCTAssertEqual(model.compare, pair)
        model.exitCompare()
        XCTAssertEqual(model.viewMode, .grid)
    }

    func testRenderedCopyRequestCapturesTargetAndCancelHasNoSideEffect() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.select(position: 3)
        model.enterPhotoEdit()
        model.requestLayeredCopy()
        model.select(position: 8)
        XCTAssertEqual(model.layeredCopyRequest?.item.id, 3)
        model.layeredCopyRequest = nil
        XCTAssertTrue(model.documents.documents.isEmpty)
        XCTAssertTrue(model.isPhotoEditing)
    }

    func testChangingSourceExitsEditWithoutRestoringOldSource() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.enterPhotoEdit()
        model.setSource(.decision(.undecided))
        XCTAssertFalse(model.isPhotoEditing)
        XCTAssertEqual(model.source, .decision(.undecided))
        model.returnToLibrary()
        XCTAssertEqual(model.source, .decision(.undecided))
    }

    func testInstallingDifferentLibraryDiscardsBookmark() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.select(position: 9)
        model.enterPhotoEdit()
        model.loadStubItems(count: 2)
        model.returnToLibrary()
        XCTAssertFalse(model.isPhotoEditing)
        XCTAssertEqual(model.focus, 0)
    }

    func testPhotoEditUndoCannotConsumeCullHistory() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.autoAdvance = false
        model.perform(.keep)
        model.enterPhotoEdit()
        model.undo()
        XCTAssertEqual(model.focusedState.decision, .keep)
        XCTAssertEqual(model.undoMenuTitle, "Undo Photo Edit")
        model.returnToLibrary()
        model.undo()
        XCTAssertEqual(model.focusedState.decision, .undecided)
    }

    func testInspectorTabKeepsTargetAndReturnState() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.select(position: 3)
        model.enterPhotoEdit()
        model.photoInspectorTab = .masks
        model.photoInspectorTab = .develop
        XCTAssertEqual(model.editTarget?.id, 3)
        model.returnToLibrary()
        XCTAssertEqual(model.focus, 3)
    }

    func testScopeUsesCompareCandidateRatherThanOriginalSelection() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.setSelectionFromUI([2, 3, 4], clicked: 3)
        XCTAssertEqual(model.workspaceScope, "3 selected · decisions apply to 3 photos")
        model.enterCompare()
        XCTAssertEqual(model.workspaceScope, "Active candidate · decisions apply to 1 photo")
    }
}
