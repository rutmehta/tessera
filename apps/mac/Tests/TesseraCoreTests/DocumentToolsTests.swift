import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// The layered editor's tools (WP B5-04): stroke coalescing and the pressure curve, brush HUD maths,
/// selection modifier mapping, transform matrix composition, the tool key map, and the engine adapter
/// of `DocumentToolsBackend` (a real session: stroke, selections, outline, transform, fill, eyedropper).
final class DocumentToolsTests: XCTestCase {
    private func close(_ a: CGPoint, _ b: CGPoint, _ tol: Double = 1e-9, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(a.x, b.x, accuracy: tol, file: file, line: line)
        XCTAssertEqual(a.y, b.y, accuracy: tol, file: file, line: line)
    }

    // MARK: Stroke coalescing and pressure

    func testCoalescerBatchesSamplesWhileAFrameIsInFlight() {
        var c = FrameStrokeCoalescer(minSpacing: 0.5)
        XCTAssertNil(c.nextBatch(), "nothing pending")
        c.add(PenSample(x: 0, y: 0))
        let first = c.nextBatch()
        XCTAssertEqual(first?.count, 1)
        XCTAssertTrue(c.inFlight)
        // A frame is in flight: samples accumulate, nothing is sent.
        for i in 1...10 { c.add(PenSample(x: Float(i), y: 0)) }
        XCTAssertNil(c.nextBatch())
        XCTAssertEqual(c.pending.count, 10)
        c.batchDone()
        let second = c.nextBatch()
        XCTAssertEqual(second?.map(\.x), (1...10).map(Float.init), "one call carries every sample of the frame, in order")
        XCTAssertEqual(c.batches, 2)
        c.batchDone()
        // Samples closer than the spacing (same pressure) are dropped; a pressure change is kept.
        c.add(PenSample(x: 10.2, y: 0))
        XCTAssertTrue(c.pending.isEmpty)
        c.add(PenSample(x: 10.2, y: 0, pressure: 0.5))
        XCTAssertEqual(c.pending.count, 1)
        c.add(PenSample(x: 20, y: 0))
        // Stroke end drains regardless of an in-flight frame.
        _ = c.nextBatch()
        c.add(PenSample(x: 30, y: 0))
        XCTAssertEqual(c.drain().map(\.x), [30])
        XCTAssertTrue(c.pending.isEmpty)
        XCTAssertEqual(c.accepted, 14)
    }

    func testPressureCurve() {
        let linear = PressureCurve()
        XCTAssertEqual(linear.map(0), 0)
        XCTAssertEqual(linear.map(0.5), 0.5, accuracy: 1e-6)
        XCTAssertEqual(linear.map(1), 1)
        XCTAssertEqual(linear.map(1.7), 1, "clamped")
        XCTAssertEqual(linear.map(-1), 0)
        XCTAssertEqual(linear.map(.nan), 1, "no reading = full pressure")
        let soft = PressureCurve(gamma: 0.5)
        XCTAssertEqual(soft.map(0.25), 0.5, accuracy: 1e-6)
        let firm = PressureCurve(gamma: 2, minimum: 0.2)
        XCTAssertEqual(firm.map(0), 0.2, accuracy: 1e-6)
        XCTAssertEqual(firm.map(0.5), 0.2 + 0.8 * 0.25, accuracy: 1e-6)
        XCTAssertEqual(firm.map(1), 1, accuracy: 1e-6)
        // Monotonic.
        let v = stride(from: 0.0, through: 1.0, by: 0.05).map { firm.map($0) }
        XCTAssertEqual(v, v.sorted())
    }

    // MARK: HUD maths

    func testBracketKeysAndHardness() {
        XCTAssertEqual(BrushHUDMath.bracket(size: 5, larger: true), 6)
        XCTAssertEqual(BrushHUDMath.bracket(size: 10, larger: true), 15)
        XCTAssertEqual(BrushHUDMath.bracket(size: 100, larger: true), 125)
        XCTAssertEqual(BrushHUDMath.bracket(size: 1, larger: false), 1, "floor")
        XCTAssertEqual(BrushHUDMath.bracket(size: 5000, larger: true), 5000, "ceiling")
        // Up then down retraces.
        for s: Float in [3, 9, 10, 45, 50, 95, 100, 175, 200, 450, 500, 900] {
            XCTAssertEqual(BrushHUDMath.bracket(size: BrushHUDMath.bracket(size: s, larger: true), larger: false), s, "\(s)")
        }
        XCTAssertEqual(BrushHUDMath.bracket(hardness: 0.5, harder: true), 0.75)
        XCTAssertEqual(BrushHUDMath.bracket(hardness: 1, harder: true), 1)
        XCTAssertEqual(BrushHUDMath.bracket(hardness: 0.1, harder: false), 0)
        XCTAssertEqual(BrushHUDMath.bracket(hardness: 0.6, harder: false), 0.25, "snaps to quarters")
    }

