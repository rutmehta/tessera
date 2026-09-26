import CoreGraphics
import Foundation
import IOSurface
import XCTest
@testable import TesseraCore

/// Document mode (WP M5-10): outline flattening and diffing, the blend-mode table, viewport
/// maths, the document key map and the stub backend's history and output.
final class DocumentOutlineTests: XCTestCase {
    private func node(_ id: DocLayerID, _ parent: DocLayerID?, _ index: UInt32, group: Bool = false) -> LayerRecord {
        LayerRecord(id: id, parent: parent, index: index, depth: 0, kind: group ? .group : .pixel, name: "L\(id)")
    }

    func testSampleDocumentFlattensTopFirst() {
        let outline = DocumentOutline(StubDocumentBackend().layers())
        XCTAssertEqual(outline.flattened, [4, 6, 5, 3, 2, 1])
        XCTAssertEqual(outline.children(of: DocumentOutline.root), [4, 3, 2, 1])
        XCTAssertEqual(outline.children(of: 4), [6, 5])
        XCTAssertEqual(outline.depth(of: 5), 1)
        XCTAssertEqual(outline.position(of: 3)?.index, 1)
    }

    func testListOrderDoesNotMatterOnlyIndices() {
        let a = DocumentOutline([node(1, nil, 0), node(2, nil, 1), node(3, nil, 2)])
        let b = DocumentOutline([node(3, nil, 2), node(1, nil, 0), node(2, nil, 1)])
        XCTAssertEqual(a.structure, b.structure)
        XCTAssertEqual(a.flattened, [3, 2, 1])
    }

    func testSingleReorderIsOneMove() throws {
        let old = DocumentOutline((1...6).map { node($0, nil, UInt32($0 - 1)) })   // top first: 6 5 4 3 2 1
        let new = try XCTUnwrap(old.moving([6], into: DocumentOutline.root, at: 6))  // top to bottom
        XCTAssertEqual(new.flattened, [5, 4, 3, 2, 1, 6])
        let changes = try XCTUnwrap(DocumentOutline.diff(from: old, to: new))
        XCTAssertEqual(changes, [.move(id: 6, fromParent: 0, fromIndex: 0, toParent: 0, toIndex: 5)])
        XCTAssertEqual(old.applying(changes), new.structure)
    }

    func testMoveIntoGroupAndOutIsOneMoveEach() throws {
        let old = DocumentOutline([node(1, nil, 0), node(2, nil, 1, group: true), node(3, 2, 0), node(4, nil, 2)])
        let into = try XCTUnwrap(old.moving([4], into: 2, at: 0))
        XCTAssertEqual(into.children(of: 2), [4, 3])
        XCTAssertEqual(DocumentOutline.diff(from: old, to: into)?.count, 1)
        let out = try XCTUnwrap(into.moving([3], into: DocumentOutline.root, at: 0))
        XCTAssertEqual(out.children(of: DocumentOutline.root), [3, 2, 1])
        XCTAssertEqual(DocumentOutline.diff(from: into, to: out)?.count, 1)
        XCTAssertNil(old.moving([2], into: 2, at: 0), "a group cannot move into itself")
    }

    func testInsertRemoveAndUngroupDiffs() throws {
        let old = DocumentOutline([node(1, nil, 0), node(2, nil, 1, group: true), node(3, 2, 0), node(4, 2, 1)])
        // Ungroup: 3 and 4 move to the root where the group was; the group goes; 5 is new on top.
        let new = DocumentOutline([node(1, nil, 0), node(3, nil, 1), node(4, nil, 2), node(5, nil, 3)])
        let changes = try XCTUnwrap(DocumentOutline.diff(from: old, to: new))
        XCTAssertEqual(old.applying(changes), new.structure)
        XCTAssertEqual(changes.filter { if case .move = $0 { true } else { false } }.count, 2)
        XCTAssertEqual(changes.filter { if case .insert = $0 { true } else { false } }.count, 1)
        XCTAssertEqual(changes.filter { if case .remove = $0 { true } else { false } }.count, 1)
    }

