import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// UX-13: the dial's mask hit-test and ring, click-to-heal operations, the Try looks' patches and
/// their inverses, and the HUD placement. Pure logic; no engine.
final class PhotoDialTests: XCTestCase {
    /// A 16 × 12 raster with a filled 6 × 4 block at (5, 4).
    private func block() -> MaskRaster {
        var alpha = [UInt8](repeating: 0, count: 16 * 12)
        for y in 4..<8 { for x in 5..<11 { alpha[y * 16 + x] = 255 } }
        return MaskRaster(width: 16, height: 12, alpha: alpha)!
    }

    func testRasterHitTestAndCoverage() {
        let r = block()
        XCTAssertTrue(r.contains(x: 0.5, y: 0.5))          // pixel (8, 6)
        XCTAssertFalse(r.contains(x: 0.1, y: 0.1))
        XCTAssertFalse(r.contains(x: 1.2, y: 0.5))         // outside the frame
        XCTAssertEqual(r.coverage(), 24.0 / 192, accuracy: 1e-9)
        XCTAssertNil(MaskRaster(width: 2, height: 2, alpha: [0, 0, 0]))
    }

    func testRasterOutlineIsOneClosedRingAroundTheBlock() {
        let rings = block().outlines()
        XCTAssertEqual(rings.count, 1)
        let ring = rings[0]
        XCTAssertGreaterThan(ring.count, 16)
        // Closed: the walk returns to its first point.
        XCTAssertEqual(ring.first!.x, ring.last!.x, accuracy: 1e-9)
        XCTAssertEqual(ring.first!.y, ring.last!.y, accuracy: 1e-9)
        // Encloses the block's pixel centres (x 5.5…10.5 of 16, y 4.5…7.5 of 12) within half a pixel.
        let xs = ring.map(\.x), ys = ring.map(\.y)
        XCTAssertEqual(xs.min()!, 5.0 / 16, accuracy: 1e-9)
        XCTAssertEqual(xs.max()!, 11.0 / 16, accuracy: 1e-9)
        XCTAssertEqual(ys.min()!, 4.0 / 12, accuracy: 1e-9)
        XCTAssertEqual(ys.max()!, 8.0 / 12, accuracy: 1e-9)
        XCTAssertTrue(MaskRaster(width: 4, height: 4, alpha: [UInt8](repeating: 0, count: 16))!.outlines().isEmpty)
    }

    func testHealOperationShapeAndSourceStaysInside() {
        let op = RetouchSpots.healOperation(id: 3, center: (0.5, 0.5), radius: 0.02)
        XCTAssertEqual(op["id"] as? UInt32, 3)
        let kind = op["kind"] as? [String: Any]
        XCTAssertEqual(kind?["kind"] as? String, "heal")
        XCTAssertEqual((kind?["source_offset"] as? [Double])?.first, 0.02 * 2.6)
        let target = op["target"] as? [String: Any]
        XCTAssertEqual(target?["kind"] as? String, "area")
        let stroke = (((target?["components"] as? [[String: Any]])?.first?["strokes"]) as? [[String: Any]])?.first
        XCTAssertEqual((stroke?["points"] as? [[Double]])?.first, [0.5, 0.5, 1.0])
        XCTAssertEqual(stroke?["radius"] as? Double, 0.02)
        XCTAssertEqual(op["feather"] as? Double, 0)
        // Near the right edge the source moves to the left; with no room sideways it goes up or down.
        XCTAssertLessThan(RetouchSpots.sourceOffset(center: (0.98, 0.5), radius: 0.02).dx, 0)
        let tight = RetouchSpots.sourceOffset(center: (0.5, 0.1), radius: 0.2)
        XCTAssertEqual(tight.dx, 0)
        XCTAssertGreaterThan(tight.dy, 0)
        // JSON-encodable as the engine patch.
        XCTAssertNotNil(DevelopController.encode(RetouchSpots.appending(op, to: [:])))
    }

    func testHealSpotsRoundTripThroughSettings() {
        var settings: [String: Any] = [:]
        let first = RetouchSpots.healOperation(id: RetouchSpots.nextID(in: settings), center: (0.2, 0.3))
        settings = DevelopController.merge(settings, RetouchSpots.appending(first, to: settings), keepNulls: false)
        XCTAssertEqual(RetouchSpots.nextID(in: settings), 1)
        let second = RetouchSpots.healOperation(id: RetouchSpots.nextID(in: settings), center: (0.7, 0.6))
        settings = DevelopController.merge(settings, RetouchSpots.appending(second, to: settings), keepNulls: false)
        let spots = RetouchSpots.spots(in: settings)
        XCTAssertEqual(spots.map(\.id), [0, 1])
        XCTAssertEqual(spots[1].center.x, 0.7, accuracy: 1e-9)
        XCTAssertEqual(spots[1].radius, RetouchSpots.defaultRadius)
        XCTAssertTrue(spots.allSatisfy(\.enabled))
    }

