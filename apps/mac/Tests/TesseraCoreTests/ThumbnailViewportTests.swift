import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor final class ThumbnailViewportTests: XCTestCase {
    func testHiddenRetainedGridDoesNotContributeViewportBudget() {
        _ = NSApplication.shared
        let model = AppModel()
        model.loadStubItems(count: 40)
        model.viewMode = .grid
        let grid = BrowserController(model: model, style: .grid)
        grid.scrollView.frame = NSRect(x: 0, y: 0, width: 900, height: 700)
        grid.libraryDidReload()
        grid.layout.prepare()
        XCTAssertGreaterThan(model.loader.queueSnapshot.pendingLimit, 0)
        model.viewMode = .loupe
        grid.layout.prepare()
        XCTAssertEqual(model.loader.queueSnapshot.pendingLimit, 0,
                       "opacity-hidden grid is retained, but is not a current viewport")
        grid.stopLoading()
        model.loader.removeAll()
    }
}
