import AppKit
import SwiftUI
import XCTest
@testable import Tessera

/// Source-only History action regressions; execution belongs to A's isolated gate.
@MainActor
final class DocumentHistoryHeightControlTests: XCTestCase {
    private var priorState: GlobalState?

    override func setUp() async throws {
        priorState = GlobalState()
        ShellHarness.prepare()
    }

    override func tearDown() async throws {
        // Test-method defers remove hosting controllers and close windows first.
        // Retain prior owners strongly; never replace an existing owner with nil.
        priorState?.assertOwnersUnchanged()
        priorState?.restore()
        priorState = nil
    }

    @MainActor private struct GlobalState {
        let activation = NSApplication.shared.activationPolicy()
        let tools = DocumentTools.shared.workspace
        let channels = DocumentChannels.shared.workspace
        let vector = DocumentVector.shared.workspace
        let text = DocumentText.shared.workspace
        let transforms = DocumentTransforms.shared.workspace
        let preferences = ["DocumentInspector.historyHeight", "InspectorPanel.History", DocumentWorkspace.inspectorTabKey]
            .map { ($0, UserDefaults.standard.object(forKey: $0)) }

        func assertOwnersUnchanged(file: StaticString = #filePath, line: UInt = #line) {
            XCTAssertTrue(DocumentTools.shared.workspace === tools, file: file, line: line)
            XCTAssertTrue(DocumentChannels.shared.workspace === channels, file: file, line: line)
            XCTAssertTrue(DocumentVector.shared.workspace === vector, file: file, line: line)
            XCTAssertTrue(DocumentText.shared.workspace === text, file: file, line: line)
            XCTAssertTrue(DocumentTransforms.shared.workspace === transforms, file: file, line: line)
        }

        func restore() {
            DocumentTools.shared.workspace = tools
            DocumentChannels.shared.workspace = channels
            DocumentVector.shared.workspace = vector
            DocumentText.shared.workspace = text
            DocumentTransforms.shared.workspace = transforms
            for (key, value) in preferences {
                if let value { UserDefaults.standard.set(value, forKey: key) }
                else { UserDefaults.standard.removeObject(forKey: key) }
            }
            _ = NSApplication.shared.setActivationPolicy(activation)
        }
    }

    func testGlobalStateRestoresExistingOwnersAndPreferenceValues() {
        let original = GlobalState()
        defer { original.restore() }
        let owner = DocumentWorkspace()
        DocumentTools.shared.workspace = owner
        DocumentChannels.shared.workspace = owner
        DocumentVector.shared.workspace = owner
        DocumentText.shared.workspace = owner
        DocumentTransforms.shared.workspace = owner
        let defaults = UserDefaults.standard
        defaults.set(321.0, forKey: "DocumentInspector.historyHeight")
        defaults.set(false, forKey: "InspectorPanel.History")
        defaults.removeObject(forKey: DocumentWorkspace.inspectorTabKey)
        let saved = GlobalState()
        let replacement = DocumentWorkspace()
        DocumentTools.shared.workspace = replacement
        DocumentChannels.shared.workspace = replacement
        DocumentVector.shared.workspace = replacement
        DocumentText.shared.workspace = replacement
        DocumentTransforms.shared.workspace = replacement
        defaults.set(100.0, forKey: "DocumentInspector.historyHeight")
        defaults.set(true, forKey: "InspectorPanel.History")
        defaults.set("channels", forKey: DocumentWorkspace.inspectorTabKey)
        saved.restore()
        saved.assertOwnersUnchanged()
        XCTAssertTrue(DocumentTools.shared.workspace === owner)
        XCTAssertEqual(defaults.double(forKey: "DocumentInspector.historyHeight"), 321)
        XCTAssertFalse(defaults.bool(forKey: "InspectorPanel.History"))
        XCTAssertNil(defaults.object(forKey: DocumentWorkspace.inspectorTabKey))
        XCTAssertEqual(NSApplication.shared.activationPolicy(), saved.activation)
    }

    func testNativeAXPressActionsChangeHeightAndExposeCurrentValue() {
        let view = DocumentHistoryHeightControl(frame: NSRect(x: 0, y: 0, width: 132, height: 24))
        let initial = Double(DocumentInspector.historyDefault)
        var changes: [Double] = []
        view.configure(requested: initial, column: 900) { changes.append($0) }
        // Plain native AppKit on A returns AXUnknown here and false from a direct
        // press even when target/action runs. External AX remains a separate gate.
        XCTAssertEqual(view.increase.accessibilityLabel(), "Increase History height")
        XCTAssertEqual(view.decrease.accessibilityLabel(), "Decrease History height")
        XCTAssertEqual(view.reset.accessibilityLabel(), "Reset History height")
        XCTAssertEqual(view.readout.accessibilityLabel(), "History height")
        XCTAssertEqual(view.readout.accessibilityValue(), String(format: "%.0f pt", initial))
        _ = view.increase.accessibilityPerformPress()
        XCTAssertEqual(changes, [initial + Double(Theme.Height.row)])
        XCTAssertEqual(view.readout.accessibilityValue(), String(format: "%.0f pt", initial + Double(Theme.Height.row)))
        _ = view.decrease.accessibilityPerformPress()
        XCTAssertEqual(changes.last, initial)
        _ = view.decrease.accessibilityPerformPress()
        _ = view.reset.accessibilityPerformPress()
        XCTAssertEqual(changes, [initial + Double(Theme.Height.row), initial, initial - Double(Theme.Height.row), initial])
    }

