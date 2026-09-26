import AppKit
import XCTest
@testable import Tessera

@MainActor
final class KeyFocusTests: XCTestCase {
    private func key(_ code: UInt16, _ character: String, window: NSWindow,
                     modifiers: NSEvent.ModifierFlags = []) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0,
                         windowNumber: window.windowNumber, context: nil, characters: character,
                         charactersIgnoringModifiers: character, isARepeat: false, keyCode: code)!
    }

    func testFocusedSliderOwnsArrows() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100),
                              styleMask: .titled, backing: .buffered, defer: false)
        let slider = ValueSlider(frame: window.contentView!.bounds)
        slider.step = 1
        window.contentView?.addSubview(slider)
        XCTAssertTrue(window.makeFirstResponder(slider))
        XCTAssertFalse(KeyRouter(model: AppModel()).handle(key(124, "\u{F703}", window: window)))
        slider.keyDown(with: key(124, "\u{F703}", window: window))
        XCTAssertEqual(slider.doubleValue, 1)
    }

    func testSearchFieldOwnsCullKeys() {
        let model = AppModel()
        model.loadStubItems(count: 10)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100),
                              styleMask: .titled, backing: .buffered, defer: false)
        let field = NSSearchField(frame: window.contentView!.bounds)
        window.contentView?.addSubview(field)
        XCTAssertTrue(window.makeFirstResponder(field))
        XCTAssertFalse(KeyRouter(model: model).handle(key(7, "x", window: window)))
        XCTAssertEqual(model.focusedState.decision, .undecided)
    }

    func testFocusedCurveOwnsArrowsWithoutSelectedPoint() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 300),
                              styleMask: .titled, backing: .buffered, defer: false)
        let curve = CurveEditorView(frame: window.contentView!.bounds)
        curve.mode = .point
        window.contentView?.addSubview(curve)
        XCTAssertTrue(window.makeFirstResponder(curve))
        XCTAssertFalse(KeyRouter(model: AppModel()).handle(key(124, "\u{F703}", window: window)))
    }

    func testUnfocusedLoupeReceivesArrows() {
        let model = AppModel()
        model.loadStubItems(count: 10)
        model.viewMode = .loupe
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100),
                              styleMask: .titled, backing: .buffered, defer: false)
        XCTAssertTrue(KeyRouter(model: model).handle(key(124, "\u{F703}", window: window)))
        XCTAssertNotNil(model.focusedPosition)
    }

    func testSliderModifiersBoundsAndBlurCommit() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100),
                              styleMask: .titled, backing: .buffered, defer: false)
        let slider = ValueSlider(frame: window.contentView!.bounds)
        slider.minValue = -10
        slider.maxValue = 10
        slider.step = 1
        window.contentView?.addSubview(slider)
        var commits = 0
        slider.onChange = { _, final in if final { commits += 1 } }
        XCTAssertTrue(window.makeFirstResponder(slider))
        slider.keyDown(with: key(124, "\u{F703}", window: window, modifiers: .option))
        XCTAssertEqual(slider.doubleValue, 0.1, accuracy: 0.0001)
        slider.keyDown(with: key(124, "\u{F703}", window: window, modifiers: .shift))
        XCTAssertEqual(slider.doubleValue, 10)
        slider.keyDown(with: key(115, "", window: window))
        XCTAssertEqual(slider.doubleValue, -10)
        slider.keyDown(with: key(36, "\r", window: window))
        XCTAssertEqual(commits, 1)
        XCTAssertFalse(window.firstResponder === slider)
    }
}