    func testHUDDragMapsToSizeAndHardness() {
        // At 50 % zoom (0.5 pt per pixel) a 10 pt drag right adds 40 px of diameter.
        var r = BrushHUDMath.drag(startSize: 30, startHardness: 0.5, dx: 10, dy: 0, zoom: 0.5)
        XCTAssertEqual(r.size, 70)
        XCTAssertEqual(r.hardness, 0.5)
        // Up (negative dy) is harder; 200 pt is the full range.
        r = BrushHUDMath.drag(startSize: 30, startHardness: 0.5, dx: 0, dy: -50, zoom: 1)
        XCTAssertEqual(r.hardness, 0.75, accuracy: 1e-6)
        r = BrushHUDMath.drag(startSize: 30, startHardness: 0.5, dx: -1000, dy: 1000, zoom: 1)
        XCTAssertEqual(r.size, 1, "clamped")
        XCTAssertEqual(r.hardness, 0)
    }

    func testOpacityDigits() {
        var k = BrushHUDMath.OpacityKeys()
        XCTAssertEqual(k.press(5, at: 0), 0.5)
        XCTAssertEqual(k.press(0, at: 10), 1, "0 alone = 100 %")
        XCTAssertEqual(k.press(4, at: 20), 0.4)
        XCTAssertEqual(k.press(5, at: 20.3), 0.45, "two quick digits")
        XCTAssertEqual(k.press(0, at: 30), 1)
        XCTAssertEqual(k.press(0, at: 30.2), 0, "00 = 0 %")
        XCTAssertEqual(k.press(7, at: 40), 0.7)
        XCTAssertEqual(k.press(3, at: 41), 0.3, "too slow: a new single digit")
    }

    // MARK: Selection modifiers

    func testSelectionModifierMapping() {
        XCTAssertEqual(SelectionModifiers.combine(shift: false, option: false), .replace)
        XCTAssertEqual(SelectionModifiers.combine(shift: true, option: false), .add)
        XCTAssertEqual(SelectionModifiers.combine(shift: false, option: true), .subtract)
        XCTAssertEqual(SelectionModifiers.combine(shift: true, option: true), .intersect)
        XCTAssertEqual(SelectionModifiers.combine(shift: false, option: false, optionsBar: .subtract), .subtract,
                       "without modifiers the options bar decides")
        XCTAssertEqual(SelectionModifiers.combine(shift: true, option: false, optionsBar: .subtract), .add, "modifiers win")
        XCTAssertEqual(SelectionModifiers.quickSelect(option: false, firstStroke: true), .replace)
        XCTAssertEqual(SelectionModifiers.quickSelect(option: false, firstStroke: false), .add)
        XCTAssertEqual(SelectionModifiers.quickSelect(option: true, firstStroke: false), .subtract)
    }

    func testMarqueeGeometry() {
        let s = CGPoint(x: 100, y: 100)
        XCTAssertEqual(SelectionModifiers.marqueeRect(start: s, current: CGPoint(x: 40, y: 130), square: false, fromCenter: false),
                       CGRect(x: 40, y: 100, width: 60, height: 30))
        XCTAssertEqual(SelectionModifiers.marqueeRect(start: s, current: CGPoint(x: 140, y: 110), square: true, fromCenter: false),
                       CGRect(x: 100, y: 100, width: 40, height: 40))
        XCTAssertEqual(SelectionModifiers.marqueeRect(start: s, current: CGPoint(x: 60, y: 90), square: true, fromCenter: false),
                       CGRect(x: 60, y: 60, width: 40, height: 40), "square up-left")
        XCTAssertEqual(SelectionModifiers.marqueeRect(start: s, current: CGPoint(x: 130, y: 110), square: false, fromCenter: true),
                       CGRect(x: 70, y: 90, width: 60, height: 20))
    }

