import AppKit
import SwiftUI
import XCTest
@testable import Tessera

/// Local native contracts and real editor installation; not proof of actual Tab reachability.
@MainActor
final class DocumentDitherCheckboxTests: XCTestCase {
    private func space(repeated: Bool = false) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
            timestamp: 0, windowNumber: 0, context: nil, characters: " ", charactersIgnoringModifiers: " ",
            isARepeat: repeated, keyCode: 49))
    }

    func testNativeCheckboxMetadataValueAndNativeFocusPolicy() {
        let box = DocumentDitherNativeCheckbox(frame: .zero)
        box.configure(isOn: true, enabled: true) { _ in }
        let responder: NSResponder = box
        XCTAssertTrue(responder is KeyOwningControl)
        XCTAssertEqual(box.accessibilityRole(), .checkBox)
        XCTAssertEqual(box.accessibilityLabel(), "Dither")
        XCTAssertEqual(box.accessibilityIdentifier(), "document.properties.colorLookup.dither")
        XCTAssertEqual(box.accessibilityValue() as? NSNumber, NSNumber(value: 1))
        XCTAssertEqual(box.toolTip, "Add fine noise so smooth gradients do not band")
        XCTAssertEqual(box.font, Theme.NSFonts.caption)
        XCTAssertEqual(box.keyEquivalent, "")
        let native = NSButton(checkboxWithTitle: "Dither", target: nil, action: nil)
        native.controlSize = .small
        XCTAssertEqual(box.acceptsFirstResponder, native.acceptsFirstResponder,
                       "retain native keyboard policy without setting global preferences")
        box.configure(isOn: false, enabled: false) { _ in }
        XCTAssertFalse(box.acceptsFirstResponder)
        XCTAssertEqual(box.accessibilityValue() as? NSNumber, NSNumber(value: 0))
    }

    func testSpaceTogglesSynchronouslyOnceAndRepeatDoesNotToggle() throws {
        _ = NSApplication.shared
        let box = DocumentDitherNativeCheckbox(frame: .zero)
        var values: [Bool] = []
        box.configure(isOn: true, enabled: true) { values.append($0) }
        box.keyDown(with: try space())
        XCTAssertEqual(values, [false])
        XCTAssertEqual(box.state, .off)
        box.keyDown(with: try space(repeated: true))
        XCTAssertEqual(values, [false])
        box.keyDown(with: try space())
        XCTAssertEqual(values, [false, true])
    }

    func testRefreshDoesNotPublishAndUsesLatestCallbackThenDismantles() {
        let box = DocumentDitherNativeCheckbox(frame: .zero)
        var old = 0
        var current: [Bool] = []
        box.configure(isOn: false, enabled: true) { _ in old += 1 }
        box.configure(isOn: true, enabled: true) { current.append($0) }
        XCTAssertEqual(old, 0)
        XCTAssertTrue(current.isEmpty, "model update must not publish an edit")
        _ = box.accessibilityPerformPress()
        XCTAssertEqual(old, 0)
        XCTAssertEqual(current, [false])
        box.configure(isOn: true, enabled: false) { current.append($0) }
        box.performClick(nil)
        XCTAssertEqual(current, [false])
        box.configure(isOn: true, enabled: true) { current.append($0) }
        DocumentDitherCheckbox.dismantleNSView(box, coordinator: ())
        box.performClick(nil)
        XCTAssertEqual(current, [false])
    }

    func testOnlySpaceIsLocallyActivatedNotReturnOrTraversal() {
        XCTAssertTrue(DocumentDitherNativeCheckbox.isToggleKey(49, modifiers: []))
        for flags in [NSEvent.ModifierFlags.command, .control, .option] {
            XCTAssertFalse(DocumentDitherNativeCheckbox.isToggleKey(49, modifiers: flags))
        }
        for code in [UInt16(48), 36, 76, 53, 123, 124] {
            XCTAssertFalse(DocumentDitherNativeCheckbox.isToggleKey(code, modifiers: []))
            XCTAssertFalse(DocumentDitherNativeCheckbox.isToggleKey(code, modifiers: .shift))
        }
    }

    func testDismantleReleasesCallbackOwnerWithControlAlive() {
        let box = DocumentDitherNativeCheckbox(frame: .zero)
        weak var released: NSObject?
        autoreleasepool {
            let owner = NSObject()
            released = owner
            box.configure(isOn: true, enabled: true) { _ in _ = owner.description }
        }
        let retained = autoreleasepool { released != nil }
        XCTAssertTrue(retained)
        autoreleasepool { DocumentDitherCheckbox.dismantleNSView(box, coordinator: ()) }
        withExtendedLifetime(box) { XCTAssertNil(released) }
    }

    func testActualEditorCheckboxPreservesLookupFieldsAndUndo() async throws {
        let priorPolicy = NSApplication.shared.activationPolicy()
        defer { _ = NSApplication.shared.setActivationPolicy(priorPolicy) }
        ShellHarness.prepare()
        let workspace = DocumentWorkspace()
        workspace.newDocument(workspace.newSettings)
        let document = try XCTUnwrap(workspace.current)
        document.addAdjustment(.colorLookup)
        let id = try XCTUnwrap(document.primary?.id)
        let data = ColorLookupFile.identity(size: 2)
        let before = AdjustmentModel.colorLookup(size: 2, data: data, sourceFilename: "fixture.cube", dither: true)
        document.setAdjustment(id, before, final: true)
        let bounds = NSRect(x: 0, y: 0, width: 288, height: 848)
        let host = NSHostingView(rootView: PropertiesPanel(document: document)
            .frame(width: bounds.width, height: bounds.height, alignment: .topLeading))
        let window = NSWindow(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        window.setContentSize(bounds.size)
        host.frame = bounds
        defer { window.orderOut(nil); window.contentView = nil; window.close() }
        window.orderBack(nil)
        host.layoutSubtreeIfNeeded()
        await Task.yield()
        host.layoutSubtreeIfNeeded()
        func checkboxes(_ view: NSView) -> [DocumentDitherNativeCheckbox] {
            if let box = view as? DocumentDitherNativeCheckbox { return [box] }
            return view.subviews.flatMap { checkboxes($0) }
        }
        let boxes = checkboxes(host)
        XCTAssertEqual(boxes.count, 1, "actual Dither must use the owned native adapter")
        let box = try XCTUnwrap(boxes.first)
        box.keyDown(with: try space())
        let expected = AdjustmentModel.colorLookup(size: 2, data: data, sourceFilename: "fixture.cube", dither: false)
        XCTAssertEqual(document.adjustment(of: id), expected, "real setter preserves all other LUT fields")
        XCTAssertEqual(AdjustmentModel(json: expected.json), expected, "persisted parameter representation retains Dither")
        document.undo()
        XCTAssertEqual(document.adjustment(of: id), before, "one Undo restores the preceding model")
        // No forced focus; actual traversal, file save/reopen and document-pan rejection remain A GUI gates.
    }
}
