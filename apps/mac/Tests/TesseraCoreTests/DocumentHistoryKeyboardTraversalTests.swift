import AppKit
import SwiftUI
import XCTest
import TesseraCore
@testable import Tessera

/// B5-16 History keyboard/bounds checks that do not need the real screen (plan H1–H9, H12).
/// Hosted windows are ordered back only; the app is never activated and no window is made key.
/// On-screen traversal into/out of the SwiftUI siblings, focus rings, pointer drag and external AX
/// remain background computer-use gates.
@MainActor
final class DocumentHistoryKeyboardTraversalTests: XCTestCase {
    private var priorState: GlobalState?
    private var windows: [NSWindow] = []

    private let heightKey = "DocumentInspector.historyHeight"
    private let expandedKey = "InspectorPanel.History"
    private var initial: Double { Double(DocumentInspector.historyDefault) }
    private var row: Double { Double(Theme.Height.row) }

    override func setUp() async throws {
        priorState = GlobalState()
        LayoutProbeHarness.prepare()
    }

    override func tearDown() async throws {
        for window in windows {
            LayoutProbeHarness.dispose(window)
        }
        windows = []
        priorState?.assertOwnersUnchanged()
        priorState?.restore()
        priorState = nil
    }

    // Same capture/restore contract as DocumentHistoryHeightControlTests (kept separate so that
    // preserved suite stays byte-identical).
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

    // MARK: Helpers

    private func documentModel(documents count: Int = 1) -> AppModel {
        let model = AppModel()
        model.viewMode = .document
        model.documents.engine = StubDocumentEngine()
        model.documents.inspectorTab = .stack
        for _ in 0..<count { model.documents.newDocument(model.documents.newSettings) }
        return model
    }

    private func settle(_ view: NSView) {
        LayoutProbeHarness.settle(view)
    }

    /// The fixture-bounds recipe: size after attachment, order back, settle.
    private func hostInspector(_ workspace: DocumentWorkspace,
                               size: NSSize = NSSize(width: 288, height: 848)) -> (NSWindow, NSView) {
        let controller = NSHostingController(rootView: LayoutProbeHarness.root(DocumentInspector(workspace: workspace)))
        let window = LayoutProbeHarness.window(contentRect: NSRect(origin: .zero, size: size),
                              styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        windows.append(window)
        window.contentViewController = controller
        window.setContentSize(size)
        controller.view.frame = NSRect(origin: .zero, size: size)
        window.orderBack(nil)
        settle(controller.view)
        return (window, controller.view)
    }

    private func resize(_ window: NSWindow, _ host: NSView, to size: NSSize) {
        window.setContentSize(size)
        host.frame = NSRect(origin: .zero, size: size)
        settle(host)
    }

    private func standalone(requested: Double, column: CGFloat,
                            onChange: @escaping (Double) -> Void = { _ in }) -> (NSWindow, DocumentHistoryHeightControl) {
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 200, height: 40),
                              styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        windows.append(window)
        let view = DocumentHistoryHeightControl(frame: window.contentView!.bounds)
        window.contentView?.addSubview(view)
        view.configure(requested: requested, column: column, onChange: onChange)
        view.layoutSubtreeIfNeeded()
        window.recalculateKeyViewLoop()
        return (window, view)
    }

    private func find<T: NSView>(_ root: NSView, _ type: T.Type = T.self) -> [T] {
        (root as? T).map { [$0] } ?? root.subviews.flatMap { find($0, type) }
    }

