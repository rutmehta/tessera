import Foundation
import IOSurface
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Masking (M2-14): stroke coalescing, the mask list state, mask-space mapping and the session path.
@MainActor
final class MaskingTests: XCTestCase {
    // MARK: Stroke coalescing

    func testStrokeCoalescerDropsDenseSamplesAndKeepsTheLast() {
        var c = StrokeCoalescer(spacing: 0.01)
        XCTAssertTrue(c.add(x: 0.1, y: 0.1))
        XCTAssertFalse(c.add(x: 0.105, y: 0.1), "closer than the spacing")
        XCTAssertTrue(c.add(x: 0.12, y: 0.1))
        XCTAssertFalse(c.add(x: 0.125, y: 0.1, pressure: 3))
        // One batch per display frame.
        let frame1 = c.take()
        XCTAssertEqual(frame1.map(\.x), [0.1, 0.12])
        XCTAssertTrue(c.isEmpty)
        XCTAssertTrue(c.take().isEmpty, "nothing new: no engine call")
        // The stroke ends under the cursor even when the last sample was too close; pressure clamps.
        let end = c.finish()
        XCTAssertEqual(end.count, 1)
        XCTAssertEqual(end[0].x, 0.125, accuracy: 1e-6)
        XCTAssertEqual(end[0].pressure, 1)
        XCTAssertEqual(c.sentCount, 3)
        // A new stroke starts afresh (no spacing against the previous stroke's end).
        XCTAssertTrue(c.add(x: 0.125, y: 0.1))
    }

    func testStrokeCoalescerBatchesManyEventsPerFrame() {
        var c = StrokeCoalescer(spacing: 0.002)
        var batches: [[BrushPoint]] = []
        // 1000 Hz tablet events over 50 ms at 60 Hz frames: ~3 batches, not 50 calls.
        for i in 0..<50 {
            c.add(x: 0.2 + Double(i) * 0.001, y: 0.5)
            if i % 16 == 15 { batches.append(c.take()) }
        }
        batches.append(c.finish())
        XCTAssertEqual(batches.count, 4)
        let xs = batches.flatMap { $0 }.map { Double($0.x) }
        XCTAssertEqual(xs, xs.sorted(), "order is kept")
        XCTAssertLessThan(xs.count, 30, "dense samples are thinned")
        XCTAssertEqual(xs.last!, 0.249, accuracy: 1e-6)
    }

    // MARK: List state

    private func group(_ id: UInt32, _ kinds: [MaskComponentType] = [.linear], ai: AiMaskState = .notAi) -> MaskGroupInfo {
        MaskGroupInfo(id: id, name: "Mask \(id)", enabled: true, amount: 100, invert: false,
                      components: kinds.map {
                          MaskComponentInfo(kind: $0, combine: .add, invert: false, title: "\($0)", definitionJson: "{}",
                                            ai: $0.isAI ? ai : .notAi, aiKey: $0.isAI ? "k\(id)" : nil, rendered: true)
                      },
                      params: LocalParam.all.map { LocalParamValue(name: $0.name, value: 0) })
    }

    func testListSelectsNewGroupsAndNeighboursOfDeletedOnes() {
        var s = MaskListState()
        XCTAssertNil(s.selected)
        s.update([group(1)])
        XCTAssertEqual(s.selectedID, 1, "the first group becomes selected")
        s.update([group(1), group(2)])
        XCTAssertEqual(s.selectedID, 2, "an added group becomes selected")
        s.select(1)
        s.update([group(1), group(2)])
        XCTAssertEqual(s.selectedID, 1, "unchanged lists keep the selection")
        s.update([group(2), group(3)])
        XCTAssertEqual(s.selectedID, 3, "a newly added group wins over the neighbour")
        s.update([group(2)])
        XCTAssertEqual(s.selectedID, 2, "deleting the selection selects its neighbour")
        s.update([])
        XCTAssertNil(s.selectedID)
        s.select(99)
        XCTAssertNil(s.selectedID, "unknown ids are not selected")
    }

