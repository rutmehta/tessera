import AppKit
import SwiftUI
import XCTest
import TesseraCore
@testable import Tessera

/// B5-21: in document mode Tab shows / hides the panels only while the canvas (or nothing) has the
/// keyboard. With a native control of the inspector, a panel or the toolbar focused (a Layers eye
/// button, a SwiftUI control's focus proxy under Full Keyboard Access), Tab / ⇧Tab walk the native
/// key-view loop. Hosted windows are ordered back only; the app is never activated and no window is
/// made key.
@MainActor
final class DocumentPanelTabTraversalTests: XCTestCase {
    private var priorState: GlobalState?
    private var windows: [NSWindow] = []

    override func setUp() async throws {
        priorState = GlobalState()
        ShellHarness.prepare()
    }

    override func tearDown() async throws {
        for window in windows {
            window.orderOut(nil)
            window.contentViewController = nil
            window.contentView = nil
            window.close()
        }
        windows = []
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

    private func documentModel() -> AppModel {
        let model = AppModel()
        model.viewMode = .document
        model.documents.engine = StubDocumentEngine()
        model.documents.inspectorTab = .stack
        model.documents.newDocument(model.documents.newSettings)
        return model
    }

    private func plainWindow() -> NSWindow {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 200),
                              styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        windows.append(window)
        return window
    }