    func testActionsUseDisplayedClampButLayoutDoesNotRewriteSavedRequest() {
        let view = DocumentHistoryHeightControl(frame: .zero)
        let column: CGFloat = 548
        let maximum = DocumentInspector.budget.historyHeight(requested: 10_000, column: column)
        var changes: [Double] = []
        view.configure(requested: 10_000, column: column) { changes.append($0) }
        XCTAssertTrue(changes.isEmpty, "short-window layout must preserve the stored request")
        XCTAssertFalse(view.increase.isEnabled)
        _ = view.decrease.accessibilityPerformPress()
        let expected = DocumentInspector.budget.historyHeight(requested: maximum - Theme.Height.row, column: column)
        XCTAssertEqual(changes.last, Double(expected), "decrease begins at the rendered height, like dragging")
        _ = view.reset.accessibilityPerformPress()
        XCTAssertEqual(changes.last, Double(DocumentInspector.historyDefault), "reset stores the unclamped default request")
        XCTAssertEqual(view.readout.accessibilityValue(),
                       String(format: "%.0f pt", Double(DocumentInspector.budget.historyHeight(requested: DocumentInspector.historyDefault, column: column))))
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
        _ = view.increase.accessibilityPerformPress()
        XCTAssertEqual(oldCalls, 0)
        XCTAssertEqual(newCalls, 1)
        DocumentHistoryHeightControls.dismantleNSView(view, coordinator: ())
        _ = view.decrease.accessibilityPerformPress()
        XCTAssertEqual(newCalls, 1)
    }

    func testActualInspectorAXActionWritesExistingPreferenceAndRestoresOnRecreation() throws {
        let defaults = UserDefaults.standard
        let heightKey = "DocumentInspector.historyHeight", expandedKey = "InspectorPanel.History"
        let initial = Double(DocumentInspector.historyDefault)
        defaults.set(initial, forKey: heightKey)
        defaults.set(true, forKey: expandedKey)
        // Host the actual inspector, not ContentView/DocumentView. The full shell
        // onAppear attaches tool/text/channel/vector/transform singletons and can
        // launch outline work unrelated to this preference/control regression.
        let workspace = DocumentWorkspace()
        workspace.inspectorTab = .stack
        workspace.newDocument(workspace.newSettings)
        XCTAssertNotNil(workspace.current)
        func hostInspector() -> (NSWindow, NSView) {
            let controller = NSHostingController(rootView: DocumentInspector(workspace: workspace))
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 288, height: 848),
                                  styleMask: .titled, backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.contentViewController = controller
            window.orderBack(nil)
            controller.view.layoutSubtreeIfNeeded()
            // Use the existing harness's event-loop settling convention, only on
            // the inspector host. No Document viewport or shared attachment.
            RunLoop.main.run(until: Date().addingTimeInterval(0.25))
            return (window, controller.view)
        }
        func find(_ root: NSView) -> DocumentHistoryHeightControl? {
            if let control = root as? DocumentHistoryHeightControl { return control }
            return root.subviews.lazy.compactMap { find($0) }.first
        }
        let (window, host) = hostInspector()
        defer { window.orderOut(nil); window.contentViewController = nil; window.close() }
        let control = try XCTUnwrap(find(host), "must expose native actions in the actual inspector")
        _ = control.increase.accessibilityPerformPress()
        XCTAssertEqual(defaults.double(forKey: heightKey), initial + Double(Theme.Height.row))
        let (restoredWindow, restoredHost) = hostInspector()
        defer { restoredWindow.orderOut(nil); restoredWindow.contentViewController = nil; restoredWindow.close() }
        let restored = try XCTUnwrap(find(restoredHost))
        XCTAssertEqual(restored.readout.accessibilityValue(), String(format: "%.0f pt", initial + Double(Theme.Height.row)))
        _ = restored.reset.accessibilityPerformPress()
        XCTAssertEqual(defaults.double(forKey: heightKey), initial)
        XCTAssertEqual(ShellLayoutAudit.containmentViolations(in: restoredHost, columnContent: true), [])
        priorState?.assertOwnersUnchanged()
    }
}
