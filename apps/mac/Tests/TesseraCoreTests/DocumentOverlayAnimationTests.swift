import AppKit
import XCTest
@testable import Tessera

/// Source-only resource repair: UNRUN on Machine B. No sleeps or real timer scheduling.
@MainActor
final class DocumentOverlayAnimationTests: XCTestCase {
    private final class Overlay: DocumentAnimatedOverlayView {
        var eligible = true
        var visible = true
        var ticks = 0
        override var wantsAnimation: Bool { eligible }
        override var animationIsVisible: Bool { visible }
        override func animationTick() { ticks += 1 }
    }

    private func makeOverlay() -> Overlay {
        let view = Overlay(frame: .zero)
        view.makeAnimationTimer = { callback in
            Timer(timeInterval: 1.0 / 30, repeats: true, block: callback)
        }
        return view
    }

    func testRepeatedUpdatesDoNotDuplicateTimerAndRestartOnce() {
        let view = makeOverlay()
        view.updateAnimation()
        let first = view.animationTimer
        XCTAssertNotNil(first)
        view.updateAnimation()
        XCTAssertTrue(first === view.animationTimer)
        first?.fire()
        XCTAssertEqual(view.ticks, 1)
        view.visible = false
        view.updateAnimation()
        XCTAssertNil(view.animationTimer)
        XCTAssertFalse(first!.isValid)
        first?.fire()
        XCTAssertEqual(view.ticks, 1)
        view.visible = true
        view.updateAnimation()
        let second = view.animationTimer
        XCTAssertNotNil(second)
        XCTAssertFalse(first === second)
        view.updateAnimation()
        XCTAssertTrue(second === view.animationTimer)
        view.eligible = false
        view.updateAnimation()
        XCTAssertNil(view.animationTimer)
        XCTAssertFalse(second!.isValid)
    }

    func testTickRechecksVisibilityAndOwnershipBeforeDrawing() {
        let view = makeOverlay()
        view.updateAnimation()
        let timer = view.animationTimer
        view.eligible = false // ownership/selection changes before a lifecycle callback
        timer?.fire()
        XCTAssertEqual(view.ticks, 0)
        XCTAssertFalse(timer!.isValid)
        view.eligible = true
        view.updateAnimation()
        let restarted = view.animationTimer
        view.visible = false
        restarted?.fire()
        XCTAssertEqual(view.ticks, 0)
        XCTAssertFalse(restarted!.isValid)
    }

    func testReleasedOwnerInvalidatesRetainedTimerWithoutTicks() {
        var view: Overlay? = makeOverlay()
        weak var weakView = view
        view?.updateAnimation()
        let timer = view?.animationTimer
        view = nil
        XCTAssertNil(weakView, "timer must not retain the view")
        timer?.fire() // models the run loop's next delivery after owner release
        XCTAssertFalse(timer!.isValid, "nil owner must stop repeating deliveries")
    }

    func testHideUnhideAndWindowDetachHooksReconcileTimer() {
        let view = makeOverlay()
        view.updateAnimation()
        let first = view.animationTimer
        view.visible = false
        view.viewDidHide()
        XCTAssertFalse(first!.isValid)
        XCTAssertNil(view.animationTimer)
        view.visible = true
        view.viewDidUnhide()
        let second = view.animationTimer
        XCTAssertNotNil(second)
        view.visible = false
        view.viewDidMoveToWindow()
        XCTAssertFalse(second!.isValid)
        XCTAssertNil(view.animationTimer)
    }

    func testConcreteOverlaysNeverScheduleWhileDetached() {
        let marquee = MarchingAntsView(frame: .zero)
        marquee.rect = CGRect(x: 0, y: 0, width: 10, height: 10)
        XCTAssertTrue(marquee.wantsAnimation)
        XCTAssertNil(marquee.animationTimer)
        let tools = ToolOverlayView(frame: .zero)
        tools.updateAnimation()
        XCTAssertFalse(tools.wantsAnimation, "an unowned overlay cannot animate global tool state")
        XCTAssertNil(tools.animationTimer)
    }
}
