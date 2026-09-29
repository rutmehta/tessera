import XCTest
@testable import Tessera

/// B5-selftest-window: which launches get the background self-test host.
@MainActor
final class SelfTestHostTests: XCTestCase {
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
