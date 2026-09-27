import Foundation
import XCTest
@testable import TesseraCore

final class BatchSettingsDraftTests: XCTestCase {
    private let sourceSettings = """
    {
      "white_balance": {"mode": "custom", "temperature": 5200, "tint": 7},
      "tone": {"exposure": 0.4, "contrast": 12, "texture": 8, "curves": {"point": "source"}},
      "color": {"vibrance": 15, "saturation": 3, "hsl": {"red": 4}, "grading": {"warm": 6}},
      "detail": {"sharpening": 20},
      "effects": {"grain": 12},
      "geometry": {"crop": {"left": 0.1}, "rotate": 9},
      "masks": [{"id": "person"}]
    }
    """

    private func patch(_ draft: BatchSettingsDraft) throws -> [String: Any] {
        try XCTUnwrap(JSONSerialization.jsonObject(with: Data(draft.settingsJSON.utf8)) as? [String: Any])
    }

    func testReferenceMustBeInExplicitSelection() {
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "other",
                                                    selectedImageIDs: ["a", "b"],
                                                    sourceSettingsJSON: sourceSettings)) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .referenceNotSelected)
        }
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: nil,
                                                    selectedImageIDs: ["a", "b"],
                                                    sourceSettingsJSON: sourceSettings)) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .referenceNotSelected)
        }
    }

    func testTargetsAreFrozenDeduplicatedAndNeverIncludeReference() throws {
        var selected = ["source", "target-b", "target-a", "target-b", "source"]
        let draft = try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                           selectedImageIDs: selected, sourceSettingsJSON: sourceSettings)
        selected = ["source", "new-arrival"]
        XCTAssertEqual(draft.libraryID, "lib-1")
        XCTAssertEqual(draft.sourceImageID, "source")
        XCTAssertEqual(draft.targetImageIDs, ["target-b", "target-a"])
        XCTAssertEqual(selected, ["source", "new-arrival"])
    }

    func testOnlyReferenceOrDuplicatesCannotApply() {
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                                    selectedImageIDs: ["source", "source"],
                                                    sourceSettingsJSON: sourceSettings)) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .noTargets)
        }
    }

    func testDefaultPatchCopiesToneAndColorButExcludesImageSpecificWork() throws {
        let draft = try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                           selectedImageIDs: ["source", "target"],
                                           sourceSettingsJSON: sourceSettings)
        let values = try patch(draft)
        XCTAssertEqual((values["white_balance"] as? [String: Any])?["temperature"] as? Int, 5200)
        XCTAssertEqual((values["tone"] as? [String: Any])?["exposure"] as? Double, 0.4)
        XCTAssertEqual((values["color"] as? [String: Any])?["vibrance"] as? Int, 15)
        XCTAssertNil(values["detail"])
        XCTAssertNil(values["effects"])
        XCTAssertNil(values["geometry"])
        XCTAssertNil(values["masks"])
        XCTAssertFalse(draft.groups.contains(.crop))
    }

    func testChangingGroupsRebuildsPreviewWithoutChangingSourceOrTargets() throws {
        let initial = try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                             selectedImageIDs: ["source", "target"],
                                             sourceSettingsJSON: sourceSettings)
        let detail = try initial.selecting([.detail])
        let values = try patch(detail)
        XCTAssertEqual((values["detail"] as? [String: Any])?["sharpening"] as? Int, 20)
        XCTAssertNil(values["tone"])
        XCTAssertEqual(detail.sourceImageID, "source")
        XCTAssertEqual(detail.targetImageIDs, ["target"])
        XCTAssertNotNil(try patch(initial)["tone"])
    }

    func testPatchLeavesExcludedAndUnselectedNestedTargetValuesIntact() throws {
        let draft = try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                           selectedImageIDs: ["source", "target"],
                                           sourceSettingsJSON: sourceSettings)
        let target: [String: Any] = [
            "tone": ["exposure": -0.2, "localOnly": 93] as [String: Any],
            "color": ["vibrance": -5, "targetOnly": 41] as [String: Any],
            "geometry": ["crop": ["left": 0.8], "rotate": 17] as [String: Any],
            "masks": [["id": "target-person"]],
            "detail": ["sharpening": 3],
        ]
        let merged = DevelopController.merge(target, try patch(draft), keepNulls: false)
        XCTAssertEqual((merged["tone"] as? [String: Any])?["exposure"] as? Double, 0.4)
        XCTAssertEqual((merged["tone"] as? [String: Any])?["localOnly"] as? Int, 93)
        XCTAssertEqual((merged["color"] as? [String: Any])?["vibrance"] as? Int, 15)
        XCTAssertEqual((merged["color"] as? [String: Any])?["targetOnly"] as? Int, 41)
        XCTAssertEqual(((merged["geometry"] as? [String: Any])?["crop"] as? [String: Any])?["left"] as? Double, 0.8)
        XCTAssertEqual((merged["geometry"] as? [String: Any])?["rotate"] as? Int, 17)
        XCTAssertEqual((merged["masks"] as? [[String: String]])?.first?["id"], "target-person")
        XCTAssertEqual((merged["detail"] as? [String: Any])?["sharpening"] as? Int, 3)
    }

    func testCropAndEmptyFieldsAreRejected() throws {
        let draft = try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                           selectedImageIDs: ["source", "target"],
                                           sourceSettingsJSON: sourceSettings)
        XCTAssertThrowsError(try draft.selecting([.crop])) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .unsupportedGroup)
        }
        XCTAssertThrowsError(try draft.selecting([])) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .noSettings)
        }
    }

    func testInvalidSourceSettingsCannotCreateMisleadingPreview() {
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                                    selectedImageIDs: ["source", "target"],
                                                    sourceSettingsJSON: "not JSON")) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .invalidSourceSettings)
        }
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                                    selectedImageIDs: ["source", "target"],
                                                    sourceSettingsJSON: "[]")) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .invalidSourceSettings)
        }
    }

    func testValidSourceWithoutSelectedPathsCannotClaimASettingsCopy() throws {
        for json in ["{}", "{\"geometry\":{\"crop\":{\"left\":0.2}},\"masks\":[{\"id\":\"person\"}]"] {
            XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                                        selectedImageIDs: ["source", "target"],
                                                        sourceSettingsJSON: json)) {
                XCTAssertEqual($0 as? BatchSettingsDraftError, .noSettings)
            }
        }
        let draft = try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                           selectedImageIDs: ["source", "target"],
                                           sourceSettingsJSON: "{\"tone\":{\"exposure\":0.2}}")
        XCTAssertThrowsError(try draft.selecting([.detail])) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .noSettings)
        }
    }

    func testEmptyOpaqueIDsAreRejectedAtDraftBoundary() {
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "", focusedImageID: "source",
                                                    selectedImageIDs: ["source", "target"],
                                                    sourceSettingsJSON: sourceSettings)) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .invalidIdentity)
        }
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "",
                                                    selectedImageIDs: ["", "target"],
                                                    sourceSettingsJSON: sourceSettings)) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .invalidIdentity)
        }
        XCTAssertThrowsError(try BatchSettingsDraft(libraryID: "lib-1", focusedImageID: "source",
                                                    selectedImageIDs: ["source", ""],
                                                    sourceSettingsJSON: sourceSettings)) {
            XCTAssertEqual($0 as? BatchSettingsDraftError, .invalidIdentity)
        }
    }
}
