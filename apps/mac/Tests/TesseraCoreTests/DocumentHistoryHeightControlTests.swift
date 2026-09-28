import AppKit
import SwiftUI
import XCTest
@testable import Tessera

/// Source-only History action regressions; execution belongs to A's isolated gate.
@MainActor
final class DocumentHistoryHeightControlTests: XCTestCase {
    override func setUp() async throws { ShellHarness.prepare() }

    func testNativeAXPressActionsChangeHeightAndExposeCurrentValue() {
        let view = DocumentHistoryHeightControl(frame: NSRect(x: 0, y: 0, width: 132, height: 24))
        let initial = Double(DocumentInspector.historyDefault)
        var changes: [Double] = []
        view.configure(requested: initial, column: 900) { changes.append($0) }
        XCTAssertEqual(view.increase.accessibilityRole(), .button)
        XCTAssertEqual(view.increase.accessibilityLabel(), "Increase History height")
        XCTAssertEqual(view.decrease.accessibilityLabel(), "Decrease History height")
        XCTAssertEqual(view.reset.accessibilityLabel(), "Reset History height")
        XCTAssertEqual(view.readout.accessibilityLabel(), "History height")
        XCTAssertEqual(view.readout.accessibilityValue() as? String, String(format: "%.0f points", initial))
        XCTAssertTrue(view.increase.accessibilityPerformPress())
        XCTAssertEqual(changes, [initial + Double(Theme.Height.row)])
        XCTAssertEqual(view.readout.accessibilityValue() as? String, String(format: "%.0f points", initial + Double(Theme.Height.row)))
        XCTAssertTrue(view.decrease.accessibilityPerformPress())
        XCTAssertEqual(changes.last, initial)
        XCTAssertTrue(view.decrease.accessibilityPerformPress())
        XCTAssertTrue(view.reset.accessibilityPerformPress())
        XCTAssertEqual(changes.last, initial)
    }

    func testActionsUseDisplayedClampButLayoutDoesNotRewriteSavedRequest() {
        let view = DocumentHistoryHeightControl(frame: .zero)
        let column: CGFloat = 548
        let maximum = DocumentInspector.budget.historyHeight(requested: 10_000, column: column)
        var changes: [Double] = []
        view.configure(requested: 10_000, column: column) { changes.append($0) }
        XCTAssertTrue(changes.isEmpty, "short-window layout must preserve the stored request")
        XCTAssertFalse(view.increase.isEnabled)
        XCTAssertTrue(view.decrease.accessibilityPerformPress())
        let expected = DocumentInspector.budget.historyHeight(requested: maximum - Theme.Height.row, column: column)
        XCTAssertEqual(changes.last, Double(expected), "decrease begins at the rendered height, like dragging")
        XCTAssertTrue(view.reset.accessibilityPerformPress())
        XCTAssertEqual(changes.last, Double(DocumentInspector.historyDefault), "reset stores the unclamped default request")
        XCTAssertEqual(view.readout.accessibilityValue() as? String,
                       String(format: "%.0f points", Double(DocumentInspector.budget.historyHeight(requested: DocumentInspector.historyDefault, column: column))))
        view.configure(requested: 0, column: 900) { changes.append($0) }
        XCTAssertFalse(view.decrease.isEnabled)
        view.configure(requested: 100, column: 200) { changes.append($0) }
        XCTAssertFalse(view.decrease.isEnabled)
        XCTAssertFalse(view.increase.isEnabled, "insufficient room cannot steal the tab/footer budget")
    }

