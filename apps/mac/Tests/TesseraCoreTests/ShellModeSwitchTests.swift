import XCTest
@testable import Tessera
@testable import TesseraCore

/// UX-15 (item 18): the mode control never leaves Deliver (or Cull) drawn while the Edit
/// workspace is still up. Deliver is entered only after the photo's session has been saved and
/// closed through the navigation path.
@MainActor
final class ShellModeSwitchTests: XCTestCase {
    func testDeliverWithAPhotoOpenLeavesEditFirst() {
        let model = AppModel()
        model.loadStubItems(count: 6)
        model.select(position: 1)
        model.selectShellMode(.edit)
        XCTAssertTrue(model.isPhotoEditing)
        XCTAssertEqual(model.shellMode, .edit)
        XCTAssertFalse(model.deliverMode)

        model.selectShellMode(.deliver)
        XCTAssertFalse(model.isPhotoEditing, "the Edit workspace is left before Deliver shows")
        XCTAssertTrue(model.deliverMode)
        XCTAssertEqual(model.shellMode, .deliver)
        XCTAssertNil(model.editTarget)
        // Back to Cull: Deliver clears and the grid is the view again.
        model.selectShellMode(.cull)
        XCTAssertEqual(model.shellMode, .cull)
        XCTAssertFalse(model.deliverMode)
        XCTAssertFalse(model.isPhotoEditing)
    }

    func testDeliverWithoutAPhotoOpenIsImmediate() {
        let model = AppModel()
        model.loadStubItems(count: 6)
        model.selectShellMode(.deliver)
        XCTAssertTrue(model.deliverMode)
        XCTAssertFalse(model.isPhotoEditing)
    }

    func testEditChromeIsGatedOnTheEditMode() {
        // The views draw the Edit panel, the Try cards, the tool strip and the HUDs only while
        // `isPhotoEditing && shellMode == .edit`; `shellMode` is Edit exactly while a photo is
        // being edited, so Deliver's flag can never coincide with the Edit chrome.
        let model = AppModel()
        model.loadStubItems(count: 6)
        model.select(position: 0)
        model.selectShellMode(.edit)
        XCTAssertTrue(model.isPhotoEditing && model.shellMode == .edit)
        model.selectShellMode(.deliver)
        XCTAssertFalse(model.isPhotoEditing && model.shellMode == .edit)
        XCTAssertFalse(model.deliverMode && model.isPhotoEditing, "never both")
    }
}