    func testInversePatchRestoresTheBase() {
        let base: [String: Any] = ["tone": ["exposure": 0.5, "contrast": 10], "color": ["saturation": 3],
                                   "locals": ["adjustments": [["id": 1]]]]
        let look: [String: Any] = ["tone": ["contrast": -12, "blacks": 14], "effects": ["grain": ["amount": 22]],
                                   "locals": ["adjustments": [["id": 1, "params": ["shadows": 45]]]]]
        let inverse = DevelopController.inverse(of: look, in: base)
        let applied = DevelopController.merge(base, look, keepNulls: false)
        let restored = DevelopController.merge(applied, inverse, keepNulls: false)
        XCTAssertEqual(restored as NSDictionary, base as NSDictionary)
        // Keys the base lacked come back as removals.
        XCTAssertTrue(inverse["effects"] is NSNull)
        XCTAssertTrue(((inverse["tone"] as? [String: Any])?["blacks"]) is NSNull)
    }

    func testDialDefaultsAndLocalParameters() {
        XCTAssertEqual(DialTarget.subject.defaultControl, .shadows)
        XCTAssertEqual(DialTarget.sky.defaultControl, .highlights)
        XCTAssertEqual(DialTarget.background.defaultControl, .exposure)
        XCTAssertEqual(DialTarget.whole.defaultControl, .exposure)
        XCTAssertEqual(DialTarget.subject.note, "Applies to the subject only.")
        XCTAssertEqual(DialTarget.whole.note, "Applies to the whole photo.")
        for c in DialControl.allCases {
            XCTAssertTrue(LocalParam.all.contains { $0.name == c.localName }, c.rawValue)
        }
        XCTAssertEqual(DialControl.warmth.global.path, ["white_balance", "temperature"])
        XCTAssertEqual(DialControl.exposure.global.path, ["tone", "exposure"])
        XCTAssertEqual(DialControl.exposure.dragScale(whole: true), 0.02)
    }

    func testTryLookPatches() {
        let settings: [String: Any] = ["locals": ["adjustments": [
            ["id": 4, "name": "Sky", "params": ["exposure": 0]], ["id": 7, "name": "Subject", "params": [:]],
        ]]]
        let relight = TryLook.relight.patch(over: settings, group: 7)
        let adjustments = ((relight?["locals"] as? [String: Any])?["adjustments"]) as? [[String: Any]]
        XCTAssertEqual(adjustments?.count, 2)
        XCTAssertEqual(((adjustments?[1]["params"]) as? [String: Any])?["shadows"] as? Double, 45)
        XCTAssertEqual((((adjustments?[0]["params"]) as? [String: Any])?["exposure"] as? NSNumber)?.doubleValue, 0)
        XCTAssertNil(TryLook.relight.patch(over: settings, group: nil))
        XCTAssertNil(TryLook.sky.patch(over: settings, group: 99))
        XCTAssertNil(TryLook.style.patch(over: settings, group: nil))
        XCTAssertNotNil(DevelopController.encode(TryLook.film.patch(over: settings, group: nil)!))
        XCTAssertEqual((((TryLook.bw.patch(over: settings, group: nil)?["color"] as? [String: Any])?["monochrome"]) as? [String: Any])?["enabled"] as? Bool, true)
        XCTAssertEqual(TryLook.allCases.count, 6)
    }

    @MainActor
    func testDialStaysInsideTheCanvas() {
        let size = CGSize(width: 1200, height: 700)
        let w = Theme.Width.dial, h = DialHUD.estimatedHeight
        let near = EditPhotoHUDs.dialCenter(anchor: CGPoint(x: 300, y: 350), in: size)
        XCTAssertEqual(near.x, 300 + Theme.Space.xxl + Theme.Space.s + w / 2)
        XCTAssertEqual(near.y, 350 - Theme.Height.dialKnob + h / 2)
        let corner = EditPhotoHUDs.dialCenter(anchor: CGPoint(x: 1190, y: 690), in: size)
        XCTAssertLessThanOrEqual(corner.x + w / 2, size.width - Theme.Space.m)
        XCTAssertLessThanOrEqual(corner.y + h / 2, size.height - Theme.Space.m)
        let top = EditPhotoHUDs.dialCenter(anchor: CGPoint(x: 10, y: 5), in: size)
        XCTAssertGreaterThanOrEqual(top.y - h / 2, Theme.Space.m)
        XCTAssertGreaterThanOrEqual(top.x - w / 2, Theme.Space.m)
    }
}
