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

    func testDisplayInfoCarriesFullLibraryNameAndPhotoEditUsesHeaderInstead() {
        let model = AppModel()
        model.loadStubItems(count: 1)
        let libraryName = LoupeOverlay(model: model).photoNameDisclosure
        XCTAssertEqual(libraryName, model.focusedItem?.name)

        model.enterPhotoEdit()
        XCTAssertNil(LoupeOverlay(model: model).photoNameDisclosure)
    }
}