    func testFocusedButtonsOwnSpaceAndReturnAndSendTheirActions() throws {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 40),
                              styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        let view = DocumentHistoryHeightControl(frame: window.contentView!.bounds)
        window.contentView?.addSubview(view)
        let initial = Double(DocumentInspector.historyDefault)
        var latest = initial
        view.configure(requested: initial, column: 900) { latest = $0 }
        let model = AppModel()
        model.viewMode = .document
        for (button, code, characters, expected) in [
            (view.increase, UInt16(49), " ", initial + Double(Theme.Height.row)),
            (view.decrease, UInt16(36), "\r", initial),
            (view.reset, UInt16(76), "\r", initial)
        ] {
            if button === view.reset {
                view.configure(requested: initial + 20, column: 900) { latest = $0 }
            }
            XCTAssertTrue(button.acceptsFirstResponder)
            XCTAssertTrue(window.makeFirstResponder(button))
            let event = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: characters,
                charactersIgnoringModifiers: characters, isARepeat: false, keyCode: code))
            XCTAssertFalse(KeyRouter(model: model).handle(event), "focused native button owns its activation key")
            button.keyDown(with: event)
            XCTAssertEqual(latest, expected)
            XCTAssertFalse(model.documents.spaceHeld, "Space must not begin viewport panning")
        }
    }

    func testConfigureRefreshesCallbackWithoutPublishingAndTeardownStopsActions() {
        let view = DocumentHistoryHeightControl(frame: .zero)
        var oldCalls = 0, newCalls = 0
        let initial = Double(DocumentInspector.historyDefault)
        view.configure(requested: initial, column: 900) { _ in oldCalls += 1 }
        view.configure(requested: initial, column: 900) { _ in newCalls += 1 }
        XCTAssertEqual(oldCalls + newCalls, 0)
        XCTAssertTrue(view.increase.accessibilityPerformPress())
        XCTAssertEqual(oldCalls, 0)
        XCTAssertEqual(newCalls, 1)
        DocumentHistoryHeightControls.dismantleNSView(view, coordinator: ())
        _ = view.decrease.accessibilityPerformPress()
        XCTAssertEqual(newCalls, 1)
    }

    func testActualInspectorAXActionWritesExistingPreferenceAndRestoresOnRecreation() throws {
        let defaults = UserDefaults.standard
        let heightKey = "DocumentInspector.historyHeight", expandedKey = "InspectorPanel.History"
        let oldHeight = defaults.object(forKey: heightKey), oldExpanded = defaults.object(forKey: expandedKey)
        defer {
            if let oldHeight { defaults.set(oldHeight, forKey: heightKey) } else { defaults.removeObject(forKey: heightKey) }
            if let oldExpanded { defaults.set(oldExpanded, forKey: expandedKey) } else { defaults.removeObject(forKey: expandedKey) }
        }
        let initial = Double(DocumentInspector.historyDefault)
        defaults.set(initial, forKey: heightKey)
        defaults.set(true, forKey: expandedKey)
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let model = try ShellHarness.model(.document, scratch: scratch)
        func find(_ root: NSView) -> DocumentHistoryHeightControl? {
            if let control = root as? DocumentHistoryHeightControl { return control }
            return root.subviews.lazy.compactMap { find($0) }.first
        }
        let (window, host) = ShellHarness.window(model, size: CGSize(width: 1440, height: 900), dark: true)
        defer { window.orderOut(nil); window.contentViewController = nil }
        let control = try XCTUnwrap(find(host), "must expose native actions in the actual inspector")
        XCTAssertTrue(control.increase.accessibilityPerformPress())
        XCTAssertEqual(defaults.double(forKey: heightKey), initial + Double(Theme.Height.row))
        let (restoredWindow, restoredHost) = ShellHarness.window(model, size: CGSize(width: 1440, height: 900), dark: false)
        defer { restoredWindow.orderOut(nil); restoredWindow.contentViewController = nil }
        let restored = try XCTUnwrap(find(restoredHost))
        XCTAssertEqual(restored.readout.accessibilityValue() as? String, String(format: "%.0f points", initial + Double(Theme.Height.row)))
        XCTAssertTrue(restored.reset.accessibilityPerformPress())
        XCTAssertEqual(defaults.double(forKey: heightKey), initial)
        XCTAssertEqual(ShellLayoutAudit.containmentViolations(in: restoredHost, columnContent: true), [])
    }
}
