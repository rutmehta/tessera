import AppKit
import XCTest
@testable import Tessera

/// Native control contract tests, NOT proof of reachability in the hosted inspector.
@MainActor
final class DocumentInspectorActionButtonTests: XCTestCase {
    private func event(_ code: UInt16, repeat repeated: Bool = false,
                       modifiers: NSEvent.ModifierFlags = []) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: 0, windowNumber: 0, context: nil, characters: "", charactersIgnoringModifiers: "",
            isARepeat: repeated, keyCode: code))
    }

    func testNativeActionHasExplicitKeyboardOwnershipAndPreservesMetadata() {
        let button = DocumentInspectorNativeActionButton(frame: .zero)
        button.configure(title: "Load 3D LUT…", identifier: "document.properties.colorLookup.load",
                         help: "Load a LUT", enabled: true, action: {})
        let responder: NSResponder = button
        XCTAssertTrue(responder is KeyOwningControl, "focused action must use the existing ownership guard")
        XCTAssertEqual(button.title, "Load 3D LUT…")
        XCTAssertEqual(button.accessibilityIdentifier(), "document.properties.colorLookup.load")
        XCTAssertEqual(button.accessibilityLabel(), "Load 3D LUT…")
        XCTAssertEqual(button.toolTip, "Load a LUT")
        XCTAssertEqual(button.keyEquivalent, "", "no new global/default key equivalent")
        XCTAssertEqual(button.intrinsicContentSize.height, Theme.Height.small)
        XCTAssertEqual(button.font, Theme.NSFonts.caption)
    }

    func testSpaceReturnAndKeypadActivateOnceWithoutRepeat() throws {
        _ = NSApplication.shared
        let button = DocumentInspectorNativeActionButton(frame: .zero)
        var actions = 0
        button.configure(title: "Safe fixture", identifier: "fixture", help: "", enabled: true) { actions += 1 }
        for code in [UInt16(49), 36, 76] {
            let before = actions
            button.keyDown(with: try event(code))
            XCTAssertEqual(actions, before + 1)
            button.keyDown(with: try event(code, repeat: true))
            XCTAssertEqual(actions, before + 1)
        }
    }

    func testReuseRefreshesActionAndDisabledOrDismantledCannotInvoke() {
        let button = DocumentInspectorNativeActionButton(frame: .zero)
        var old = 0, current = 0
        button.configure(title: "Old", identifier: "old", help: "old", enabled: true) { old += 1 }
        button.configure(title: "Reset", identifier: "document.properties.colorLookup.reset",
                         help: "Back to identity", enabled: true) { current += 1 }
        _ = button.accessibilityPerformPress()
        XCTAssertEqual(old, 0)
        XCTAssertEqual(current, 1)
        XCTAssertEqual(button.accessibilityLabel(), "Reset")
        button.configure(title: "Reset", identifier: "document.properties.colorLookup.reset",
                         help: "Back to identity", enabled: false) { current += 1 }
        XCTAssertFalse(button.isEnabled)
        XCTAssertFalse(button.acceptsFirstResponder)
        button.performClick(nil)
        XCTAssertEqual(current, 1)
        button.configure(title: "Reset", identifier: "document.properties.colorLookup.reset",
                         help: "Back to identity", enabled: true) { current += 1 }
        DocumentInspectorActionButton.dismantleNSView(button, coordinator: ())
        button.performClick(nil)
        XCTAssertEqual(current, 1)
    }

    func testOnlyUnmodifiedNativeActivationKeysAreOwnedLocally() {
        for code in [UInt16(49), 36, 76] {
            XCTAssertTrue(DocumentInspectorNativeActionButton.isActivationKey(code, modifiers: []))
            XCTAssertTrue(DocumentInspectorNativeActionButton.isActivationKey(code, modifiers: .shift))
            for modifier in [NSEvent.ModifierFlags.command, .control, .option] {
                XCTAssertFalse(DocumentInspectorNativeActionButton.isActivationKey(code, modifiers: modifier))
            }
        }
        for code in [UInt16(48), 53, 0, 9, 123, 124] {
            XCTAssertFalse(DocumentInspectorNativeActionButton.isActivationKey(code, modifiers: []),
                           "Tab/ShiftTab and other keys must remain native responder processing")
        }
    }
}
