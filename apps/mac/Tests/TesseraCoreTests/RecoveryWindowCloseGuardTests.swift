import AppKit
import XCTest
@testable import Tessera

@MainActor
final class RecoveryWindowCloseGuardTests: XCTestCase {
    private final class PriorDelegate: NSObject, NSWindowDelegate {
        var allow = true
        var closeChecks = 0
        var willCloseCount = 0
        func windowShouldClose(_ sender: NSWindow) -> Bool {
            closeChecks += 1
            return allow
        }
        func windowWillClose(_ notification: Notification) { willCloseCount += 1 }
    }

    private func window() -> NSWindow {
        NSWindow(contentRect: NSRect(x: 50, y: 50, width: 240, height: 180),
                 styleMask: [.titled, .closable], backing: .buffered, defer: false)
    }

    func testBlockedCloseDoesNotCallPriorDelegateOrDiscardWindow() {
        let target = window()
        let prior = PriorDelegate()
        var blockedCount = 0
        let guardDelegate = RecoveryWindowCloseGuard.testing(window: target, previous: prior,
            shouldBlock: { true }, blocked: { _ in blockedCount += 1 })
        target.delegate = guardDelegate

        XCTAssertFalse(guardDelegate.windowShouldClose(target))
        XCTAssertEqual(blockedCount, 1)
        XCTAssertEqual(prior.closeChecks, 0)
        XCTAssertNotNil(target.contentView)
        XCTAssertTrue(target.delegate === guardDelegate)
    }

    func testAllowedCloseDefersToPriorDelegateExactlyOnce() {
        let target = window()
        let prior = PriorDelegate()
        let guardDelegate = RecoveryWindowCloseGuard.testing(window: target, previous: prior,
            shouldBlock: { false }, blocked: { _ in XCTFail("Allowed close must not show recovery") })

        prior.allow = false
        XCTAssertFalse(guardDelegate.windowShouldClose(target))
        XCTAssertEqual(prior.closeChecks, 1)
        prior.allow = true
        XCTAssertTrue(guardDelegate.windowShouldClose(target))
        XCTAssertEqual(prior.closeChecks, 2)
        guardDelegate.windowWillClose(Notification(name: NSWindow.willCloseNotification, object: target))
        XCTAssertEqual(prior.willCloseCount, 1)
    }

    func testInstallingOnOneWindowLeavesUnrelatedWindowDelegateUntouched() {
        let target = window()
        let unrelated = window()
        let targetPrior = PriorDelegate()
        let unrelatedPrior = PriorDelegate()
        target.delegate = targetPrior
        unrelated.delegate = unrelatedPrior

        RecoveryWindowCloseGuard.install(on: target)
        XCTAssertFalse(target.delegate === targetPrior)
        XCTAssertTrue(unrelated.delegate === unrelatedPrior)
        target.close()
    }
}
