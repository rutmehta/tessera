import AppKit
import XCTest
import TesseraCore
import TesseraFFI
@testable import Tessera

/// B5-selftest-window: which launches get the background self-test host.
@MainActor
final class SelfTestHostTests: XCTestCase {
    func testLiveDocumentFilterSheetResizeP19() async throws {
        _ = NSApplication.shared
        let model = AppModel()
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("B5-43-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: scratch) }
        let engine = try Engine.open(appSupportDir: scratch.path)
        model.documents.engine = EngineDocumentEngine.for(engine)
        model.documents.newDocument(NewDocumentSettings(width: 512, height: 512, depth: .u8, profile: "sRGB IEC61966-2.1"))
        let doc = try XCTUnwrap(model.documents.current)
        doc.addAdjustment(.exposure)
        let window = SelfTestHost.makeWindow(model: model)
        defer { window.close() }
        window.order(.below, relativeTo: 0)
        try await Task.sleep(for: .seconds(1))
        let failures = await FilterLayoutReproduction.run(model: model, window: window)
        XCTAssertTrue(failures.isEmpty, failures.joined(separator: "; "))
    }

    func testBackgroundHostResizesWithoutClampingToItsInitialContentSize() {
        _ = NSApplication.shared
        let window = SelfTestHost.makeWindow(model: AppModel())
        defer { window.close() }
        XCTAssertFalse(window.canBecomeKey)
        XCTAssertFalse(window.canBecomeMain)
        XCTAssertFalse(window.isVisible)
        // Exercise the real hosting hierarchy in both directions, including the 4K resize.
        // Never order the window, activate the application, or capture the screen.
        for size in [NSSize(width: 2400, height: 1300), NSSize(width: 1440, height: 900),
                     NSSize(width: 2800, height: 1500)] {
            window.setFrame(NSRect(origin: window.frame.origin, size: size), display: true)
            window.contentView?.layoutSubtreeIfNeeded()
            XCTAssertEqual(window.frame.width, size.width, accuracy: 1)
            XCTAssertEqual(window.frame.height, size.height, accuracy: 1)
        }
    }

    func testArgumentFlagsInBothForms() {
        XCTAssertTrue(SelfTestHost.requested(["Tessera", "--vector-selftest=/tmp/x"], environment: [:]))
        XCTAssertTrue(SelfTestHost.requested(["Tessera", "--camera-raw-selftest", "/tmp/x"], environment: [:]))
        XCTAssertTrue(SelfTestHost.requested(["Tessera", "--transform-selftest=/tmp/x"], environment: [:]))
    }

    func testEnvironmentSelfTests() {
        XCTAssertTrue(SelfTestHost.requested(["Tessera"], environment: ["TESSERA_CHANNELS_SELFTEST": "/tmp/x"]))
        XCTAssertTrue(SelfTestHost.requested(["Tessera"], environment: ["TESSERA_STACK_SELFTEST": "/tmp/x"]))
    }

    func testOtherLaunchesAreNotSelfTests() {
        XCTAssertFalse(SelfTestHost.requested(["Tessera", "--nonactivating", "--timing-selftest"], environment: [:]))
        XCTAssertFalse(SelfTestHost.requested(["Tessera", "--vector-selftest-hold", "1"], environment: [:]))
        XCTAssertFalse(SelfTestHost.requested(["Tessera", "--new-document"], environment: ["TESSERA_APP_DIR": "/tmp"]))
    }
}
