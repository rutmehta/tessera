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
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 400, height: 200),
                              styleMask: .titled, backing: .buffered, defer: false)
        windows.append(window)
        return window
    }

    private func settle(_ view: NSView) {
        LayoutProbeHarness.settle(view, timeout: 5)
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

    // MARK: 5. Review blocker B1: a stray first responder behind document mode never swallows Tab

    /// ContentView's arrangement: the Library grid stays in the window at opacity 0 (not hidden, still
    /// attached) while document mode shows the document view; optionally the inspector beside it,
    /// removed while the panels are hidden.
    private struct ModeFixture: View {
        let model: AppModel
        var inspector = false

        var body: some View {
            HStack(spacing: 0) {
                ZStack {
                    ThumbnailBrowser(model: model, style: .grid)
                        .opacity(model.viewMode == .grid ? 1 : 0)
                        .allowsHitTesting(model.viewMode == .grid)
                    if model.viewMode == .document {
                        DocumentView(workspace: model.documents)
                    }
                }
                if inspector, model.viewMode == .document, !model.documents.panelsHidden {
                    DocumentInspector(workspace: model.documents)
                        .frame(width: 288)
                }
            }
        }
    }

    private func hostFixture(_ model: AppModel, inspector: Bool = false) -> (NSWindow, NSView) {
        let size = NSSize(width: 1100, height: 848)
        let controller = NSHostingController(rootView: LayoutProbeHarness.root(ModeFixture(model: model, inspector: inspector)))
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

    private func gridModel() -> AppModel {
        let model = AppModel()
        model.documents.engine = StubDocumentEngine()
        model.documents.inspectorTab = .stack
        model.documents.newDocument(model.documents.newSettings)
        model.viewMode = .grid
        return model
    }

    /// B5-49c: the app enters document mode after its window exists. AppKit then leaves each Layers row
    /// with a closed key-view loop of its own, and Tab from a row's eye button found no next key view:
    /// the keyboard stayed on the eye (Machine A, Full Keyboard Access on). The outline now walks out of
    /// the row itself, in either keyboard-access mode and whatever the machine's own setting is.
    func testTabFromRowEyeLeavesTheLayersListWhenTheInspectorJoinsAnExistingWindow() throws {
        for fka in [true, false] {
            try KeyboardAccessHarness.withMode(fka) {
                UserDefaults.standard.set(Double(DocumentInspector.historyDefault + Theme.Height.row),
                                          forKey: "DocumentInspector.historyHeight")
                UserDefaults.standard.set(true, forKey: "InspectorPanel.History")
                let model = gridModel()
                let router = KeyRouter(model: model)
                let (window, host) = hostFixture(model, inspector: true)
                XCTAssertTrue(find(host, LayersOutlineView.self).isEmpty, "fixture: no inspector in the grid")
                model.viewMode = .document
                settle(host)
                let outline = try XCTUnwrap(find(host, LayersOutlineView.self).first)
                let control = try XCTUnwrap(find(host, DocumentHistoryHeightControl.self).first)
                XCTAssertTrue(control.decrease.isEnabled && control.increase.isEnabled && control.reset.isEnabled)
                outline.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
                let row = try XCTUnwrap(outline.rowView(atRow: 0, makeIfNecessary: false))
                let eye = try XCTUnwrap(eyeButtons(row).first, "FKA \(fka): row 0 has an eye button")
                XCTAssertEqual(eye.canBecomeKeyView, fka, "the pinned mode decides the eye's key-view membership")

                // Tab from the eye (forced with FKA off, where it is not a key view) reaches − → + → ↺.
                XCTAssertTrue(window.makeFirstResponder(eye))
                var trail = [name(window.firstResponder)]
                var reached: [HistoryHeightButton] = []
                for _ in 0..<60 {
                    XCTAssertFalse(try tab(window, router: router), "FKA \(fka): Tab taken as the panels key; trail: \(trail)")
                    XCTAssertFalse(model.documents.panelsHidden)
                    trail.append(name(window.firstResponder))
                    if let button = window.firstResponder as? HistoryHeightButton, !reached.contains(where: { $0 === button }) {
                        reached.append(button)
                    }
                    if reached.count == 3 || window.firstResponder === eye { break }
                }
                XCTAssertTrue(reached.count == 3 && reached[0] === control.decrease && reached[1] === control.increase
                              && reached[2] === control.reset, "FKA \(fka): Tab from the eye must reach − → + → ↺; trail: \(trail)")

                // ⇧Tab walks back through the row's key views (a group row has a disclosure button
                // before the eye) to the list that Tab entered the row from.
                XCTAssertTrue(window.makeFirstResponder(eye))
                trail = [name(window.firstResponder)]
                for _ in 0..<4 where window.firstResponder !== outline {
                    XCTAssertFalse(try tab(window, shift: true, router: router))
                    trail.append(name(window.firstResponder))
                    XCTAssertTrue(window.firstResponder === outline || (window.firstResponder as? NSView)?.isDescendant(of: row) == true,
                                  "FKA \(fka): ⇧Tab stays in the row until it reaches the list; trail: \(trail)")
                }
                XCTAssertTrue(window.firstResponder === outline, "FKA \(fka): ⇧Tab from the eye returns to the Layers list; trail: \(trail)")

                // With Full Keyboard Access the list hands Tab to the selected row's controls and the eye
                // is one of them; without it the row is skipped. Either way Tab then leaves the list for −.
                trail = [name(window.firstResponder)]
                var visitedEye = false
                for _ in 0..<4 where window.firstResponder !== control.decrease {
                    XCTAssertFalse(try tab(window, router: router))
                    trail.append(name(window.firstResponder))
                    visitedEye = visitedEye || window.firstResponder === eye
                }
                XCTAssertTrue(window.firstResponder === control.decrease, "FKA \(fka): Tab from the list reaches −; trail: \(trail)")
                XCTAssertEqual(visitedEye, fka, "FKA \(fka): the eye is a Tab stop only with Full Keyboard Access; trail: \(trail)")
                XCTAssertFalse(model.documents.panelsHidden)
                LayoutProbeHarness.dispose(window)
                windows.removeAll { $0 === window }
            }
        }
    }

    /// (1) A focused view inside an alpha-0 container (the grid behind document mode) does not hold Tab:
    /// the router takes it as the panels key.
    func testTabWithFocusedViewUnderAlphaZeroContainerTogglesPanels() throws {
        let model = documentModel()
        let router = KeyRouter(model: model)
        let window = plainWindow()
        let container = NSView(frame: window.contentView!.bounds)
        let proxy = FocusProxyView(frame: NSRect(x: 10, y: 10, width: 40, height: 20))
        container.addSubview(proxy)
        window.contentView?.addSubview(container)
        container.alphaValue = 0
        XCTAssertTrue(window.makeFirstResponder(proxy))
        XCTAssertFalse(KeyRouter.panelViewHasKeyboard(in: window), "an alpha-0 view is not a panel view")
        XCTAssertTrue(router.handle(try key(48, "\t", window: window)), "Tab is not swallowed by an invisible view")
        XCTAssertTrue(model.documents.panelsHidden)
        XCTAssertTrue(router.handle(try key(48, "\t", window: window)))
        XCTAssertFalse(model.documents.panelsHidden)
    }

    /// (1b) The real arrangement: the grid, forced back to first responder behind the document view,
    /// still does not hold Tab.
    func testTabWithGridFocusedBehindDocumentModeTogglesPanels() throws {
        let model = gridModel()
        let router = KeyRouter(model: model)
        let (window, host) = hostFixture(model)
        model.viewMode = .document
        settle(host)
        let grid = try XCTUnwrap(find(host, ThumbnailCollectionView.self).first)
        XCTAssertTrue(window.makeFirstResponder(grid))
        XCTAssertFalse(KeyRouter.panelViewHasKeyboard(in: window), "the grid behind document mode is not a panel view")
        XCTAssertTrue(try tab(window, router: router), "Tab over the invisible grid toggles the panels")
        XCTAssertTrue(model.documents.panelsHidden)
    }

    /// (2) Entering document mode with the grid as first responder (click a thumbnail, then ⌘E; or
    /// --open-document) hands the keyboard to the viewport; so does a document change.
    func testEnteringDocumentModeMovesKeyboardFromGridToViewport() throws {
        let model = gridModel()
        let router = KeyRouter(model: model)
        let (window, host) = hostFixture(model)
        let grid = try XCTUnwrap(find(host, ThumbnailCollectionView.self).first)
        XCTAssertTrue(window.makeFirstResponder(grid), "fixture: the grid has the keyboard")
        model.viewMode = .document
        settle(host)
        let viewport = try XCTUnwrap(find(host, DocumentViewportView.self).first)
        XCTAssertTrue(window.firstResponder === viewport,
                      "document mode gives the viewport the keyboard, got \(name(window.firstResponder))")
        XCTAssertTrue(try tab(window, router: router), "Tab over the canvas hides the panels")
        XCTAssertTrue(model.documents.panelsHidden)
        XCTAssertTrue(try tab(window, router: router))
        XCTAssertFalse(model.documents.panelsHidden)

        // A new current document while the (invisible) grid holds the keyboard.
        XCTAssertTrue(window.makeFirstResponder(grid))
        model.documents.newDocument(model.documents.newSettings)
        settle(host)
        let now = try XCTUnwrap(find(host, DocumentViewportView.self).first)
        XCTAssertTrue(window.firstResponder === now,
                      "a document change gives the viewport the keyboard, got \(name(window.firstResponder))")
    }

    /// (2b) A panel control that has the keyboard keeps it across a document change.
    func testDocumentChangeLeavesAFocusedPanelControlAlone() throws {
        let model = gridModel()
        model.viewMode = .document
        let (window, host) = hostFixture(model, inspector: true)
        let eye = try XCTUnwrap(eyeButtons(host).first)
        XCTAssertTrue(window.makeFirstResponder(eye))
        let viewport = try XCTUnwrap(find(host, DocumentViewportView.self).first)
        viewport.attach(model.documents.current)
        XCTAssertTrue(window.firstResponder === eye, "got \(name(window.firstResponder))")
    }

    /// (3) Realistic restore: a Layers eye button has the keyboard; View ▸ Hide Panels (or the F screen-mode
    /// path) removes the inspector and AppKit drops the focus. Tab then brings the panels back and the
    /// canvas can take the keyboard (and Tab over it hides the panels again).
    func testHidingPanelsWithFocusedPanelControlThenTabRestoresThem() throws {
        for path in ["menu", "F"] {
            let model = gridModel()
            model.viewMode = .document
            let router = KeyRouter(model: model)
            let (window, host) = hostFixture(model, inspector: true)
            let eye = try XCTUnwrap(eyeButtons(host).first, path)
            XCTAssertTrue(window.makeFirstResponder(eye), path)
            XCTAssertTrue(KeyRouter.panelViewHasKeyboard(in: window), path)
            if path == "menu" {
                model.documents.togglePanels()
            } else {
                model.documents.cycleScreenMode()   // standard → full screen (no window here)
                model.documents.cycleScreenMode()   // → full screen without panels
            }
            settle(host)
            XCTAssertTrue(model.documents.panelsHidden, path)
            XCTAssertTrue(eyeButtons(host).isEmpty, "\(path): the inspector left the window")
            XCTAssertFalse(window.firstResponder === eye, "\(path): AppKit moved the focus off the removed button")
            XCTAssertFalse(KeyRouter.panelViewHasKeyboard(in: window),
                           "\(path): nothing that holds Tab is left focused, got \(name(window.firstResponder))")
            XCTAssertTrue(try tab(window, router: router), "\(path): Tab brings the panels back")
            XCTAssertFalse(model.documents.panelsHidden, path)
            settle(host)
            XCTAssertFalse(eyeButtons(host).isEmpty, "\(path): the inspector is back")
            let viewport = try XCTUnwrap(find(host, DocumentViewportView.self).first, path)
            XCTAssertTrue(window.firstResponder === viewport,
                          "\(path): restoring panels must give stray focus to the visible canvas before another key")
            XCTAssertTrue(window.makeFirstResponder(viewport), "\(path): the canvas can take the keyboard")
            XCTAssertTrue(try tab(window, router: router), "\(path): Tab over the canvas hides the panels")
            XCTAssertTrue(model.documents.panelsHidden, path)
            XCTAssertTrue(window.firstResponder === viewport, path)
            for w in windows { w.orderOut(nil); w.contentViewController = nil; w.close() }
            windows = []
        }
    }
    private func assertNonTabPanelRestoration(_ restore: (DocumentWorkspace) -> Void) throws {
        let model = gridModel()
        model.viewMode = .document
        let (window, host) = hostFixture(model, inspector: true)
        let eye = try XCTUnwrap(eyeButtons(host).first)
        XCTAssertTrue(window.makeFirstResponder(eye))
        model.documents.cycleScreenMode()
        model.documents.cycleScreenMode()
        settle(host)
        XCTAssertTrue(model.documents.panelsHidden)
        XCTAssertTrue(eyeButtons(host).isEmpty)
        // Deterministic reproduction of AppKit's fallback after detaching a focused panel.
        XCTAssertTrue(window.makeFirstResponder(nil))
        restore(model.documents)
        XCTAssertFalse(model.documents.panelsHidden)
        let viewport = try XCTUnwrap(find(host, DocumentViewportView.self).first)
        XCTAssertTrue(window.firstResponder === viewport, "Restore must immediately claim stray focus")
        settle(host)
        XCTAssertTrue(window.firstResponder === viewport)
        XCTAssertFalse(window.isKeyWindow)
        XCTAssertFalse(window.isMainWindow)
        XCTAssertFalse(NSApp.isActive)
    }

    func testShowPanelsMenuActionClaimsStrayFocus() throws {
        try assertNonTabPanelRestoration { $0.togglePanels() }
    }

    func testScreenModeReturningToStandardClaimsStrayFocus() throws {
        try assertNonTabPanelRestoration { $0.cycleScreenMode() }
    }

    func testLeavingDocumentModeRestoresPanelsAndClaimsStrayFocus() throws {
        try assertNonTabPanelRestoration { $0.didLeaveDocumentMode() }
    }

}
