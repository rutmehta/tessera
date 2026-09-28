import XCTest
@testable import Tessera

// UNRUN on B. Models notification-time identity, without native windows or timing.
final class DocumentSaveSheetAttachmentTests: XCTestCase {
    private final class WindowIdentity {
        var attachedSheet: WindowIdentity?
        weak var sheetParent: WindowIdentity?
    }

    func testParentNoLongerAttachesOldSheetDespiteStaleChildParentMetadata() {
        let parent = WindowIdentity(), oldSheet = WindowIdentity()
        parent.attachedSheet = oldSheet
        oldSheet.sheetParent = parent
        parent.attachedSheet = nil // Actual didEndSheet trace state on A.
        XCTAssertTrue(oldSheet.sheetParent === parent)
        XCTAssertTrue(DocumentSaveSheetAttachment.hasDetached(
            capturedSheet: ObjectIdentifier(oldSheet),
            parentAttachedSheet: parent.attachedSheet.map { ObjectIdentifier($0) }))
    }

    func testUnrelatedEventWhileOldSheetStillAttachedDoesNotComplete() {
        let parent = WindowIdentity(), oldSheet = WindowIdentity()
        parent.attachedSheet = oldSheet
        oldSheet.sheetParent = parent
        XCTAssertFalse(DocumentSaveSheetAttachment.hasDetached(
            capturedSheet: ObjectIdentifier(oldSheet),
            parentAttachedSheet: parent.attachedSheet.map { ObjectIdentifier($0) }))
    }

    func testNewSheetIsNotModifiedWhenOldSheetHasDetached() {
        let parent = WindowIdentity(), oldSheet = WindowIdentity(), newerSheet = WindowIdentity()
        parent.attachedSheet = newerSheet
        oldSheet.sheetParent = parent // Stale metadata must not control another sheet.
        XCTAssertTrue(DocumentSaveSheetAttachment.hasDetached(
            capturedSheet: ObjectIdentifier(oldSheet),
            parentAttachedSheet: parent.attachedSheet.map { ObjectIdentifier($0) }))
        XCTAssertTrue(parent.attachedSheet === newerSheet)
        // The production Replace admission separately requires attachedSheet == nil.
    }
}