    // MARK: Transform matrices

    func testAffineComposition() {
        let t = AffineTransform2D.translation(10, 20)
        let s = AffineTransform2D.scale(2, 3)
        // `t ∘ s`: scale, then translate.
        close(t.concatenating(after: s).apply(CGPoint(x: 1, y: 1)), CGPoint(x: 12, y: 23))
        close(s.concatenating(after: t).apply(CGPoint(x: 1, y: 1)), CGPoint(x: 22, y: 63))
        let r = AffineTransform2D.rotation(degrees: 90)
        close(r.apply(CGPoint(x: 1, y: 0)), CGPoint(x: 0, y: 1), 1e-12)   // y down: clockwise on screen
        let m = t.concatenating(after: r).concatenating(after: s)
        let inv = try! XCTUnwrap(m.inverse)
        close(inv.concatenating(after: m).apply(CGPoint(x: 7, y: -3)), CGPoint(x: 7, y: -3), 1e-9)
        XCTAssertTrue(inv.concatenating(after: m).apply(.zero) == inv.concatenating(after: m).apply(.zero))
        XCTAssertNil(AffineTransform2D.scale(0, 1).inverse)
        XCTAssertEqual(AffineTransform2D.identity.concatenating(after: m), m)
        let k = AffineTransform2D.skew(x: 45, y: 0)
        close(k.apply(CGPoint(x: 0, y: 1)), CGPoint(x: 1, y: 1), 1e-12)
        // The engine record carries the same six numbers.
        XCTAssertEqual(m.ffi, TransformMatrix(a: m.a, b: m.b, c: m.c, d: m.d, e: m.e, f: m.f))
    }

    func testFreeTransformModel() {
        var m = FreeTransformModel(bounds: CGRect(x: 100, y: 50, width: 200, height: 100))
        XCTAssertTrue(m.isIdentity)
        XCTAssertEqual(m.matrix, .identity)
        XCTAssertEqual(m.reference, CGPoint(x: 200, y: 100))
        // Scale about the reference point: the centre stays.
        m.sx = 2
        close(m.matrix.apply(CGPoint(x: 200, y: 100)), CGPoint(x: 200, y: 100))
        close(m.transformed(.topLeft), CGPoint(x: 0, y: 50))
        // Rotate 90° about the centre.
        m = FreeTransformModel(bounds: CGRect(x: 100, y: 50, width: 200, height: 100))
        m.angle = 90
        close(m.transformed(.right), CGPoint(x: 200, y: 200), 1e-9)
        // Numeric W / H and translation compose with rotation: T · R · K · S · T(−ref).
        m.sx = 0.5
        m.tx = 10
        let expected = AffineTransform2D.translation(210, 100)
            .concatenating(after: .rotation(degrees: 90)).concatenating(after: .scale(0.5, 1))
            .concatenating(after: .translation(-200, -100))
        let got = m.matrix
        for (a, b) in zip([got.a, got.b, got.c, got.d, got.e, got.f], [expected.a, expected.b, expected.c, expected.d, expected.e, expected.f]) {
            XCTAssertEqual(a, b, accuracy: 1e-9)
        }
        XCTAssertEqual(m.widthPercent, 50)
    }

