import XCTest
@testable import Tessera

final class TimingSelfTestLaunchModeTests: XCTestCase {
    func testVisibleTimingSelfTestUsesExplicitMode() {
        XCTAssertEqual(TimingSelfTestLaunchMode.resolve(arguments: [
            "Tessera", "--timing-visible", "--timing-selftest", "--timing-output", "/tmp/trace.json"
        ]), .visible)
        XCTAssertFalse(TimingSelfTestLaunchMode.resolve(arguments: [
            "Tessera", "--timing-visible", "--timing-selftest"
        ]).shouldStartUpdater)
    }

    func testNonactivatingTimingSelfTestRetainsBackgroundMode() {
        XCTAssertEqual(TimingSelfTestLaunchMode.resolve(arguments: [
            "Tessera", "--nonactivating", "--timing-selftest", "--timing-output", "/tmp/trace.json"
        ]), .background)
    }

    func testConflictingAndUnscopedVisibleRequestsFailClosed() {
        XCTAssertEqual(TimingSelfTestLaunchMode.resolve(arguments: [
            "Tessera", "--nonactivating", "--timing-visible", "--timing-selftest"
        ]), .conflictingArguments)
        XCTAssertEqual(TimingSelfTestLaunchMode.resolve(arguments: ["Tessera", "--timing-visible"]),
                       .conflictingArguments)
    }

    func testLegacyTimingSelfTestWithoutHostModeDoesNotChangeBehavior() {
        XCTAssertEqual(TimingSelfTestLaunchMode.resolve(arguments: ["Tessera", "--timing-selftest"]),
                       .notRequested)
        XCTAssertEqual(TimingSelfTestLaunchMode.resolve(arguments: ["Tessera"]), .notRequested)
        XCTAssertTrue(TimingSelfTestLaunchMode.resolve(arguments: ["Tessera"]).shouldStartUpdater)
    }
}
