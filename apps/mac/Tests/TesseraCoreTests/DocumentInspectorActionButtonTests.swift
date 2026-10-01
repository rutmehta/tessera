import AppKit
import SwiftUI
import XCTest
@testable import Tessera

/// Native control contract tests, NOT proof of reachability in the hosted inspector.
@MainActor
final class DocumentInspectorActionButtonTests: XCTestCase {
    private func event(_ code: UInt16, repeated: Bool = false,
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
            button.keyDown(with: try event(code, repeated: true))
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

    func testDismantleReleasesActionOwnerWhileNativeControlRemainsAlive() {
        let button = DocumentInspectorNativeActionButton(frame: .zero)
        weak var released: NSObject?
        autoreleasepool {
            let owner = NSObject()
            released = owner
            button.configure(title: "Fixture", identifier: "fixture", help: "", enabled: true) {
                _ = owner.description
            }
        }
        let retainedByAction = autoreleasepool { released != nil }
        XCTAssertTrue(retainedByAction, "action owns its current callback context")
        autoreleasepool { DocumentInspectorActionButton.dismantleNSView(button, coordinator: ()) }
        withExtendedLifetime(button) {
            XCTAssertNil(released, "detached representable must drop its action context")
        }
    }

    func testActualColorLookupEditorInstallsNativeActionsAndDisabledReset() async throws {
        let priorPolicy = NSApplication.shared.activationPolicy()
        defer { _ = NSApplication.shared.setActivationPolicy(priorPolicy) }
        LayoutProbeHarness.prepare()
        let workspace = DocumentWorkspace()
        workspace.newDocument(workspace.newSettings)
        let document = try XCTUnwrap(workspace.current)
        document.addAdjustment(.colorLookup)
        let host = NSHostingView(rootView: PropertiesPanel(document: document)
            .frame(width: 288, height: 848, alignment: .topLeading))
        let bounds = NSRect(x: 0, y: 0, width: 288, height: 848)
        let window = LayoutProbeHarness.window(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        window.setContentSize(bounds.size)
        host.frame = bounds
        defer { window.orderOut(nil); window.contentView = nil; window.close() }
        window.orderBack(nil)
        await LayoutProbeHarness.settleAsync(host)
        func actions(_ view: NSView) -> [DocumentInspectorNativeActionButton] {
            if let action = view as? DocumentInspectorNativeActionButton { return [action] }
            return view.subviews.flatMap { actions($0) }
        }
        let buttons = actions(host)
        XCTAssertEqual(buttons.count, 2, "actual editor must use the adapter, not an unused test-only control")
        let load = try XCTUnwrap(buttons.first { $0.accessibilityIdentifier() == "document.properties.colorLookup.load" })
        let reset = try XCTUnwrap(buttons.first { $0.accessibilityIdentifier() == "document.properties.colorLookup.reset" })
        XCTAssertTrue(load.isEnabled)
        XCTAssertFalse(reset.isEnabled, "identity LUT must retain disabled Reset")
        XCTAssertEqual(host.bounds, bounds)
        // Deliberately no focus assignment or reachability assertion. A must
        // qualify Name -> Tab -> action -> Tab in the real owned app.
    }
}
