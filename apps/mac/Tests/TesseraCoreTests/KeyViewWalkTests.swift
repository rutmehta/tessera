import XCTest
@testable import Tessera

/// B5-49d: the walk's bounding and identity logic on scripted trails, with no AppKit involved, so it
/// is checked the same way on a machine with Full Keyboard Access on or off. The first two trails
/// are the ones Machine A reported (real setting on): unidentified SwiftUI `KeyViewProxy` stops
/// between the Layers list and History −, in both pinned variants.
@MainActor
final class KeyViewWalkTests: XCTestCase {
    private final class View {}
    private let outline = View(), disclosure = View(), eye = View(), decrease = View(), increase = View()

    private func stop(_ object: AnyObject?, _ name: String, controlled: Bool = true) -> KeyViewWalk.Stop {
        KeyViewWalk.Stop(object: object, name: name, controlled: controlled)
    }
    private func proxy(_ object: AnyObject? = View()) -> KeyViewWalk.Stop { stop(object, "KeyViewProxy", controlled: false) }

    /// Replays `script` (repeating its last stop for ever, as a stalled walk would).
    private func walk(_ script: [KeyViewWalk.Stop?], budget: Int = 4, limit: Int = KeyViewWalk.uncontrolledLimit,
                      to target: AnyObject) -> (KeyViewWalk, presses: Int) {
        var index = 0
        let result = KeyViewWalk.run(from: stop(outline, "document.layers.outline"), budget: budget,
                                     uncontrolledLimit: limit, isTarget: { $0.object === target }) {
            defer { index += 1 }
            return script[min(index, script.count - 1)]
        }
        return (result, index)
    }

    func testMachineATrailWithFullKeyboardAccessPinnedOnReachesDecreaseThroughProxies() {
        // [outline, NSButton, row.0.visibility, KeyViewProxy, KeyViewProxy, …] then −.
        for proxies in [2, 9, 40] {
            let script = [stop(disclosure, "NSButton"), stop(eye, "document.layers.row.0.visibility")]
                + (0..<proxies).map { _ in proxy() } + [stop(decrease, "document.history.height.decrease")]
            let (result, presses) = walk(script, to: decrease)
            XCTAssertEqual(result.outcome, .reached, "\(result)")
            XCTAssertEqual(result.controlledStops, 3, "disclosure, eye and −: \(result)")
            XCTAssertEqual(result.uncontrolledStops, proxies)
            XCTAssertEqual(presses, proxies + 3, "the walk stops on the target")
            XCTAssertTrue(result.visited(eye))
        }
    }

    func testMachineATrailWithFullKeyboardAccessPinnedOffReachesDecreaseThroughProxies() {
        // [outline, KeyViewProxy, KeyViewProxy, KeyViewProxy, KeyViewProxy, …] then −.
        for proxies in [4, 9, 40] {
            let script = (0..<proxies).map { _ in proxy() } + [stop(decrease, "document.history.height.decrease")]
            let (result, _) = walk(script, to: decrease)
            XCTAssertEqual(result.outcome, .reached, "\(result)")
            XCTAssertEqual(result.controlledStops, 1, "only −: \(result)")
            XCTAssertEqual(result.uncontrolledStops, proxies)
            XCTAssertFalse(result.visited(eye), "the eye is not a stop with Full Keyboard Access off")
        }
    }

    /// The trail does not say whether consecutive proxy stops are one view or several. One proxy that
    /// keeps the keyboard for several presses (focus moving inside SwiftUI) is not a closed loop.
    func testOneProxyHoldingSeveralPressesIsNotTakenForALoop() {
        let held = View()
        let script = (0..<6).map { _ in proxy(held) } + [stop(decrease, "document.history.height.decrease")]
        let (result, _) = walk(script, to: decrease)
        XCTAssertEqual(result.outcome, .reached, "\(result)")
        XCTAssertEqual(result.uncontrolledStops, 6)
    }

    func testProxyStopsThatNeverEndAreBounded() {
        let (result, presses) = walk([proxy(View())], limit: 16, to: decrease)
        XCTAssertEqual(result.outcome, .uncontrolledLimit, "\(result)")
        XCTAssertEqual(presses, 17, "the walk gives up one press past the limit")
        XCTAssertEqual(result.controlledStops, 0)
    }

    func testAControlledStopComingRoundAgainEndsTheWalkAtOnce() {
        // The list ↔ row cycle B5-49c fixed: list → disclosure → eye → list → …
        let script = [stop(disclosure, "NSButton"), stop(eye, "eye"), stop(outline, "document.layers.outline"),
                      stop(disclosure, "NSButton")]
        let (result, presses) = walk(script, budget: 40, to: decrease)
        XCTAssertEqual(result.outcome, .repeated, "\(result)")
        XCTAssertEqual(presses, 3, "stopped on the first repeat, the start included")
    }

    func testProxiesDoNotHideAClosedLoop() {
        let script = [proxy(), stop(eye, "eye"), proxy(), stop(eye, "eye")]
        let (result, presses) = walk(script, budget: 40, to: decrease)
        XCTAssertEqual(result.outcome, .repeated, "\(result)")
        XCTAssertEqual(presses, 4)
    }

    func testControlledStopsBeyondTheBudgetFail() {
        let script = (0..<10).map { stop(View(), "button \($0)") }
        let (result, presses) = walk(script, budget: 4, to: decrease)
        XCTAssertEqual(result.outcome, .overBudget, "\(result)")
        XCTAssertEqual(presses, 5, "the fifth controlled stop is over a budget of four")
    }

    func testTheTargetCountsAgainstTheBudget() {
        let script = [stop(disclosure, "a"), stop(eye, "b"), stop(increase, "c"), stop(decrease, "−")]
        XCTAssertEqual(walk(script, budget: 4, to: decrease).0.outcome, .reached)
        XCTAssertEqual(walk(script, budget: 3, to: decrease).0.outcome, .overBudget)
    }

    func testAConsumedKeyEndsTheWalk() {
        let (result, presses) = walk([proxy(), nil, stop(decrease, "−")], to: decrease)
        XCTAssertEqual(result.outcome, .consumed, "\(result)")
        XCTAssertEqual(presses, 2)
    }

    func testDescriptionNamesEveryStop() {
        let (result, _) = walk([proxy(), stop(decrease, "document.history.height.decrease")], to: decrease)
        XCTAssertEqual(result.description, "reached after 2 presses (1 controlled, 1 uncontrolled): "
            + "document.layers.outline → KeyViewProxy → document.history.height.decrease")
    }
}