    func testRandomTreesDiffToTheirTarget() throws {
        var rng = SystemRandomNumberGenerator()
        for _ in 0..<300 {
            let old = randomTree(ids: Array(1...12).filter { _ in Bool.random(using: &rng) || true }, rng: &rng)
            // Target: keep a random subset, add new ids, shuffle parents and order.
            var ids = old.nodes.keys.filter { _ in Int.random(in: 0..<5, using: &rng) > 0 }
            ids += (100..<100 + Int.random(in: 0...3, using: &rng)).map(DocLayerID.init)
            let new = randomTree(ids: ids, rng: &rng)
            guard let changes = DocumentOutline.diff(from: old, to: new) else { continue }   // swapped nesting
            // Random regeneration may turn a group into a leaf (a kind change reloads the row), so
            // compare the non-empty child lists.
            XCTAssertEqual(old.applying(changes).filter { !$0.value.isEmpty }, new.structure.filter { !$0.value.isEmpty })
        }
    }

    func testMinimalMovesForUnchangedTree() {
        let t = DocumentOutline(StubDocumentBackend().layers())
        XCTAssertEqual(DocumentOutline.diff(from: t, to: t), [])
    }

    func testLongestIncreasingSubsequence() {
        XCTAssertEqual(DocumentOutline.longestIncreasingSubsequence([3, 0, 1, 2]), [1, 2, 3])
        XCTAssertEqual(DocumentOutline.longestIncreasingSubsequence([]), [])
    }

    private func randomTree(ids: [DocLayerID], rng: inout SystemRandomNumberGenerator) -> DocumentOutline {
        var nodes: [LayerRecord] = []
        var groups: [DocLayerID?] = [nil]
        var counts: [DocLayerID: UInt32] = [:]
        for id in ids.shuffled(using: &rng) {
            let parent = groups.randomElement(using: &rng)!
            let isGroup = Int.random(in: 0..<3, using: &rng) == 0
            let key = parent ?? 0
            nodes.append(node(id, parent, counts[key, default: 0], group: isGroup))
            counts[key, default: 0] += 1
            if isGroup { groups.append(id) }
        }
        return DocumentOutline(nodes)
    }
}

final class DocumentBlendModeTests: XCTestCase {
    func testTwentySevenModesInPhotoshopOrder() {
        XCTAssertEqual(DocBlendMode.allCases.count, 27)
        XCTAssertEqual(DocBlendMode.allCases.map(\.backendName), [
            "normal", "dissolve", "darken", "multiply", "color_burn", "linear_burn", "darker_color",
            "lighten", "screen", "color_dodge", "linear_dodge", "lighter_color",
            "overlay", "soft_light", "hard_light", "vivid_light", "linear_light", "pin_light", "hard_mix",
            "difference", "exclusion", "subtract", "divide", "hue", "saturation", "color", "luminosity",
        ])
        XCTAssertEqual(DocBlendMode.linearDodge.title, "Linear Dodge (Add)")
        XCTAssertEqual(DocBlendMode.softLight.index, 13)
    }

    func testRoundTripAndGroups() {
        for m in DocBlendMode.allCases { XCTAssertEqual(DocBlendMode(backendName: m.backendName), m) }
        XCTAssertNil(DocBlendMode(backendName: "pass_through"))
        XCTAssertEqual(DocBlendMode.grouped.map(\.1.count), [2, 5, 5, 7, 4, 4])
        XCTAssertEqual(Set(DocBlendMode.allCases.map(\.title)).count, 27)
        XCTAssertEqual(DocBlendMode.title(backendName: "normal", groupMode: .passThrough), "Pass Through")
        XCTAssertEqual(DocBlendMode.title(backendName: "screen", groupMode: .isolated), "Screen")
    }

    func testStubAcceptsEveryModeAndRejectsOthers() throws {
        let doc = StubDocumentBackend()
        for m in DocBlendMode.allCases { _ = try doc.setBlendMode(id: 2, mode: m.backendName) }
        XCTAssertEqual(doc.layers().first { $0.id == 2 }?.blendMode, "luminosity")
        XCTAssertThrowsError(try doc.setBlendMode(id: 2, mode: "pass_through"))
    }
}