    private func key(_ code: UInt16, _ characters: String, shift: Bool = false, window: NSWindow) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: shift ? .shift : [],
            timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: characters,
            charactersIgnoringModifiers: characters, isARepeat: false, keyCode: code))
    }

    /// The app pipeline minus key-window status: the local monitor first, then the window.
    @discardableResult
    private func tab(_ window: NSWindow, shift: Bool = false, router: KeyRouter,
                     file: StaticString = #filePath, line: UInt = #line) throws -> NSResponder? {
        let event = try key(48, shift ? "\u{19}" : "\t", shift: shift, window: window)
        if window.firstResponder is KeyOwningControl {
            XCTAssertFalse(router.handle(event), "KeyRouter must leave Tab to a focused key-owning control",
                           file: file, line: line)
        }
        window.sendEvent(event)
        return window.firstResponder
    }

    private func name(_ responder: NSResponder?) -> String {
        guard let responder else { return "nil" }
        if let button = responder as? HistoryHeightButton { return button.accessibilityIdentifier() }
        return String(describing: type(of: responder))
    }

    /// The History list's content height. In-process AX exposes no SwiftUI rows (only AppKit-backed
    /// elements), so the SwiftUI ScrollView's document height stands in for the rendered row count.
    /// On the Stack tab the only SwiftUI-hosted scroll view is History's (Layers is an NSOutlineView).
    private func historyContentHeight(_ host: NSView) -> CGFloat? {
        let scrollers = find(host, NSScrollView.self).filter { String(describing: type(of: $0)).contains("HostingScrollView") }
        guard scrollers.count == 1 else { return nil }
        return scrollers[0].documentView?.frame.height
    }

    // MARK: 1. H1/H2 native traversal

    func testTabTraversesNativeHeightButtonsInOrderSkippingReadout() throws {
        let model = documentModel(documents: 0)
        let router = KeyRouter(model: model)
        var changes: [Double] = []
        let (window, view) = standalone(requested: initial + row, column: 900) { changes.append($0) }
        XCTAssertTrue(view.decrease.isEnabled && view.increase.isEnabled && view.reset.isEnabled)
        XCTAssertTrue(window.makeFirstResponder(view.decrease))
        var seen: [String] = [name(window.firstResponder)]
        seen.append(name(try tab(window, router: router)))
        XCTAssertTrue(window.firstResponder === view.increase, "Tab from − reaches +, got \(seen)")
        seen.append(name(try tab(window, router: router)))
        XCTAssertTrue(window.firstResponder === view.reset, "Tab from + reaches ↺, got \(seen)")
        seen.append(name(try tab(window, shift: true, router: router)))
        XCTAssertTrue(window.firstResponder === view.increase, "Shift-Tab from ↺ returns to +, got \(seen)")
        seen.append(name(try tab(window, shift: true, router: router)))
        XCTAssertTrue(window.firstResponder === view.decrease, "Shift-Tab from + returns to −, got \(seen)")
        XCTAssertFalse(seen.contains(view.readout.accessibilityIdentifier()), "the readout is never focused")
        XCTAssertFalse(view.readout.acceptsFirstResponder)
        XCTAssertTrue(changes.isEmpty, "traversal must not change the height")
    }

    // MARK: 2. H3 disabled-at-clamp skipping

    func testTabSkipsDisabledButtonsAtEachClamp() throws {
        let router = KeyRouter(model: documentModel(documents: 0))
        func assertLanding(_ window: NSWindow, file: StaticString = #filePath, line: UInt = #line) {
            if let button = window.firstResponder as? HistoryHeightButton {
                XCTAssertTrue(button.isEnabled, "focus landed on a disabled \(name(button))", file: file, line: line)
                XCTAssertTrue(button.acceptsFirstResponder, file: file, line: line)
            }
        }
        // (a) increase disabled at the maximum.
        do {
            let (window, view) = standalone(requested: 10_000, column: 548)
            XCTAssertFalse(view.increase.isEnabled)
            XCTAssertTrue(view.decrease.isEnabled && view.reset.isEnabled)
            XCTAssertTrue(window.makeFirstResponder(view.decrease))
            try tab(window, router: router)
            XCTAssertTrue(window.firstResponder === view.reset, "Tab skips disabled +, got \(name(window.firstResponder))")
            assertLanding(window)
            try tab(window, shift: true, router: router)
            XCTAssertTrue(window.firstResponder === view.decrease, "Shift-Tab skips disabled +, got \(name(window.firstResponder))")
            assertLanding(window)
        }
        // (b) decrease disabled at the minimum.
        do {
            let (window, view) = standalone(requested: 0, column: 900)
            XCTAssertFalse(view.decrease.isEnabled)
            // makeFirstResponder does not consult acceptsFirstResponder; key-view membership is the gate.
            XCTAssertFalse(view.decrease.acceptsFirstResponder, "a disabled button refuses focus")
            XCTAssertFalse(view.decrease.canBecomeKeyView, "a disabled button leaves the key-view loop")
            XCTAssertTrue(window.makeFirstResponder(view.increase))
            try tab(window, shift: true, router: router)
            XCTAssertFalse(window.firstResponder === view.decrease, "Shift-Tab must not land on disabled −")
            assertLanding(window)
        }
        // (c) reset disabled at the default request.
        do {
            let (window, view) = standalone(requested: initial, column: 900)
            XCTAssertFalse(view.reset.isEnabled)
            XCTAssertTrue(window.makeFirstResponder(view.increase))
            try tab(window, router: router)
            XCTAssertFalse(window.firstResponder === view.reset, "Tab must not land on disabled ↺")
            assertLanding(window)
        }
    }

    // MARK: 3. H1 inside SwiftUI hosting

    func testHostedInspectorKeyViewLoopReachesAllEnabledHistoryButtons() throws {
        UserDefaults.standard.set(initial + row, forKey: heightKey)
        UserDefaults.standard.set(true, forKey: expandedKey)
        let model = documentModel()
        let router = KeyRouter(model: model)
        let (window, host) = hostInspector(model.documents)
        let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertTrue(control.decrease.isEnabled && control.increase.isEnabled && control.reset.isEnabled)
        XCTAssertTrue(window.makeFirstResponder(control.decrease), "forced entry point")
        var trail: [String] = [name(window.firstResponder)]
        var reached: [HistoryHeightButton] = []
        for _ in 0..<40 {
            let next = try tab(window, router: router)
            trail.append(name(next))
            if let view = next as? NSView { XCTAssertTrue(view.window === window, "stale responder \(name(next))") }
            if let button = next as? HistoryHeightButton {
                if button === control.decrease { break }
                reached.append(button)
            }
        }
        XCTAssertTrue(reached.first === control.increase && reached.dropFirst().first === control.reset,
                      "hosted key-view loop must chain − → + → ↺; trail: \(trail)")
        XCTAssertTrue(window.firstResponder === control.decrease, "the loop returns to − within 40 Tabs; trail: \(trail)")
    }

    // MARK: 4. H4 collapse with focus

    func testCollapseWithFocusedHeightButtonSettlesFocusOnLiveView() throws {
        UserDefaults.standard.set(initial + row, forKey: heightKey)
        UserDefaults.standard.set(true, forKey: expandedKey)
        let model = documentModel()
        let router = KeyRouter(model: model)
        let (window, host) = hostInspector(model.documents)
        let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        let readout = control.readout.stringValue
        XCTAssertTrue(window.makeFirstResponder(control.increase))
        UserDefaults.standard.set(false, forKey: expandedKey)
        settle(host)
        XCTAssertTrue(find(host, DocumentHistoryHeightControl.self).isEmpty, "collapsed History removes the controls")
        let responder = window.firstResponder
        XCTAssertFalse(responder is HistoryHeightButton, "focus must not stay on a removed button: \(name(responder))")
        if let view = responder as? NSView {
            XCTAssertTrue(view.window === window, "focus must settle on a live view, got \(name(responder))")
        }
        let event = try key(48, "\t", window: window)
        _ = router.handle(event)
        window.sendEvent(event)
        UserDefaults.standard.set(true, forKey: expandedKey)
        settle(host)
        let restored = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertEqual(restored.readout.stringValue, readout)
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), initial + row)
    }

    // MARK: 5. H5 bounds across window resize

    func testWindowResizeClampsDisplayWithoutRewritingOversizedRequest() throws {
        UserDefaults.standard.set(10_000.0, forKey: heightKey)
        UserDefaults.standard.set(true, forKey: expandedKey)
        let model = documentModel()
        let (window, host) = hostInspector(model.documents)
        func expected(_ column: CGFloat) -> String {
            String(format: "%.0f pt", Double(DocumentInspector.budget.historyHeight(requested: 10_000, column: column)))
        }
        let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertEqual(control.readout.stringValue, expected(host.bounds.height))
        XCTAssertFalse(control.increase.isEnabled)
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), 10_000)
        resize(window, host, to: NSSize(width: 288, height: 548))
        XCTAssertEqual(host.bounds.height, 548)
        let short = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertEqual(short.readout.stringValue, expected(548))
        XCTAssertNotEqual(expected(548), expected(848), "fixture must actually re-clamp")
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), 10_000, "layout must not rewrite the request")
        XCTAssertEqual(ShellLayoutAudit.containmentViolations(in: host, columnContent: true), [])
        resize(window, host, to: NSSize(width: 288, height: 848))
        let tall = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertEqual(tall.readout.stringValue, expected(848))
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), 10_000)
    }

    // MARK: 6. H6 readout fit

    func testReadoutFitsWidestValueWithNumericLabelToken() {
        let view = DocumentHistoryHeightControl(frame: .zero)
        XCTAssertEqual(view.readout.font, Theme.NSFonts.labelNumeric)
        for (requested, column) in [(999.0, CGFloat(5000)), (0.0, CGFloat(0))] {
            view.configure(requested: requested, column: column) { _ in }
            XCTAssertLessThanOrEqual(view.readout.intrinsicContentSize.width, 48,
                                     "\(view.readout.stringValue) must fit the 48 pt readout")
        }
        // Widest three-digit value the column can reach on supported displays.
        view.readout.stringValue = "888 pt"
        XCTAssertLessThanOrEqual(view.readout.intrinsicContentSize.width, 48)
    }

    // MARK: 7. H7 proxy: external preference writes

    func testExternalPreferenceWriteUpdatesHostedReadout() throws {
        UserDefaults.standard.set(initial, forKey: heightKey)
        UserDefaults.standard.set(true, forKey: expandedKey)
        let model = documentModel()
        let (_, host) = hostInspector(model.documents)
        let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        UserDefaults.standard.set(initial + 2 * row, forKey: heightKey)
        settle(host)
        let after = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertTrue(after === control, "a preference write must not recreate the control")
        XCTAssertEqual(after.readout.stringValue, String(format: "%.0f pt", initial + 2 * row))
        XCTAssertTrue(after.reset.isEnabled)
        after.reset.performClick(nil)
        settle(host)
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), initial)
        XCTAssertEqual(after.readout.stringValue, String(format: "%.0f pt", initial))
    }

    // MARK: 8. H8 inspector tabs

    func testInspectorTabSwitchKeepsHeightControlIdentityFocusAndValue() throws {
        UserDefaults.standard.set(initial + row, forKey: heightKey)
        UserDefaults.standard.set(true, forKey: expandedKey)
        let model = documentModel()
        let workspace = model.documents
        let (window, host) = hostInspector(workspace)
        let original = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        let readout = original.readout.stringValue
        XCTAssertTrue(window.makeFirstResponder(original.increase))
        for tab in [DocumentInspectorTab.properties, .channels, .stack] {
            workspace.inspectorTab = tab
            settle(host)
            let controls = find(host, DocumentHistoryHeightControl.self)
            XCTAssertEqual(controls.count, 1, "\(tab)")
            XCTAssertTrue(controls.first === original, "\(tab): History controls must keep their identity")
            XCTAssertTrue(window.firstResponder === original.increase,
                          "\(tab): focus must stay on +, got \(name(window.firstResponder))")
            XCTAssertEqual(controls.first?.readout.stringValue, readout, "\(tab)")
            XCTAssertEqual(ShellLayoutAudit.containmentViolations(in: host, columnContent: true), [], "\(tab)")
        }
        XCTAssertEqual(UserDefaults.standard.string(forKey: DocumentWorkspace.inspectorTabKey), DocumentInspectorTab.stack.rawValue)
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), initial + row)
    }

    // MARK: 9. H9 document switch

    func testDocumentSwitchKeepsGlobalHistoryPreferencesAndShowsSelectedDocument() throws {
        UserDefaults.standard.set(initial, forKey: heightKey)
        UserDefaults.standard.set(true, forKey: expandedKey)
        let model = documentModel(documents: 2)
        let workspace = model.documents
        XCTAssertEqual(workspace.documents.count, 2)
        let docA = workspace.documents[0], docB = workspace.documents[1]
        docB.addLayer(.pixel)
        docB.addLayer(.pixel)
        XCTAssertGreaterThan(docB.history.count, docA.history.count, "fixture: docB has more History states")
        workspace.select(docA, activateDocument: false)
        let (_, host) = hostInspector(workspace)
        let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        control.increase.performClick(nil)
        settle(host)
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), initial + row)
        let heightA = try XCTUnwrap(historyContentHeight(host), "one SwiftUI History scroller on the Stack tab")
        workspace.select(docB, activateDocument: false)
        settle(host)
        XCTAssertTrue(workspace.current === docB)
        XCTAssertEqual(UserDefaults.standard.double(forKey: heightKey), initial + row, "height is global, not per document")
        XCTAssertTrue(UserDefaults.standard.bool(forKey: expandedKey))
        let after = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertTrue(after === control)
        XCTAssertEqual(after.readout.stringValue, String(format: "%.0f pt", initial + row))
        let heightB = try XCTUnwrap(historyContentHeight(host))
        XCTAssertEqual(heightB - heightA, CGFloat(docB.history.count - docA.history.count) * Theme.Height.row, accuracy: 0.5,
                       "History must list docB's states (A \(heightA) pt, B \(heightB) pt)")
        workspace.select(docA, activateDocument: false)
        settle(host)
        XCTAssertEqual(try XCTUnwrap(historyContentHeight(host)), heightA, accuracy: 0.5, "switching back shows docA again")
    }

    // MARK: 10. H12 tool letter over a focused History button

    func testToolLetterOverFocusedHistoryButtonSelectsToolNotText() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        doc.tool = .move
        var changes: [Double] = []
        let (window, view) = standalone(requested: initial, column: 900) { changes.append($0) }
        XCTAssertTrue(window.makeFirstResponder(view.increase))
        let event = try key(11, "b", window: window)
        XCTAssertTrue(KeyRouter(model: model).handle(event), "tool letter is consumed by the router")
        XCTAssertEqual(doc.tool, .brush)
        XCTAssertTrue(changes.isEmpty)
        XCTAssertTrue(window.firstResponder === view.increase)
    }
}
