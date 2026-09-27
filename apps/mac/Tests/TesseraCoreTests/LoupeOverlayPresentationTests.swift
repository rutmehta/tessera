import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor
final class LoupeOverlayPresentationTests: XCTestCase {
    func testProofBadgeReflectsReadyPendingAndUnavailableStates() {
        XCTAssertEqual(
            LoupeOverlay.proofBadgeText(enabled: true, profileName: "Studio Matte", status: "", gamutWarning: true),
            "Proof · Studio Matte · gamut warning"
        )
        XCTAssertEqual(LoupeOverlay.proofHeading(profileName: "Studio Matte"), "Soft proof · Studio Matte")
        XCTAssertEqual(LoupeOverlay.proofHeading(profileName: nil), "Soft proof")
        XCTAssertEqual(
            LoupeOverlay.proofBadgeText(enabled: true, profileName: nil, status: "Preparing proof…", gamutWarning: false),
            "Preparing proof…"
        )
        XCTAssertEqual(
            LoupeOverlay.proofBadgeText(enabled: true, profileName: nil,
                                        status: "No printer profiles installed. Choose one with Other…", gamutWarning: false),
            "No printer profiles installed. Choose one with Other…"
        )
        XCTAssertEqual(
            LoupeOverlay.proofBadgeText(enabled: true, profileName: nil, status: "", gamutWarning: false),
            "Proof unavailable"
        )
        XCTAssertNil(LoupeOverlay.proofBadgeText(enabled: false, profileName: "Studio Matte",
                                                  status: "", gamutWarning: false))
    }

    func testReviewEditShortcutDisclosureNamesQueueAndReturnDestination() {
        let model = AppModel()
        model.loadStubItems(count: 8)
        model.enterReview()
        model.openReviewPhotoForEditing(5)

        XCTAssertTrue(model.isReviewEditing)
        let shortcuts = LoupeOverlay(model: model).shortcutText
        XCTAssertTrue(shortcuts.contains("previous / next review photo"))
        XCTAssertTrue(shortcuts.contains("Esc tool / Back to Review"))
        XCTAssertFalse(shortcuts.contains("Back to Library"))
    }

    func testDisplayInfoCarriesFullLibraryNameAndPhotoEditUsesHeaderInstead() {
        let model = AppModel()
        model.loadStubItems(count: 1)
        let libraryName = LoupeOverlay(model: model).photoNameDisclosure
        XCTAssertEqual(libraryName, model.focusedItem?.name)

        model.enterPhotoEdit()
        XCTAssertNil(LoupeOverlay(model: model).photoNameDisclosure)
    }
}