final class DocumentViewportMathTests: XCTestCase {
    func testLevelFromZoom() {
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 4), 0)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 1), 0)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 0.75), 0)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 0.5), 1)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 0.49), 1)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 0.25), 2)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 0.2), 2)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 1.0 / 64), 6)
        XCTAssertEqual(DocumentViewportMath.level(forZoom: 1.0 / 64, maxLevel: 3), 3)
        let m = DocumentViewportMath(canvasWidth: 1000, canvasHeight: 500, viewWidth: 100, viewHeight: 100, zoom: 1.0 / 64)
        XCTAssertEqual(m.maxLevel, 9)
    }

    func testFitZoomAndSteps() {
        var m = DocumentViewportMath(canvasWidth: 4000, canvasHeight: 2000, viewWidth: 2000, viewHeight: 2000)
        XCTAssertEqual(m.fitZoom, 0.47, accuracy: 1e-9)
        XCTAssertEqual(m.zoom, m.fitZoom)
        XCTAssertEqual(m.level, 1)
        XCTAssertEqual(DocumentViewportMath.zoomIn(from: 0.47), 0.5)
        XCTAssertEqual(DocumentViewportMath.zoomOut(from: 0.5), 1.0 / 3, accuracy: 1e-12)
        XCTAssertEqual(DocumentViewportMath.zoomIn(from: 1), 2)
        XCTAssertEqual(DocumentViewportMath.zoomIn(from: 32), 32)
        m.setZoom(100)
        XCTAssertEqual(m.zoom, DocumentViewportMath.maxZoom)
        XCTAssertEqual(DocumentViewportMath.percentText(1), "100 %")
        XCTAssertEqual(DocumentViewportMath.percentText(1.0 / 3), "33.3 %")
    }

    func testAnchoredZoomKeepsThePointUnderThePointer() {
        var m = DocumentViewportMath(canvasWidth: 4000, canvasHeight: 3000, viewWidth: 1200, viewHeight: 800, zoom: 0.25)
        let anchor = CGPoint(x: 300, y: 200)
        let before = m.canvasPoint(view: anchor)
        m.setZoom(1, anchor: anchor)
        let after = m.canvasPoint(view: anchor)
        XCTAssertEqual(before.x, after.x, accuracy: 1e-9)
        XCTAssertEqual(before.y, after.y, accuracy: 1e-9)
        XCTAssertEqual(m.viewPoint(canvas: after).x, 300, accuracy: 1e-9)
    }

    func testVisibleRectAndPan() {
        var m = DocumentViewportMath(canvasWidth: 4000, canvasHeight: 3000, viewWidth: 1000, viewHeight: 500, zoom: 1)
        XCTAssertEqual(m.visibleCanvasRect, CanvasRect(x: 1500, y: 1250, width: 1000, height: 500))
        m.pan(dx: 100, dy: -50)   // content follows the pointer: the view moves left / down over the canvas
        XCTAssertEqual(m.visibleCanvasRect, CanvasRect(x: 1400, y: 1300, width: 1000, height: 500))
        m.pan(dx: 1e6, dy: 0)
        XCTAssertEqual(m.center.x, 0)
        XCTAssertEqual(DocumentViewportMath.levelSize(CanvasRect(x: 0, y: 0, width: 1001, height: 500), level: 1).width, 501)
        m.fit()
        XCTAssertEqual(m.visibleCanvasRect, CanvasRect(x: 0, y: 0, width: 4000, height: 3000))
    }
}

final class DocumentKeyMapTests: XCTestCase {
    private func action(_ code: UInt16, _ ch: String, _ mods: DocumentKeyMap.Mods = []) -> DocumentKeyAction? {
        DocumentKeyMap.action(keyCode: code, characters: ch, mods: mods)
    }

    func testToolsAndPanelKeys() {
        XCTAssertEqual(action(9, "v"), .tool(.move))
        XCTAssertEqual(action(46, "m"), .tool(.marquee))
        XCTAssertEqual(action(49, " "), .panHold)
        XCTAssertEqual(action(48, "\t"), .togglePanels)
        XCTAssertEqual(action(3, "f"), .cycleScreenMode)
        XCTAssertEqual(action(51, "\u{7F}"), .deleteLayer)
    }

