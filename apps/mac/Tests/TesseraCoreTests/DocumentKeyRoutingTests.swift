import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// Keyboard routing by mode (WP B5-02): in document mode the document key map runs and the
/// culling single-key shortcuts never fire; outside it, V / M / Tab mean nothing new.
@MainActor
final class DocumentKeyRoutingTests: XCTestCase {
    private func key(_ code: UInt16, _ character: String, window: NSWindow, type: NSEvent.EventType = .keyDown,
                     modifiers: NSEvent.ModifierFlags = []) -> NSEvent {
        NSEvent.keyEvent(with: type, location: .zero, modifierFlags: modifiers, timestamp: 0, windowNumber: window.windowNumber,
                         context: nil, characters: character, charactersIgnoringModifiers: character, isARepeat: false,
                         keyCode: code)!
    }

    private func makeWindow() -> NSWindow {
        LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100), styleMask: .titled, backing: .buffered, defer: false)
    }

    func testCullingKeysDoNotFireInDocumentMode() throws {
        let model = AppModel()
        model.loadStubItems(count: 10)
        model.documents.engine = StubDocumentEngine()
        model.documents.newDocument(NewDocumentSettings(width: 400, height: 300))
        XCTAssertEqual(model.viewMode, .document)
        let router = KeyRouter(model: model)
        let window = makeWindow()
        for (code, ch) in [(7, "x"), (35, "p"), (18, "1"), (11, "b"), (40, "k"), (8, "c"), (5, "g"), (14, "e")] as [(UInt16, String)] {
            XCTAssertFalse(router.handle(key(code, ch, window: window)), ch)
        }
        XCTAssertEqual(model.focusedState.decision, .undecided)
        XCTAssertEqual(model.viewMode, .document, "G / E do not leave document mode")
        XCTAssertFalse(router.handle(key(124, "\u{F703}", window: window)), "arrows do not navigate the library")
        XCTAssertEqual(model.focusedPosition, 0)
    }

    /// B5-v step 144: ⌘E in the grid runs Library ▸ Edit in Layers on the focused photo, also after a
    /// round trip through document mode (where ⌘E is Merge Down and the router leaves it to the menu).
    func testCommandEInGridDisclosesRenderedCopyBeforeExistingHandoff() throws {
        let model = AppModel()
        model.loadStubItems(count: 5)
        model.documents.engine = StubDocumentEngine()
        let router = KeyRouter(model: model)
        let window = makeWindow()
        let cmdE = key(14, "e", window: window, modifiers: .command)
        model.viewMode = .grid
        XCTAssertNotNil(model.focusedItem)
        XCTAssertTrue(router.handle(cmdE), "⌘E is handled in the grid")
        XCTAssertEqual(model.layeredCopyRequest?.item.id, model.focusedItem?.id)
        XCTAssertTrue(model.documents.documents.isEmpty, "disclosure does not create a document")
        model.createRequestedLayeredCopy()
        XCTAssertEqual(model.statusMessage, "Edit in Layers needs a photo file (stub items have none)",
                       "the route reached editInLayers with the grid's focused photo")

        model.documents.newDocument(NewDocumentSettings(width: 400, height: 300))
        XCTAssertEqual(model.viewMode, .document)
        XCTAssertFalse(router.handle(cmdE), "in document mode ⌘E is the menu's Merge Down")
        model.viewMode = .grid
        model.statusMessage = nil
        XCTAssertTrue(router.handle(cmdE), "back in the grid ⌘E discloses the rendered-copy action again")
        XCTAssertNotNil(model.layeredCopyRequest)
        model.createRequestedLayeredCopy()
        XCTAssertEqual(model.statusMessage, "Edit in Layers needs a photo file (stub items have none)")
        model.viewMode = .loupe
        XCTAssertTrue(router.handle(cmdE), "the loupe too")

        XCTAssertFalse(router.handle(key(14, "e", window: window, modifiers: [.command, .shift])), "⇧⌘E is not Edit in Layers")
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 100, height: 20))
        window.contentView?.addSubview(field)
        window.makeFirstResponder(field)
        XCTAssertFalse(router.handle(cmdE), "a focused text field keeps ⌘E")
    }

    func testDocumentKeys() throws {
        let model = AppModel()
        model.documents.engine = StubDocumentEngine()
        model.documents.newDocument(NewDocumentSettings(width: 400, height: 300))
        let doc = try XCTUnwrap(model.documents.current)
        let router = KeyRouter(model: model)
        let window = makeWindow()
        XCTAssertTrue(router.handle(key(46, "m", window: window)))
        XCTAssertEqual(doc.tool, .marquee)
        XCTAssertTrue(router.handle(key(9, "v", window: window)))
        XCTAssertEqual(doc.tool, .move)
        XCTAssertTrue(router.handle(key(49, " ", window: window)))
        XCTAssertTrue(model.documents.spaceHeld)
        XCTAssertTrue(router.handleKeyUp(key(49, " ", window: window, type: .keyUp)))
        XCTAssertFalse(model.documents.spaceHeld)
        XCTAssertTrue(router.handle(key(48, "\t", window: window)))
        XCTAssertTrue(model.documents.panelsHidden)
        XCTAssertFalse(model.showInspector)
        model.viewMode = .grid
        XCTAssertFalse(model.documents.panelsHidden, "leaving document mode restores the panels")
        XCTAssertFalse(router.handle(key(9, "v", window: window)), "V is not a library key")
    }

    func testUndoRoutesToTheDocumentInDocumentMode() throws {
        let model = AppModel()
        model.loadStubItems(count: 5)
        model.documents.engine = StubDocumentEngine()
        model.documents.newDocument(NewDocumentSettings(width: 400, height: 300))
        let doc = try XCTUnwrap(model.documents.current)
        doc.setVisible(3, false)
        XCTAssertEqual(doc.node(3)?.visible, false)
        model.undo()
        XCTAssertEqual(doc.node(3)?.visible, true)
        model.redo()
        XCTAssertEqual(doc.node(3)?.visible, false)
        XCTAssertEqual(doc.history.count, 1)
    }

    func testControllerGroupsAndDragsAsOneStepEach() throws {
        let model = AppModel()
        model.documents.engine = StubDocumentEngine()
        model.documents.newDocument(NewDocumentSettings(width: 400, height: 300))
        let doc = try XCTUnwrap(model.documents.current)
        doc.selection = [2, 3]
        doc.groupSelection()
        XCTAssertEqual(doc.history.map(\.label), ["Group Layers"])
        let group = try XCTUnwrap(doc.primary)
        XCTAssertEqual(group.kind, .group)
        XCTAssertEqual(doc.outline.children(of: group.id), [3, 2])
        doc.ungroupSelection()
        XCTAssertEqual(doc.outline.children(of: DocumentOutline.root), [4, 3, 2, 1])
        XCTAssertTrue(doc.moveLayers([1], into: DocumentOutline.root, at: 0))
        XCTAssertEqual(doc.outline.children(of: DocumentOutline.root).first, 1)
        doc.setOpacity(40, final: false)
        doc.setOpacity(30, final: true)
        XCTAssertEqual(doc.history.last?.label, "Opacity 30 %")
        doc.selectAll()
        XCTAssertEqual(doc.marquee, CanvasRect(x: 0, y: 0, width: 400, height: 300))
        doc.deselect()
        XCTAssertNil(doc.marquee)
        model.documents.close(doc)   // dirty, but no window: closes without the sheet
        XCTAssertNil(model.documents.current)
    }
}