    func testListCyclesAndEditsTheSelectedGroupOptimistically() {
        var s = MaskListState()
        s.update([group(1), group(2, [.brush, .linear]), group(3)])
        s.select(1)
        s.selectNext(-1)
        XCTAssertEqual(s.selectedID, 3, "wraps")
        s.selectNext(2)
        XCTAssertEqual(s.selectedID, 2)
        XCTAssertEqual(s.selectedBrushIndex, 0)
        s.setParam("exposure", 0.75)
        XCTAssertEqual(s.param("exposure"), 0.75)
        s.setAmount(40)
        XCTAssertEqual(s.selected?.amount, 40)
        s.select(1)
        XCTAssertEqual(s.param("exposure"), 0, "other groups keep their values")
        XCTAssertNil(s.selectedBrushIndex)
    }

    func testAIProgressFollowsJobsAndTheEngineList() {
        var s = MaskListState()
        s.update([group(1, [.subject], ai: .pending(fraction: 0, message: "Queued"))])
        XCTAssertTrue(s.busy)
        s.progressUpdate(MaskJobUpdate(key: "k1", title: "Subject", fraction: 0.4, message: "Segmenting", done: false, error: nil))
        XCTAssertEqual(s.progress["k1"]?.message, "Segmenting")
        s.progressUpdate(MaskJobUpdate(key: "k1", title: "Subject", fraction: 1, message: "Ready", done: true, error: nil))
        XCTAssertFalse(s.busy)
        s.update([group(1, [.subject], ai: .ready)])
        XCTAssertFalse(s.busy)
    }

    // MARK: Mask space

    func testMaskSpaceRoundTripsThroughCropAndOrientation() {
        for o in 1...8 {
            let size = o >= 5 ? (400.0, 600.0) : (600.0, 400.0)
            var crop = CropGeometry(width: size.0, height: size.1)
            crop.cropWidth *= 0.5
            crop.cropHeight *= 0.6
            crop.centerX += 40
            crop.rotate(to: 7.5, constrain: false)
            for space in [MaskSpace(orientation: o, crop: nil), MaskSpace(orientation: o, crop: crop)] {
                for (u, v) in [(0.1, 0.2), (0.5, 0.5), (0.9, 0.7)] {
                    let m = space.toMask(u, v)
                    let back = space.fromMask(m.x, m.y)
                    XCTAssertEqual(back.u, u, accuracy: 1e-9, "o=\(o)")
                    XCTAssertEqual(back.v, v, accuracy: 1e-9, "o=\(o)")
                }
            }
        }
        // Uncropped, orientation 6 (displayed rotated 90° clockwise): displayed top-left is stored bottom-left.
        let m = MaskSpace(orientation: 6, crop: nil).toMask(0, 0)
        XCTAssertEqual(m.x, 0, accuracy: 1e-12)
        XCTAssertEqual(m.y, 1, accuracy: 1e-12)
        // An unrotated centred half crop maps its corners to the quarter points.
        var half = CropGeometry(width: 600, height: 400)
        half.cropWidth = 300
        half.cropHeight = 200
        let q = MaskSpace(orientation: 1, crop: half).toMask(0, 0)
        XCTAssertEqual(q.x, 0.25, accuracy: 1e-12)
        XCTAssertEqual(q.y, 0.25, accuracy: 1e-12)
        // Brush radius: a tenth of the displayed (cropped) width in sensor-width units.
        XCTAssertEqual(MaskSpace(orientation: 1, crop: half).maskRadius(displayedFraction: 0.1, imageWidth: 600, imageHeight: 400),
                       0.05, accuracy: 1e-12)
        XCTAssertEqual(MaskSpace(orientation: 6, crop: nil).maskRadius(displayedFraction: 0.1, imageWidth: 600, imageHeight: 400),
                       0.1 * 400 / 600, accuracy: 1e-12)
    }