    func testCullingKeysDoNothingInDocumentMode() {
        for ch in ["x", "u", "p", "1", "2", "3", "6", "b", "k", "c", "g", "e", "y", "n", "a", "s"] {
            XCTAssertNil(action(0, ch), ch)
        }
    }

    func testCommandShortcuts() {
        XCTAssertEqual(action(0, "a", .command), .selectAll)
        XCTAssertEqual(action(2, "d", .command), .deselect)
        XCTAssertEqual(action(6, "z", .command), .undo)
        XCTAssertEqual(action(6, "z", [.command, .shift]), .redo)
        XCTAssertEqual(action(38, "j", .command), .duplicate)
        XCTAssertEqual(action(5, "g", .command), .group)
        XCTAssertEqual(action(5, "g", [.command, .shift]), .ungroup)
        XCTAssertEqual(action(14, "e", .command), .mergeDown)
        XCTAssertEqual(action(24, "=", .command), .zoomIn)
        XCTAssertEqual(action(27, "-", .command), .zoomOut)
        XCTAssertEqual(action(29, "0", .command), .zoomFit)
        XCTAssertEqual(action(18, "1", .command), .zoomActual)
        XCTAssertNil(action(9, "v", .control))
    }
}

final class DocumentAdjustmentModelTests: XCTestCase {
    func testEveryAdjustmentRoundTripsThroughJSON() {
        for kind in AdjustmentModel.Kind.allCases {
            let m = kind.neutral
            XCTAssertEqual(AdjustmentModel(json: m.json), m, kind.title)
            XCTAssertEqual(m.kind, kind)
        }
        let levels = AdjustmentModel(json: #"{"kind":"levels","master":{"gamma":2},"rgb":[{},{},{}]}"#)
        guard case .levels(let master, _) = levels else { return XCTFail("levels") }
        XCTAssertEqual(master.gamma, 2)
        XCTAssertEqual(master.inWhite, 1, "serde defaults fill missing fields")
    }

    func testFillsRoundTrip() {
        for kind in FillModel.Kind.allCases {
            let f = FillModel.neutral(kind, width: 100, height: 50)
            XCTAssertEqual(FillModel(json: f.json), f, kind.title)
        }
    }
}

@MainActor
final class StubDocumentBackendTests: XCTestCase {
    func testInteractiveEditsWaitForCommit() throws {
        let doc = StubDocumentBackend()
        XCTAssertTrue(doc.historyItems().isEmpty)
        XCTAssertFalse(doc.info().dirty)
        for v: Float in [0.9, 0.7, 0.5, 0.4] { _ = try doc.setOpacity(id: 2, value: v, interactive: true) }
        XCTAssertTrue(doc.historyItems().isEmpty, "no history node until commit")
        XCTAssertTrue(doc.info().dirty)
        try doc.commit(label: "Opacity 40 %")
        XCTAssertEqual(doc.historyItems().map(\.label), ["Opacity 40 %"])
        XCTAssertEqual(doc.layers().first { $0.id == 2 }!.opacity, 0.4, accuracy: 1e-5)
    }

    func testUndoRedoCheckoutAndSnapshots() throws {
        let doc = StubDocumentBackend()
        let original = doc.layers()
        let added = try doc.addLayer(kind: .pixel, name: "", parent: nil, index: nil)
        let newID = try XCTUnwrap(added.layersChanged.first)
        XCTAssertEqual(doc.layers().first?.name, "Layer 1", "new layers go on top")
        _ = try doc.setVisible(id: 3, visible: false)
        _ = try doc.moveLayer(id: newID, parent: 4, index: 0)
        XCTAssertEqual(doc.historyItems().count, 3)
        XCTAssertTrue(try doc.undo())
        XCTAssertNil(doc.layers().first { $0.id == newID }?.parent)
        XCTAssertTrue(try doc.undo())
        XCTAssertTrue(try doc.undo())
        XCTAssertEqual(doc.layers(), original)
        XCTAssertFalse(try doc.undo())
        XCTAssertFalse(doc.info().dirty)
        XCTAssertTrue(try doc.redo())
        XCTAssertTrue(try doc.redo())
        XCTAssertEqual(doc.layers().first { $0.id == 3 }?.visible, false)
        try doc.snapshot(name: "Hidden vignette")
        try doc.checkoutHistory(id: 1)
        XCTAssertEqual(doc.layers().first { $0.id == 3 }?.visible, true)
        XCTAssertEqual(doc.historyItems().first { $0.isCurrent }?.id, 1)
        try doc.restoreSnapshot(name: "Hidden vignette")
        XCTAssertEqual(doc.layers().first { $0.id == 3 }?.visible, false)
        XCTAssertEqual(doc.historyItems().last?.label, "Snapshot “Hidden vignette”")
        XCTAssertGreaterThan(doc.historyMemoryBytes(), 0)
        doc.setMaxStates(2)
        XCTAssertEqual(doc.historyItems().count, 2)
    }