    func testFreeTransformHandleDrags() {
        let b = CGRect(x: 0, y: 0, width: 100, height: 50)
        // Bottom-right corner to (200, 150): scales about the top-left, which stays put.
        var m = FreeTransformModel(bounds: b)
        m.drag(.bottomRight, to: CGPoint(x: 200, y: 150), constrain: false, fromCenter: false)
        XCTAssertEqual(m.sx, 2, accuracy: 1e-9)
        XCTAssertEqual(m.sy, 3, accuracy: 1e-9)
        close(m.transformed(.topLeft), .zero, 1e-9)
        close(m.transformed(.bottomRight), CGPoint(x: 200, y: 150), 1e-9)
        // ⇧ keeps the proportions.
        m = FreeTransformModel(bounds: b)
        m.drag(.bottomRight, to: CGPoint(x: 200, y: 60), constrain: true, fromCenter: false)
        XCTAssertEqual(m.sx, 2, accuracy: 1e-9)
        XCTAssertEqual(m.sy, 2, accuracy: 1e-9)
        close(m.transformed(.topLeft), .zero, 1e-9)
        // An edge handle scales one axis.
        m = FreeTransformModel(bounds: b)
        m.drag(.right, to: CGPoint(x: 50, y: 999), constrain: false, fromCenter: false)
        XCTAssertEqual(m.sx, 0.5, accuracy: 1e-9)
        XCTAssertEqual(m.sy, 1)
        // ⌥ scales about the centre.
        m = FreeTransformModel(bounds: b)
        m.drag(.bottomRight, to: CGPoint(x: 150, y: 75), constrain: false, fromCenter: true)
        XCTAssertEqual(m.sx, 2, accuracy: 1e-9)
        XCTAssertEqual(m.sy, 2, accuracy: 1e-9)
        close(m.transformed(.topLeft), CGPoint(x: -50, y: -25), 1e-9)
        // Dragging on a rotated box works in the box's frame.
        m = FreeTransformModel(bounds: b)
        m.angle = 90
        let anchor = m.transformed(.topLeft)
        let target = m.matrix.apply(CGPoint(x: 200, y: 50))   // where (200, 50) goes at scale 1
        m.drag(.bottomRight, to: target, constrain: false, fromCenter: false)
        XCTAssertEqual(m.sx, 2, accuracy: 1e-6)
        XCTAssertEqual(m.sy, 1, accuracy: 1e-6)
        close(m.transformed(.topLeft), anchor, 1e-6)
    }

    func testFreeTransformRotateMoveAndHitTest() {
        var m = FreeTransformModel(bounds: CGRect(x: 0, y: 0, width: 100, height: 100))
        // From the right of the centre to below it: +90° (clockwise on screen, y down).
        m.rotate(from: CGPoint(x: 200, y: 50), to: CGPoint(x: 50, y: 200), startAngle: 0, snap: false)
        XCTAssertEqual(m.angle, 90, accuracy: 1e-9)
        close(m.referenceNow, CGPoint(x: 50, y: 50), 1e-9)
        m.rotate(from: CGPoint(x: 200, y: 50), to: CGPoint(x: 200, y: 60), startAngle: 0, snap: true)
        XCTAssertEqual(m.angle, 0, "⇧ snaps to 15°")
        m.rotate(from: CGPoint(x: 200, y: 50), to: CGPoint(x: 200, y: 100), startAngle: 0, snap: true)
        XCTAssertEqual(m.angle.truncatingRemainder(dividingBy: 15), 0)
        m.move(by: CGSize(width: 5, height: -5))
        XCTAssertEqual(m.tx, 5)
        XCTAssertTrue(m.contains(CGPoint(x: 55, y: 45)))
        XCTAssertFalse(m.contains(CGPoint(x: 500, y: 45)))
        XCTAssertEqual(FreeTransformModel.Handle.topLeft.opposite, .bottomRight)
        XCTAssertEqual(FreeTransformModel.Handle.left.opposite, .right)
    }

    // MARK: Keys

