import AppKit
import SwiftUI
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-11b: the shapes / Pen / document-mode defects of the on-screen verification of B5-11 (and the
/// B5-10c Type-tool re-check), through the real app paths — an engine document, a
/// `DocumentViewportView` in an off-screen window, synthesized mouse events into the viewport, key
/// events through `KeyRouter` / `DocumentTools.handleKey`:
///  2. a newly enabled stroke takes the foreground colour (or a contrasting one), not the fill;
///  3. keyboard steps of a shape inspector slider are one history node;
///  4. the Pen draft keeps the previous anchor's handles;
///  5. the Layers row shows a vector-mask thumbnail next to the raster one;
///  6. a rejected recolour reverts the colour well;
///  7. tool letters work while a slider or the Layers list has the keyboard;
///  8. Path Selection: a click on empty canvas deselects;
///  9. status hints follow the tool and session (no stale "Pen path discarded" / Remove hint);
/// 10. Properties reports the shape's real bounds, live during drags;
/// 11. new area text resized before the first apply is one "Add Text";
/// 12. the idle Type hint returns after applying;
/// 13. an auto-named text layer's name follows its first line.
/// (Item 1, A cycling Path ↔ Direct Selection, is in `DocumentVectorTests.testToolRoutingKeysAndGroups`.)
@MainActor
final class DocumentVectorVerifyFixesTests: XCTestCase {
    private var model: AppModel!
    private var doc: DocumentController!
    private var window: NSWindow!
    private var viewport: DocumentViewportView!
    private var vector: DocumentVector { .shared }
    private var tools: DocumentTools { .shared }
    private var text: DocumentText { .shared }

    override func setUp() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vector-fixes-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        model = AppModel()
        try model.documents.install(EngineDocumentBackend(session: try engine.newDocument(width: 1200, height: 800, depth: .u8, profile: nil)))
        doc = try XCTUnwrap(model.documents.current)
        window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 900, height: 600), styleMask: [.titled, .resizable],
                          backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        viewport = DocumentViewportView(frame: window.contentView!.bounds)
        viewport.autoresizingMask = [.width, .height]
        window.contentView!.addSubview(viewport)
        viewport.workspace = model.documents
        viewport.attach(doc)
        viewport.zoomToFit()
        tools.attach(model.documents)
        vector.attach(model.documents)
        text.attach(model.documents)
        vector.keyboardCommitDelay = 0.15
    }

    override func tearDown() async throws {
        if text.isEditing { text.cancel() }
        await text.idle()
        await vector.idle()
        if !vector.pen.isEmpty { vector.cancelDraft() }
        await vector.idle()
        vector.keyboardCommitDelay = DocumentVector.defaultKeyboardCommitDelay
        viewport?.attach(nil)
        window?.close()
    }

    // MARK: Helpers

    private var backend: any DocumentVectorBackend { doc.backend as! any DocumentVectorBackend }
    private let red: [Double] = [1, 0, 0, 1]

    @discardableResult
    private func addRect(_ r: CGRect, fill: [Double]? = nil, stroke: (ShapeStroke, ShapePaint)? = nil) throws -> DocLayerID {
        let src = ShapeSource(live: .rectangle(rect: ShapeRect(r), radii: [0, 0, 0, 0]), fill: .solid(fill ?? red), stroke: stroke)
        let c = try backend.addShapeLayer(name: "", parent: nil, index: nil, source: src, transform: .identity)
        doc.reloadModel()
        doc.reloadHistory()
        let id = try XCTUnwrap(c.created.first)
        doc.select(id)
        vector.invalidate()
        return id
    }

    private func mouse(_ type: NSEvent.EventType, _ canvas: CGPoint, flags: NSEvent.ModifierFlags = []) -> NSEvent {
        let p = viewport.convert(viewport.viewPoint(canvas: canvas), to: nil)
        return NSEvent.mouseEvent(with: type, location: p, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
    }

    private func click(_ p: CGPoint) async {
        viewport.mouseDown(with: mouse(.leftMouseDown, p))
        viewport.mouseUp(with: mouse(.leftMouseUp, p))
        await settle()
    }

    private func drag(_ a: CGPoint, _ b: CGPoint, release: Bool = true) async {
        viewport.mouseDown(with: mouse(.leftMouseDown, a))
        for i in 1...6 {
            let t = Double(i) / 6
            viewport.mouseDragged(with: mouse(.leftMouseDragged, CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)))
        }
        if release { viewport.mouseUp(with: mouse(.leftMouseUp, b)) }
        await settle()
    }

    private func settle() async {
        await vector.idle()
        await text.idle()
        await tools.idle()
    }

    private func key(_ code: UInt16, _ chars: String, _ flags: NSEvent.ModifierFlags = []) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0, windowNumber: window.windowNumber,
                         context: nil, characters: chars, charactersIgnoringModifiers: chars, isARepeat: false, keyCode: code)!
    }

    private func labels(after n: Int) -> [String] { Array(doc.history.dropFirst(n).map(\.label)) }

    private func assertRect(_ r: CGRect?, _ x: Double, _ y: Double, _ w: Double, _ h: Double, _ what: String,
                            file: StaticString = #filePath, line: UInt = #line) {
        guard let r else { return XCTFail("\(what): no bounds", file: file, line: line) }
        XCTAssertEqual(Double(r.minX), x, accuracy: 0.51, what, file: file, line: line)
        XCTAssertEqual(Double(r.minY), y, accuracy: 0.51, what, file: file, line: line)
        XCTAssertEqual(Double(r.width), w, accuracy: 1.01, what, file: file, line: line)
        XCTAssertEqual(Double(r.height), h, accuracy: 1.01, what, file: file, line: line)
    }

    // MARK: 2. Stroke colour

    func testNewStrokeTakesTheForegroundOrAContrastingColourNotTheFill() throws {
        let black: [Double] = [0, 0, 0, 1], white: [Double] = [1, 1, 1, 1]
        XCTAssertEqual(ShapeToolOptions.newStrokeColor(fill: .solid(red), foreground: black), black, "foreground differs from the fill")
        XCTAssertEqual(ShapeToolOptions.newStrokeColor(fill: .solid(black), foreground: black), white, "dark fill: white")
        XCTAssertEqual(ShapeToolOptions.newStrokeColor(fill: .solid(white), foreground: white), black, "light fill: black")
        XCTAssertEqual(ShapeToolOptions.newStrokeColor(fill: .solid([0.9, 0.9, 0.2, 1]), foreground: [0.9, 0.9, 0.2, 1]), black)
        XCTAssertEqual(ShapeToolOptions.newStrokeColor(fill: nil, foreground: [0.2, 0.4, 0.6, 1]), [0.2, 0.4, 0.6, 1], "no fill")
        // Through the app: the inspector's Stroke ▸ Solid on a red shape with a red foreground.
        let id = try addRect(CGRect(x: 100, y: 100, width: 200, height: 150))
        tools.colors.foreground = ToolColor(r: 1, g: 0, b: 0)
        let info = try XCTUnwrap(vector.info(for: doc, layer: id))
        guard case .solid(let c) = vector.newStrokePaint(for: info) else { return XCTFail("a solid stroke") }
        XCTAssertNotEqual(c, info.source.fill?.representativeColor, "the stroke never takes the fill colour")
        XCTAssertEqual(c, [1, 1, 1, 1], "a contrasting default (by luminance) on a red fill with a red foreground")
    }

    // MARK: 3. Keyboard slider steps

    func testKeyboardStepsOfAShapeSliderAreOneHistoryNode() async throws {
        let id = try addRect(CGRect(x: 100, y: 100, width: 200, height: 150),
                             stroke: (ShapeStroke(width: 8, dashes: [20, 10]), .solid([0, 0, 0, 1])))
        let base = doc.history.count
        func source(offset: Double) throws -> ShapeSource {
            var s = try XCTUnwrap(vector.info(for: doc, layer: id)).source
            s.stroke?.0.dashOffset = offset
            return s
        }
        let start = try source(offset: 0)
        // Arrow steps (drafts), a Return commit, more steps, a blur commit: one node after the idle.
        for (i, final) in [(1, false), (2, false), (3, true), (4, false), (5, true)] {
            var s = start
            s.stroke?.0.dashOffset = Double(i)
            vector.setSource(doc, layer: id, s, final: final, keyboard: true)
            await vector.idle()
        }
        XCTAssertEqual(doc.history.count, base, "nothing is recorded while keys keep stepping")
        try await Task.sleep(for: .milliseconds(400))
        await vector.idle()
        XCTAssertEqual(labels(after: base), ["Edit Shape"], "one node for the whole keyboard adjustment")
        XCTAssertEqual(try backend.shapeLayer(id).source.stroke?.0.dashOffset, 5)
        // A mouse release after keyboard steps commits at once, still one node.
        let n = doc.history.count
        var s = start
        s.stroke?.0.dashOffset = 9
        vector.setSource(doc, layer: id, s, final: false, keyboard: true)
        s.stroke?.0.dashOffset = 12
        vector.setSource(doc, layer: id, s, final: true, keyboard: false)
        await vector.idle()
        try await Task.sleep(for: .milliseconds(400))
        await vector.idle()
        XCTAssertEqual(labels(after: n), ["Edit Shape"])
        XCTAssertEqual(try backend.shapeLayer(id).source.stroke?.0.dashOffset, 12)
        // Mask sliders coalesce too.
        _ = try backend.setVectorMask(id, mask: VectorMaskInfo(path: ShapePrimitives.rectangle(ShapeRect(x0: 0, y0: 0, x1: 600, y1: 400),
                                                                                            radii: [0, 0, 0, 0])), interactive: false)
        doc.reloadModel(); doc.reloadHistory(); vector.invalidate()
        let m0 = doc.history.count
        var mask = try XCTUnwrap(try backend.vectorMask(id))
        for d in [0.9, 0.8, 0.7] as [Float] {
            mask.density = d
            vector.setMask(doc, layer: id, mask, final: false, keyboard: true)
            await vector.idle()
        }
        try await Task.sleep(for: .milliseconds(400))
        await vector.idle()
        XCTAssertEqual(labels(after: m0), ["Vector Mask Density"])
        XCTAssertEqual(try backend.vectorMask(id)?.density ?? 0, 0.7, accuracy: 1e-6)
    }

    // MARK: 4. Pen handles

    func testPenDraftKeepsThePreviousAnchorsHandles() {
        var pen = PenDraft()
        pen.click(ShapePoint(x: 10, y: 10), radius: 4)
        XCTAssertEqual(pen.handleAnchors, [0])
        pen.click(ShapePoint(x: 60, y: 10), radius: 4)
        pen.drag(to: ShapePoint(x: 80, y: 30), independent: false)
        XCTAssertEqual(pen.handleAnchors, [0, 1])
        pen.click(ShapePoint(x: 60, y: 70), radius: 4)
        XCTAssertEqual(pen.handleAnchors, [1, 2], "the previous anchor's handles stay visible after the next point")
        pen.click(ShapePoint(x: 10, y: 11), radius: 4)
        XCTAssertTrue(pen.closed)
        XCTAssertEqual(pen.handleAnchors, [0, 2], "closing: the first anchor and the last one")
    }

    // MARK: 5. Vector mask thumbnail

    func testLayersRowShowsAVectorMaskThumbnailNextToTheRasterMask() async throws {
        let id = try addRect(CGRect(x: 100, y: 100, width: 200, height: 150))
        let controller = LayersOutlineController()
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 300, height: 400))
        scroll.documentView = controller.outline
        window.contentView!.addSubview(scroll)
        controller.attach(doc)
        func cell() throws -> LayerRowCell {
            let row = controller.outline.row(forItem: controller.outline.item(atRow: 0))
            XCTAssertEqual(controller.outline.numberOfRows, 2)
            let c = try XCTUnwrap(controller.outline.view(atColumn: 0, row: row, makeIfNecessary: true) as? LayerRowCell)
            XCTAssertEqual(c.layerID, id, "the shape is the top row")
            return c
        }
        XCTAssertFalse(try cell().vectorMaskShown, "no vector mask yet")
        _ = try doc.backend.addMask(id: id, mask: .revealAll)
        _ = try backend.setVectorMask(id, mask: VectorMaskInfo(path: ShapePrimitives.rectangle(ShapeRect(x0: 0, y0: 0, x1: 600, y1: 400),
                                                                                            radii: [0, 0, 0, 0])), interactive: false)
        doc.reloadModel()
        let c = try cell()
        XCTAssertTrue(c.rasterMaskShown, "the raster mask thumbnail stays")
        XCTAssertTrue(c.vectorMaskShown, "a separate vector-mask thumbnail")
        XCTAssertNotNil(c.vectorMaskImage)
        _ = try backend.setVectorMask(id, mask: nil, interactive: false)
        doc.reloadModel()
        XCTAssertFalse(try cell().vectorMaskShown, "gone with the mask")
        scroll.removeFromSuperview()
    }

    // MARK: 6. Rejected recolour

    func testRejectedRecolourRevertsTheColourWell() async throws {
        let id = try addRect(CGRect(x: 100, y: 100, width: 200, height: 150))
        doc.toggleLock(.pixels)
        XCTAssertTrue(doc.node(id)?.locks.pixels == true)
        let host = NSHostingView(rootView: ShapeInspector(document: doc, vector: vector, layer: id).frame(width: 280))
        host.frame = NSRect(x: 0, y: 0, width: 280, height: 900)
        window.contentView!.addSubview(host)
        host.layoutSubtreeIfNeeded()
        func well() throws -> NSColorWell {
            func find(_ v: NSView) -> NSColorWell? {
                if let w = v as? NSColorWell, w.accessibilityIdentifier() == "document.shape.fill.color" { return w }
                return v.subviews.lazy.compactMap(find).first
            }
            return try XCTUnwrap(find(host))
        }
        let w = try well()
        let before = vector.rejections
        w.color = NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1)   // lint:allow (test colour)
        XCTAssertTrue(w.sendAction(w.action, to: w.target))
        await vector.idle()
        for _ in 0..<5 { try await Task.sleep(for: .milliseconds(50)); host.layoutSubtreeIfNeeded() }
        XCTAssertEqual(vector.rejections, before + 1, "the engine rejected the locked recolour")
        let shown = try XCTUnwrap(try well().color.usingColorSpace(.sRGB))
        XCTAssertEqual(Double(shown.redComponent), 1, accuracy: 0.01, "the well shows the model colour again")
        XCTAssertEqual(Double(shown.blueComponent), 0, accuracy: 0.01)
        XCTAssertEqual(try backend.shapeLayer(id).source.fill, .solid(red))
        host.removeFromSuperview()
    }

    // MARK: 7. Tool letters with a slider / the Layers list focused

    func testToolLettersWorkWhileASliderOrTheLayersListHasTheKeyboard() throws {
        let router = KeyRouter(model: model)
        let slider = ValueSlider(frame: NSRect(x: 0, y: 0, width: 200, height: 24))
        slider.step = 1
        window.contentView!.addSubview(slider)
        XCTAssertTrue(window.makeFirstResponder(slider))
        XCTAssertTrue(router.handle(key(32, "u")), "U with a slider focused")
        XCTAssertEqual(doc.tool, .rectangleShape)
        XCTAssertTrue(router.handle(key(32, "U", .shift)), "⇧U with a slider focused")
        XCTAssertEqual(doc.tool, .ellipseShape)
        XCTAssertFalse(router.handle(key(124, "\u{F703}")), "the slider keeps its arrows")
        XCTAssertTrue(window.firstResponder === slider)
        let outline = LayersOutlineView(frame: NSRect(x: 0, y: 0, width: 200, height: 200))
        window.contentView!.addSubview(outline)
        XCTAssertTrue(window.makeFirstResponder(outline))
        XCTAssertTrue(router.handle(key(6, "z")), "Z with the Layers list focused")
        XCTAssertEqual(doc.tool, .zoom)
        XCTAssertTrue(router.handle(key(0, "a")))
        XCTAssertTrue(router.handle(key(0, "a")))
        XCTAssertEqual(doc.tool, .directSelect, "A twice through the router")
        // Text input keeps its letters.
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 100, height: 20))
        window.contentView!.addSubview(field)
        XCTAssertTrue(window.makeFirstResponder(field))
        XCTAssertFalse(router.handle(key(32, "u")), "a focused text field keeps U")
        XCTAssertEqual(doc.tool, .directSelect)
        window.contentView!.addSubview(text.input)
        XCTAssertTrue(window.makeFirstResponder(text.input))
        XCTAssertFalse(router.handle(key(9, "v")), "the Type tool's input keeps V")
        XCTAssertEqual(doc.tool, .directSelect)
        text.input.removeFromSuperview()
    }

    // MARK: 8. Path Selection deselect

    func testPathSelectionClickOnEmptyCanvasDeselects() async throws {
        let id = try addRect(CGRect(x: 100, y: 100, width: 200, height: 150))
        tools.select(.pathSelect)
        XCTAssertNotNil(vector.affine, "the selected shape shows its box")
        let n = doc.history.count
        await click(CGPoint(x: 1000, y: 700))
        XCTAssertNil(vector.affine, "a click on empty canvas deselects the path")
        vector.refreshAffine()
        XCTAssertNil(vector.affine, "and the box does not come back on the next redraw")
        XCTAssertEqual(doc.history.count, n, "no history for a deselect")
        XCTAssertEqual(try backend.shapeLayer(id).transform, .identity)
        await click(CGPoint(x: 200, y: 175))
        XCTAssertNotNil(vector.affine, "clicking the shape selects it again")
        XCTAssertEqual(vector.affineLayer, id)
        // A drag outside the box still rotates.
        await drag(CGPoint(x: 700, y: 175), CGPoint(x: 700, y: 400))
        XCTAssertEqual(labels(after: n), ["Transform Shape"])
    }

    // MARK: 9. Status hints

    func testStatusHintsFollowTheToolAndPenState() async throws {
        tools.select(.pen)
        let idle = try XCTUnwrap(tools.hint(for: doc))
        XCTAssertEqual(model.statusMessage, idle, "choosing the Pen shows its hint")
        await click(CGPoint(x: 100, y: 100))
        XCTAssertNotEqual(model.statusMessage, idle, "drawing a path: the drafting hint")
        await click(CGPoint(x: 300, y: 120))
        XCTAssertTrue(tools.handleKey(key(53, "\u{1b}")))
        XCTAssertEqual(model.statusMessage, "Pen path discarded")
        await click(CGPoint(x: 100, y: 100))
        await click(CGPoint(x: 300, y: 120))
        await click(CGPoint(x: 200, y: 300))
        let n = doc.history.count
        XCTAssertTrue(tools.handleKey(key(36, "\r")))
        await settle()
        XCTAssertEqual(labels(after: n).count, 1, "Return created the shape")
        XCTAssertEqual(model.statusMessage, idle, "no stale \"Pen path discarded\" after Return created a shape")
        // Switching tools replaces another tool's hint (the Remove tool's, here).
        model.statusMessage = "Remove: paint over what to remove; release removes it."
        tools.select(.move)
        XCTAssertEqual(model.statusMessage, DocumentTool.move.idleHint)
        tools.select(.pathSelect)
        XCTAssertEqual(model.statusMessage, DocumentTool.pathSelect.idleHint)
        for t in DocumentTool.allCases { XCTAssertFalse(t.idleHint.isEmpty, "\(t) has a hint") }
    }

    // MARK: 10. Bounds

    func testPropertiesReportTheShapesRealBoundsLiveDuringDrags() async throws {
        let id = try addRect(CGRect(x: 100, y: 100, width: 200, height: 150))
        assertRect(vector.displayBounds(doc, layer: id), 100, 100, 200, 150, "new shape")
        let node = try XCTUnwrap(doc.node(id))
        XCTAssertEqual(PropertiesPanel.boundsText(node, shapeBounds: vector.displayBounds(doc, layer: id)), "100, 100 · 200 × 150 px")
        XCTAssertEqual(ShapeInspector.positionText(vector.displayBounds(doc, layer: id)), "100.0, 100.0 px")
        tools.select(.pathSelect)
        await drag(CGPoint(x: 200, y: 175), CGPoint(x: 260, y: 205), release: false)
        assertRect(vector.displayBounds(doc, layer: id), 160, 130, 200, 150, "live during the drag")
        viewport.mouseUp(with: mouse(.leftMouseUp, CGPoint(x: 260, y: 205)))
        await settle()
        assertRect(vector.displayBounds(doc, layer: id), 160, 130, 200, 150, "after the drag")
        XCTAssertEqual(PropertiesPanel.boundsText(try XCTUnwrap(doc.node(id)), shapeBounds: vector.displayBounds(doc, layer: id)),
                       "160, 130 · 200 × 150 px")
    }

    // MARK: 11–13. Type tool

    func testNewAreaTextResizedBeforeTheFirstApplyIsOneAddText() async throws {
        tools.select(.type)
        let n = doc.history.count
        await drag(CGPoint(x: 100, y: 100), CGPoint(x: 400, y: 300))
        XCTAssertEqual(text.session?.edit.model.textBox.isParagraph, true)
        text.input.insertText("abc", replacementRange: NSRange(location: NSNotFound, length: 0))
        await settle()
        // Bottom-right handle of the new 300 × 200 box.
        await drag(CGPoint(x: 400, y: 300), CGPoint(x: 450, y: 350))
        XCTAssertEqual(text.session?.edit.model.textBox.size?.width ?? 0, 350, accuracy: 1, "the handle resized the box")
        text.apply()
        await settle()
        XCTAssertEqual(labels(after: n), ["Add Text"], "one Add Text, the resize folded in")
        let id = try XCTUnwrap(doc.primary?.id)
        XCTAssertEqual(try (doc.backend as! any DocumentTextBackend).textLayer(id: id).model.textBox.size?.width ?? 0, 350, accuracy: 1)
        // 12: the idle Type hint returns after applying.
        XCTAssertEqual(model.statusMessage, DocumentTool.type.idleHint, "the idle Type hint after applying")
    }

    func testAutoNamedTextLayerNameFollowsItsFirstLine() async throws {
        tools.select(.type)
        let t = doc.backend as! any DocumentTextBackend
        let c = try t.addTextLayer(name: "", parent: nil, index: nil, model: .point("Hi", family: "Helvetica", size: 48),
                                   transform: .translation(200, 200), interactive: false)
        doc.reloadModel(); doc.reloadHistory()
        let id = try XCTUnwrap(c.created.first)
        XCTAssertEqual(doc.node(id)?.name, "Hi")
        XCTAssertTrue(text.beginExisting(doc, layer: id))
        text.input.insertText("ZQ", replacementRange: NSRange(location: NSNotFound, length: 0))
        await settle()
        text.apply()
        await settle()
        let typed = try t.textLayer(id: id).model.text
        XCTAssertTrue(typed.contains("ZQ"), typed)
        XCTAssertEqual(doc.node(id)?.name, typed, "the auto name follows the text")
        doc.rename(id, to: "Title")
        XCTAssertTrue(text.beginExisting(doc, layer: id))
        text.input.insertText("!", replacementRange: NSRange(location: NSNotFound, length: 0))
        await settle()
        text.apply()
        await settle()
        XCTAssertEqual(doc.node(id)?.name, "Title", "a user rename sticks")
    }
}
