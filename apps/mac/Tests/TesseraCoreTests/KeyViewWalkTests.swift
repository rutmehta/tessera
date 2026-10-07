import AppKit
import SwiftUI
import XCTest
@testable import Tessera

/// B5-49d: the walk's bounding and identity logic on scripted trails, with no AppKit involved, so it
/// is checked the same way on a machine with Full Keyboard Access on or off. The first two trails
/// are the ones Machine A reported (real setting on): unidentified SwiftUI `KeyViewProxy` stops
/// between the Layers list and History −, in both pinned variants.
@MainActor
final class KeyViewWalkTests: XCTestCase {
    private final class Fake {}
    private let outline = Fake(), disclosure = Fake(), eye = Fake(), decrease = Fake(), increase = Fake()

    private func stop(_ object: AnyObject?, _ name: String, controlled: Bool = true) -> KeyViewWalk.Stop {
        KeyViewWalk.Stop(object: object, name: name, controlled: controlled)
    }
    private func proxy(_ object: AnyObject? = Fake()) -> KeyViewWalk.Stop { stop(object, "KeyViewProxy", controlled: false) }

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
        let held = Fake()
        let script = (0..<6).map { _ in proxy(held) } + [stop(decrease, "document.history.height.decrease")]
        let (result, _) = walk(script, to: decrease)
        XCTAssertEqual(result.outcome, .reached, "\(result)")
        XCTAssertEqual(result.uncontrolledStops, 6)
    }

    func testProxyStopsThatNeverEndAreBounded() {
        let (result, presses) = walk([proxy(Fake())], limit: 16, to: decrease)
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
        let script = (0..<10).map { stop(Fake(), "button \($0)") }
        let (result, presses) = walk(script, budget: 4, to: decrease)
        XCTAssertEqual(result.outcome, .overBudget, "\(result)")
        XCTAssertEqual(presses, 5, "the fifth controlled stop is over a budget of four")
    }

    func testTheTargetCountsAgainstTheBudget() {
        let script = [stop(disclosure, "a"), stop(eye, "b"), stop(increase, "c"), stop(decrease, "−")]
        XCTAssertEqual(walk(script, budget: 4, to: decrease).0.outcome, .reached)
        XCTAssertEqual(walk(script, budget: 3, to: decrease).0.outcome, .overBudget)
    }

    func testWrongInitialHistoryEntryIsRejectedEvenWithInterveningProxies() {
        let reset = Fake()
        for count in [0, 1, 9, 40] {
            let proxies = (0..<count).map { _ in proxy() }
            for wrong in [increase, reset] {
                let (result, _) = walk([stop(wrong, "wrong History button")] + proxies
                    + [stop(decrease, "−")], to: decrease)
                XCTAssertTrue(result.reached, "the destination alone does not prove order")
                XCTAssertFalse(result.reachesFirst(decrease, before: [increase, reset]), "\(result)")
            }
            let (ordered, _) = walk(proxies + [stop(decrease, "−")], to: decrease)
            XCTAssertTrue(ordered.reachesFirst(decrease, before: [increase, reset]), "\(ordered)")
        }
    }

    func testSelectedRowWalkRejectsOtherRowsAndUncontrolledStops() {
        let list = NSView(), selected = NSView(), other = NSView()
        let eye = NSButton(), disclosure = NSButton(), otherDisclosure = NSButton()
        list.addSubview(selected); list.addSubview(other)
        selected.addSubview(eye); selected.addSubview(disclosure); other.addSubview(otherDisclosure)
        let (valid, _) = walk([stop(disclosure, "selected disclosure"), stop(list, "outline")], to: list)
        // Start belongs to the selected row, just as the hosted reverse walk does.
        func rowWalk(_ middle: KeyViewWalk.Stop, forward: Bool = false) -> KeyViewWalk {
            var stops = [middle, stop(forward ? eye : list, "target")].makeIterator()
            return KeyViewWalk.run(from: stop(forward ? list : eye, "start"), budget: 2,
                                   isTarget: { $0.object === (forward ? eye : list) }) { stops.next() }
        }
        XCTAssertTrue(valid.reached)
        XCTAssertTrue(rowWalk(stop(disclosure, "selected disclosure")).staysInSelectedRow(selected, outline: list))
        XCTAssertFalse(rowWalk(stop(otherDisclosure, "other row disclosure")).staysInSelectedRow(selected, outline: list))
        for forward in [true, false] {
            XCTAssertFalse(rowWalk(proxy(NSView()), forward: forward).staysInSelectedRow(selected, outline: list))
            XCTAssertFalse(rowWalk(proxy(disclosure), forward: forward).staysInSelectedRow(selected, outline: list))
        }
    }

    func testUncontrolledTargetMustFitTheLimit() {
        let target = Fake()
        for count in [63, 64, 65] {
            let script = (0..<(count - 1)).map { _ in proxy() } + [proxy(target)]
            let (result, presses) = walk(script, to: target)
            XCTAssertEqual(result.outcome, count <= 64 ? .reached : .uncontrolledLimit)
            XCTAssertEqual(presses, count)
        }
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

    // MARK: AppKit adapter

    func testStopsAreClassifiedByWhatThePinControls() {
        let button = NSButton(frame: .zero)
        button.setAccessibilityIdentifier("document.layers.row.0.visibility")
        let stop = KeyViewWalk.stop(button, in: nil)
        XCTAssertTrue(stop.controlled && stop.object === button)
        XCTAssertEqual(stop.name, "document.layers.row.0.visibility")
        XCTAssertTrue(KeyViewWalk.isControlled(NSTextField(frame: .zero)))
        XCTAssertTrue(KeyViewWalk.isControlled(NSOutlineView(frame: .zero)))
        XCTAssertTrue(KeyViewWalk.isControlled(HistoryHeightButton(frame: .zero)))
        XCTAssertFalse(KeyViewWalk.isControlled(NSSlider(frame: .zero)), "AppKit sliders follow the real setting")
        XCTAssertFalse(KeyViewWalk.isControlled(NSSegmentedControl(frame: .zero)))
        XCTAssertEqual(KeyViewWalk.stop(nil, in: nil).name, "nil")
    }

    func testProxyNameFallbackFormatWithoutAccessibilityOrWindow() {
        let host = NSView(frame: NSRect(x: 0, y: 0, width: 200, height: 100))
        let proxy = NSView(frame: NSRect(x: 12, y: 23, width: 80, height: 24))
        host.addSubview(proxy)
        proxy.setAccessibilityIdentifier("")
        proxy.setAccessibilityLabel("")
        XCTAssertNil(proxy.window, "no hosting hit test can supply semantics")
        XCTAssertEqual(KeyViewWalk.proxyName(proxy, host: host), "KeyViewProxy(12,23 80×24)")
    }

    func testProxyNamePrefersIdentifierThenLabel() {
        let proxy = NSView(frame: .zero)
        proxy.setAccessibilityLabel("Synthetic action")
        XCTAssertEqual(KeyViewWalk.proxyName(proxy, host: nil), "KeyViewProxy(Synthetic action)")
        proxy.setAccessibilityIdentifier("synthetic.action")
        XCTAssertEqual(KeyViewWalk.proxyName(proxy, host: nil), "KeyViewProxy(synthetic.action)")
    }

    private struct OneButton: View {
        var body: some View {
            Button("Press") {}.frame(width: 80, height: 24).padding(20)
        }
    }

    /// A window hosting one SwiftUI button, and the proxy SwiftUI made for it.
    private func hostedProxy() throws -> (NSWindow, NSView, NSView) {
        let controller = NSHostingController(rootView: LayoutProbeHarness.root(OneButton()))
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 120, height: 64),
                                               styleMask: .titled, backing: .buffered, defer: false)
        window.contentViewController = controller
        window.orderBack(nil)
        LayoutProbeHarness.settle(controller.view, timeout: 5)
        window.recalculateKeyViewLoop()
        func views(_ root: NSView) -> [NSView] { [root] + root.subviews.flatMap { views($0) } }
        let proxy = try XCTUnwrap(views(controller.view).first { KeyViewWalk.isSwiftUIProxy($0) },
                                  "SwiftUI made no KeyViewProxy for a button on this system")
        return (window, controller.view, proxy)
    }

    /// A real SwiftUI proxy is an uncontrolled stop, named by the control it stands for: its
    /// accessibility identity when it has one, else the control's frame in the host.
    func testASwiftUIProxyIsNamedByTheControlItStandsFor() throws {
        let (window, host, proxy) = try hostedProxy()
        defer { LayoutProbeHarness.dispose(window) }
        let stop = KeyViewWalk.stop(proxy, in: host)
        XCTAssertFalse(stop.controlled)
        XCTAssertTrue(stop.object === proxy)
        let frame = proxy.convert(proxy.bounds, to: host)
        let fallback = "KeyViewProxy(\(Int(frame.minX)),\(Int(frame.minY)) \(Int(frame.width))×\(Int(frame.height)))"
        var supported = [fallback]
        func addIdentity(_ element: (any NSAccessibilityProtocol)?) {
            guard let element else { return }
            if let id = element.accessibilityIdentifier(), !id.isEmpty { supported.append("KeyViewProxy(\(id))") }
            if let label = element.accessibilityLabel(), !label.isEmpty { supported.append("KeyViewProxy(\(label))") }
        }
        addIdentity(proxy)
        if let hosting = proxy.superview {
            let centre = proxy.convert(NSPoint(x: proxy.bounds.midX, y: proxy.bounds.midY), to: nil)
            let hit = hosting.accessibilityHitTest(window.convertPoint(toScreen: centre)) as AnyObject?
            if hit !== hosting && hit !== proxy { addIdentity(hit as? any NSAccessibilityProtocol) }
        }
        XCTAssertTrue(supported.contains(stop.name), "proxy must have a supported semantic or frame name: \(stop.name)")
        XCTAssertTrue(frame.width > 0 && frame.height > 0 && host.bounds.contains(frame), "the proxy has its control's frame: \(frame)")
        proxy.setAccessibilityIdentifier("document.layers.add")
        XCTAssertEqual(KeyViewWalk.stop(proxy, in: host).name, "KeyViewProxy(document.layers.add)")
    }

    func testForcedProxiesJoinAndLeaveTheKeyViewLoopAndRestore() throws {
        let (window, _, proxy) = try hostedProxy()
        defer { LayoutProbeHarness.dispose(window) }
        let natural = proxy.acceptsFirstResponder
        XCTAssertEqual(KeyboardAccessHarness.withSwiftUIProxies(focusable: true) { proxy.canBecomeKeyView }, true)
        XCTAssertEqual(KeyboardAccessHarness.withSwiftUIProxies(focusable: false) { proxy.canBecomeKeyView }, false)
        XCTAssertEqual(proxy.acceptsFirstResponder, natural, "the scope restores SwiftUI's own answer")
        XCTAssertFalse(NSView(frame: .zero).acceptsFirstResponder, "only the proxy class is touched")
    }

    /// L3: AppKit may ask the exchanged accessor from any thread; the pin must answer without
    /// touching main-actor state.
    func testPinnedAccessorAnswersOffTheMainThread() {
        nonisolated(unsafe) let application: AnyObject = NSApplication.shared
        for mode in [true, false] {
            KeyboardAccessHarness.withMode(mode) {
                nonisolated(unsafe) var answer: Bool?
                let done = DispatchSemaphore(value: 0)
                Thread.detachNewThread {
                    answer = application.value(forKey: "fullKeyboardAccessEnabled") as? Bool
                    done.signal()
                }
                XCTAssertEqual(done.wait(timeout: .now() + 10), .success)
                XCTAssertEqual(answer, mode)
                XCTAssertEqual(NSApplication.shared.isFullKeyboardAccessEnabled, mode)
            }
        }
    }
}
