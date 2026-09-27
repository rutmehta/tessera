import AppKit
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-10c: the Type tool defects of the on-screen verification of B5-10, through the real app paths
/// (an engine document, a `DocumentViewportView` in an off-screen window, synthesized mouse events
/// into the viewport, key events through `KeyRouter`):
///  1. ⌘Return / keypad Enter / Esc apply or cancel after a box handle drag and with any view of the
///     document window focused;
///  2. a click just right of the last glyph resumes the text layer;
///  3. the status hint follows the session;
///  4. typing latency counts keystrokes only and excludes inactive time;
///  5. the area-text box outline is dashed.
@MainActor
final class DocumentTextSessionTests: XCTestCase {
    private var model: AppModel!
    private var doc: DocumentController!
    private var window: NSWindow!
    private var viewport: DocumentViewportView!
    private var text: DocumentText { .shared }

    override func setUp() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("text-session-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        model = AppModel()
        try model.documents.install(EngineDocumentBackend(session: try engine.newDocument(width: 1200, height: 800, depth: .u8, profile: nil)))
        doc = try XCTUnwrap(model.documents.current)
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 600), styleMask: [.titled, .resizable],
                          backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        viewport = DocumentViewportView(frame: window.contentView!.bounds)
        viewport.autoresizingMask = [.width, .height]
        window.contentView!.addSubview(viewport)
        viewport.workspace = model.documents
        viewport.attach(doc)
        viewport.zoomToFit()
        DocumentTools.shared.attach(model.documents)
        text.attach(model.documents)
        doc.tool = .type
    }

    override func tearDown() async throws {
        if text.isEditing { text.cancel() }
        await text.idle()
        viewport?.attach(nil)
        window?.close()
    }

    // MARK: Helpers

    private var backend: any DocumentTextBackend { doc.backend as! any DocumentTextBackend }

    private func addText(_ m: TextSourceModel, at t: AffineTransform2D) throws -> DocLayerID {
        let c = try backend.addTextLayer(name: "", parent: nil, index: nil, model: m, transform: t, interactive: false)
        doc.reloadModel()
        doc.reloadHistory()
        return try XCTUnwrap(c.created.first)
    }

    private func mouse(_ type: NSEvent.EventType, _ canvas: CGPoint, flags: NSEvent.ModifierFlags = []) -> NSEvent {
        let p = viewport.convert(viewport.viewPoint(canvas: canvas), to: nil)
        return NSEvent.mouseEvent(with: type, location: p, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
    }

    private func click(_ p: CGPoint) async {
        viewport.mouseDown(with: mouse(.leftMouseDown, p))
        viewport.mouseUp(with: mouse(.leftMouseUp, p))
        await text.idle()
    }

    private func drag(_ a: CGPoint, _ b: CGPoint) async {
        viewport.mouseDown(with: mouse(.leftMouseDown, a))
        for i in 1...6 {
            let t = Double(i) / 6
            viewport.mouseDragged(with: mouse(.leftMouseDragged, CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)))
        }
        viewport.mouseUp(with: mouse(.leftMouseUp, b))
        await text.idle()
    }

    private func key(_ code: UInt16, _ chars: String, _ flags: NSEvent.ModifierFlags = []) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0, windowNumber: window.windowNumber,
                         context: nil, characters: chars, charactersIgnoringModifiers: chars, isARepeat: false, keyCode: code)!
    }
    private var commandReturn: NSEvent { key(36, "\r", .command) }
    private var keypadEnter: NSEvent { key(76, "\u{3}", .numericPad) }
    private var escape: NSEvent { key(53, "\u{1b}") }

    private func areaLayer() throws -> DocLayerID {
        var m = TextSourceModel(runs: [TextRunModel(text: "Area text wraps inside its box", family: "Helvetica", size: 48)])
        m.textBox = .paragraph(width: 300, height: 160)
        return try addText(m, at: .translation(100, 100))
    }

    private func pointLayer() throws -> (DocLayerID, TextLayoutInfo, AffineTransform2D) {
        let m = TextSourceModel.point("Hello", family: "Helvetica", size: 48)
        let t = AffineTransform2D.translation(600, 500)
        let id = try addText(m, at: t)
        return (id, try backend.layoutText(m), t)
    }

    // MARK: 1. Apply / cancel keys

    func testCommandReturnAppliesAfterABoxHandleDrag() async throws {
        let id = try areaLayer()
        XCTAssertTrue(text.beginExisting(doc, layer: id))
        XCTAssertTrue(window.firstResponder === text.input)
        text.input.insertText("!", replacementRange: NSRange(location: NSNotFound, length: 0))
        await text.idle()
        let h = doc.history.count
        // Bottom-right handle of the 300 × 160 box at (100, 100).
        await drag(CGPoint(x: 400, y: 260), CGPoint(x: 330, y: 330))
        await text.idle()
        XCTAssertEqual(text.session?.edit.model.textBox.size?.width ?? 0, 230, accuracy: 1, "the handle resized the box")
        XCTAssertTrue(window.firstResponder === text.input, "the handle drag gives the keyboard back to the text")
        XCTAssertEqual(doc.history.last?.label, "Resize Text Box")
        text.input.insertText("?", replacementRange: NSRange(location: NSNotFound, length: 0))
        await text.idle()
        // ⌘Return through the app's key routing (the monitor, then the first responder).
        let router = KeyRouter(model: model)
        if !router.handle(commandReturn) { window.sendEvent(commandReturn) }
        await text.idle()
        XCTAssertFalse(text.isEditing, "⌘Return applied")
        XCTAssertEqual(doc.history.last?.label, "Edit Text")
        XCTAssertGreaterThan(doc.history.count, h)
    }

    func testApplyAndCancelKeysReachTheSessionWhateverViewIsFocused() async throws {
        let id = try areaLayer()
        let router = KeyRouter(model: model)
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 100, height: 20))
        window.contentView!.addSubview(field)
        for (name, event, applies) in [("⌘Return", commandReturn, true), ("keypad Enter", keypadEnter, true), ("Esc", escape, false)] {
            for focus in ["viewport", "field", "none"] {
                XCTAssertTrue(text.beginExisting(doc, layer: id))
                text.input.insertText("x", replacementRange: NSRange(location: NSNotFound, length: 0))
                await text.idle()
                let h = doc.history.count
                switch focus {
                case "viewport": window.makeFirstResponder(viewport)
                case "field": window.makeFirstResponder(field)
                default: window.makeFirstResponder(nil)
                }
                XCTAssertTrue(router.handle(event), "\(name) with the \(focus) focused is routed to the text session")
                await text.idle()
                XCTAssertFalse(text.isEditing, "\(name) (\(focus)) ends the session")
                XCTAssertEqual(doc.history.count, applies ? h + 1 : h, "\(name) (\(focus))")
            }
        }
        // Without a session the keys keep their usual meaning (nothing to apply: not handled).
        window.makeFirstResponder(viewport)
        XCTAssertFalse(router.handle(commandReturn))
    }

    // MARK: 2. Resume on a click right of the last glyph

    func testClickJustRightOfTheLastGlyphResumesEditing() async throws {
        let (id, layout, t) = try pointLayer()
        let bounds = try XCTUnwrap(layout.bounds)
        let line = try XCTUnwrap(layout.lines.first)
        let layers = doc.layers.count
        // 10 px right of the last glyph's advance, mid x-height.
        let right = t.apply(CGPoint(x: bounds.maxX + 10, y: line.baseline - line.ascent * 0.4))
        await click(right)
        XCTAssertEqual(text.session?.layer, id, "the click resumes the layer")
        XCTAssertEqual(text.session?.edit.caret, 5, "caret at the end")
        // With the session open, another click there keeps it (no apply, no new text).
        await click(t.apply(CGPoint(x: bounds.maxX + 14, y: line.baseline - 2)))
        XCTAssertEqual(text.session?.layer, id)
        text.cancel()
        await text.idle()
        XCTAssertEqual(doc.layers.count, layers, "no new text layer")
        // Clearly away from the text still starts new text.
        await click(t.apply(CGPoint(x: bounds.maxX + 3 * (line.ascent + line.descent), y: line.baseline)))
        XCTAssertTrue(text.isEditing)
        XCTAssertNil(text.session?.layer, "far right starts new text")
    }

    func testHitRegionIsTheLineBoxPlusTheTrailingMargin() {
        let g = [TextGlyphInfo(run: 0, cluster: 0, x: 0, y: 40, advance: 20, rtl: false),
                 TextGlyphInfo(run: 0, cluster: 1, x: 20, y: 40, advance: 20, rtl: false)]
        let layout = TextLayoutInfo(glyphs: g, lines: [TextLineInfo(source: 0..<2, glyphs: 0..<2, x: 0, baseline: 40, width: 40,
                                                                    availableWidth: nil, ascent: 36, descent: 12)],
                                    overflow: false, textLength: 2)
        // Line height 48 → trailing margin 24 px (above the 12 pt minimum at 1 px per point).
        XCTAssertTrue(TextHitRegion.contains(CGPoint(x: 63, y: 30), layout: layout, box: nil, pixelsPerPoint: 1))
        XCTAssertFalse(TextHitRegion.contains(CGPoint(x: 65, y: 30), layout: layout, box: nil, pixelsPerPoint: 1))
        XCTAssertTrue(TextHitRegion.contains(CGPoint(x: -23, y: 30), layout: layout, box: nil, pixelsPerPoint: 1))
        XCTAssertFalse(TextHitRegion.contains(CGPoint(x: 30, y: 57), layout: layout, box: nil, pixelsPerPoint: 1))
        // Zoomed out (4 px per point): the 12 pt minimum wins (48 px).
        XCTAssertTrue(TextHitRegion.contains(CGPoint(x: 87, y: 30), layout: layout, box: nil, pixelsPerPoint: 4))
        // Area text: the box plus 4 px.
        XCTAssertTrue(TextHitRegion.contains(CGPoint(x: 103, y: 50), layout: layout, box: CGSize(width: 100, height: 60), pixelsPerPoint: 1))
        XCTAssertFalse(TextHitRegion.contains(CGPoint(x: 105, y: 50), layout: layout, box: CGSize(width: 100, height: 60), pixelsPerPoint: 1))
    }

    // MARK: 3. Status hint

    func testStatusHintFollowsTheSession() async throws {
        let (id, layout, t) = try pointLayer()
        let bounds = try XCTUnwrap(layout.bounds)
        let onText = t.apply(CGPoint(x: bounds.midX, y: bounds.midY))
        // New area text.
        await drag(CGPoint(x: 100, y: 100), CGPoint(x: 400, y: 300))
        XCTAssertEqual(text.session?.edit.model.textBox.isParagraph, true)
        XCTAssertTrue(model.statusMessage?.hasPrefix("Area text") == true, model.statusMessage ?? "-")
        text.cancel()
        await text.idle()
        // Resuming point text: the hint is the point-text one, not the stale area-text one.
        await click(onText)
        XCTAssertEqual(text.session?.layer, id)
        XCTAssertTrue(model.statusMessage?.hasPrefix("Point text") == true, model.statusMessage ?? "-")
        text.input.insertText("!", replacementRange: NSRange(location: NSNotFound, length: 0))
        await text.idle()
        text.cancel()
        await text.idle()
        XCTAssertEqual(model.statusMessage, "Type: cancelled")
        // A new edit after a cancel shows the editing hint, not "Type: cancelled".
        await click(onText)
        XCTAssertTrue(text.isEditing)
        XCTAssertTrue(model.statusMessage?.hasPrefix("Point text") == true, model.statusMessage ?? "-")
        // Point ⇄ area while editing updates it too.
        text.toggleBox(doc)
        await text.idle()
        XCTAssertTrue(model.statusMessage?.hasPrefix("Area text") == true, model.statusMessage ?? "-")
    }

    // MARK: 4. Latency

    func testLatencyCountsKeystrokesOnlyAndExcludesInactiveTime() {
        var m = TypingLatencyMeter()
        m.keystroke(at: 10.00)
        m.keystroke(at: 10.01)   // coalesced: the oldest unsent keystroke is measured
        let k = m.takePending()
        m.previewAccepted(epoch: 3, key: k)
        XCTAssertEqual(m.frame(epoch: 2, at: 10.02), [])
        XCTAssertEqual(m.frame(epoch: 3, at: 10.03).first ?? 0, 30, accuracy: 0.001)
        // A box / move / rotate preview long after the last key carries no keystroke.
        let g = m.takePending()
        XCTAssertNil(g)
        m.previewAccepted(epoch: 4, key: g)
        XCTAssertEqual(m.frame(epoch: 4, at: 90), [])
        // Time while the window is hidden or the app inactive is excluded.
        m.keystroke(at: 100)
        m.previewAccepted(epoch: 5, key: m.takePending())
        m.setActive(false, at: 100.01)
        m.setActive(true, at: 160)
        XCTAssertEqual(m.frame(epoch: 5, at: 160.02).first ?? 0, 30, accuracy: 0.001)
        // A keystroke typed while inactive is not counted.
        m.setActive(false, at: 200)
        m.keystroke(at: 201)
        m.previewAccepted(epoch: 6, key: m.takePending())
        m.setActive(true, at: 230)
        XCTAssertEqual(m.frame(epoch: 6, at: 230.01), [])
        XCTAssertEqual(m.samples.count, 2)
        XCTAssertEqual(m.excluded, 1)
        let r = m.readout ?? ""
        XCTAssertTrue(r.hasPrefix("Keystroke → rendered frame: median 30.0 ms · p95 30.0 ms (2 keys, inactive time excluded"), r)
    }

    func testSessionMeasuresKeystrokesNotGestures() async throws {
        let id = try areaLayer()
        XCTAssertTrue(text.beginExisting(doc, layer: id))
        let seen = text.latencyKeysSeen
        text.input.insertText("!", replacementRange: NSRange(location: NSNotFound, length: 0))
        await text.idle()
        XCTAssertEqual(text.latencyKeysSeen - seen, 1, "the keystroke's preview is measured (awaiting, measured or excluded)")
        // A handle drag later: its previews carry no keystroke.
        await drag(CGPoint(x: 400, y: 260), CGPoint(x: 330, y: 330))
        await text.idle()
        XCTAssertEqual(text.latencyKeysSeen - seen, 1, "gesture previews are not measured")
    }

    // MARK: 5. Dashed area box

    func testTextFrameIsDashedForPointAndAreaText() {
        XCTAssertTrue(DocumentText.frameDashed(.point))
        XCTAssertTrue(DocumentText.frameDashed(.paragraph(width: 300, height: 160)), "the area-text box is dashed (DESIGN.md)")
    }
}