    func testStructuralEdits() throws {
        let doc = StubDocumentBackend()
        let dup = try doc.duplicateLayer(id: 2)
        XCTAssertEqual(doc.layers().first { $0.id == dup.layersChanged[0] }?.name, "Landscape copy")
        _ = try doc.mergeDown(id: dup.layersChanged[0])
        XCTAssertEqual(doc.layers().count, 6)
        XCTAssertThrowsError(try doc.mergeDown(id: 1), "nothing below the bottom layer")
        _ = try doc.addMask(id: 3, initial: .hideAll)
        XCTAssertTrue(doc.layers().first { $0.id == 3 }!.hasMask)
        _ = try doc.setMaskEnabled(id: 3, enabled: false)
        XCTAssertFalse(doc.layers().first { $0.id == 3 }!.maskEnabled)
        XCTAssertThrowsError(try doc.addMask(id: 2, initial: .fromSelection))
        try doc.setSelectionRect(x: 10, y: 10, width: 100, height: 50, feather: 0)
        _ = try doc.addMask(id: 2, initial: .fromSelection)
        _ = try doc.setClipped(id: 3, clipped: true)
        XCTAssertTrue(doc.layers().first { $0.id == 3 }!.clipped)
        _ = try doc.setGroupMode(id: 4, mode: .isolated)
        XCTAssertEqual(doc.layers().first { $0.id == 4 }?.groupMode, .isolated)
        let curves = AdjustmentModel.curves(master: [[0, 0.1], [1, 0.9]], rgb: [[], [], []]).json
        _ = try doc.setAdjustmentJson(id: 5, json: curves, interactive: false)
        XCTAssertEqual(AdjustmentModel(json: doc.layers().first { $0.id == 5 }?.adjustmentJson), AdjustmentModel(json: curves))
        _ = try doc.flatten()
        XCTAssertEqual(doc.layers().map(\.name), ["Background"])
    }

    func testDragPlanAppliesThroughMoveLayer() throws {
        let doc = StubDocumentBackend()
        let outline = DocumentOutline(doc.layers())
        let target = try XCTUnwrap(outline.moving([3, 1], into: 4, at: 1))
        let moves = try XCTUnwrap(outline.backendMoves(to: target))
        XCTAssertEqual(moves.count, 2)
        for m in moves { _ = try doc.moveLayer(id: m.id, parent: m.parent, index: m.index) }
        XCTAssertEqual(DocumentOutline(doc.layers()).structure, target.structure)
        XCTAssertEqual(DocumentOutline(doc.layers()).children(of: 4), [6, 3, 1, 5])
    }

    func testSaveOpenAndExport() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let engine = StubDocumentEngine()
        let doc = try engine.newDocument(width: 400, height: 300, depth: .u16, profile: nil)
        XCTAssertEqual(doc.info().title, "Untitled-1")
        XCTAssertThrowsError(try doc.save(), "a new document needs Save As")
        _ = try doc.setOpacity(id: 3, value: 0.25, interactive: false)
        XCTAssertTrue(doc.info().dirty)
        XCTAssertThrowsError(try doc.saveAs(path: dir.appendingPathComponent("a.psd").path)) { e in
            guard case DocumentError.unsupported = e else { return XCTFail("\(e)") }
        }
        let path = dir.appendingPathComponent("Poster.tessera-doc").path
        try doc.saveAs(path: path)
        XCTAssertFalse(doc.info().dirty)
        XCTAssertEqual(doc.info().title, "Poster.tessera-doc")
        XCTAssertTrue(try engine.openDocument(path: path) === doc, "the same path opened twice is the same session")
        doc.close()
        let reopened = try engine.openDocument(path: path)
        XCTAssertFalse(reopened === doc)
        XCTAssertEqual(reopened.layers().map(\.name), doc.layers().map(\.name))
        XCTAssertEqual(reopened.layers().first { $0.id == 3 }!.opacity, 0.25, accuracy: 1e-6)
        XCTAssertEqual(reopened.info().depth, .u16)

