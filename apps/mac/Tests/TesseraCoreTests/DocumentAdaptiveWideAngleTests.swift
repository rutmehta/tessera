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

    /// B5-20b (A's review): re-editing keeps the stored camera and output focal lengths exactly, even outside the
    /// slider's 4–400 mm range and when they differ, until the focal length is actually edited.
    func testReEditKeepsStoredFocalLengthsUntilEdited() throws {
        var stored = try object(engineDefault)
        let tiny = 2.0 / 36 * 400 // 2 mm equivalent, below the slider's range
        stored["camera"] = ["Manual": ["focal_px": tiny, "center": [200.0, 150.0], "projection": "Equidistant"]]
        stored["output_focal_px"] = 123.456
        let json = String(decoding: try JSONSerialization.data(withJSONObject: stored), as: UTF8.self)
        var d = try AdaptiveWideAngleDraft(recipeJson: json)
        XCTAssertEqual(d.focal35, AdaptiveWideAngleFilter.focalRange.lowerBound, "the slider shows the clamped value")
        var r = try object(d.recipeJson)
        var cam = try XCTUnwrap((r["camera"] as? [String: Any])?["Manual"] as? [String: Any])
        XCTAssertEqual(cam["focal_px"] as? Double, tiny, "stored camera focal kept exactly")
        XCTAssertEqual(r["output_focal_px"] as? Double, 123.456, "stored output focal kept exactly")
        d.setScalePercent(90)
        d.projection = .rectilinear
        r = try object(d.recipeJson)
        XCTAssertEqual(r["output_focal_px"] as? Double, 123.456, "other edits keep it too")
        d.setFocal35(24)
        r = try object(d.recipeJson)
        cam = try XCTUnwrap((r["camera"] as? [String: Any])?["Manual"] as? [String: Any])
        XCTAssertEqual(try XCTUnwrap(cam["focal_px"] as? Double), 24.0 / 36 * 400, accuracy: 1e-9)
        XCTAssertEqual(try XCTUnwrap(r["output_focal_px"] as? Double), 24.0 / 36 * 400, accuracy: 1e-9)
    }

    /// Smart filter rows after the B5-18b merge: the engine names Camera Raw itself ("Camera Raw Filter"),
    /// Adaptive Wide Angle rows carry the id and show the title, other names pass through.
    func testSmartFilterRowsNameCameraRawAndAdaptiveWideAngle() {
        func row(_ id: String, _ name: String) -> SmartFilterRow {
            SmartFilterRow(SmartFilterRecord(index: 0, filterId: id, name: name, enabled: true, filterJson: "{}",
                                             opacity: 1, blendMode: "normal", hasMask: false))
        }
        XCTAssertEqual(row("camera_raw", "Camera Raw Filter").name, "Camera Raw Filter")
        XCTAssertEqual(row("adaptive_wide_angle", "adaptive_wide_angle").name, "Adaptive Wide Angle")
        XCTAssertEqual(row("gaussian_blur", "Gaussian Blur").name, "Gaussian Blur")
        XCTAssertEqual(row("adaptive_wide_angle", "adaptive_wide_angle").filterId, "adaptive_wide_angle")
    }

    func testNamesAndRefusals() {
        XCTAssertEqual(AdaptiveWideAngleFilter.displayName("adaptive_wide_angle"), "Adaptive Wide Angle")
        XCTAssertEqual(AdaptiveWideAngleFilter.displayName("Gaussian Blur"), "Gaussian Blur")
        XCTAssertNil(AdaptiveWideAngleFilter.refusal(kind: .pixel))
        XCTAssertEqual(AdaptiveWideAngleFilter.maxPixels, 100_000_000, "B5-20b: the engine's one limit, over FFI")
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
        await waitFor("the re-trace") { m.draft.lines[0].points != traced }
        XCTAssertEqual(try backend.historyItems().count, rows, "editing records no history")
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.ok()
        await waitFor("the apply") { ended != nil }
        if case .failure(let e) = ended { XCTFail("apply failed: \(e)") }
        XCTAssertEqual(try backend.historyItems().count, rows + 1)
        XCTAssertNil(DocumentAdaptiveWideAngle.shared.workspace)
    }

    /// B5-26: dragging the focal slider re-traces the constraints off the main actor, latest wins: a tick never waits
    /// for the tracer, a slow tracer runs far fewer times than there are ticks, results for superseded focal lengths
    /// are never shown, and the curves end on exactly the released value (then a preview of them lands).
    @MainActor func testFocalSliderRetracesOffTheMainThreadLatestWins() async throws {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        let backend = EngineDocumentBackend(session: try engine.newDocument(width: 120, height: 90, depth: .u8, profile: nil))
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let b = try XCTUnwrap(backend as (any DocumentBackend) as? any DocumentAdaptiveWideAngleBackend)
        let layer = try XCTUnwrap(doc.layers.first).id
        let info = try b.beginAdaptiveWideAngle(layer: layer, stageIndex: nil)
        let m = AdaptiveWideAngleWorkspaceModel(doc: doc, backend: b, info: info,
                                                draft: try AdaptiveWideAngleDraft(recipeJson: info.recipeJson), layerName: "Layer 1")
        defer { m.cancel() }
        m.start()
        await waitFor("the first preview") { m.original != nil && m.corrected != nil }
        m.projection = .equidistant
        m.setFocal(18)
        await waitFor("the fisheye preview") { m.corrected != nil }
        XCTAssertTrue(m.addLine(from: CGPoint(x: 20, y: 10), to: CGPoint(x: 20, y: 80), orientation: .vertical))
        let initial = m.draft.lines[0].points

        let log = TraceLog()
        m.tracer = { json, from, to in
            log.record(json: json, onMain: Thread.isMainThread)
            Thread.sleep(forTimeInterval: 0.08)
            return try b.adaptiveWideAngleCurve(recipeJson: json, from: from, to: to)
        }
        var previews = 0
        m.onPreview = { previews += 1 }
        let ticks = Array(stride(from: 20.0, through: 40.0, by: 0.5))
        let clock = ContinuousClock()
        var longest = Duration.zero
        for v in ticks {
            let t0 = clock.now
            m.setFocal(v)
            longest = max(longest, clock.now - t0)
        }
        XCTAssertEqual(m.draft.focal35, 40, "the slider value itself is applied at once")
        XCTAssertLessThan(longest, .milliseconds(80), "a slider tick waited for the tracer on the main actor")

        var want = m.draft
        want.setCurve(try b.adaptiveWideAngleCurve(recipeJson: want.recipeJson, from: want.lines[0].from, to: want.lines[0].to), at: 0)
        let final = want.lines[0].points
        XCTAssertNotEqual(final, initial)
        var seen: [[CGPoint]] = []
        await waitFor("the final re-trace") {
            let p = m.draft.lines[0].points
            if seen.last != p { seen.append(p) }
            return p == final
        }
        // Let any superseded trace still in flight land, then check it was discarded.
        let settle = Date().addingTimeInterval(0.4)
        while Date() < settle {
            let p = m.draft.lines[0].points
            if seen.last != p { seen.append(p) }
            try? await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(seen.last, final, "a stale trace replaced the final curves")
        XCTAssertTrue(seen.allSatisfy { $0 == initial || $0 == final }, "curves for a superseded focal length were shown")

        XCTAssertFalse(log.onMain, "the tracer ran on the main thread")
        XCTAssertGreaterThanOrEqual(log.count, 1)
        XCTAssertLessThanOrEqual(log.count, 3, "\(ticks.count) ticks traced \(log.count) times: not coalesced")
        let last = try AdaptiveWideAngleDraft(recipeJson: try XCTUnwrap(log.last))
        XCTAssertEqual(last.focal35, 40, "the last trace is for the released value")
        await waitFor("a preview of the final curves") { previews > 0 }
    }

    /// A workspace on a fresh 120 × 90 document (optionally a smart object) with a fisheye 18 mm camera and `lines`
    /// traced by the real tracer.
    @MainActor private func fisheyeWorkspace(smartObject: Bool = false, lines: [(CGPoint, CGPoint)]) async throws
        -> (m: AdaptiveWideAngleWorkspaceModel, b: any DocumentAdaptiveWideAngleBackend, backend: EngineDocumentBackend,
            doc: DocumentController, layer: DocLayerID) {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        let backend = EngineDocumentBackend(session: try engine.newDocument(width: 120, height: 90, depth: .u8, profile: nil))
        let doc = try DocumentController(backend: backend)
        let b = try XCTUnwrap(backend as (any DocumentBackend) as? any DocumentAdaptiveWideAngleBackend)
        let layer = try XCTUnwrap(doc.layers.first).id
        if smartObject {
            let filters = try XCTUnwrap(backend as (any DocumentBackend) as? any DocumentFiltersBackend)
            _ = try filters.convertForSmartFilters(layer: layer)
            doc.reloadModel()
        }
        let info = try b.beginAdaptiveWideAngle(layer: layer, stageIndex: nil)
        let m = AdaptiveWideAngleWorkspaceModel(doc: doc, backend: b, info: info,
                                                draft: try AdaptiveWideAngleDraft(recipeJson: info.recipeJson), layerName: "Layer 1")
        m.start()
        await waitFor("the first preview") { m.original != nil && m.corrected != nil }
        m.projection = .equidistant
        m.setFocal(18)
        for (a, c) in lines { XCTAssertTrue(m.addLine(from: a, to: c, orientation: .vertical)) }
        return (m, b, backend, doc, layer)
    }

    /// Follow-up to B5-26 review: a line whose final trace fails at OK is not committed with its curve from the
    /// previous camera. OK fails with the same "Constraint n: …" message a drag shows, and records no history.
    @MainActor func testOKSurfacesAFailedFinalTrace() async throws {
        let w = try await fisheyeWorkspace(lines: [(CGPoint(x: 20, y: 10), CGPoint(x: 20, y: 80))])
        defer { w.doc.close() }
        let (m, b) = (w.m, w.b)
        let rows = try w.backend.historyItems().count
        let t = ScriptedTracer(delays: [0.2], failure: "outside the camera's field of view")
        m.tracer = { json, a, c in try t.trace(json) { try b.adaptiveWideAngleCurve(recipeJson: json, from: a, to: c) } }
        m.setFocal(30)
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.ok()
        await waitFor("OK to end") { ended != nil }
        if case .success = ended { XCTFail("OK committed a constraint whose final trace failed") }
        XCTAssertEqual(m.error, "Constraint 1: outside the camera's field of view")
        XCTAssertEqual(try w.backend.historyItems().count, rows, "nothing committed")
        m.cancel()
    }

    @MainActor func testLateSliderTraceCannotEraseFinalApplyError() async throws {
        let w = try await fisheyeWorkspace(lines: [(CGPoint(x: 20, y: 10), CGPoint(x: 20, y: 80))])
        defer { w.m.cancel(); w.doc.close() }
        let m = w.m
        let t = ScriptedTracer(delays: [0.8, 0], failure: "outside the camera's field of view")
        m.tracer = { json, _, _ in try t.trace(json) { [] } }
        m.setFocal(30)
        await waitFor("slider trace to start") { t.started == 1 }
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.ok()
        await waitFor("failed final apply") { ended != nil }
        XCTAssertEqual(m.error, "Constraint 1: outside the camera's field of view")
        let deadline = Date().addingTimeInterval(1.2)
        while Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertEqual(m.error, "Constraint 1: outside the camera's field of view",
                       "A pre-OK slider trace and its preview must not clear the final apply error")
    }

    /// OK pressed while a slider re-trace is still pending commits curves traced for the released focal length.
    @MainActor func testOKDuringAPendingRetraceCommitsTheFinalFocal() async throws {
        let w = try await fisheyeWorkspace(smartObject: true, lines: [(CGPoint(x: 20, y: 10), CGPoint(x: 20, y: 80))])
        defer { w.doc.close() }
        let (m, b) = (w.m, w.b)
        let t = ScriptedTracer(delays: [0.15])
        m.tracer = { json, a, c in try t.trace(json) { try b.adaptiveWideAngleCurve(recipeJson: json, from: a, to: c) } }
        for v in stride(from: 20.0, through: 40.0, by: 1) { m.setFocal(v) }
        var want = m.draft
        want.setCurve(try b.adaptiveWideAngleCurve(recipeJson: want.recipeJson, from: want.lines[0].from, to: want.lines[0].to), at: 0)
        XCTAssertNotEqual(m.draft.lines[0].points, want.lines[0].points, "the re-trace is still pending")
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.ok()
        await waitFor("the apply") { ended != nil }
        if case .failure(let e) = ended { return XCTFail("apply failed: \(e)") }
        let stored = try b.beginAdaptiveWideAngle(layer: w.layer, stageIndex: 0)
        defer { b.cancelAdaptiveWideAngle(token: stored.token) }
        let committed = try AdaptiveWideAngleDraft(recipeJson: stored.recipeJson)
        XCTAssertEqual(committed.focal35, 40, accuracy: 1e-9)
        XCTAssertEqual(committed.lines.map(\.points), want.lines.map(\.points), "committed curves are for 40 mm")
    }

    /// Cancel while a slow re-trace is in flight: its late result is dropped (no curves applied, no preview
    /// submitted) and the queued one never runs.
    @MainActor func testCancelDropsALateRetrace() async throws {
        let w = try await fisheyeWorkspace(lines: [(CGPoint(x: 20, y: 10), CGPoint(x: 20, y: 80))])
        defer { w.doc.close() }
        let (m, b) = (w.m, w.b)
        let t = ScriptedTracer(delays: [0.3])
        m.tracer = { json, a, c in try t.trace(json) { try b.adaptiveWideAngleCurve(recipeJson: json, from: a, to: c) } }
        var previews = 0
        m.onPreview = { previews += 1 }
        m.setFocal(30)
        m.setFocal(35)
        await waitFor("the trace to start") { t.started >= 1 }
        let points = m.draft.lines[0].points
        m.cancel()
        let (revision, landed) = (m.revision, previews)
        await waitFor("the late trace") { t.finished >= 1 }
        try? await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(m.draft.lines[0].points, points, "a trace landed after Cancel")
        XCTAssertEqual(m.revision, revision, "a preview was scheduled after Cancel")
        XCTAssertEqual(previews, landed, "a preview landed after Cancel")
        XCTAssertEqual(t.started, 1, "the queued re-trace ran after Cancel")
    }

    /// OK closes the workspace while a slow re-trace from the slider is still in flight: that late result is dropped.
    @MainActor func testCloseAfterOKDropsALateRetrace() async throws {
        let w = try await fisheyeWorkspace(lines: [(CGPoint(x: 20, y: 10), CGPoint(x: 20, y: 80))])
        defer { w.doc.close() }
        let (m, b) = (w.m, w.b)
        let t = ScriptedTracer(delays: [0.5, 0])
        m.tracer = { json, a, c in try t.trace(json) { try b.adaptiveWideAngleCurve(recipeJson: json, from: a, to: c) } }
        var previews = 0
        m.onPreview = { previews += 1 }
        m.setFocal(30)
        await waitFor("the trace to start") { t.started >= 1 }
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.ok()
        await waitFor("the apply") { ended != nil }
        if case .failure(let e) = ended { return XCTFail("apply failed: \(e)") }
        XCTAssertLessThan(t.finished, 2, "OK waited for the slow slider trace")
        let (points, revision, landed) = (m.draft.lines[0].points, m.revision, previews)
        await waitFor("the late trace") { t.finished >= 2 }
        try? await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(m.draft.lines[0].points, points, "a trace landed after the workspace closed")
        XCTAssertEqual(m.revision, revision, "a preview was scheduled after the workspace closed")
        XCTAssertEqual(previews, landed)
        XCTAssertEqual(t.started, 2)
    }

    /// Lines removed and drawn while a re-trace is in flight: its curves are matched by their ends, so the surviving
    /// line gets its own new curve and the new line keeps the one traced when it was drawn.
    @MainActor func testLinesEditedDuringARetraceMatchByEnds() async throws {
        let (a0, a1) = (CGPoint(x: 20, y: 10), CGPoint(x: 20, y: 80))
        let (b0, b1) = (CGPoint(x: 100, y: 10), CGPoint(x: 100, y: 80))
        let (c0, c1) = (CGPoint(x: 45, y: 5), CGPoint(x: 45, y: 85))
        let w = try await fisheyeWorkspace(lines: [(a0, a1), (b0, b1)])
        defer { w.doc.close() }
        let (m, b) = (w.m, w.b)
        let t = ScriptedTracer(delays: [0.3, 0])
        m.tracer = { json, a, c in try t.trace(json) { try b.adaptiveWideAngleCurve(recipeJson: json, from: a, to: c) } }
        m.setFocal(30)
        await waitFor("the trace to start") { t.started >= 1 }
        m.selected = 0
        m.removeSelected()
        XCTAssertTrue(m.addLine(from: c0, to: c1, orientation: .vertical))
        XCTAssertEqual(m.draft.lines.map { [$0.from, $0.to] }, [[b0, b1], [c0, c1]])
        let json = m.draft.recipeJson
        let wantB = try b.adaptiveWideAngleCurve(recipeJson: json, from: b0, to: b1)
        let wantC = try b.adaptiveWideAngleCurve(recipeJson: json, from: c0, to: c1)
        XCTAssertEqual(m.draft.lines[1].points, wantC, "the new line is traced under the new camera")
        XCTAssertNotEqual(m.draft.lines[0].points, wantB, "the surviving line waits for the re-trace")
        await waitFor("the re-trace") { t.finished >= 2 && m.draft.lines[0].points == wantB }
        try? await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(m.draft.lines.count, 2)
        XCTAssertEqual(m.draft.lines[0].points, wantB)
        XCTAssertEqual(m.draft.lines[1].points, wantC, "the new line's curve was replaced by another line's")
        XCTAssertNil(m.error)
        m.cancel()
    }

    /// B5-20c: OK on a smart object over the compositor's smart-filter pass limit (6000 × 6000) keeps the sheet open
    /// with the engine's plain message and records no history.
    @MainActor func testSheetShowsTheSmartObjectSizeLimit() async throws {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        let backend = EngineDocumentBackend(session: try engine.newDocument(width: 6000, height: 6000, depth: .u8, profile: nil))
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let b = try XCTUnwrap(backend as (any DocumentBackend) as? any DocumentAdaptiveWideAngleBackend)
        let filters = try XCTUnwrap(backend as (any DocumentBackend) as? any DocumentFiltersBackend)
        let layer = try XCTUnwrap(doc.layers.first).id
        _ = try filters.convertForSmartFilters(layer: layer)
        let info = try b.beginAdaptiveWideAngle(layer: layer, stageIndex: nil)
        XCTAssertTrue(info.smartObject)
        let m = AdaptiveWideAngleWorkspaceModel(doc: doc, backend: b, info: info,
                                                draft: try AdaptiveWideAngleDraft(recipeJson: info.recipeJson), layerName: "Layer 1")
        let rows = try backend.historyItems().count
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.ok()
        await waitFor("the refusal") { ended != nil }
        guard case .failure = ended else { return XCTFail("a 36 MP smart object applied") }
        XCTAssertEqual(m.error, "Adaptive Wide Angle on a Smart Object is limited to about 33 MP. "
                       + "Rasterize the layer, or apply to a pixel layer.")
        XCTAssertEqual(try backend.historyItems().count, rows, "no history on refusal")
        m.cancel()
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

/// What the injected tracer saw (called off the main actor).
private final class TraceLog: @unchecked Sendable {
    private let lock = NSLock()
    private var jsons: [String] = []
    private var main = false

    func record(json: String, onMain: Bool) { lock.withLock { jsons.append(json); main = main || onMain } }
    var count: Int { lock.withLock { jsons.count } }
    var last: String? { lock.withLock { jsons.last } }
    var onMain: Bool { lock.withLock { main } }
}

/// A tracer with per-call delays (the last repeats) that can fail every call; counts starts and ends.
private final class ScriptedTracer: @unchecked Sendable {
    private let lock = NSLock()
    private let delays: [Double]
    private let failure: String?
    private var starts = 0
    private var ends = 0

    init(delays: [Double], failure: String? = nil) { self.delays = delays; self.failure = failure }

    var started: Int { lock.withLock { starts } }
    var finished: Int { lock.withLock { ends } }

    func trace(_ json: String, _ real: () throws -> [CGPoint]) throws -> [CGPoint] {
        let n = lock.withLock { starts += 1; return starts - 1 }
        defer { lock.withLock { ends += 1 } }
        Thread.sleep(forTimeInterval: delays[min(n, delays.count - 1)])
        if let failure { throw DocumentError.invalid(failure) }
        return try real()
    }
}