    private func settle(_ view: NSView) {
        view.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.25))
        view.layoutSubtreeIfNeeded()
    }

    private func hostInspector(_ workspace: DocumentWorkspace) -> (NSWindow, NSView) {
        let size = NSSize(width: 288, height: 848)
        let controller = NSHostingController(rootView: DocumentInspector(workspace: workspace))
        let window = NSWindow(contentRect: NSRect(origin: .zero, size: size),
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

    private func find<T: NSView>(_ root: NSView, _ type: T.Type = T.self) -> [T] {
        (root as? T).map { [$0] } ?? root.subviews.flatMap { find($0, type) }
    }

    private func eyeButtons(_ root: NSView) -> [NSButton] {
        find(root, NSButton.self).filter {
            let id = $0.accessibilityIdentifier()
            return id.hasPrefix("document.layers.row.") && id.hasSuffix(".visibility")
        }
    }

    private func key(_ code: UInt16, _ characters: String, shift: Bool = false, window: NSWindow) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: shift ? .shift : [],
            timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: characters,
            charactersIgnoringModifiers: characters, isARepeat: false, keyCode: code))
    }

    /// The app pipeline minus key-window status: the local monitor first, then (if it left the event
    /// alone) the window. Returns whether the monitor consumed the Tab.
    private func tab(_ window: NSWindow, shift: Bool = false, router: KeyRouter) throws -> Bool {
        let event = try key(48, shift ? "\u{19}" : "\t", shift: shift, window: window)
        if router.handle(event) { return true }
        window.sendEvent(event)
        return false
    }

    private func name(_ responder: NSResponder?) -> String {
        guard let responder else { return "nil" }
        if let view = responder as? NSView, !view.accessibilityIdentifier().isEmpty { return view.accessibilityIdentifier() }
        return String(describing: type(of: responder))
    }

    /// A stand-in for a SwiftUI control's focus proxy (a plain view that takes the keyboard).
    private final class FocusProxyView: NSView {
        override var acceptsFirstResponder: Bool { true }
    }

    // MARK: 1. A focused plain native button leaves Tab to the key-view loop

    func testTabFromFocusedPlainButtonMovesFocusInsteadOfTogglingPanels() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let first = NSButton(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        let second = NSButton(frame: NSRect(x: 60, y: 10, width: 40, height: 20))
        window.contentView?.addSubview(first)
        window.contentView?.addSubview(second)
        first.nextKeyView = second
        second.nextKeyView = first
        XCTAssertTrue(window.makeFirstResponder(first))
        XCTAssertFalse(router.handle(try key(48, "\t", window: window)), "Tab over a focused button is not the panels key")
        XCTAssertFalse(model.documents.panelsHidden)
        XCTAssertFalse(router.handle(try key(48, "\u{19}", shift: true, window: window)))
        XCTAssertFalse(model.documents.panelsHidden)
    }

    func testTabFromFocusedNonControlFocusProxyIsLeftToTheWindow() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let proxy = FocusProxyView(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        window.contentView?.addSubview(proxy)
        XCTAssertTrue(window.makeFirstResponder(proxy))
        XCTAssertFalse(router.handle(try key(48, "\t", window: window)))
        XCTAssertFalse(model.documents.panelsHidden)
    }

    // MARK: 2. Canvas / nothing focused keep the panels toggle

    func testTabOverTheCanvasOrWithNothingFocusedTogglesPanels() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        XCTAssertTrue(window.firstResponder === window, "fixture: nothing focused")
        XCTAssertTrue(router.handle(try key(48, "\t", window: window)))
        XCTAssertTrue(model.documents.panelsHidden)
        XCTAssertTrue(router.handle(try key(48, "\t", window: window)))
        XCTAssertFalse(model.documents.panelsHidden)

        let viewport = DocumentViewportView(frame: window.contentView!.bounds)
        window.contentView?.addSubview(viewport)
        XCTAssertTrue(window.makeFirstResponder(viewport))
        XCTAssertTrue(router.handle(try key(48, "\t", window: window)), "Tab over the canvas hides the panels")
        XCTAssertTrue(model.documents.panelsHidden)
        XCTAssertTrue(router.handle(try key(48, "\t", window: window)), "and shows them again")
        XCTAssertFalse(model.documents.panelsHidden)
    }

    func testTabWithAHiddenFocusedButtonStillTogglesPanels() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let container = NSView(frame: window.contentView!.bounds)
        let button = NSButton(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        container.addSubview(button)
        window.contentView?.addSubview(container)
        XCTAssertTrue(window.makeFirstResponder(button))
        container.isHidden = true
        XCTAssertTrue(router.handle(try key(48, "\t", window: window)), "a hidden control does not hold Tab")
        XCTAssertTrue(model.documents.panelsHidden)
    }

    // MARK: 3. Text input and tool letters unchanged

    func testTextFieldKeepsTabAndToolLettersStillRouteOverAFocusedButton() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        DocumentTools.shared.workspace = model.documents
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let field = NSTextField(frame: NSRect(x: 10, y: 40, width: 100, height: 22))
        let button = NSButton(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        window.contentView?.addSubview(field)
        window.contentView?.addSubview(button)
        XCTAssertTrue(window.makeFirstResponder(field))
        XCTAssertFalse(router.handle(try key(48, "\t", window: window)), "a text field keeps Tab")
        XCTAssertFalse(router.handle(try key(11, "b", window: window)), "a text field keeps letters")
        XCTAssertFalse(model.documents.panelsHidden)

        XCTAssertTrue(window.makeFirstResponder(button))
        doc.tool = .move
        XCTAssertTrue(router.handle(try key(11, "b", window: window)), "B over a focused button chooses the Brush")
        XCTAssertEqual(doc.tool, .brush)
        XCTAssertTrue(router.handle(try key(9, "v", window: window)))
        XCTAssertEqual(doc.tool, .move)
        XCTAssertTrue(window.firstResponder === button)
    }

    // MARK: 4. On-screen path: Layers eye button → History −/+/↺

    func testTabFromLayersEyeButtonReachesHistoryHeightButtons() throws {
        UserDefaults.standard.set(Double(DocumentInspector.historyDefault + Theme.Height.row),
                                  forKey: "DocumentInspector.historyHeight")
        UserDefaults.standard.set(true, forKey: "InspectorPanel.History")
        let model = documentModel()
        let router = KeyRouter(model: model)
        let (window, host) = hostInspector(model.documents)
        let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        XCTAssertTrue(control.decrease.isEnabled && control.increase.isEnabled && control.reset.isEnabled)
        let eye = try XCTUnwrap(eyeButtons(host).first, "the Stack tab shows Layers rows with eye buttons")
        XCTAssertTrue(window.makeFirstResponder(eye))
        var trail = [name(window.firstResponder)]
        var reached: [HistoryHeightButton] = []
        for _ in 0..<60 {
            let consumed = try tab(window, router: router)
            XCTAssertFalse(consumed, "Tab from \(trail.last ?? "?") was taken as the panels key; trail: \(trail)")
            XCTAssertFalse(model.documents.panelsHidden)
            if consumed { break }
            trail.append(name(window.firstResponder))
            if let button = window.firstResponder as? HistoryHeightButton, !reached.contains(where: { $0 === button }) {
                reached.append(button)
            }
            if reached.count == 3 { break }
        }
        XCTAssertTrue(reached.count == 3 && reached[0] === control.decrease && reached[1] === control.increase
                      && reached[2] === control.reset, "Tab from the eye button must reach − → + → ↺; trail: \(trail)")
        // ⇧Tab walks back from ↺ to +.
        XCTAssertFalse(try tab(window, shift: true, router: router))
        XCTAssertTrue(window.firstResponder === control.increase, "⇧Tab from ↺ returns to +, got \(name(window.firstResponder))")
        XCTAssertFalse(model.documents.panelsHidden)
    }

    func testTabFromLayersOutlineReachesHistoryHeightButtons() throws {
        UserDefaults.standard.set(Double(DocumentInspector.historyDefault + Theme.Height.row),
                                  forKey: "DocumentInspector.historyHeight")
        UserDefaults.standard.set(true, forKey: "InspectorPanel.History")
        let model = documentModel()
        let router = KeyRouter(model: model)
        let (window, host) = hostInspector(model.documents)
        let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
        let outline = try XCTUnwrap(find(host, LayersOutlineView.self).first)
        XCTAssertTrue(window.makeFirstResponder(outline))
        var trail = [name(window.firstResponder)]
        for _ in 0..<60 {
            let consumed = try tab(window, router: router)
            XCTAssertFalse(consumed, "trail: \(trail)")
            if consumed { break }
            trail.append(name(window.firstResponder))
            if window.firstResponder === control.decrease { break }
        }
        XCTAssertTrue(window.firstResponder === control.decrease, "Tab from the Layers list reaches −; trail: \(trail)")
        XCTAssertFalse(model.documents.panelsHidden)
    }
}