        let png = dir.appendingPathComponent("flat.png").path
        try reopened.exportFlat(path: png, format: .png, quality: 90, color: .srgb)
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(URL(fileURLWithPath: png) as CFURL, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(src, 0, nil))
        XCTAssertEqual(image.width, 400)
        XCTAssertEqual(image.height, 300)
        let jpg = dir.appendingPathComponent("flat.jpg").path
        try reopened.exportFlat(path: jpg, format: .jpeg, quality: 80, color: .displayP3)
        XCTAssertTrue(FileManager.default.fileExists(atPath: jpg))

        // A flat image opens as one pixel layer named after the file.
        let flat = try engine.openDocument(path: png)
        XCTAssertEqual(flat.layers().map(\.name), ["flat"])
        XCTAssertEqual(flat.info().width, 400)
    }

    func testCompositeHasTransparentMarginAndThumbnailsAreCached() throws {
        let doc = StubDocumentBackend(sampleWidth: 200, height: 100)
        let (w, h, rgba) = doc.renderCanvas()
        XCTAssertEqual(w, 200)
        XCTAssertEqual(rgba[3], 0, "the margin is transparent (checkerboard)")
        XCTAssertEqual(rgba[((h / 2) * w + w / 2) * 4 + 3], 255)
        let before = doc.renderCount
        let a = try doc.layerThumbnail(id: 2, maxPx: 48)
        let b = try doc.layerThumbnail(id: 2, maxPx: 48)
        XCTAssertEqual(a, b)
        XCTAssertEqual(doc.renderCount, before + 1, "cached per revision")
        _ = try doc.setVisible(id: 2, visible: false)
        XCTAssertNotEqual(try doc.layerThumbnail(id: 2, maxPx: 48), a)
        let surface = try XCTUnwrap(IOSurfaceLookup(a))
        XCTAssertEqual(IOSurfaceGetWidth(surface), 48)
        XCTAssertNoThrow(try doc.maskThumbnail(id: 2, maxPx: 32))
        XCTAssertThrowsError(try doc.maskThumbnail(id: 1, maxPx: 32))
    }

    func testViewportFramesArriveOnTheListener() throws {
        final class Listener: DocumentBackendListener, @unchecked Sendable {
            var frames: [DocFrame] = []
            let expectation: XCTestExpectation
            init(_ e: XCTestExpectation) { expectation = e }
            func onFrame(_ frame: DocFrame) { frames.append(frame); expectation.fulfill() }
            func onLayersChanged() {}
            func onHistoryChanged() {}
        }
        let doc = StubDocumentBackend(sampleWidth: 800, height: 500)
        let listener = Listener(expectation(description: "frame"))
        listener.expectation.assertForOverFulfill = false
        doc.setListener(listener)
        let plan = doc.planSurface(width: 400, height: 250)
        let surface = try XCTUnwrap(DocumentSurfaces.make(width: Int(plan.width), height: Int(plan.height)))
        try doc.attachSurface(iosurfaceId: IOSurfaceGetID(surface), width: plan.width, height: plan.height)
        doc.setViewport(level: 1, x: 0, y: 0, width: 800, height: 500, zoom: 0.5)
        wait(for: [listener.expectation], timeout: 10)
        let f = try XCTUnwrap(listener.frames.first)
        XCTAssertEqual(f.surfaceId, IOSurfaceGetID(surface))
        XCTAssertEqual(f.level, 1)
        XCTAssertEqual(f.width, 400)
        XCTAssertEqual(f.rect, CanvasRect(x: 0, y: 0, width: 800, height: 500))
        withExtendedLifetime(surface) {}
    }
}