    func testToolKeyMap() {
        func a(_ ch: String, _ mods: DocumentKeyMap.Mods = [], current: DocumentTool = .move, code: UInt16 = 0) -> ToolKeyAction? {
            ToolKeyMap.action(keyCode: code, characters: ch, mods: mods, current: current)
        }
        XCTAssertEqual(a("b"), .tool(.brush))
        XCTAssertEqual(a("E"), .tool(.eraser))
        XCTAssertEqual(a("s"), .tool(.cloneStamp))
        XCTAssertEqual(a("j"), .tool(.heal))
        XCTAssertEqual(a("i"), .tool(.eyedropper))
        XCTAssertEqual(a("h"), .tool(.hand))
        XCTAssertEqual(a("z"), .tool(.zoom))
        XCTAssertEqual(a("g"), .tool(.gradient))
        XCTAssertEqual(a("c"), .tool(.crop))
        XCTAssertEqual(a("t"), .tool(.type))
        XCTAssertEqual(a("v"), .tool(.move))
        XCTAssertEqual(a("m"), .tool(.marquee))
        XCTAssertEqual(a("m", current: .ellipseMarquee), .tool(.ellipseMarquee), "the key keeps the group's current tool")
        XCTAssertEqual(a("m", .shift, current: .marquee), .tool(.ellipseMarquee), "⇧M cycles")
        XCTAssertEqual(a("m", .shift, current: .ellipseMarquee), .tool(.marquee))
        XCTAssertEqual(a("l", .shift, current: .lasso), .tool(.polygonLasso))
        XCTAssertEqual(a("l", .shift, current: .polygonLasso), .tool(.magneticLasso))
        XCTAssertEqual(a("w"), .tool(.quickSelect))
        XCTAssertEqual(a("w", .shift, current: .quickSelect), .tool(.wand))
        XCTAssertEqual(a("["), .brushSize(larger: false))
        XCTAssertEqual(a("]"), .brushSize(larger: true))
        XCTAssertEqual(a("{", .shift), .brushHardness(harder: false))
        XCTAssertEqual(a("}", .shift), .brushHardness(harder: true))
        XCTAssertEqual(a("7"), .opacityDigit(7))
        XCTAssertEqual(a("x"), .swapColors)
        XCTAssertEqual(a("d"), .defaultColors)
        XCTAssertEqual(a("", code: 36), .commit)
        XCTAssertEqual(a("", code: 53), .cancel)
        XCTAssertNil(a("b", .command), "⌘ keys are menu items")
        XCTAssertNil(a("b", .option))
        XCTAssertNil(a("f"), "F is the screen mode (DocumentKeyMap)")
        XCTAssertNil(a(" "))
        // Every tool has a key, a symbol and a title; groups share keys.
        for t in DocumentTool.allCases {
            XCTAssertFalse(t.title.isEmpty)
            XCTAssertFalse(t.symbol.isEmpty)
            XCTAssertTrue(t.group.contains(t))
            XCTAssertTrue(t.group.allSatisfy { $0.key == t.key })
        }
        XCTAssertEqual(Set(DocumentTool.paletteSlots.flatMap(\.group)), Set(DocumentTool.allCases), "every tool is in the palette")
    }

    func testToolColors() {
        var c = ToolColors()
        XCTAssertEqual(c.foreground, .black)
        c.foreground = ToolColor(r: 1, g: 0, b: 0)
        c.swap()
        XCTAssertEqual(c.background, ToolColor(r: 1, g: 0, b: 0))
        XCTAssertEqual(c.foreground, .white)
        c.reset()
        XCTAssertEqual(c, ToolColors())
        XCTAssertEqual(ToolColor(r: 1, g: 0.5, b: 0).hex, "#FF8000")
    }

