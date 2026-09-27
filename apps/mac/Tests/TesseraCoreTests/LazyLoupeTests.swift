import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor
final class LazyLoupeTests: XCTestCase {
    func testBackgroundAuditWindowCannotBecomeKeyOrMain() {
        let panel = BackgroundAuditWindow(model: AppModel())
        XCTAssertFalse(panel.canBecomeKey)
        XCTAssertFalse(panel.canBecomeMain)
        XCTAssertTrue(panel.styleMask.contains(.nonactivatingPanel))
        XCTAssertNotNil(panel.contentView)
        XCTAssertFalse(panel.isVisible, "construction must not order or activate the window")
        XCTAssertEqual(panel.contentView?.bounds.width, CGFloat(1440))
        // ContentView extends into the titlebar; its height includes OS-dependent chrome.
        XCTAssertGreaterThanOrEqual(panel.contentView?.bounds.height ?? 0, CGFloat(900))
    }

    func testLateLoupeObserverSynchronizesExistingSelection() {
        let model = AppModel()
        model.install(StubLibrary.synthetic(count: 1))
        model.viewMode = .loupe
        XCTAssertEqual(model.developStatus, .none)
        let controller = LoupeController(model: model)
        withExtendedLifetime(controller) {
            XCTAssertNotEqual(model.developStatus, .none, "late view must process the already-selected item")
        }
    }

    func testEmptyLoupeConstructionDoesNotPrepareMetal() {
        let view = MetalLoupeView(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
        view.present(image: nil, isFinal: true)
        view.setFrameSize(NSSize(width: 1000, height: 700))
        view.layoutSubtreeIfNeeded()
        XCTAssertFalse(view.preparationStarted)
    }

    func testPreparedResourcesAreSharedAndBuiltOffMain() async throws {
        let preparedFirst = await LoupeRenderer.prepared()
        let preparedSecond = await LoupeRenderer.prepared()
        let first = try XCTUnwrap(preparedFirst)
        let second = try XCTUnwrap(preparedSecond)
        XCTAssertTrue(first === second)
        XCTAssertFalse(first.createdOnMainThread)
    }
}
