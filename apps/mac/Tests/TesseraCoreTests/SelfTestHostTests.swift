import AppKit
import XCTest
@testable import Tessera

/// B5-selftest-window: which launches get the background self-test host.
@MainActor
final class SelfTestHostTests: XCTestCase {
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

    func testLibraryDevelopFlagsRequestBackgroundHost() {
        for flag in ["--develop-selftest", "--develop-panels-selftest", "--hdr-selftest", "--masks-selftest"] {
            XCTAssertTrue(SelfTestHost.requested(["Tessera", "--nonactivating", flag], environment: [:]), flag)
            XCTAssertFalse(SelfTestHost.requested(["Tessera", flag + "-hold"], environment: [:]), flag)
            XCTAssertFalse(SelfTestHost.requested(["Tessera", flag + "=/tmp/x"], environment: [:]), flag)
        }
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