    func testGradientShapesRoundTripTheirEngineJSON() throws {
        let l = LinearGradientShape(start: (0.5, 0.1), end: (0.5, 0.6))
        XCTAssertEqual(LinearGradientShape(json: l.json), l)
        let r = RadialGradientShape(center: (0.4, 0.5), radii: (0.2, 0.3), angle: 30, feather: 20)
        XCTAssertEqual(RadialGradientShape(json: r.json), r)
        XCTAssertNil(RadialGradientShape(json: l.json))
        // The outline passes through the rotated x-radius handle.
        let p = r.outline(segments: 4)[0]
        XCTAssertEqual(p.x, 0.4 + cos(.pi / 6) * 0.2, accuracy: 1e-12)
        XCTAssertEqual(p.y, 0.5 + sin(.pi / 6) * 0.2, accuracy: 1e-12)
    }

    // MARK: Session

    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    func testMaskEditsAreCoalescedPerFrameAndCommittedAsOneStep() async throws {
        let fixtures = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        let raw = try XCTUnwrap(try FileManager.default.contentsOfDirectory(at: fixtures, includingPropertiesForKeys: nil)
            .first { $0.pathExtension.lowercased() == "arw" }, "fetch fixtures/raw first")
        let temp = root.appendingPathComponent("build/mask-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("raw")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        try FileManager.default.copyItem(at: raw, to: folder.appendingPathComponent(raw.lastPathComponent))
        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let item = try XCTUnwrap(library.items.first)
        let c = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        _ = try c.attachSurfaces(viewWidth: 480, viewHeight: 360)
        var ticks = 0
        c.onNeedsFlush = { ticks += 1 }
        var overlays: [MaskOverlayFrame] = []
        c.onMaskOverlay = { overlays.append($0) }

        // A brush stroke: samples wait for the display-frame flush.
        let id = try XCTUnwrap(c.beginBrushStroke(group: nil, radius: 0.04, feather: 50, flow: 100, erase: false))
        for i in 0...20 { c.addBrushSample(x: 0.2 + Double(i) * 0.02, y: 0.5, pressure: 1) }
        XCTAssertTrue(c.hasPendingMaskChanges)
        XCTAssertGreaterThan(ticks, 0, "the host is asked for a display-link tick")
        XCTAssertTrue(c.flushPending())
        XCTAssertFalse(c.hasPendingMaskChanges)
        c.addBrushSample(x: 0.65, y: 0.5, pressure: 1)
        c.endBrushStroke()
        // Slider drags coalesce to the last value per frame.
        for v in stride(from: 0.0, through: 1.0, by: 0.1) { c.setMaskParam(id, "exposure", v, interactive: true) }
        XCTAssertEqual(c.pendingMaskParams.count, 1)
        c.flushPending()
        c.setMaskParam(id, "exposure", 1, interactive: false)
        XCTAssertTrue(c.commit(label: "Brush Stroke"))

        let groups = c.maskGroups()
        XCTAssertEqual(groups.count, 1)
        XCTAssertEqual(groups[0].components.first?.kind, .brush)
        XCTAssertEqual(groups[0].params.first { $0.name == "exposure" }?.value, 1)
        let def = try XCTUnwrap(groups[0].components.first?.definitionJson)
        let strokes = try XCTUnwrap((try JSONSerialization.jsonObject(with: Data(def.utf8)) as? [String: Any])?["strokes"] as? [[String: Any]])
        XCTAssertEqual(strokes.count, 1, "one stroke from several batches")
        XCTAssertEqual((strokes[0]["points"] as? [Any])?.count, 22)

        // The overlay arrives as an R8 plane of the planned size.
        c.setMaskOverlay(id)
        let deadline = Date().addingTimeInterval(30)
        while overlays.isEmpty, Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        let o = try XCTUnwrap(overlays.last)
        let surface = try XCTUnwrap(c.maskOverlaySurface(o.surfaceId))
        XCTAssertEqual(IOSurfaceGetBytesPerElement(surface), 1)
        XCTAssertGreaterThan(o.coverage, 0)

        // One undo removes the whole stroke.
        XCTAssertTrue(try c.undo())
        XCTAssertTrue(c.maskGroups().isEmpty)
        await c.close()
    }
}