    // MARK: Engine adapter

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-tools-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testEngineToolsThroughTheAdapter() throws {
        let dir = try temp()
        let e = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(e).newDocument(width: 256, height: 128, depth: .u8, profile: nil)
        let t = try XCTUnwrap(doc as? DocumentToolsBackend)
        let layer = try doc.layers()[0].id
        // Fill, then a brush stroke in two frames, one history node.
        _ = try t.fillSelection(layer: layer, fill: .color(ToolColor(r: 0, g: 0, b: 1)), opacity: 1)
        var brush = BrushOptions()
        brush.size = 12
        brush.hardness = 1
        try t.beginStroke(layer: layer, target: .pixels, tool: .brush, brush: brush, color: ToolColor(r: 1, g: 0, b: 0))
        let f1 = try t.strokePoints((0...4).map { PenSample(x: 20 + Float($0) * 10, y: 64) })
        XCTAssertNotNil(f1.dirtyRect)
        XCTAssertGreaterThan(f1.dabs, 0)
        let f2 = try t.strokePoints([PenSample(x: 80, y: 64, pressure: 0.5, tiltX: 0.2, tiltY: -0.1, timestamp: 1)])
        XCTAssertGreaterThan(f2.totalDabs, f1.totalDabs)
        let before = try doc.historyItems().count
        let c = try t.endStroke()
        XCTAssertTrue(c.layersChanged.contains(layer))
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Brush Tool")
        XCTAssertEqual(try t.sampleColor(at: CanvasPoint(x: 40, y: 64), sampleAll: true, layer: nil, radius: 0), ToolColor(r: 1, g: 0, b: 0))
        XCTAssertEqual(try t.sampleColor(at: CanvasPoint(x: 40, y: 10), sampleAll: false, layer: layer, radius: 1), ToolColor(r: 0, g: 0, b: 1))
        _ = try doc.undo()
        XCTAssertEqual(try t.sampleColor(at: CanvasPoint(x: 40, y: 64), sampleAll: true, layer: nil, radius: 0), ToolColor(r: 0, g: 0, b: 1))

        // Selections, outline, modify, channels.
        _ = try t.selectMarquee(.rect, rect: CGRect(x: 10, y: 20, width: 50, height: 40), feather: 0, antialias: true, op: .replace)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 10, y: 20, width: 50, height: 40))
        _ = try t.selectMarquee(.rect, rect: CGRect(x: 30, y: 20, width: 50, height: 40), feather: 0, antialias: true, op: .add)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 10, y: 20, width: 70, height: 40))
        let outline = try t.selectionOutline(level: 0)
        XCTAssertEqual(outline.count, 1)
        XCTAssertTrue(outline[0].closed)
        XCTAssertEqual(outline[0].points.map(\.x).min() ?? 0, 10, accuracy: 0.6)
        XCTAssertEqual(outline[0].points.map(\.x).max() ?? 0, 80, accuracy: 0.6)
        _ = try t.modifySelection(.expand, px: 5)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 5, y: 15, width: 80, height: 50))
        try t.saveSelection(name: "Alpha 1")
        XCTAssertEqual(try t.selectionChannels(), ["Alpha 1"])
        _ = try t.selectInverse()
        _ = try t.loadSelection(name: "Alpha 1", op: .replace)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 5, y: 15, width: 80, height: 50))
        _ = try t.selectLasso([CanvasPoint(x: 0, y: 0), CanvasPoint(x: 100, y: 0), CanvasPoint(x: 0, y: 100)], mode: .polygon,
                              feather: 0, antialias: false, op: .replace)
        XCTAssertEqual(try doc.historyItems().last?.label, "Polygonal Lasso")
        _ = try t.selectWand(at: CanvasPoint(x: 200, y: 100), tolerance: 32, contiguous: true, sampleAll: true, antialias: false, op: .replace)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 0, y: 0, width: 256, height: 128))
        _ = try t.selectAll()
        _ = try doc.clearSelection()
        XCTAssertTrue(try t.selectionOutline(level: 0).isEmpty)

        // Free Transform: move right by 10 px, one node.
        _ = try t.selectMarquee(.rect, rect: CGRect(x: 0, y: 0, width: 50, height: 128), feather: 0, antialias: false, op: .replace)
        _ = try t.deleteSelection(layer: layer, background: .white)
        _ = try doc.clearSelection()
        let start = try t.beginTransform(layers: [layer])
        XCTAssertEqual(start.bounds, CanvasRect(x: 50, y: 0, width: 206, height: 128))
        _ = try t.setTransform(.translation(-10, 0), interpolation: .nearest)
        _ = try t.commitTransform()
        XCTAssertEqual(try doc.historyItems().last?.label, "Free Transform")
        XCTAssertEqual(try doc.layers()[0].bounds.map { $0.x }, 0, "tile-granular bounds")

        // Brush tips.
        XCTAssertTrue(t.brushTips().contains { $0.id == "builtin:chalk" })
        let bmp = try t.brushTipPreview(id: "round:1", maxPx: 16)
        XCTAssertEqual(bmp.pixels.count, 256)
        XCTAssertThrowsError(try t.importAbr(path: dir.appendingPathComponent("none.abr").path))
        doc.close()
    }

    func testStubAdoptsTheGeometricSubset() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 400, height: 300, depth: .u8, profile: nil)
        let t = try XCTUnwrap(doc as? DocumentToolsBackend)
        _ = try t.selectMarquee(.ellipse, rect: CGRect(x: 10, y: 10, width: 100, height: 50), feather: 0, antialias: true, op: .replace)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 10, y: 10, width: 100, height: 50))
        XCTAssertEqual(try t.selectionOutline(level: 0).first?.points.count, 4)
        _ = try t.selectMarquee(.rect, rect: CGRect(x: 100, y: 10, width: 100, height: 50), feather: 0, antialias: true, op: .add)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 10, y: 10, width: 190, height: 50))
        XCTAssertThrowsError(try t.beginStroke(layer: 1, target: .pixels, tool: .brush, brush: BrushOptions(), color: .black))
        XCTAssertEqual(try t.brushTipPreview(id: "round:0.5", maxPx: 8).pixels.count, 64)
        _ = try t.selectAll()
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 0, y: 0, width: 400, height: 300))
        doc.close()
    }
}
