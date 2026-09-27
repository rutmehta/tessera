import Foundation
import CoreGraphics
import ImageIO
import XCTest
import TesseraFFI
@testable import TesseraCore

/// M2-48 panels: Detail ▸ AI Denoise, Transform / Upright (with Guided guides) and Lens Blur.
/// Pure JSON-patch tests first; the session tests at the end use the ARW fixture like DevelopTests.
final class TransformLensBlurTests: XCTestCase {
    private func json(_ obj: [String: Any]) -> String { DevelopController.encode(obj)! }

    // MARK: Upright

    func testUprightButtonsWriteTheirModeAndClearGuides() {
        XCTAssertEqual(UprightMode.allCases.map(\.rawValue), ["off", "auto", "guided", "level", "vertical", "full"])
        for m in UprightMode.allCases where m != .guided {
            XCTAssertEqual(json(UprightControls.patch(mode: m)), #"{"geometry":{"upright":{"guides":[],"mode":"\#(m.rawValue)"}}}"#)
            XCTAssertEqual(UprightControls.historyLabel(m), "Upright: \(m.title)")
        }
        let g = UprightGuide(start: (0.2, 0.1), end: (0.25, 0.9))
        XCTAssertEqual(json(UprightControls.patch(mode: .guided, guides: [g, g])),
                       #"{"geometry":{"upright":{"guides":[{"end":[0.25,0.9],"start":[0.2,0.1]},{"end":[0.25,0.9],"start":[0.2,0.1]}],"mode":"guided"}}}"#)
        XCTAssertEqual(json(UprightControls.patch(mode: .auto, guides: [g])), #"{"geometry":{"upright":{"guides":[],"mode":"auto"}}}"#,
                       "the engine rejects guides outside Guided")
        XCTAssertEqual(json(UprightControls.resetPatch), #"{"geometry":{"upright":{"guides":[],"mode":"off"}}}"#)
        XCTAssertEqual(json(UprightControls.constrainCropPatch(true)), #"{"geometry":{"constrain_crop":true}}"#)
        XCTAssertEqual(UprightControls.mode(in: ["geometry": ["upright": ["mode": "vertical"]]]), .vertical)
        XCTAssertEqual(UprightControls.mode(in: [:]), .off)
        XCTAssertEqual(UprightControls.mode(in: ["geometry": ["upright": ["mode": "bogus"]]]), .off)
    }

    func testGuidedEditorKeepsTwoToFourGuidesInTheImage() {
        var e = UprightGuides()
        XCTAssertNil(e.add(UprightGuide(start: (0.5, 0.5), end: (0.502, 0.5))), "a click is not a guide")
        XCTAssertEqual(e.add(UprightGuide(start: (0.1, 0.1), end: (0.12, 0.9))), 0)
        XCTAssertFalse(e.isComplete)
        XCTAssertEqual(json(e.patch), #"{"geometry":{"upright":{"guides":[],"mode":"off"}}}"#, "one guide cannot be Guided")
        XCTAssertEqual(e.historyLabel, "Upright: Off")
        e.add(UprightGuide(start: (0.9, 0.1), end: (0.88, 0.9)))
        XCTAssertTrue(e.isComplete)
        XCTAssertEqual(e.historyLabel, "Upright: Guided (2 guides)")
        XCTAssertEqual(e.selected, 1)
        e.add(UprightGuide(start: (0.1, 0.5), end: (0.9, 0.52)))
        e.add(UprightGuide(start: (0.1, 0.8), end: (0.9, 0.78)))
        XCTAssertTrue(e.isFull)
        XCTAssertNil(e.add(UprightGuide(start: (0.3, 0.3), end: (0.6, 0.6))), "at most four")
        // Endpoint drags clamp to the image.
        e.move(0, end: true, to: (1.4, -0.2))
        XCTAssertEqual(e.guides[0].end.x, 1)
        XCTAssertEqual(e.guides[0].end.y, 0)
        XCTAssertEqual(e.guides[0].start.x, 0.1, "the other end stays")
        e.remove(3)
        XCTAssertEqual(e.guides.count, 3)
        XCTAssertNil(e.selected)
        // Round trip through the settings document.
        let settings = DevelopController.merge([:], e.patch, keepNulls: false)
        XCTAssertEqual(UprightGuides(settings: settings).guides, e.guides)
        XCTAssertEqual(UprightControls.mode(in: settings), .guided)
        let stored = (DevelopController.value(in: settings, at: UprightControls.guidesPath) as? [[String: Any]])?.count
        XCTAssertEqual(stored, 3)
        e.removeAll()
        XCTAssertEqual(json(e.patch), #"{"geometry":{"upright":{"guides":[],"mode":"off"}}}"#)
        XCTAssertNil(UprightGuide(json: ["start": [0.1], "end": [0.2, 0.3]]), "malformed guides are skipped")
    }

    // MARK: Manual transform

    func testTransformSlidersWriteTheirPatchesAndResetPerGroup() {
        let expected: [(DevelopControl, Double, String)] = [
            (TransformControls.vertical, -25, #"{"geometry":{"transform":{"vertical":-25}}}"#),
            (TransformControls.horizontal, 130, #"{"geometry":{"transform":{"horizontal":100}}}"#),
            (TransformControls.rotate, 12.5, #"{"geometry":{"transform":{"rotate":10}}}"#),
            (TransformControls.rotate, -2.5, #"{"geometry":{"transform":{"rotate":-2.5}}}"#),
            (TransformControls.aspect, 40, #"{"geometry":{"transform":{"aspect":40}}}"#),
            (TransformControls.scale, 20, #"{"geometry":{"transform":{"scale":50}}}"#),
            (TransformControls.offsetX, 7.5, #"{"geometry":{"transform":{"offset_x":7.5}}}"#),
            (TransformControls.offsetY, -150, #"{"geometry":{"transform":{"offset_y":-100}}}"#),
        ]
        for (c, v, j) in expected { XCTAssertEqual(json(c.patch(v)), j, c.title) }
        XCTAssertEqual(TransformControls.all.map(\.title), ["Vertical", "Horizontal", "Rotate", "Aspect", "Scale", "Offset X", "Offset Y"])
        XCTAssertEqual(TransformControls.scale.defaultValue, 100)
        XCTAssertEqual(TransformControls.vertical.historyLabel(20), "Transform Vertical +20")
        XCTAssertEqual(TransformControls.rotate.historyLabel(-1.5), "Transform Rotate -1.5°")
        XCTAssertEqual(TransformControls.scale.historyLabel(110), "Transform Scale 110%")
        XCTAssertEqual(json(TransformControls.resetPatch),
                       #"{"geometry":{"transform":{"aspect":0,"horizontal":0,"offset_x":0,"offset_y":0,"rotate":0,"scale":100,"vertical":0}}}"#)
    }

    // MARK: AI Denoise

    func testAIDenoiseTogglesTheNeuralModeAndAmount() {
        let on = AIDenoise.patch(enabled: true, amount: 70)
        XCTAssertEqual(json(on),
                       #"{"denoise":{"amount":70,"method":{"joint_demosaic":false,"kind":"neural","model":{"id":"enhance\/cfa-unet-fp32","version":"a138c59a65846c10967839e85817231ec6ea92b318a57814cb153e8ac8bb311b"}}}}"#)
        var settings = DevelopController.merge([:], on, keepNulls: false)
        XCTAssertTrue(AIDenoise.isEnabled(in: settings))
        settings = DevelopController.merge(settings, AIDenoise.patch(enabled: false), keepNulls: false)
        XCTAssertFalse(AIDenoise.isEnabled(in: settings))
        XCTAssertEqual(json(settings), #"{"denoise":{"amount":70,"method":{"kind":"off"}}}"#, "off leaves no model members")
        XCTAssertEqual(json(AIDenoise.amount.patch(140)), #"{"denoise":{"amount":100}}"#)
        XCTAssertEqual(AIDenoise.amount.defaultValue, 50)
        XCTAssertEqual(AIDenoise.historyLabel(true), "AI Denoise On")
        XCTAssertNotNil(DevelopEngineGaps.aiDenoise, "enable the toggle (and update ACCEPTANCE X) once the engine renders and exports it")
    }

    // MARK: Lens Blur

    func testLensBlurApplyBokehAndFocalRange() {
        XCTAssertEqual(json(LensBlurControls.applyPatch(true)),
                       #"{"effects":{"lens_blur":{"amount":50,"bokeh":"circle","focus_range":[0,0.1]}}}"#)
        XCTAssertEqual(json(LensBlurControls.applyPatch(false)), #"{"effects":{"lens_blur":null}}"#)
        var settings = DevelopController.merge([:], LensBlurControls.applyPatch(true), keepNulls: false)
        XCTAssertTrue(LensBlurControls.isApplied(in: settings))
        settings = DevelopController.merge(settings, LensBlurControls.bokehPatch(.hexagon), keepNulls: false)
        XCTAssertEqual(LensBlurControls.bokeh(in: settings), .hexagon)
        XCTAssertEqual(json(LensBlurControls.amount.patch(30)), #"{"effects":{"lens_blur":{"amount":30}}}"#)
        XCTAssertEqual(BokehShape.allCases.map(\.rawValue), ["circle", "hexagon", "octagon"], "the engine's supported ids")

        XCTAssertEqual(FocalRange(settings: settings), .standard)
        var r = FocalRange(near: 0.6, far: 0.2)
        XCTAssertEqual(r.near, 0.2); XCTAssertEqual(r.far, 0.6)
        r.setNear(0.9)
        XCTAssertEqual(r.near, 0.58, accuracy: 1e-12, "near stays a minimum width below far")
        r.setFar(-1)
        XCTAssertEqual(r.far, 0.6, accuracy: 1e-12)
        r = FocalRange(near: 0.3, far: 0.5)
        r.shift(by: 0.7)
        XCTAssertEqual(r.near, 0.8, accuracy: 1e-12)
        XCTAssertEqual(r.far, 1, accuracy: 1e-12, "the band keeps its width inside 0…1")
        XCTAssertEqual(json(FocalRange(near: 0.25, far: 0.5).patch), #"{"effects":{"lens_blur":{"focus_range":[0.25,0.5]}}}"#)
        XCTAssertEqual(FocalRange(near: 0.25, far: 0.5).historyLabel, "Focal Range 25–50")
        let settled = DevelopController.merge(settings, FocalRange(near: 0.25, far: 0.5).patch, keepNulls: false)
        XCTAssertEqual(FocalRange(settings: settled), FocalRange(near: 0.25, far: 0.5))
        XCTAssertEqual(FocalRange(near: 1, far: 1).near, 0.98, accuracy: 1e-12)
        XCTAssertNotNil(DevelopEngineGaps.lensBlur, "enable the Lens Blur panel once the engine renders it")
    }
}

/// The M2-48 controls on a real session (ARW fixture, scratch copy): drags are one history step,
/// Upright modes and guides reach the engine unchanged, and Upright Auto changes the rendered frame.
@MainActor
final class TransformSessionTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    private func scratchLibrary() throws -> (EngineLibrary, URL) {
        let fixtures = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        let raw = try XCTUnwrap(try FileManager.default.contentsOfDirectory(at: fixtures, includingPropertiesForKeys: nil)
            .first { $0.pathExtension.lowercased() == "arw" }, "fetch fixtures/raw first")
        let temp = root.appendingPathComponent("build/transform-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("raw")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        try FileManager.default.copyItem(at: raw, to: folder.appendingPathComponent(raw.lastPathComponent))
        return (try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support")), temp)
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

    private func engineSettings(_ c: DevelopController) throws -> [String: Any] {
        try XCTUnwrap(JSONSerialization.jsonObject(with: Data(try c.session.getSettingsJson().utf8)) as? [String: Any])
    }

    /// What `ControlSlider` does for one drag: coalesced interactive values, then the final value
    /// and one commit.
    private func drag(_ c: DevelopController, _ control: DevelopControl, to target: Double, steps: Int = 12) -> Bool {
        for i in 1..<steps {
            c.apply(patch: control.patch(control.defaultValue + (target - control.defaultValue) * Double(i) / Double(steps)),
                    interactive: true)
            c.flushPending()
        }
        c.apply(patch: control.patch(target), interactive: false)
        return c.commit(label: control.historyLabel(target))
    }

    func testTransformDragsAndUprightButtonsAreOneHistoryStepEach() async throws {
        let (library, _) = try scratchLibrary()
        let item = try XCTUnwrap(library.items.first)
        let c = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        var sent: [String] = []
        c.onPatchSent = { sent.append($0) }
        c.onNeedsFlush = {}
        _ = try c.attachSurfaces(viewWidth: 640, viewHeight: 480)
        var frame = try await nextFrame(c, after: 0)

        // Manual sliders: every drag is exactly one step, labelled with the final value.
        for (control, target) in [(TransformControls.vertical, 20.0), (TransformControls.rotate, -1.5), (TransformControls.scale, 110)] {
            let before = c.historyItems().count
            sent.removeAll()
            XCTAssertTrue(drag(c, control, to: target))
            XCTAssertEqual(c.historyItems().count, before + 1, control.title)
            XCTAssertEqual(c.historyItems().last?.label, control.historyLabel(target))
            XCTAssertGreaterThan(sent.count, 1, "interactive values were sent while dragging")
            let prefix = "{\"geometry\":{\"transform\":{\"" + control.path.last! + "\":"
            XCTAssertTrue(sent.allSatisfy { $0.hasPrefix(prefix) }, "\(sent)")
            XCTAssertEqual(DevelopController.value(in: try engineSettings(c), at: control.path) as? Double, target)
        }

        // Upright buttons: one step each; the engine stores the mode with no guides.
        for mode in [UprightMode.auto, .vertical] {
            let before = c.historyItems().count
            c.apply(patch: UprightControls.patch(mode: mode), interactive: false)
            XCTAssertTrue(c.commit(label: UprightControls.historyLabel(mode)))
            XCTAssertEqual(c.historyItems().count, before + 1)
            let s = try engineSettings(c)
            XCTAssertEqual(UprightControls.mode(in: s), mode)
            XCTAssertEqual((DevelopController.value(in: s, at: UprightControls.guidesPath) as? [Any])?.count ?? 0, 0)
        }

        // Guided: two guides from the loupe editor, one step; the engine keeps them verbatim.
        var guides = UprightGuides()
        guides.add(UprightGuide(start: (0.2, 0.1), end: (0.22, 0.9)))
        guides.add(UprightGuide(start: (0.8, 0.1), end: (0.78, 0.9)))
        let before = c.historyItems().count
        c.apply(patch: guides.patch, interactive: false)
        XCTAssertTrue(c.commit(label: guides.historyLabel))
        XCTAssertEqual(c.historyItems().count, before + 1)
        XCTAssertEqual(c.historyItems().last?.label, "Upright: Guided (2 guides)")
        let s = try engineSettings(c)
        XCTAssertEqual(UprightControls.mode(in: s), .guided)
        XCTAssertEqual(UprightGuides(settings: s).guides, guides.guides)

        // Constrain Crop and the group resets.
        c.apply(patch: UprightControls.constrainCropPatch(true), interactive: false)
        XCTAssertTrue(c.commit(label: "Constrain Crop On"))
        c.apply(patch: TransformControls.resetPatch, interactive: false)
        XCTAssertTrue(c.commit(label: "Reset Transform"))
        XCTAssertEqual(c.number(at: TransformControls.scale.path), 100)
        XCTAssertEqual(c.number(at: TransformControls.vertical.path), 0)
        XCTAssertEqual(UprightControls.mode(in: try engineSettings(c)), .guided, "the Transform reset leaves Upright alone")

        // Engine gap (REPORT.md): the develop viewport keeps Upright/Transform in the recipe but does
        // not draw them; the Transform panel shows its note from this list. Update both when it lands.
        XCTAssertTrue(c.ignores("/geometry/upright"), "\(c.ignoredSettings)")
        XCTAssertTrue(c.ignores("/geometry/constrain_crop"), "\(c.ignoredSettings)")
        // The session keeps rendering (the settings never fail a frame).
        c.apply(patch: ["tone": ["exposure": 0.25]], interactive: false)
        frame = try await nextFrame(c, after: frame.generation)
        XCTAssertTrue(frame.isFinal)
        await c.close()
    }

    /// Exports the photo as a 640 px PNG and decodes it to RGBA8.
    private func exportRender(_ ref: EngineImageReference, into dir: URL) async throws -> (width: Int, height: Int, pixels: [UInt8]) {
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let options = "{\"format\":\"png\",\"resize\":{\"mode\":\"long_edge\",\"long_edge\":640},\"metadata\":\"none\",\"destination\":\"" + dir.path + "\"}"
        let engine = ref.engine
        let target = ExportTarget.images(imageIds: [ref.imageID])
        let report: ExportReport = try await Task.detached {
            try engine.exportBatch(target: target, settingsJson: options, listener: nil, cancel: nil)
        }.value
        XCTAssertEqual(report.exported, 1, "\(report.items.map { $0.error ?? "" })")
        let path = try XCTUnwrap(report.items.first?.outputPath)
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(src, 0, nil))
        let width = image.width, height = image.height
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let space = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        pixels.withUnsafeMutableBytes { buf in
            let ctx = CGContext(data: buf.baseAddress, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            ctx?.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }
        return (width, height, pixels)
    }

    /// Upright Auto on a fixtures/raw photo changes the rendered frame. The develop viewport does not
    /// draw Upright yet (engine gap), so the frame is the engine's full render, via export.
    func testUprightAutoChangesTheRenderedFrame() async throws {
        let (library, temp) = try scratchLibrary()
        let item = try XCTUnwrap(library.items.first)
        let ref = try XCTUnwrap(item.engineImage)

        let off = try await exportRender(ref, into: temp.appendingPathComponent("off"))
        let c = try await DevelopController.open(ref, itemID: item.id)
        c.apply(patch: UprightControls.patch(mode: .auto), interactive: false)
        XCTAssertTrue(c.commit(label: UprightControls.historyLabel(.auto)))
        await c.close()
        let auto = try await exportRender(ref, into: temp.appendingPathComponent("auto"))

        var changed = 0
        if off.width == auto.width, off.height == auto.height {
            for i in stride(from: 0, to: off.pixels.count, by: 4)
            where (0..<3).contains(where: { abs(Int(off.pixels[i + $0]) - Int(auto.pixels[i + $0])) > 8 }) {
                changed += 1
            }
        } else {
            changed = max(off.width * off.height, auto.width * auto.height)   // Upright changed the output extent
        }
        let fraction = Double(changed) / Double(off.width * off.height)
        XCTAssertGreaterThan(fraction, 0.05, "Upright Auto should warp the picture (\(off.width)×\(off.height) → \(auto.width)×\(auto.height))")
        print("upright-auto: \(off.width)x\(off.height) -> \(auto.width)x\(auto.height), \(String(format: "%.1f", fraction * 100))% of pixels changed")
    }
}
