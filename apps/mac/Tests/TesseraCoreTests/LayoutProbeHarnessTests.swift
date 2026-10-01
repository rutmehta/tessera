import AppKit
import XCTest

@MainActor
final class LayoutProbeHarnessTests: XCTestCase {
    func testWaitsForDeferredGeometryAndFlushesItsLayout() {
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 300, height: 200))
        LayoutProbeHarness.prepare()
        let window = LayoutProbeHarness.window(contentRect: root.bounds, styleMask: .titled, backing: .buffered, defer: false)
        XCTAssertEqual(window.animationBehavior, .none)
        window.isReleasedWhenClosed = false
        window.contentView = root
        window.orderBack(nil)
        defer { LayoutProbeHarness.dispose(window) }
        let child = NSView(frame: NSRect(x: 0, y: 0, width: 20, height: 20))
        root.addSubview(child)
        var delivered = false
        let timer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: false) { _ in
            MainActor.assumeIsolated {
                child.frame.origin.x = 80
                root.needsLayout = true
                delivered = true
            }
        }
        defer { timer.invalidate() }
        XCTAssertTrue(LayoutProbeHarness.settle(root))
        XCTAssertTrue(delivered, "two early equal frames must not beat the next layout transaction")
        XCTAssertEqual(child.frame.minX, 80)
        XCTAssertFalse(root.needsLayout)
    }

    func testUnstableMeasurementFailsAtTheBound() {
        var sample = 0
        XCTExpectFailure("A continuously changing probe must fail instead of accepting transient geometry") {
            XCTAssertFalse(LayoutProbeHarness.settle(nil, timeout: 0.03, measurement: {
                sample += 1
                return [CGRect(x: sample, y: 0, width: 1, height: 1)]
            }))
        }
        XCTAssertGreaterThan(sample, 1)
    }
}
