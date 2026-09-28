import AppKit
import CoreGraphics
import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Warp, Perspective Warp, Puppet Warp and Content-Aware Scale host models (WP B5-12): the engine
/// `TransformOp` JSON schema, Bézier net evaluation and editing, exact engine splits and presets, the
/// linked perspective grid (convexity, split without a jump, layout vs warp drags), puppet pins,
/// the content-aware box, child ↔ document mapping, and a real `DocumentSession` session through
/// `EngineDocumentBackend` (consent, cancel, one node, stale tokens, re-edit).
final class DocumentTransformTests: XCTestCase {
    private func close(_ a: CGPoint, _ b: CGPoint, _ eps: Double = 1e-6, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(Double(a.x), Double(b.x), accuracy: eps, file: file, line: line)
        XCTAssertEqual(Double(a.y), Double(b.y), accuracy: eps, file: file, line: line)
    }

    @MainActor
    func testTransformReturnAndEnterUseTheSameModifierGuard() {
        for key in [UInt16(36), 76] {
            XCTAssertEqual(DocumentTransforms.keyAction(keyCode: key, modifiers: []), .apply)
            XCTAssertEqual(DocumentTransforms.keyAction(keyCode: key, modifiers: [.command]), .apply)
            let blocked: [NSEvent.ModifierFlags] = [.shift, .option, .control, [.command, .shift]]
            for mods in blocked {
                XCTAssertNil(DocumentTransforms.keyAction(keyCode: key, modifiers: mods))
            }
        }
    }

    @MainActor
    func testTransformDeleteVariantsAndEscapeRequireUnmodifiedKeys() {
        for key in [UInt16(51), 117] {
            XCTAssertEqual(DocumentTransforms.keyAction(keyCode: key, modifiers: []), .removePin)
            let blocked: [NSEvent.ModifierFlags] = [.command, .shift, .option, .control]
            for mods in blocked {
                XCTAssertNil(DocumentTransforms.keyAction(keyCode: key, modifiers: mods))
            }
        }
        XCTAssertEqual(DocumentTransforms.keyAction(keyCode: 53, modifiers: []), .cancel)
        XCTAssertNil(DocumentTransforms.keyAction(keyCode: 53, modifiers: [.command]))
        XCTAssertNil(DocumentTransforms.keyAction(keyCode: 0, modifiers: []))
    }

    @MainActor
    func testTransformKeyRoutingIgnoresOnlyIncidentalFlags() {
        let incidental: NSEvent.ModifierFlags = [.numericPad, .function, .capsLock]
        XCTAssertEqual(DocumentTransforms.keyAction(keyCode: 76, modifiers: incidental), .apply)
        XCTAssertEqual(DocumentTransforms.keyAction(keyCode: 117, modifiers: incidental), .removePin)
        XCTAssertEqual(DocumentTransforms.keyAction(keyCode: 53, modifiers: incidental), .cancel)
        XCTAssertNil(DocumentTransforms.keyAction(keyCode: 36, modifiers: incidental.union(.shift)))
        XCTAssertNil(DocumentTransforms.keyAction(keyCode: 51, modifiers: incidental.union(.command)))
    }

    // MARK: Warp

