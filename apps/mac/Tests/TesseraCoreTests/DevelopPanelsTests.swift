import Foundation
import IOSurface
import XCTest
import TesseraFFI
@testable import TesseraCore

/// The develop panels' JSON patches and editing models (M2-13). Pure tests; the session
/// integration test at the end uses the ARW fixture like DevelopTests.
final class DevelopPanelsTests: XCTestCase {
    /// The same encoding the controller sends to the engine.
    private func json(_ obj: Any) -> String {
        if let d = obj as? [String: Any] { return DevelopController.encode(d)! }
        return String(decoding: try! JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys, .fragmentsAllowed]), as: UTF8.self)
    }

    // MARK: Patches per panel

    func testEachPanelControlWritesItsPatch() {
        XCTAssertEqual(json(ParametricRegion.lights.control.patch(35)),
                       #"{"tone":{"curves":{"parametric":{"lights":35}}}}"#)
        XCTAssertEqual(json(ParametricRegion.shadows.control.patch(-250)),
                       #"{"tone":{"curves":{"parametric":{"shadows":-100}}}}"#, "clamped to the range")
        XCTAssertEqual(json(DevelopController.patch(CurveChannel.red.path, PointCurve(json: [["x": 0.5, "y": 0.6]]).json)),
                       #"{"tone":{"curves":{"red":[{"x":0,"y":0},{"x":0.5,"y":0.6},{"x":1,"y":1}]}}}"#)
        XCTAssertEqual(json(DevelopController.patch(CurveChannel.rgb.path, PointCurve.identity.json)),
                       #"{"tone":{"curves":{"rgb":[]}}}"#, "identity is the empty default")
        XCTAssertEqual(json(HSLProperty.saturation.control(.blue).patch(-40)),
                       #"{"color":{"hsl":{"saturation":{"blue":-40}}}}"#)
        XCTAssertEqual(json(HSLProperty.hue.control(.orange).patch(12)),
                       #"{"color":{"hsl":{"hue":{"orange":12}}}}"#)
        XCTAssertEqual(json(GradeRange.shadows.wheelPatch(hue: 220, saturation: 30)),
                       #"{"color":{"grading":{"shadows":{"hue":220,"saturation":30}}}}"#)
        XCTAssertEqual(json(GradeRange.highlights.luminance.patch(-10)),
                       #"{"color":{"grading":{"highlights":{"luminance":-10}}}}"#)
        XCTAssertEqual(json(GradeRange.blending.patch(70)), #"{"color":{"grading":{"blending":70}}}"#)
        XCTAssertEqual(json(DetailControls.amount.patch(120)), #"{"detail":{"sharpening":{"amount":120}}}"#)
        XCTAssertEqual(json(DetailControls.radius.patch(5)), #"{"detail":{"sharpening":{"radius":3}}}"#)
        XCTAssertEqual(json(DetailControls.colorSmoothness.patch(60)),
                       #"{"detail":{"noise_reduction":{"color_smoothness":60}}}"#)
        XCTAssertEqual(json(EffectsControls.amount.patch(-30)), #"{"effects":{"vignette":{"amount":-30}}}"#)
        XCTAssertEqual(json(EffectsControls.grainSize.patch(40)), #"{"effects":{"grain":{"size":40}}}"#)
        XCTAssertEqual(json(DevelopController.patch(VignetteStyle.path, VignetteStyle.paintOverlay.rawValue)),
                       #"{"effects":{"vignette":{"style":"paint_overlay"}}}"#)
        // Crop: displayed-orientation geometry → sensor rect + angle.
        var g = CropGeometry(width: 600, height: 400)
        g.cropWidth = 300; g.cropHeight = 200; g.angle = 2
        XCTAssertEqual(json(g.patch(orientation: 1, aspectHint: [3, 2])),
                       #"{"geometry":{"crop":{"angle":2,"aspect":[3,2],"rect":{"bottom":0.75,"left":0.25,"right":0.75,"top":0.25}}}}"#)
        XCTAssertEqual(json(CropGeometry(width: 600, height: 400).patch(orientation: 1, aspectHint: nil)),
                       #"{"geometry":{"crop":{"angle":0,"aspect":null,"rect":{"bottom":1,"left":0,"right":1,"top":0}}}}"#)
    }

    func testMergePatchAccumulatesLikeRFC7386() {
        var pending: [String: Any] = [:]
        pending = DevelopController.merge(pending, HSLProperty.hue.control(.red).patch(5), keepNulls: true)
        pending = DevelopController.merge(pending, HSLProperty.hue.control(.aqua).patch(-5), keepNulls: true)
        pending = DevelopController.merge(pending, ["geometry": ["crop": ["aspect": NSNull()]]], keepNulls: true)
        XCTAssertEqual(json(pending), #"{"color":{"hsl":{"hue":{"aqua":-5,"red":5}}},"geometry":{"crop":{"aspect":null}}}"#)
        let applied = DevelopController.merge(["geometry": ["crop": ["aspect": [3, 2], "angle": 1]]], pending, keepNulls: false)
        XCTAssertEqual(json(applied["geometry"]!), #"{"crop":{"angle":1}}"#)
    }

    // MARK: Curve editor

    func testCurveEditorKeepsMonotoneValidPoints() {
        var rng = SystemRandomNumberGenerator()
        for _ in 0..<200 {
            var c = PointCurve.identity
            for _ in 0..<40 {
                switch Int.random(in: 0..<4, using: &rng) {
                case 0: c.insert(x: .random(in: -0.2...1.2), y: .random(in: -0.2...1.2))
                case 1: c.move(Int.random(in: 0..<c.knots.count), to: CurveKnot(.random(in: -1...2), .random(in: -1...2)))
                case 2: c.nudge(Int.random(in: 0..<c.knots.count), dx: .random(in: -0.1...0.1), dy: .random(in: -0.1...0.1))
                default: c.remove(Int.random(in: 0..<c.knots.count))
                }
                XCTAssertTrue(c.isValid, "\(c.knots)")
            }
            // What the engine receives is monotone, and so is the drawn spline.
            let knots = c.json
            for (a, b) in zip(knots, knots.dropFirst()) {
                XCTAssertLessThan(a["x"]!, b["x"]!)
                XCTAssertLessThanOrEqual(a["y"]!, b["y"]!)
            }
            var last = -Double.infinity
            for i in 0...500 {
                let y = c.evaluate(Double(i) / 500)
                XCTAssertGreaterThanOrEqual(y, last - 1e-12)
                last = y
            }
        }
    }

    func testCurveEvaluationMatchesKnotsAndIdentity() {
        XCTAssertEqual(PointCurve.identity.evaluate(0.3), 0.3)
        let c = PointCurve(json: [["x": 0.25, "y": 0.2], ["x": 0.75, "y": 0.8]])
        for k in c.knots { XCTAssertEqual(c.evaluate(k.x), k.y, accuracy: 1e-12) }
        XCTAssertLessThan(c.evaluate(0.2), 0.2)
        XCTAssertGreaterThan(c.evaluate(0.8), 0.8)
        // Out-of-order or descending input is repaired into an accepted curve.
        let repaired = PointCurve(json: [["x": 0.7, "y": 0.2], ["x": 0.3, "y": 0.6]])
        XCTAssertTrue(repaired.isValid)
        // Parametric: identity at zero, lifts its own region only, fixed ends.
        var p = ParametricCurveModel()
        XCTAssertEqual(p.evaluate(0.4), 0.4)
        p.amounts[2] = 60   // lights
        XCTAssertGreaterThan(p.evaluate(0.62), 0.62)
        XCTAssertEqual(p.evaluate(0.2), 0.2)
        XCTAssertEqual(p.evaluate(0.5), 0.5, accuracy: 1e-12)
        p.setSplit(1, 90)
        XCTAssertEqual(p.splits[1], 74, "split points stay ordered")
    }

    // MARK: HSL targeted adjustment

    func testHueBandWeightsAndTargetedAdjustment() {
        for band in HueBand.allCases {
            let w = HueBandWeights.weights(hue: band.center)
            XCTAssertEqual(w[band] ?? 0, 1, accuracy: 1e-9, "\(band)")
        }
        for h in stride(from: 0.0, to: 360, by: 7.3) {
            XCTAssertEqual(HueBandWeights.weights(hue: h).values.reduce(0, +), 1, accuracy: 1e-9)
        }
        // A skin/orange sample moves orange (and some red/yellow); up is positive.
        let t = TargetedHSLAdjustment(property: .saturation, sample: (220, 140, 90)) { _ in 0 }
        XCTAssertEqual(t.dominant, .orange)
        let patch = t.patch(delta: 20)
        let values = ((patch["color"] as! [String: Any])["hsl"] as! [String: Any])["saturation"] as! [String: Double]
        XCTAssertEqual(values["orange"], 20)
        XCTAssertTrue(values.keys.allSatisfy { ["red", "orange", "yellow"].contains($0) })
        XCTAssertTrue(TargetedHSLAdjustment(property: .hue, sample: (128, 128, 128)) { _ in 0 }.isEmpty, "neutral")
        // A blue sky sample.
        XCTAssertEqual(TargetedHSLAdjustment(property: .luminance, sample: (70, 130, 220)) { _ in 0 }.dominant, .blue)
    }

    // MARK: Crop

    func testCropOrientationRoundTripAndConstraints() {
        for o in 1...8 {
            let (w, h) = o >= 5 ? (400.0, 600.0) : (600.0, 400.0)
            var g = CropGeometry(width: w, height: h)
            g.centerX = w * 0.4; g.centerY = h * 0.55; g.cropWidth = w * 0.5; g.cropHeight = h * 0.3; g.angle = 3.25
            let e = g.engineValues(orientation: o)
            let back = CropGeometry(engineRect: (e.left, e.top, e.right, e.bottom), angle: e.angle,
                                    width: w, height: h, orientation: o)
            XCTAssertEqual(back.centerX, g.centerX, accuracy: 0.01, "o=\(o)")
            XCTAssertEqual(back.centerY, g.centerY, accuracy: 0.01, "o=\(o)")
            XCTAssertEqual(back.cropWidth, g.cropWidth, accuracy: 0.01, "o=\(o)")
            XCTAssertEqual(back.cropHeight, g.cropHeight, accuracy: 0.01, "o=\(o)")
            XCTAssertEqual(back.angle, g.angle, accuracy: 1e-9, "o=\(o)")
            XCTAssertEqual(e.angle, [2, 4, 5, 7].contains(o) ? -3.25 : 3.25, "mirrors flip the rotation")
        }
        var g = CropGeometry(width: 600, height: 400)
        g.rotate(to: 10, constrain: true)
        XCTAssertTrue(g.fitsImage)
        XCTAssertLessThan(g.cropWidth, 600)
        XCTAssertEqual(g.aspect, 1.5, accuracy: 1e-6, "rotation keeps the crop's aspect")
        g.rotate(to: 80, constrain: true)
        XCTAssertEqual(g.angle, 45)
        g.move(dx: 1000, dy: 0, constrain: true)
        XCTAssertTrue(g.fitsImage)
        var r = CropGeometry(width: 600, height: 400)
        r.resize(sx: 1, sy: 0, dx: -100, dy: 0, aspect: nil, constrain: true)
        XCTAssertEqual(r.cropWidth, 500)
        XCTAssertEqual(r.centerX, 250, "the left edge stays put")
        r.fitLargest(aspect: 1)
        XCTAssertEqual(r.cropWidth, r.cropHeight, accuracy: 1e-6)
        XCTAssertTrue(r.fitsImage)
        // Straighten: a line 2° clockwise of horizontal is levelled by −2°.
        let s = CropGeometry(width: 600, height: 400)
        let a = s.straightenAngle(from: (0, 0), to: (100, 100 * tan(2 * .pi / 180)))!
        XCTAssertEqual(a, -2, accuracy: 1e-9)
        XCTAssertEqual(s.straightenAngle(from: (0, 0), to: (3, 100))!, atan2(3, 100) * 180 / .pi, accuracy: 1e-9,
                       "near-vertical lines level to vertical")
        XCTAssertEqual(CropAspect(hint: [6000, 4000], imageWidth: 6000, imageHeight: 4000), .original)
        XCTAssertEqual(CropAspect.ratio(4, 5).hint(imageWidth: 6000, imageHeight: 4000, portrait: true), [4, 5])
    }

    // MARK: Presets

    func testPresetsArePartialRecipesSavedAsJSON() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("presets-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        let store = PresetStore(folder: folder)
        let full: [String: Any] = [
            "tone": ["exposure": 0.5, "contrast": 10, "texture": 20,
                     "curves": ["rgb": [["x": 0, "y": 0.1], ["x": 1, "y": 1]]]],
            "color": ["vibrance": 15, "hsl": ["hue": ["red": 5]]],
            "effects": ["vignette": ["amount": -20], "grain": ["amount": 10], "lens_blur": NSNull()],
            "geometry": ["crop": ["angle": 3]],
        ]
        let p = DevelopPreset(name: "Warm/Film", groups: [.basicTone, .toneCurve, .effects], from: full)
        XCTAssertEqual(p.settingsJSON,
                       #"{"effects":{"grain":{"amount":10},"vignette":{"amount":-20}},"tone":{"contrast":10,"curves":{"rgb":[{"x":0,"y":0.1},{"x":1,"y":1}]},"exposure":0.5}}"#,
                       "only the chosen groups; crop and presence are left out")
        let url = try store.save(p)
        XCTAssertEqual(url.lastPathComponent, "Warm-Film.json")
        let listed = store.list()
        XCTAssertEqual(listed.count, 1)
        XCTAssertEqual(listed[0].name, "Warm/Film")
        XCTAssertEqual(listed[0].groups, [.basicTone, .toneCurve, .effects])
        XCTAssertEqual(listed[0].settingsJSON, p.settingsJSON)
        try store.delete("Warm/Film")
        XCTAssertTrue(store.list().isEmpty)
        XCTAssertFalse(PresetGroup.defaultSelection.contains(.crop))
    }
}

/// The panels on a real session: each panel's patch reaches the engine unchanged, renders, and
/// history, crop and step toggles behave (ARW fixture, scratch copy).
@MainActor
final class DevelopPanelsSessionTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    private func nextFrame(_ c: DevelopController, after generation: UInt64) async throws -> DevelopFrame {
        let deadline = Date().addingTimeInterval(60)
        while Date() < deadline {
            if let f = c.lastFrame, f.isFinal, f.generation > generation { return f }
            try await Task.sleep(for: .milliseconds(10))
        }
        struct Timeout: Error {}
        XCTFail("no frame within 60 s")
        throw Timeout()
    }

    func testPanelsDriveTheSession() async throws {
        let fixtures = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        let raw = try XCTUnwrap(try FileManager.default.contentsOfDirectory(at: fixtures, includingPropertiesForKeys: nil)
            .first { $0.pathExtension.lowercased() == "arw" }, "fetch fixtures/raw first")
        let temp = root.appendingPathComponent("build/panels-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("raw")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        try FileManager.default.copyItem(at: raw, to: folder.appendingPathComponent(raw.lastPathComponent))
        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let item = try XCTUnwrap(library.items.first)
        let c = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        var sent: [String] = []
        c.onPatchSent = { sent.append($0) }
        let plan = try c.attachSurfaces(viewWidth: 640, viewHeight: 480)
        var frame = try await nextFrame(c, after: 0)

        let steps: [(String, [String: Any], String)] = [
            ("Curve", ParametricRegion.darks.control.patch(-25), #"{"tone":{"curves":{"parametric":{"darks":-25}}}}"#),
            ("HSL", HSLProperty.luminance.control(.green).patch(30), #"{"color":{"hsl":{"luminance":{"green":30}}}}"#),
            ("Grade", GradeRange.midtones.wheelPatch(hue: 40, saturation: 20),
             #"{"color":{"grading":{"midtones":{"hue":40,"saturation":20}}}}"#),
            ("Detail", DetailControls.luminance.patch(30), #"{"detail":{"noise_reduction":{"luminance":30}}}"#),
            ("Effects", EffectsControls.amount.patch(-35), #"{"effects":{"vignette":{"amount":-35}}}"#),
        ]
        for (label, patch, expected) in steps {
            c.apply(patch: patch, interactive: false)
            XCTAssertEqual(sent.last, expected, label)
            frame = try await nextFrame(c, after: frame.generation)
            XCTAssertTrue(c.commit(label: label))
        }
        XCTAssertTrue(c.ignoredSettings.isEmpty)
        XCTAssertEqual(c.number(at: ["color", "hsl", "luminance", "green"]), 30)

        // Crop through the geometry model: the frame reports the cropped picture.
        let size = c.info.orientation >= 5 ? (Double(c.info.height), Double(c.info.width))
            : (Double(c.info.width), Double(c.info.height))
        var g = CropGeometry(width: size.0, height: size.1)
        g.cropWidth = size.0 / 2; g.cropHeight = size.1 / 2
        c.apply(patch: g.patch(orientation: Int(c.info.orientation), aspectHint: nil), interactive: false)
        frame = try await nextFrame(c, after: frame.generation)
        XCTAssertEqual(Double(frame.displayWidth), Double(plan.width) / 2, accuracy: 1)
        XCTAssertTrue(c.commit(label: "Crop"))
        c.setCropEditing(true)
        frame = try await nextFrame(c, after: frame.generation)
        XCTAssertEqual(frame.displayWidth, Int(plan.width), "the crop tool shows the whole frame")
        c.setCropEditing(false)
        frame = try await nextFrame(c, after: frame.generation)

        // History list and a step toggle.
        let items = c.historyItems()
        XCTAssertEqual(items.map(\.label), ["Curve", "HSL", "Grade", "Detail", "Effects", "Crop"])
        XCTAssertTrue(try c.setHistoryStep(items[1].id, enabled: false))
        XCTAssertNil(c.number(at: ["color", "hsl", "luminance", "green"]).flatMap { $0 == 30 ? $0 : nil })
        XCTAssertFalse(c.historyItems()[1].enabled)
        XCTAssertTrue(try c.checkoutHistory(items[0].id))
        XCTAssertEqual(c.number(at: ["tone", "curves", "parametric", "darks"]), -25)
        XCTAssertEqual(c.number(at: ["effects", "vignette", "amount"]), 0)

        // 1:1 detail preview.
        let surface = try XCTUnwrap(DevelopController.makeDetailSurface(width: 96, height: 64))
        let session = c.session
        nonisolated(unsafe) let target = surface
        let d = try await Task.detached { try DevelopController.renderDetail(session: session, into: target, centerX: 0.5, centerY: 0.5) }.value
        XCTAssertEqual(d.width, 96)
        await c.close()
    }
}
