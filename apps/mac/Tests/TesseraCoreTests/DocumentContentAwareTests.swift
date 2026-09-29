import CoreGraphics
import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-13: Content-Aware Move / Extend options, drag → integer offset, enablement, and the engine / stub adapters
/// (destination outside the selection, one node per apply, cancel leaves everything as it was).
final class DocumentContentAwareTests: XCTestCase {
    func testOptionsBecomeTheAdapterFillJson() throws {
        var o = ContentAwareOptions()
        XCTAssertEqual(o.mode, .move)
        o.setStructure(9)
        XCTAssertEqual(o.structure, 7)
        o.setStructure(0)
        XCTAssertEqual(o.structure, 1)
        o.setStructure(5)
        o.seed = 42
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(o.fillJson.utf8)) as? [String: Any])
        XCTAssertEqual(json["patch_radius"] as? Int, 5)
        XCTAssertEqual(json["seed"] as? Int, 42)
        XCTAssertEqual(json["iterations"] as? Int, 5)
        XCTAssertEqual(Set(json.keys), ["patch_radius", "seed", "iterations"])
    }

    func testModesAndSeamLevelsMapToTheEngine() {
        XCTAssertEqual(ContentAwareMoveMode.move.ffi, .move)
        XCTAssertEqual(ContentAwareMoveMode.extend.ffi, .extend)
        XCTAssertEqual(ContentAwareMoveMode.move.historyLabel, "Content-Aware Move")
        XCTAssertEqual(ContentAwareMoveMode.extend.historyLabel, "Content-Aware Extend")
        XCTAssertEqual(ContentAwareSeamLevel.allCases.map { "\($0.ffi)" }, ["none", "default", "high", "veryHigh"])
        XCTAssertEqual(ContentAwareSeamLevel.allCases.map(\.title), ["None", "Default", "High", "Very High"])
    }

    func testDragOffsetIsWholeDocumentPixelsWhateverTheZoom() {
        let b = CanvasRect(x: 100, y: 100, width: 50, height: 40)
        // Canvas points come from the viewport's inverse mapping: fractional at any zoom.
        let d = ContentAwareDrag.offset(from: CGPoint(x: 120.3, y: 110.6), to: CGPoint(x: 150.8, y: 104.2), bounds: b,
                                        canvasWidth: 1000, canvasHeight: 800)
        XCTAssertEqual(d.dx, 31)
        XCTAssertEqual(d.dy, -6)
        let z = ContentAwareDrag.offset(from: CGPoint(x: 10, y: 10), to: CGPoint(x: 10.49, y: 9.51), bounds: b,
                                        canvasWidth: 1000, canvasHeight: 800)
        XCTAssertEqual(z.dx, 0)
        XCTAssertEqual(z.dy, 0)
    }

    func testDragOffsetKeepsTheSelectionPartlyOnTheCanvas() {
        let b = CanvasRect(x: 100, y: 100, width: 50, height: 40)
        let far = ContentAwareDrag.offset(from: .zero, to: CGPoint(x: 5000, y: -5000), bounds: b, canvasWidth: 1000, canvasHeight: 800)
        XCTAssertEqual(far.dx, 899, "the selection's left column stays on the canvas")
        XCTAssertEqual(far.dy, -139, "its bottom row stays on the canvas")
        let moved = ContentAwareDrag.moved(b, dx: far.dx, dy: far.dy)
        XCTAssertEqual(moved, CanvasRect(x: 999, y: -39, width: 50, height: 40))
    }

    func testStartNeedsARasterLayerAndASelection() {
        XCTAssertNil(ContentAwareMenuState.canStart(layerKind: .pixel, hasSelection: true))
        XCTAssertNil(ContentAwareMenuState.canStart(layerKind: .smartObject, hasSelection: true))
        XCTAssertNotNil(ContentAwareMenuState.canStart(layerKind: .pixel, hasSelection: false))
        XCTAssertNotNil(ContentAwareMenuState.canStart(layerKind: .adjustment, hasSelection: true))
        XCTAssertNotNil(ContentAwareMenuState.canStart(layerKind: nil, hasSelection: true))
    }

    // MARK: Adapters

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-cam-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    /// Grey document with a red 8 × 8 subject at (10, 10), selected with a margin.
    private func engineDoc() throws -> (any DocumentBackend, DocLayerID, any DocumentToolsBackend) {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: 80, height: 48, depth: .u8, profile: nil)
        let tools = try XCTUnwrap(doc as? any DocumentToolsBackend)
        let layer = try doc.layers()[0].id
        _ = try tools.selectAll()
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.4, g: 0.5, b: 0.4)), opacity: 1)
        _ = try tools.selectMarquee(.rect, rect: CGRect(x: 10, y: 10, width: 8, height: 8), feather: 0, antialias: false, op: .replace)
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.9, g: 0.1, b: 0.1)), opacity: 1)
        _ = try tools.selectMarquee(.rect, rect: CGRect(x: 8, y: 8, width: 12, height: 12), feather: 0, antialias: false, op: .replace)
        return (doc, layer, tools)
    }

    func testEngineMoveOutsideTheSelectionIsOneNode() throws {
        let (doc, layer, _) = try engineDoc()
        defer { doc.close() }
        let c = try XCTUnwrap(doc as? any DocumentContentAwareBackend)
        let s = try c.beginContentAwareMove(layer: layer, mode: .move)
        XCTAssertEqual(s.selectionBounds, CanvasRect(x: 8, y: 8, width: 12, height: 12))
        XCTAssertFalse(s.smartObject)
        let n = try doc.historyItems().count
        var o = ContentAwareOptions()
        o.seed = 3
        let p = try c.previewContentAwareMove(token: s.token, dx: 40, dy: 10, fillJson: o.fillJson, seam: .none)
        XCTAssertEqual(p.affected, CanvasRect(x: 8, y: 8, width: 52, height: 22), "source ∪ destination, outside the selection")
        XCTAssertEqual(try doc.historyItems().count, n, "previews add no history")
        let change = try c.commitContentAwareMove(token: s.token)
        XCTAssertTrue(change.layersChanged.contains(layer))
        XCTAssertEqual(try doc.historyItems().count, n + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Content-Aware Move")
        XCTAssertThrowsError(try c.commitContentAwareMove(token: s.token), "closed after apply")
    }

    func testEngineCancelAndLocksLeaveEverythingUnchanged() throws {
        let (doc, layer, tools) = try engineDoc()
        defer { doc.close() }
        let c = try XCTUnwrap(doc as? any DocumentContentAwareBackend)
        let n = try doc.historyItems().count
        let s = try c.beginContentAwareMove(layer: layer, mode: .extend)
        _ = try c.previewContentAwareMove(token: s.token, dx: 30, dy: 0, fillJson: "{}", seam: .standard)
        c.cancelContentAwareMove(token: s.token)
        XCTAssertThrowsError(try c.commitContentAwareMove(token: s.token))
        XCTAssertEqual(try doc.historyItems().count, n)
        let t = try c.beginContentAwareMove(layer: layer, mode: .move)
        XCTAssertThrowsError(try c.commitContentAwareMove(token: t.token), "nothing previewed yet")
        XCTAssertThrowsError(try c.previewContentAwareMove(token: t.token, dx: 500, dy: 0, fillJson: "{}", seam: .none), "off canvas")
        c.cancelContentAwareMove(token: t.token)
        _ = try doc.setLocks(id: layer, locks: LayerLockFlags(pixels: true))
        XCTAssertThrowsError(try c.beginContentAwareMove(layer: layer, mode: .move))
        _ = try doc.setLocks(id: layer, locks: LayerLockFlags())
        _ = try doc.clearSelection()
        XCTAssertThrowsError(try c.beginContentAwareMove(layer: layer, mode: .move)) { e in
            XCTAssertTrue(e.localizedDescription.contains("selection"), e.localizedDescription)
        }
        _ = tools
    }

    func testEngineStaleLayerRefusesTheApply() throws {
        let (doc, layer, tools) = try engineDoc()
        defer { doc.close() }
        let c = try XCTUnwrap(doc as? any DocumentContentAwareBackend)
        let s = try c.beginContentAwareMove(layer: layer, mode: .move)
        _ = try c.previewContentAwareMove(token: s.token, dx: 30, dy: 0, fillJson: "{}", seam: .none)
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0, g: 0, b: 1)), opacity: 1)
        let n = try doc.historyItems().count
        XCTAssertThrowsError(try c.commitContentAwareMove(token: s.token)) { e in
            XCTAssertTrue(e.localizedDescription.contains("changed"), e.localizedDescription)
        }
        XCTAssertEqual(try doc.historyItems().count, n)
    }

    /// B5-09 parity: an Apply cancelled from the tool whose engine job committed anyway (the cancel arrived after
    /// the engine's last check) is undone when it returns as discarded, so engine history and the panels agree;
    /// a discarded failure changes nothing.
    @MainActor func testADiscardedApplyTheEngineCommittedIsUndone() throws {
        let (backend, layer, _) = try engineDoc()
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let c = try XCTUnwrap(backend as? any DocumentContentAwareBackend)
        let m = DocumentContentAware.shared
        var ended: [Result<DocumentChange, Error>] = []
        m.onApplied = { ended.append($0) }
        defer { m.onApplied = nil }
        let head = doc.info.historyHead
        let engineHead = try backend.historyItems().first { $0.isCurrent }?.id
        let s = try c.beginContentAwareMove(layer: layer, mode: .move)
        _ = try c.previewContentAwareMove(token: s.token, dx: 30, dy: 4, fillJson: "{}", seam: .none)
        // The engine committed before it saw the cancel.
        let late = try c.commitContentAwareMove(token: s.token)
        XCTAssertNotEqual(try backend.historyItems().first { $0.isCurrent }?.id, engineHead)
        m.applyEnded(.discarded(.success(late)), label: "Content-Aware Move", doc: doc)
        XCTAssertEqual(try backend.historyItems().first { $0.isCurrent }?.id, engineHead, "the late step is undone")
        XCTAssertEqual(doc.info.historyHead, head, "the panels show the same history as the engine")
        if case .success = ended.last { XCTFail("a discarded apply never reports success") }
        // A discarded failure (the engine refused to write) undoes nothing.
        let before = try backend.historyItems()
        m.applyEnded(.discarded(.failure(DocumentError.invalid("cancelled"))), label: "Content-Aware Move", doc: doc)
        XCTAssertEqual(try backend.historyItems().map(\.id), before.map(\.id))
        XCTAssertEqual(try backend.historyItems().first { $0.isCurrent }?.id, engineHead)
        XCTAssertEqual(ended.count, 2)
    }

    func testTheStubNeedsTheEngine() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 32, height: 32, depth: .u8, profile: nil)
        defer { doc.close() }
        let c = try XCTUnwrap(doc as? any DocumentContentAwareBackend)
        XCTAssertThrowsError(try c.beginContentAwareMove(layer: 1, mode: .move)) { e in
            XCTAssertTrue(e.localizedDescription.contains("engine"))
        }
        XCTAssertThrowsError(try c.commitContentAwareMove(token: 1))
        c.cancelContentAwareMove(token: 1)
    }
}
