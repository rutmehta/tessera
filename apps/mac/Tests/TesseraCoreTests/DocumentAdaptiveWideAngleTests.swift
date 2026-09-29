import CoreGraphics
import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Filter ▸ Adaptive Wide Angle… (WP B5-20): the recipe draft behind the sheet (parse, camera and scale, lines,
/// hit testing, JSON that keeps untouched engine fields, refusals) and the workspace against the engine (traced
/// lines, preview without history, OK as one node, smart filter re-edit, cancel, the stub).
final class DocumentAdaptiveWideAngleTests: XCTestCase {
    /// What `begin_adaptive_wide_angle` returns for a new 400 × 300 layer (serde of `Adaptive::new`).
    private let engineDefault = """
    {"source_width":400,"source_height":300,"output_width":400,"output_height":300,\
    "camera":{"Manual":{"focal_px":266.6666666666667,"center":[200.0,150.0],"projection":"Rectilinear"}},\
    "output_focal_px":266.6666666666667,"scale":1.0,"crop_factor":1.0,"crop":[0.0,0.0],"lines":[],\
    "mesh_size":[17,17],"smoothness":0.1,"regularization":0.0001,"line_tolerance":0.25,"max_iterations":2000}
    """

    private func object(_ json: String) throws -> [String: Any] {
        try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
    }

    // MARK: Draft

    func testDefaultRecipeParsesAndKeepsEngineFields() throws {
        let d = try AdaptiveWideAngleDraft(recipeJson: engineDefault)
        XCTAssertEqual([d.width, d.height], [400, 300])
        XCTAssertEqual(d.projection, .rectilinear)
        XCTAssertEqual(d.focal35, 24, accuracy: 1e-9, "266.67 px on a 400 px long edge is 24 mm equivalent")
        XCTAssertEqual(d.scalePercent, 100)
        XCTAssertTrue(d.lines.isEmpty)
        let r = try object(d.recipeJson)
        XCTAssertEqual(r["mesh_size"] as? [Int], [17, 17])
        XCTAssertEqual(r["line_tolerance"] as? Double, 0.25)
        XCTAssertEqual(r["max_iterations"] as? Int, 2000)
        XCTAssertEqual(r["source_width"] as? Int, 400)
        let cam = try XCTUnwrap((r["camera"] as? [String: Any])?["Manual"] as? [String: Any])
        XCTAssertEqual(cam["center"] as? [Double], [200, 150])
        XCTAssertEqual(try XCTUnwrap(cam["focal_px"] as? Double), 266.6666666666667, accuracy: 1e-9)
        XCTAssertEqual(try AdaptiveWideAngleDraft(recipeJson: d.recipeJson), d, "encode → parse is stable")
    }

    func testCameraFocalAndScaleConvertAndClamp() throws {
        var d = try AdaptiveWideAngleDraft(recipeJson: engineDefault)
        d.projection = .equidistant
        d.setFocal35(12)
        XCTAssertEqual(d.focalPx, 12.0 / 36 * 400, accuracy: 1e-9)
        d.setScalePercent(85)
        let r = try object(d.recipeJson)
        let cam = try XCTUnwrap((r["camera"] as? [String: Any])?["Manual"] as? [String: Any])
        XCTAssertEqual(cam["projection"] as? String, "Equidistant")
        XCTAssertEqual(try XCTUnwrap(r["output_focal_px"] as? Double), d.focalPx, accuracy: 1e-9)
        XCTAssertEqual(try XCTUnwrap(r["scale"] as? Double), 0.85, accuracy: 1e-12)
        d.setFocal35(1)
        XCTAssertEqual(d.focal35, AdaptiveWideAngleFilter.focalRange.lowerBound)
        d.setFocal35(.nan)
        XCTAssertEqual(d.focal35, 24)
        d.setScalePercent(500)
        XCTAssertEqual(d.scalePercent, AdaptiveWideAngleFilter.scaleRange.upperBound)
        let key = d.curveKey
        d.setScalePercent(100)
        XCTAssertEqual(d.curveKey, key, "scale does not bend constraint curves")
        d.projection = .rectilinear
        XCTAssertNotEqual(d.curveKey, key)
    }

    func testLinesAddClampSelectOrientAndRemove() throws {
        var d = try AdaptiveWideAngleDraft(recipeJson: engineDefault)
        XCTAssertNil(d.addLine(from: CGPoint(x: 10, y: 10), to: CGPoint(x: 11, y: 10.5), orientation: .straight), "too short")
        let i = try XCTUnwrap(d.addLine(from: CGPoint(x: -20, y: 50), to: CGPoint(x: 500, y: 60), orientation: .horizontal))
        XCTAssertEqual(d.lines[i].from, CGPoint(x: 0, y: 50))
        XCTAssertEqual(d.lines[i].to, CGPoint(x: 400, y: 60))
        d.setCurve([CGPoint(x: 1, y: 1), CGPoint(x: 200, y: 40), CGPoint(x: 2, y: 2)], at: i)
        XCTAssertEqual(d.lines[i].points, [CGPoint(x: 0, y: 50), CGPoint(x: 200, y: 40), CGPoint(x: 400, y: 60)],
                       "ends stay the drawn ends")
        XCTAssertEqual(d.line(near: CGPoint(x: 200, y: 43), tolerance: 4), i)
        XCTAssertNil(d.line(near: CGPoint(x: 200, y: 100), tolerance: 4))
        d.setOrientation(.vertical, at: i)
        let r = try object(d.recipeJson)
        let line = try XCTUnwrap((r["lines"] as? [[String: Any]])?.first)
        XCTAssertEqual(line["orientation"] as? String, "Vertical")
        XCTAssertEqual(line["weight"] as? Double, 1)
        XCTAssertEqual((line["points"] as? [[Double]])?.count, 3)
        d.addLine(from: CGPoint(x: 10, y: 10), to: CGPoint(x: 10, y: 200), orientation: .straight)
        d.removeLine(at: 0)
        XCTAssertEqual(d.lines.map(\.orientation), [.straight])
        d.removeAllLines()
        XCTAssertTrue(d.lines.isEmpty)
    }

    func testShiftDragPicksTheDominantAxis() {
        let o = CGPoint.zero
        XCTAssertEqual(AdaptiveLineOrientation.forDrag(from: o, to: CGPoint(x: 10, y: 3), constrain: false), .straight)
        XCTAssertEqual(AdaptiveLineOrientation.forDrag(from: o, to: CGPoint(x: 10, y: 3), constrain: true), .horizontal)
        XCTAssertEqual(AdaptiveLineOrientation.forDrag(from: o, to: CGPoint(x: -2, y: -9), constrain: true), .vertical)
    }

    func testReEditParsesStoredLinesAndRefusesProfiles() throws {
        var stored = try object(engineDefault)
        stored["camera"] = ["Manual": ["focal_px": 100.0, "center": [190.0, 140.0], "projection": "Equidistant"]]
        stored["scale"] = 0.9
        stored["lines"] = [["points": [[20.0, 30.0], [25.0, 150.0], [20.0, 270.0]], "orientation": "Vertical", "weight": 2.0]]
        let json = String(decoding: try JSONSerialization.data(withJSONObject: stored), as: UTF8.self)
        let d = try AdaptiveWideAngleDraft(recipeJson: json)
        XCTAssertEqual(d.projection, .equidistant)
        XCTAssertEqual(d.focal35, 9, accuracy: 1e-9)
        XCTAssertEqual(d.scalePercent, 90, accuracy: 1e-9)
        XCTAssertEqual(d.lines.count, 1)
        XCTAssertEqual(d.lines[0].from, CGPoint(x: 20, y: 30))
        XCTAssertEqual(d.lines[0].to, CGPoint(x: 20, y: 270))
        XCTAssertEqual(d.lines[0].points.count, 3)
        XCTAssertEqual(d.lines[0].weight, 2)
        let cam = try XCTUnwrap((try object(d.recipeJson)["camera"] as? [String: Any])?["Manual"] as? [String: Any])
        XCTAssertEqual(cam["center"] as? [Double], [190, 140], "the stored optical centre is kept")

        stored["camera"] = ["Profile": ["focal_px": 100.0, "center": [200.0, 150.0], "calibration": [String: Any]()]]
        let profile = String(decoding: try JSONSerialization.data(withJSONObject: stored), as: UTF8.self)
        XCTAssertThrowsError(try AdaptiveWideAngleDraft(recipeJson: profile)) { e in
            XCTAssertTrue(e.localizedDescription.contains("lens profile"))
        }
        XCTAssertThrowsError(try AdaptiveWideAngleDraft(recipeJson: "{}"))
    }

    func testNamesAndRefusals() {
        XCTAssertEqual(AdaptiveWideAngleFilter.displayName("adaptive_wide_angle"), "Adaptive Wide Angle")
        XCTAssertEqual(AdaptiveWideAngleFilter.displayName("Gaussian Blur"), "Gaussian Blur")
        XCTAssertNil(AdaptiveWideAngleFilter.refusal(kind: .pixel))
        XCTAssertNil(AdaptiveWideAngleFilter.refusal(kind: .smartObject))
        XCTAssertNotNil(AdaptiveWideAngleFilter.refusal(kind: .adjustment))
        XCTAssertNotNil(AdaptiveWideAngleFilter.refusal(kind: nil))
        XCTAssertEqual(AdaptiveProjection.equidistant.title, "Fisheye")
        let row = SmartFilterRecord(index: 0, filterId: "adaptive_wide_angle", name: "adaptive_wide_angle", enabled: true,
                                    filterJson: "{}", opacity: 1, blendMode: "normal", hasMask: false)
        XCTAssertEqual(SmartFilterRow(row).name, "Adaptive Wide Angle")
    }

    // MARK: Engine

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-awa-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testEngineTracesAppliesAndReEditsASmartFilter() throws {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: 160, height: 120, depth: .u8, profile: nil)
        defer { doc.close() }
        let b = try XCTUnwrap(doc as? any DocumentAdaptiveWideAngleBackend)
        let filters = try XCTUnwrap(doc as? any DocumentFiltersBackend)
        let layer = try doc.layers()[0].id
        _ = try filters.convertForSmartFilters(layer: layer)
        let info = try b.beginAdaptiveWideAngle(layer: layer, stageIndex: nil)
        XCTAssertTrue(info.smartObject)
        XCTAssertNil(info.exifFocal35mm)
        var d = try AdaptiveWideAngleDraft(recipeJson: info.recipeJson)
        d.projection = .equidistant
        d.setFocal35(20)
        let i = try XCTUnwrap(d.addLine(from: CGPoint(x: 30, y: 15), to: CGPoint(x: 30, y: 105), orientation: .vertical))
        d.setCurve(try b.adaptiveWideAngleCurve(recipeJson: d.recipeJson, from: d.lines[i].from, to: d.lines[i].to), at: i)
        XCTAssertGreaterThan(d.lines[i].points.count, 2)
        XCTAssertLessThan(d.lines[i].points[d.lines[i].points.count / 2].x, 30, "a fisheye bows left-side verticals outward")
        let before = try doc.historyItems().count
        let p = try b.previewAdaptiveWideAngle(token: info.token, recipeJson: d.recipeJson)
        XCTAssertFalse(p.original)
        XCTAssertEqual(try doc.historyItems().count, before, "previews record no history")
        _ = try b.commitAdaptiveWideAngle(token: info.token, recipeJson: d.recipeJson)
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        let rows = try filters.smartFilters(layer: layer)
        XCTAssertEqual(rows.map(\.filterId), ["adaptive_wide_angle"])
        XCTAssertEqual(rows.first?.name, "Adaptive Wide Angle")

        let again = try b.beginAdaptiveWideAngle(layer: layer, stageIndex: 0)
        XCTAssertEqual(try AdaptiveWideAngleDraft(recipeJson: again.recipeJson), d, "re-edit returns the stored recipe")
        var edited = d
        edited.setScalePercent(90)
        _ = try b.commitAdaptiveWideAngle(token: again.token, recipeJson: edited.recipeJson)
        XCTAssertEqual(try filters.smartFilters(layer: layer).count, 1, "replaced in place")

        // A conflicting constraint: an error, no history.
        let bad = try b.beginAdaptiveWideAngle(layer: layer, stageIndex: 0)
        var c = try AdaptiveWideAngleDraft(recipeJson: bad.recipeJson)
        c.projection = .rectilinear
        c.removeAllLines()
        c.addLine(from: CGPoint(x: 80, y: 10), to: CGPoint(x: 80, y: 110), orientation: .horizontal)
        let n = try doc.historyItems().count
        XCTAssertThrowsError(try b.commitAdaptiveWideAngle(token: bad.token, recipeJson: c.recipeJson))
        XCTAssertEqual(try doc.historyItems().count, n)
        b.cancelAdaptiveWideAngle(token: bad.token)
        XCTAssertThrowsError(try b.previewAdaptiveWideAngle(token: bad.token, recipeJson: nil))
    }

    @MainActor func testWorkspaceDrawsTracedLinesPreviewsAndAppliesOnce() async throws {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        let backend = EngineDocumentBackend(session: try engine.newDocument(width: 120, height: 90, depth: .u8, profile: nil))
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let b = try XCTUnwrap(backend as (any DocumentBackend) as? any DocumentAdaptiveWideAngleBackend)
        let layer = try XCTUnwrap(doc.layers.first).id
        let info = try b.beginAdaptiveWideAngle(layer: layer, stageIndex: nil)
        let m = AdaptiveWideAngleWorkspaceModel(doc: doc, backend: b, info: info,
                                                draft: try AdaptiveWideAngleDraft(recipeJson: info.recipeJson), layerName: "Layer 1")
        let rows = try backend.historyItems().count
        m.start()
        await waitFor("the first preview") { m.original != nil && m.corrected != nil }
        m.projection = .equidistant
        m.setFocal(18)
        m.pointerDown(CGPoint(x: 20, y: 10), tolerance: 2)
        XCTAssertFalse(m.preview, "drawing shows the original with its lines")
        m.pointerDragged(CGPoint(x: 21, y: 40))
        m.pointerUp(CGPoint(x: 20, y: 80), constrain: true)
        XCTAssertEqual(m.draft.lines.map(\.orientation), [.vertical])
        XCTAssertGreaterThan(m.draft.lines[0].points.count, 2, "traced along the camera model")
        XCTAssertEqual(m.selected, 0)
        m.pointerDown(CGPoint(x: 5, y: 5), tolerance: 2)
        m.pointerUp(CGPoint(x: 6, y: 5), constrain: false)
        XCTAssertEqual(m.draft.lines.count, 1, "a click is not a line")
        // Changing the camera re-traces the curve.
        let traced = m.draft.lines[0].points
        m.setFocal(30)
        XCTAssertNotEqual(m.draft.lines[0].points, traced)
        XCTAssertEqual(try backend.historyItems().count, rows, "editing records no history")
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.ok()
        await waitFor("the apply") { ended != nil }
        if case .failure(let e) = ended { XCTFail("apply failed: \(e)") }
        XCTAssertEqual(try backend.historyItems().count, rows + 1)
        XCTAssertNil(DocumentAdaptiveWideAngle.shared.workspace)
    }

    func testTheStubNeedsTheEngine() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 32, height: 32, depth: .u8, profile: nil)
        defer { doc.close() }
        let b = try XCTUnwrap(doc as? any DocumentAdaptiveWideAngleBackend)
        XCTAssertThrowsError(try b.beginAdaptiveWideAngle(layer: 1, stageIndex: nil)) { e in
            XCTAssertTrue(e.localizedDescription.contains("engine"))
        }
        XCTAssertThrowsError(try b.previewAdaptiveWideAngle(token: 1, recipeJson: nil))
        XCTAssertThrowsError(try b.adaptiveWideAngleCurve(recipeJson: "{}", from: .zero, to: CGPoint(x: 1, y: 1)))
        b.cancelAdaptiveWideAngle(token: 1)
    }

    @MainActor private func waitFor(_ what: String, timeout: Double = 60, _ condition: () -> Bool) async {
        let end = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > end { XCTFail("timed out waiting for \(what)"); return }
            try? await Task.sleep(for: .milliseconds(10))
        }
    }
}
