import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor
final class WorkspaceBrowserRestoreTests: XCTestCase {
    func testReviewEditRoundTripRestoresNativeAnchorAfterColumnResize() {
        let model = AppModel()
        model.loadStubItems(count: 300)
        model.setSelectionFromUI([1, 2], clicked: 2)
        let browser = BrowserController(model: model, style: .grid)
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 640, height: 400),
                              styleMask: .titled, backing: .buffered, defer: false)
        window.contentView = browser.scrollView
        browser.libraryDidReload()
        browser.scrollView.layoutSubtreeIfNeeded()
        browser.scrollView.contentView.scroll(to: CGPoint(x: 0, y: 800))
        browser.scrollView.reflectScrolledClipView(browser.scrollView.contentView)
        browser.collectionView.layoutSubtreeIfNeeded()
        let before = browser.scrollView.contentView.bounds.origin
        model.enterReview()
        window.setContentSize(NSSize(width: 960, height: 400))
        browser.scrollView.layoutSubtreeIfNeeded()
        model.openReviewPhotoForEditing(250)
        model.returnFromPhotoEdit()
        XCTAssertTrue(model.isReviewing)
        model.returnToLibrary()
        window.setContentSize(NSSize(width: 640, height: 400))
        browser.scrollView.layoutSubtreeIfNeeded()
        XCTAssertEqual(browser.scrollView.contentView.bounds.origin.y, before.y, accuracy: 1)
        XCTAssertEqual(model.selection, [1, 2])
        XCTAssertEqual(model.focus, 2)
        XCTAssertFalse(window.isVisible)
    }

    func testReturnRestoresGridAnchorWithoutFocusSnap() {
        let model = AppModel()
        model.loadStubItems(count: 300)
        let browser = BrowserController(model: model, style: .grid)
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 640, height: 400),
                              styleMask: .titled, backing: .buffered, defer: false)
        window.contentView = browser.scrollView
        browser.libraryDidReload()
        browser.scrollView.layoutSubtreeIfNeeded()
        browser.scrollView.contentView.scroll(to: CGPoint(x: 0, y: 800))
        browser.scrollView.reflectScrolledClipView(browser.scrollView.contentView)
        browser.collectionView.layoutSubtreeIfNeeded()
        let before = browser.scrollView.contentView.bounds.origin
        model.enterPhotoEdit()
        model.select(position: 250)
        XCTAssertEqual(browser.scrollView.contentView.bounds.origin.y, before.y, accuracy: 1)
        // Edit hides the source column. Returning notifies observers before SwiftUI
        // restores its narrower Library viewport on the subsequent native layout pass.
        window.setContentSize(NSSize(width: 960, height: 400))
        browser.scrollView.layoutSubtreeIfNeeded()
        browser.scrollView.contentView.scroll(to: CGPoint(x: 0, y: 1600))
        model.returnToLibrary()
        window.setContentSize(NSSize(width: 640, height: 400))
        browser.scrollView.layoutSubtreeIfNeeded()
        XCTAssertEqual(browser.scrollView.contentView.bounds.origin.y, before.y, accuracy: 1)
        XCTAssertEqual(model.focus, 0)
        // A live reorder ends the retained post-return restore, while the saved
        // entry identity used during Photo Edit remains independent.
        let keys = model.visible.map { model.workspaceKey(for: model.item(id: $0)) }
        var reordered = keys
        reordered.swapAt(0, 1)
        browser.libraryDidUpdate(VisibleChange(oldKeys: keys, newKeys: reordered, changed: [], thumbnails: [], remap: { $0 }))
        browser.scrollView.contentView.scroll(to: CGPoint(x: 0, y: 1200))
        window.setContentSize(NSSize(width: 960, height: 400))
        browser.scrollView.layoutSubtreeIfNeeded()
        window.setContentSize(NSSize(width: 640, height: 400))
        browser.scrollView.layoutSubtreeIfNeeded()
        XCTAssertEqual(browser.scrollView.contentView.bounds.origin.y, 1200, accuracy: 1)
        XCTAssertFalse(window.isVisible)
    }
}