    func testWarpMeshJSONMatchesTheEngineSchema() throws {
        let m = WarpMeshModel.identity(width: 300, height: 150)
        let obj = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(m.json.utf8)) as? [String: Any])
        XCTAssertEqual(Set(obj.keys), ["width", "height", "control_points", "u_splits", "v_splits"])
        let pts = try XCTUnwrap(obj["control_points"] as? [[[Double]]])
        XCTAssertEqual(pts.count, 4)
        XCTAssertEqual(pts[3][3], [300, 150])
        XCTAssertEqual(pts[1][2], [200, 50])
        XCTAssertEqual(WarpMeshModel(json: m.json), m)
        // The engine's own preset JSON decodes into the same model.
        let engine = try TransformBridge.preset(width: 300, height: 150, name: "Arc", bend: 0)
        XCTAssertEqual(engine, m)
    }

    func testWarpSurfaceInterpolatesCornersAndIsLinearAtIdentity() {
        let m = WarpMeshModel.identity(width: 400, height: 200)
        close(m.point(u: 0, v: 0), .zero)
        close(m.point(u: 1, v: 1), CGPoint(x: 400, y: 200))
        close(m.point(u: 0.25, v: 0.5), CGPoint(x: 100, y: 100))
        XCTAssertEqual(m.gridLines(samples: 8).count, 4)
        XCTAssertEqual(m.visibleControls().count, 12, "anchors and edge tangents; interior twist points hidden")
        XCTAssertEqual(m.handleSegments().count, 8)
    }

    func testWarpAnchorDragCarriesItsHandles() {
        var m = WarpMeshModel.identity(width: 300, height: 300)
        let handle = m.controlPoints[0][1], interior = m.controlPoints[1][1]
        m.move(row: 0, column: 0, to: CGPoint(x: 30, y: 12))
        close(m.controlPoints[0][0], CGPoint(x: 30, y: 12))
        close(m.controlPoints[0][1], CGPoint(x: handle.x + 30, y: handle.y + 12))
        close(m.controlPoints[1][1], CGPoint(x: interior.x + 30, y: interior.y + 12))
        // A handle moves alone.
        let anchor = m.controlPoints[0][3]
        m.move(row: 0, column: 2, to: CGPoint(x: 250, y: -40))
        close(m.controlPoints[0][3], anchor)
        XCTAssertNotNil(m.hit(CGPoint(x: 250, y: -38), radius: 5))
        XCTAssertNil(m.hit(CGPoint(x: 150, y: 150), radius: 5), "interior points are not handles")
    }

    func testEngineSplitAddsLinesWithoutMovingTheSurface() throws {
        let bent = try TransformBridge.preset(width: 400, height: 240, name: "Flag", bend: 0.6)
        let split = try TransformBridge.split(bent, at: CGPoint(x: 150, y: 90), vertical: true, horizontal: true)
        XCTAssertEqual(split.columns, 7)
        XCTAssertEqual(split.rows, 7)
        XCTAssertTrue(split.isStructurallyValid)
        for (u, v) in [(0.1, 0.2), (0.5, 0.5), (0.77, 0.9), (0.33, 0.61)] {
            close(split.point(u: u, v: v), bent.point(u: u, v: v), 1e-6)
        }
        XCTAssertThrowsError(try TransformBridge.split(bent, at: CGPoint(x: -900, y: -900), vertical: true, horizontal: false))
        let grid = try TransformBridge.subdivide(bent, columns: 4, rows: 4)
        XCTAssertEqual(grid.columns, 13)
        close(grid.point(u: 0.4, v: 0.3), bent.point(u: 0.4, v: 0.3), 1e-6)
    }

    func testPresetsBendZeroIsIdentityAndInvalidBendFails() throws {
        let names = TransformBridge.presetNames()
        XCTAssertEqual(names.count, 14)
        XCTAssertEqual(WarpPresetInfo.title("ArcLower"), "Arc Lower")
        let identity = WarpMeshModel.identity(width: 120, height: 80)
        for n in names { XCTAssertEqual(try TransformBridge.preset(width: 120, height: 80, name: n, bend: 0), identity, n) }
        XCTAssertNotEqual(try TransformBridge.preset(width: 120, height: 80, name: "Bulge", bend: 0.5), identity)
        XCTAssertThrowsError(try TransformBridge.preset(width: 120, height: 80, name: "Arc", bend: 1.2))
        XCTAssertThrowsError(try TransformBridge.preset(width: 120, height: 80, name: "Spiral", bend: 0.2))
    }

    // MARK: Perspective

    func testPerspectiveQuadsArePerimeterOrderedAndRoundTrip() throws {
        var p = PerspectiveModel(rect: CGRect(x: 10, y: 20, width: 200, height: 100))
        p.splitColumn(0)
        let obj = p.jsonObject
        let src = try XCTUnwrap(obj["source_quads"] as? [[[Double]]])
        XCTAssertEqual(src.count, 2)
        XCTAssertEqual(src[0], [[10, 20], [110, 20], [110, 120], [10, 120]])
        XCTAssertEqual(src[1][0], src[0][1], "the planes share their edge")
        let dst = try XCTUnwrap(obj["destination_quads"] as? [[[Double]]])
        let back = try XCTUnwrap(PerspectiveModel(quads: src, destination: dst))
        XCTAssertEqual(back, p)
        let json = TransformOperationModel.perspective(p).json(kernel: .bilinear)
        guard case .perspective(let parsed)? = TransformOperationModel.parse(json, canvasWidth: 1, canvasHeight: 1)?.0 else {
            return XCTFail("perspective JSON did not parse")
        }
        XCTAssertEqual(parsed, p)
    }

    func testPerspectiveSplitKeepsTheWarpedImageInPlace() throws {
        var p = PerspectiveModel(rect: CGRect(x: 0, y: 0, width: 100, height: 100))
        p.destination = [[CGPoint(x: 5, y: 0), CGPoint(x: 90, y: 10)], [CGPoint(x: 0, y: 100), CGPoint(x: 100, y: 95)]]
        let h = try XCTUnwrap(Homography(from: p.quad(p.source, 0, 0), to: p.quad(p.destination, 0, 0)))
        p.splitColumn(0)
        p.splitRow(0)
        XCTAssertEqual(p.quads.count, 4)
        XCTAssertTrue(p.isValid)
        for row in 0...p.rows {
            for c in 0...p.columns { close(p.destination[row][c], h.map(p.source[row][c]), 1e-6) }
        }
    }

    func testPerspectiveLayoutMovesBothSpacesAndWarpOnlyTheDestination() {
        var p = PerspectiveModel(rect: CGRect(x: 0, y: 0, width: 100, height: 100))
        p.destination[1][1] = CGPoint(x: 110, y: 104)
        p.move(row: 1, column: 1, to: CGPoint(x: 90, y: 95), layout: true)
        close(p.source[1][1], CGPoint(x: 90, y: 95))
        close(p.destination[1][1], CGPoint(x: 100, y: 99))
        p.move(row: 0, column: 0, to: CGPoint(x: 8, y: 6), layout: false)
        close(p.source[0][0], .zero)
        close(p.destination[0][0], CGPoint(x: 8, y: 6))
        XCTAssertNotNil(p.hit(CGPoint(x: 9, y: 6), radius: 3, layout: false))
        XCTAssertNil(p.hit(CGPoint(x: 9, y: 6), radius: 3, layout: true))
    }

    func testCrossingAndDegenerateQuadsAreRejectedBeforeTheEngine() {
        XCTAssertTrue(PerspectiveModel.convex([CGPoint(x: 0, y: 0), CGPoint(x: 10, y: 0), CGPoint(x: 10, y: 10), CGPoint(x: 0, y: 10)]))
        XCTAssertFalse(PerspectiveModel.convex([CGPoint(x: 0, y: 0), CGPoint(x: 10, y: 10), CGPoint(x: 10, y: 0), CGPoint(x: 0, y: 10)]))
        XCTAssertFalse(PerspectiveModel.convex([CGPoint(x: 0, y: 0), CGPoint(x: 10, y: 0), CGPoint(x: 10, y: 0), CGPoint(x: 0, y: 10)]))
        XCTAssertFalse(PerspectiveModel.convex([CGPoint(x: 0, y: 0), CGPoint(x: 10, y: 0), CGPoint(x: 3, y: 3), CGPoint(x: 0, y: 10)]))
        var p = PerspectiveModel(rect: CGRect(x: 0, y: 0, width: 100, height: 100))
        p.move(row: 1, column: 1, to: CGPoint(x: -50, y: -50), layout: false)
        XCTAssertFalse(p.isValid)
    }

    // MARK: Puppet

    private func puppetJSON() -> String {
        #"{"rest_vertices":[[0,0],[4,0],[4,4],[0,4],[8,0],[8,4]],"triangles":[[0,1,2],[0,2,3],[1,4,5],[1,5,2]],"#
            + #""pins":[],"density":"Normal","expansion":2,"mode":"Normal","iterations":20}"#
    }

    func testPuppetModelRoundTripsAndPinsNeverJump() throws {
        var p = try XCTUnwrap(PuppetModel(json: puppetJSON()))
        XCTAssertEqual(p.edges.count, 9)
        let deformed = p.restVertices.map { CGPoint(x: $0.x + 1, y: $0.y) }
        let i = try XCTUnwrap(p.addPin(near: CGPoint(x: 9, y: 4.5), positions: deformed, radius: 2))
        XCTAssertEqual(p.pins[i].vertex, 5)
        close(p.pins[i].target, CGPoint(x: 9, y: 4), 0)
        XCTAssertEqual(p.addPin(near: CGPoint(x: 9, y: 4), positions: deformed, radius: 2), i, "no duplicate pin")
        XCTAssertNil(p.addPin(near: CGPoint(x: 40, y: 40), positions: deformed, radius: 2))
        p.pins[i].rotation = 0.5
        let obj = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(p.json.utf8)) as? [String: Any])
        let pins = try XCTUnwrap(obj["pins"] as? [[String: Any]])
        XCTAssertEqual(pins.first?["vertex"] as? Int, 5)
        XCTAssertEqual(pins.first?["rotation"] as? Double, 0.5)
        XCTAssertEqual(PuppetModel(json: p.json), p)
        XCTAssertEqual(p.pinHit(CGPoint(x: 9.5, y: 4), radius: 1), 0)
    }

    func testPuppetRebaseKeepsPinsOnTheNearestNewVertex() throws {
        var p = try XCTUnwrap(PuppetModel(json: puppetJSON()))
        p.pins = [PuppetPinModel(vertex: 5, target: CGPoint(x: 20, y: 20), rotation: 0.3), PuppetPinModel(vertex: 0, target: .zero)]
        p.mode = "Rigid"
        var dense = try XCTUnwrap(PuppetModel(json: puppetJSON()))
        dense.restVertices = dense.restVertices.map { CGPoint(x: $0.x + 0.4, y: $0.y) }
        let r = p.rebased(onto: dense)
        XCTAssertEqual(r.mode, "Rigid")
        XCTAssertEqual(r.pins.map(\.vertex), [5, 0])
        XCTAssertEqual(r.pins[0].rotation, 0.3)
        close(r.pins[0].target, CGPoint(x: 20, y: 20), 0)
    }

    // MARK: Content-aware scale, JSON, mapping

    func testContentAwareBoxHandlesProportionsAndLimits() {
        var c = ContentAwareScaleModel(canvasWidth: 400, canvasHeight: 200)
        c.drag(.right, to: CGPoint(x: 300.4, y: 0), proportional: false)
        XCTAssertEqual([c.width, c.height], [300, 200])
        c.drag(.corner, to: CGPoint(x: 200, y: 999), proportional: true)
        XCTAssertEqual([c.width, c.height], [200, 100])
        c.drag(.bottom, to: CGPoint(x: 0, y: -50), proportional: false)
        XCTAssertEqual(c.height, 1)
        c.drag(.right, to: CGPoint(x: 1e9, y: 0), proportional: false)
        XCTAssertEqual(c.width, 1600)
        close(c.point(.corner), CGPoint(x: 1600, y: 1))
    }

    func testOperationJSONCarriesKernelAndNeverAProtectMask() throws {
        var c = ContentAwareScaleModel(canvasWidth: 64, canvasHeight: 32)
        c.width = 48; c.amount = 0.25; c.protectChannel = 9
        let json = TransformOperationModel.contentAwareScale(c).json(kernel: .bilinear)
        let obj = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
        XCTAssertEqual(obj["version"] as? Int, 1)
        XCTAssertEqual(obj["kernel"] as? String, "Bilinear")
        let cas = try XCTUnwrap((obj["operation"] as? [String: Any])?["ContentAwareScale"] as? [String: Any])
        XCTAssertTrue(cas["protect"] is NSNull)
        XCTAssertEqual(cas["target_width"] as? Int, 48)
        for k in TransformKernel.allCases {
            let w = TransformOperationModel.warp(.identity(width: 10, height: 10)).json(kernel: k)
            XCTAssertEqual(TransformOperationModel.parse(w, canvasWidth: 10, canvasHeight: 10)?.1, k)
        }
        XCTAssertEqual(AdvancedTransformTag.allCases.filter(\.editable).count, 4)
        for tag in AdvancedTransformTag.allCases { XCTAssertEqual(AdvancedTransformTag(tag.ffi), tag) }
    }

    func testChildMappingUsesTheRowMajorAffineWithSkewAndTranslation() {
        let m = AffineTransform2D(TransformMatrix(a: 0.5, b: 0.2, c: 40, d: -0.1, e: 0.75, f: 30))
        let map = ChildMapping(m)
        let doc = map.document(CGPoint(x: 100, y: 200))
        close(doc, CGPoint(x: 0.5 * 100 + 0.2 * 200 + 40, y: -0.1 * 100 + 0.75 * 200 + 30))
        close(map.child(doc), CGPoint(x: 100, y: 200), 1e-9)
        let start = AdvancedTransformStart(token: 1, layer: 2, layerKind: .smartObject, kind: .warp, editingIndex: nil, insertIndex: 0,
                                           needsConversion: false, childWidth: 640, childHeight: 480, childToDocument: m,
                                           contentBounds: CanvasRect(x: 10, y: 20, width: 30, height: 40), existing: nil, revision: 0,
                                           draftLevel: 0, otherStages: 0, limitations: [])
        XCTAssertEqual(start.contentRect, CGRect(x: 10, y: 20, width: 30, height: 40))
        XCTAssertEqual(start.childRect.size, CGSize(width: 640, height: 480))
    }

    // MARK: Engine session

    private func session() throws -> (EngineDocumentBackend, URL) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("b512-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let s = try engine.newDocument(width: 96, height: 64, depth: .u8, profile: nil)
        _ = try s.addLayer(kind: .fill(json: #"{"kind":"solid","color":[0.8,0.4,0.2]}"#), name: "Fill", parent: nil, index: nil)
        return (EngineDocumentBackend(session: s), dir)
    }

    func testEngineSessionNeedsConsentCancelsCleanlyAndAppliesOneNode() throws {
        let (b, _) = try session()
        let layer = try XCTUnwrap(b.session.layers().first?.id)
        let nodes = try b.historyItems().count
        let s = try b.beginAdvancedTransform(layer: layer, index: nil, kind: .warp)
        XCTAssertTrue(s.needsConversion)
        XCTAssertEqual(s.childWidth, 96)
        let bent = try TransformBridge.preset(width: 96, height: 64, name: "Arc", bend: 0.4)
        _ = try b.previewAdvancedTransform(token: s.token, json: TransformOperationModel.warp(bent).json(kernel: .bicubic), draft: false)
        _ = try b.cancelAdvancedTransform(token: s.token)
        XCTAssertEqual(try b.session.layer(id: layer).kind, .fill)
        XCTAssertEqual(try b.historyItems().count, nodes)
        XCTAssertThrowsError(try b.commitAdvancedTransform(token: s.token, convert: true), "stale token")

        let s2 = try b.beginAdvancedTransform(layer: layer, index: nil, kind: .warp)
        _ = try b.previewAdvancedTransform(token: s2.token, json: TransformOperationModel.warp(bent).json(kernel: .bicubic), draft: false)
        XCTAssertThrowsError(try b.commitAdvancedTransform(token: s2.token, convert: false), "consent required")
        _ = try b.commitAdvancedTransform(token: s2.token, convert: true)
        XCTAssertEqual(try b.historyItems().count, nodes + 1)
        XCTAssertEqual(try b.session.layer(id: layer).kind, .smartObject)
        let stages = try b.transformStages(layer: layer)
        XCTAssertEqual(stages.map(\.kind), [.warp])
        // Re-edit: the stage parses back into the editor's model.
        let s3 = try b.beginAdvancedTransform(layer: layer, index: stages[0].index, kind: .warp)
        let existing = try XCTUnwrap(s3.existing)
        guard case .warp(let m)? = TransformOperationModel.parse(existing.json, canvasWidth: 96, canvasHeight: 64)?.0 else {
            return XCTFail("stage did not parse")
        }
        XCTAssertEqual(m, bent)
        // An invalid perspective is rejected with the session intact.
        XCTAssertThrowsError(try b.previewAdvancedTransform(token: s3.token, json: "{\"version\":1}", draft: false))
        _ = try b.cancelAdvancedTransform(token: s3.token)
    }

    func testEnginePuppetMeshAndProtectionChannels() throws {
        let (b, _) = try session()
        let layer = try XCTUnwrap(b.session.layers().first?.id)
        let mesh = try b.puppetMesh(layer: layer, density: .sparse, expansion: 0)
        XCTAssertGreaterThan(mesh.vertexCount, 20)
        XCTAssertEqual(mesh.mesh.pins.count, 0)
        XCTAssertEqual(mesh.mesh.restVertices.count, Int(mesh.vertexCount))
        XCTAssertThrowsError(try b.puppetMesh(layer: layer, density: .normal, expansion: 65))
        XCTAssertTrue(b.protectionChannels().isEmpty)
        _ = try b.session.setSelectionRect(x: 0, y: 0, width: 20, height: 64, feather: 0)
        _ = try b.session.saveSelectionChannel(name: "Keep", target: nil, op: .replace)
        XCTAssertEqual(b.protectionChannels().map(\.name), ["Keep"])
    }
}
