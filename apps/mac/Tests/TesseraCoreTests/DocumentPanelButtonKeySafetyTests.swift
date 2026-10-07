import AppKit
import SwiftUI
import XCTest
import TesseraCore
@testable import Tessera

/// B5-25: with the keyboard on a native panel / toolbar button (a Layers eye button, a toolbar tool button)
/// ⌫ / ⌦ never delete anything and Space presses the button once instead of panning the canvas. The Layers
/// list keeps its ⌫, the canvas keeps ⌫ and Space-pan, text fields keep both keys. Hosted windows are
/// ordered back only; the app is never activated and no window is made key.
@MainActor
final class DocumentPanelButtonKeySafetyTests: XCTestCase {
    private var priorState: GlobalState?
    private var windows: [NSWindow] = []

    override func setUp() async throws {
        priorState = GlobalState()
        LayoutProbeHarness.prepare()
    }

    override func tearDown() async throws {
        for window in windows {
            LayoutProbeHarness.dispose(window)
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
        let preferences = [DocumentWorkspace.inspectorTabKey].map { ($0, UserDefaults.standard.object(forKey: $0)) }

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
        DocumentTools.shared.workspace = model.documents
        return model
    }

    private func plainWindow() -> NSWindow {
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 400, height: 200),
                              styleMask: .titled, backing: .buffered, defer: false)
        windows.append(window)
        return window
    }

    private func settle(_ view: NSView) {
        LayoutProbeHarness.settle(view)
    }

    private func hostInspector(_ workspace: DocumentWorkspace) -> (NSWindow, NSView) {
        let size = NSSize(width: 288, height: 848)
        let controller = NSHostingController(rootView: LayoutProbeHarness.root(DocumentInspector(workspace: workspace)))
        let window = LayoutProbeHarness.window(contentRect: NSRect(origin: .zero, size: size),
                              styleMask: .titled, backing: .buffered, defer: false)
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

    private func eyeLayerID(_ eye: NSButton) -> DocLayerID? {
        var ancestor = eye.superview
        while let v = ancestor {
            if let cell = v as? LayerRowCell { return cell.layerID }
            ancestor = v.superview
        }
        return nil
    }

    private func key(_ code: UInt16, _ characters: String, repeat isARepeat: Bool = false,
                     type: NSEvent.EventType = .keyDown, window: NSWindow) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [],
            timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: characters,
            charactersIgnoringModifiers: characters, isARepeat: isARepeat, keyCode: code))
    }

    /// The app pipeline minus key-window status: the local monitor first, then (if it left the event
    /// alone) the window, which hands it to the first responder and up the responder chain.
    @discardableResult
    private func press(_ event: NSEvent, _ window: NSWindow, router: KeyRouter) -> Bool {
        let handled = event.type == .keyUp ? router.handleKeyUp(event) : router.handle(event)
        if handled { return true }
        window.sendEvent(event)
        return false
    }

    private func backspace(_ window: NSWindow) throws -> NSEvent { try key(51, "\u{7f}", window: window) }
    private func forwardDelete(_ window: NSWindow) throws -> NSEvent { try key(117, "\u{f728}", window: window) }
    private func space(_ window: NSWindow, repeat r: Bool = false, up: Bool = false) throws -> NSEvent {
        try key(49, " ", repeat: r, type: up ? .keyUp : .keyDown, window: window)
    }

    private final class Counter: NSObject {
        var count = 0
        @objc func fire(_ sender: Any?) { count += 1 }
    }

    // MARK: 1. ⌫ / ⌦ over a focused Layers eye button delete nothing

    func testDeleteOverFocusedLayersEyeButtonDeletesNothing() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let (window, host) = hostInspector(model.documents)
        let eye = try XCTUnwrap(eyeButtons(host).first, "the Stack tab shows Layers rows with eye buttons")
        XCTAssertFalse(doc.selection.isEmpty, "fixture: a layer is selected")
        let before = doc.layers.count
        let selection = doc.selection
        XCTAssertTrue(window.makeFirstResponder(eye))

        press(try backspace(window), window, router: router)
        XCTAssertEqual(doc.layers.count, before, "⌫ over a focused eye button must not delete the selected layer")
        press(try forwardDelete(window), window, router: router)
        XCTAssertEqual(doc.layers.count, before, "⌦ over a focused eye button must not delete the selected layer")
        XCTAssertEqual(doc.selection, selection)
        XCTAssertTrue(window.firstResponder === eye, "focus stays on the eye button")
        XCTAssertFalse(model.documents.panelsHidden)
    }

    func testDeleteOverFocusedPlainToolbarButtonDeletesNothing() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let button = NSButton(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        window.contentView?.addSubview(button)
        XCTAssertTrue(window.makeFirstResponder(button))
        let before = doc.layers.count
        press(try backspace(window), window, router: router)
        press(try forwardDelete(window), window, router: router)
        XCTAssertEqual(doc.layers.count, before, "⌫ / ⌦ over a focused toolbar button delete nothing")
    }

    // MARK: 2. The Layers list and the canvas keep ⌫

    func testDeleteOverFocusedLayersListStillDeletesTheSelectedLayer() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let (window, host) = hostInspector(model.documents)
        let outline = try XCTUnwrap(find(host, LayersOutlineView.self).first)
        let selected = try XCTUnwrap(doc.selection.first)
        let before = doc.layers.count
        XCTAssertTrue(window.makeFirstResponder(outline))
        press(try backspace(window), window, router: router)
        XCTAssertNil(doc.node(selected), "⌫ over the Layers list deletes the selected layer")
        XCTAssertLessThan(doc.layers.count, before)
    }

    func testDeleteOverTheCanvasStillDeletesTheSelectedLayer() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let viewport = DocumentViewportView(frame: window.contentView!.bounds)
        window.contentView?.addSubview(viewport)
        XCTAssertTrue(window.makeFirstResponder(viewport))
        let selected = try XCTUnwrap(doc.selection.first)
        let before = doc.layers.count
        XCTAssertTrue(router.handle(try backspace(window)), "⌫ over the canvas is the document key")
        XCTAssertNil(doc.node(selected), "⌫ over the canvas deletes the selected layer (unchanged)")
        XCTAssertLessThan(doc.layers.count, before)
    }

    // MARK: 3. Space over a focused button presses it once and never pans

    func testSpaceOverFocusedPlainButtonPressesOnceAndDoesNotPan() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let counter = Counter()
        let button = NSButton(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        button.target = counter
        button.action = #selector(Counter.fire(_:))
        window.contentView?.addSubview(button)
        XCTAssertTrue(window.makeFirstResponder(button))

        press(try space(window), window, router: router)
        XCTAssertEqual(counter.count, 1, "Space presses the focused button")
        XCTAssertFalse(model.documents.spaceHeld, "Space over a focused button must not start a canvas pan")
        press(try space(window, repeat: true), window, router: router)
        press(try space(window, repeat: true), window, router: router)
        XCTAssertEqual(counter.count, 1, "key repeat does not press the button again")
        XCTAssertFalse(model.documents.spaceHeld)
        press(try space(window, up: true), window, router: router)
        XCTAssertEqual(counter.count, 1)
        XCTAssertFalse(model.documents.spaceHeld)
        XCTAssertTrue(window.firstResponder === button)
    }

    func testSpaceOverFocusedLayersEyeButtonTogglesVisibilityOnce() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let (window, host) = hostInspector(model.documents)
        let eye = try XCTUnwrap(eyeButtons(host).first)
        let id = try XCTUnwrap(eyeLayerID(eye))
        let visible = try XCTUnwrap(doc.node(id)).visible
        XCTAssertTrue(window.makeFirstResponder(eye))
        press(try space(window), window, router: router)
        press(try space(window, repeat: true), window, router: router)
        XCTAssertEqual(doc.node(id)?.visible, !visible, "Space toggles the eye exactly once")
        XCTAssertFalse(model.documents.spaceHeld, "no canvas pan")
        press(try space(window, up: true), window, router: router)
        XCTAssertFalse(model.documents.spaceHeld)
    }

    /// AppKit drops the keyboard from a control that becomes disabled, so Space falls back to the
    /// nothing-focused behaviour (pan) and never fires the disabled button.
    func testDisablingAFocusedButtonNeverFiresItOnSpace() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let counter = Counter()
        let button = NSButton(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        button.target = counter
        button.action = #selector(Counter.fire(_:))
        window.contentView?.addSubview(button)
        XCTAssertTrue(window.makeFirstResponder(button))
        button.isEnabled = false
        XCTAssertFalse(window.firstResponder === button, "a disabled button gives up the keyboard")
        press(try space(window), window, router: router)
        XCTAssertEqual(counter.count, 0, "Space never fires a disabled button")
        XCTAssertTrue(model.documents.spaceHeld, "with nothing focused Space pans, as before")
        press(try space(window, up: true), window, router: router)
        XCTAssertFalse(model.documents.spaceHeld)
    }

    /// A stand-in for a SwiftUI control's focus proxy under Full Keyboard Access (a plain view that takes
    /// the keyboard): the first Space is left to it, repeats are dropped, ⌫ is swallowed, and nothing pans.
    private final class FocusProxyView: NSView {
        override var acceptsFirstResponder: Bool { true }
    }

    func testFocusProxyGetsFirstSpaceButNoPanRepeatOrDelete() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let proxy = FocusProxyView(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        window.contentView?.addSubview(proxy)
        XCTAssertTrue(window.makeFirstResponder(proxy))
        let before = doc.layers.count
        XCTAssertFalse(router.handle(try space(window)), "the first Space goes to the focused control")
        XCTAssertTrue(router.handle(try space(window, repeat: true)), "key repeat is dropped")
        XCTAssertFalse(model.documents.spaceHeld)
        XCTAssertTrue(router.handle(try backspace(window)), "⌫ is swallowed")
        XCTAssertEqual(doc.layers.count, before)
    }

    // MARK: 4. Canvas Space-pan and text fields unchanged

    func testSpaceOverTheCanvasStillPans() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let viewport = DocumentViewportView(frame: window.contentView!.bounds)
        window.contentView?.addSubview(viewport)
        XCTAssertTrue(window.makeFirstResponder(viewport))
        XCTAssertTrue(router.handle(try space(window)))
        XCTAssertTrue(model.documents.spaceHeld, "Space over the canvas holds the pan")
        XCTAssertTrue(router.handleKeyUp(try space(window, up: true)))
        XCTAssertFalse(model.documents.spaceHeld)

        let fresh = documentModel()
        let bare = KeyRouter(model: fresh)
        let empty = plainWindow()
        XCTAssertTrue(empty.firstResponder === empty, "fixture: nothing focused")
        XCTAssertTrue(bare.handle(try space(empty)), "with nothing focused Space still pans")
        XCTAssertTrue(fresh.documents.spaceHeld)
    }

    func testTextFieldKeepsDeleteAndSpace() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let field = NSTextField(frame: NSRect(x: 10, y: 40, width: 100, height: 22))
        window.contentView?.addSubview(field)
        XCTAssertTrue(window.makeFirstResponder(field))
        let before = doc.layers.count
        XCTAssertFalse(router.handle(try backspace(window)), "a text field keeps ⌫")
        XCTAssertFalse(router.handle(try forwardDelete(window)))
        XCTAssertFalse(router.handle(try space(window)), "a text field keeps Space")
        XCTAssertFalse(model.documents.spaceHeld)
        XCTAssertEqual(doc.layers.count, before)
    }

    func testToolLettersOverAFocusedButtonUnchanged() throws {
        let model = documentModel()
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let button = NSButton(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        window.contentView?.addSubview(button)
        XCTAssertTrue(window.makeFirstResponder(button))
        doc.tool = .move
        XCTAssertTrue(router.handle(try key(11, "b", window: window)))
        XCTAssertEqual(doc.tool, .brush)
    }
}
