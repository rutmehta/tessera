import CoreGraphics
import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Live shapes, Pen / Direct Selection and vector masks (WP B5-11): the vector crate's JSON schema
/// both ways (including opaque patterns), the three affine layouts with skew and translation,
/// primitive construction against the engine, fill-rule containment, inverse-affine hits, exact
/// splitting and handles, coincident-anchor merging, target priority, command JSON, the Pen draft,
/// tool constraints, and a real engine session behind `DocumentVectorBackend` (Shape rows, one-node
/// drafts, cancel, masks, booleans, conversion undo).
final class DocumentVectorTests: XCTestCase {
    private func p(_ x: Double, _ y: Double) -> ShapePoint { ShapePoint(x: x, y: y) }

    private func square(_ x0: Double, _ y0: Double, _ x1: Double, _ y1: Double) -> ShapeSubpath {
        ShapeSubpath(anchors: [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)].map(ShapeAnchor.corner), closed: true)
    }

    /// The engine's serde output for a rounded rectangle with a dashed gradient stroke (crates/vector).
    private let engineJSON = #"""
    {"path":{"subpaths":[{"anchors":[{"point":{"x":1.0,"y":0.0},"incoming":{"x":0.44771525016920644,"y":0.0},"outgoing":{"x":1.0,"y":0.0}},{"point":{"x":10.0,"y":0.0},"incoming":{"x":10.0,"y":0.0},"outgoing":{"x":10.0,"y":0.0}}],"closed":true}],"fill_rule":"NonZero"},"fill":{"Solid":[1.0,0.0,0.0,1.0]},"stroke":[{"width":1.0,"alignment":"Center","dashes":[2.0,1.0],"dash_offset":0.0,"cap":"Butt","join":"Miter","miter_limit":4.0},{"Gradient":{"kind":"Linear","start":{"x":0.0,"y":0.0},"end":{"x":1.0,"y":0.0},"stops":[{"position":0.0,"color":[0.0,0.0,0.0,1.0]},{"position":1.0,"color":[1.0,1.0,1.0,1.0]}],"dither":false}}],"live_shape":{"Rectangle":{"rect":{"x0":0.0,"y0":0.0,"x1":10.0,"y1":5.0},"radii":[1.0,0.0,0.0,0.0]}}}
    """#

    // MARK: Models

    func testShapeSourceDecodesAndReencodesTheEngineSchema() throws {
        let s = try ShapeSource(json: engineJSON)
        XCTAssertEqual(s.path.subpaths[0].anchors[0].incoming.x, 0.44771525016920644)
        XCTAssertEqual(s.path.fillRule, .nonZero)
        XCTAssertEqual(s.fill, .solid([1, 0, 0, 1]))
        XCTAssertEqual(s.stroke?.0, ShapeStroke(width: 1, alignment: .center, dashes: [2, 1]))
        guard case .gradient(let g)? = s.stroke?.1 else { return XCTFail("gradient stroke paint") }
        XCTAssertEqual(g.kind, .linear)
        XCTAssertTrue(g.isValid)
        XCTAssertEqual(s.liveShape, .rectangle(rect: ShapeRect(x0: 0, y0: 0, x1: 10, y1: 5), radii: [1, 0, 0, 0]))
        // Re-encoding keeps every field under the engine's names; decoding again is identical.
        let again = try ShapeSource(json: s.json)
        XCTAssertEqual(again, s)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(s.json.utf8)) as? [String: Any])
        XCTAssertEqual(Set(object.keys), ["path", "fill", "stroke", "live_shape"])
        XCTAssertTrue(s.json.contains(#""miter_limit":4"#) && s.json.contains(#""dash_offset":0"#) && s.json.contains(#""fill_rule":"NonZero""#))
        // Nulls stay explicit (the engine's Option fields).
        let bare = ShapeSource(path: ShapePath(), fill: nil, stroke: nil, liveShape: nil)
        XCTAssertTrue(bare.json.contains(#""fill":null"#) && bare.json.contains(#""live_shape":null"#))
        XCTAssertEqual(try ShapeSource(json: bare.json), bare)
        // Every live variant round-trips under its Rust tag.
        let shapes: [LiveShape] = [
            .ellipse(center: p(1, 2), radii: p(3, 4)),
            .polygon(center: p(0, 0), radius: 5, sides: 5, rotation: 0.5, innerRadius: 2),
            .polygon(center: p(0, 0), radius: 5, sides: 6, rotation: 0, innerRadius: nil),
            .line(start: p(0, 0), end: p(1, 1)),
            .custom(ShapePath(subpaths: [square(0, 0, 1, 1)], fillRule: .evenOdd)),
        ]
        for shape in shapes {
            let json = VectorJSON.string(shape)
            XCTAssertEqual(try VectorJSON.decoder.decode(LiveShape.self, from: Data(json.utf8)), shape, json)
        }
        XCTAssertTrue(VectorJSON.string(shapes[0]).hasPrefix(#"{"Ellipse":"#))
        XCTAssertTrue(VectorJSON.string(shapes[1]).contains(#""inner_radius":2"#))
        XCTAssertThrowsError(try ShapeSource(json: #"{"path":1}"#))
    }

    func testPatternPaintIsCarriedOpaquely() throws {
        let json = #"{"Pattern":{"width":2,"height":1,"pixels":[[1.0,0.0,0.0,1.0],[0.0,0.0,1.0,1.0]],"document_to_tile":[0.25,-0.0,-0.0,0.25,0.0,0.0]}}"#
        let paint = try VectorJSON.decoder.decode(ShapePaint.self, from: Data(json.utf8))
        XCTAssertTrue(paint.isPattern)
        let out = VectorJSON.string(paint)
        XCTAssertTrue(out.contains(#""width":2"#), "integral fields stay integral: \(out)")
        XCTAssertEqual(try VectorJSON.decoder.decode(ShapePaint.self, from: Data(out.utf8)), paint)
        // Editing the stroke of a pattern-filled source keeps the pattern.
        var s = ShapeSource(live: .rectangle(rect: ShapeRect(x0: 0, y0: 0, x1: 4, y1: 4), radii: [0, 0, 0, 0]), fill: paint, stroke: nil)
        s.stroke = (ShapeStroke(width: 3), .solid([0, 0, 0, 1]))
        XCTAssertEqual(try ShapeSource(json: s.json).fill, paint)
    }

    // MARK: Affine layouts

    func testAffineLayoutsConvertExplicitlyWithSkewAndTranslation() throws {
        let m = AffineTransform2D(a: 1.5, b: 0.25, c: 10, d: -0.5, e: 2, f: -3)
        let cg = m.cgAffineTransform
        for q in [CGPoint(x: 0, y: 0), CGPoint(x: 3, y: -2), CGPoint(x: 7.5, y: 11)] {
            let a = m.apply(q), b = q.applying(cg)
            XCTAssertEqual(a.x, b.x, accuracy: 1e-12)
            XCTAssertEqual(a.y, b.y, accuracy: 1e-12)
            // The kurbo layout maps (a·x + c·y + e, b·x + d·y + f).
            let k = m.kurboCoefficients
            XCTAssertEqual(k[0] * q.x + k[2] * q.y + k[4], a.x, accuracy: 1e-12)
            XCTAssertEqual(k[1] * q.x + k[3] * q.y + k[5], a.y, accuracy: 1e-12)
        }
        XCTAssertEqual(AffineTransform2D(cg), m)
        XCTAssertEqual(AffineTransform2D(kurbo: m.kurboCoefficients), m)
        XCTAssertNil(AffineTransform2D(kurbo: [1, 2]))
        // Reinterpreting instead of converting would be wrong: the translation is not in slot b.
        XCTAssertNotEqual(CGAffineTransform(a: m.a, b: m.b, c: m.c, d: m.d, tx: m.e, ty: m.f), cg)
        // Inverse and composition.
        let inv = try XCTUnwrap(m.inverse)
        let id = m.concatenating(after: inv)
        XCTAssertEqual(id.a, 1, accuracy: 1e-12); XCTAssertEqual(id.b, 0, accuracy: 1e-12); XCTAssertEqual(id.c, 0, accuracy: 1e-12)
        XCTAssertFalse(m.isSimilarity)
        XCTAssertTrue(AffineTransform2D.rotation(degrees: 30).concatenating(after: .scale(2, 2)).isSimilarity)
        XCTAssertEqual(AffineTransform2D.rotation(degrees: 30).rotationDegrees, 30, accuracy: 1e-9)
        XCTAssertFalse(AffineTransform2D(a: 1, b: 2, c: 0, d: 2, e: 4, f: 0).isFiniteAndInvertible)
        XCTAssertFalse(AffineTransform2D(a: .nan, b: 0, c: 0, d: 0, e: 1, f: 0).isFiniteAndInvertible)
    }

    // MARK: Primitives

    func testRectanglePrimitiveMatchesTheEngineConstruction() throws {
        let rounded = ShapePrimitives.rectangle(ShapeRect(x0: 0, y0: 0, x1: 10, y1: 5), radii: [1, 0, 0, 0])
        let engine = try ShapeSource(json: engineJSON)
        XCTAssertEqual(rounded.subpaths[0].anchors[0], engine.path.subpaths[0].anchors[0])
        XCTAssertEqual(rounded.subpaths[0].anchors.count, 8)
        XCTAssertTrue(rounded.subpaths[0].closed)
        // Oversized radii scale together to fit (the engine's rule): 30 + 30 on a 40-wide edge.
        XCTAssertEqual(ShapePrimitives.fittedRadii(ShapeRect(x0: 0, y0: 0, x1: 40, y1: 100), [30, 30, 0, 0]), [20, 20, 0, 0])
        XCTAssertEqual(ShapePrimitives.fittedRadii(ShapeRect(x0: 0, y0: 0, x1: 40, y1: 100), [5, -1, 0, 0]), [5, 0, 0, 0])
        // Against the real engine, anchor for anchor.
        for radii in [[0.0, 0, 0, 0], [3, 7, 0, 12], [50, 50, 50, 50]] {
            let live = LiveShape.rectangle(rect: ShapeRect(x0: 2, y0: 3, x1: 42, y1: 33), radii: radii)
            XCTAssertEqual(ShapePrimitives.path(live), try enginePrimitivePath(live), "\(radii)")
        }
    }

    func testPolygonStarAndLinePrimitivesMatchTheEngine() throws {
        let star = LiveShape.polygon(center: p(50, 50), radius: 30, sides: 5, rotation: -Double.pi / 2, innerRadius: 12)
        let path = ShapePrimitives.path(star)
        XCTAssertEqual(path.subpaths[0].anchors.count, 10)
        XCTAssertEqual(path.subpaths[0].anchors[0].point.x, 50, accuracy: 1e-9)
        XCTAssertEqual(path.subpaths[0].anchors[0].point.y, 20, accuracy: 1e-9)
        XCTAssertEqual(path.subpaths[0].anchors[1].point.distance(to: p(50, 50)), 12, accuracy: 1e-9)
        let hex = LiveShape.polygon(center: p(0, 0), radius: 10, sides: 6, rotation: 0, innerRadius: nil)
        XCTAssertEqual(ShapePrimitives.path(hex).subpaths[0].anchors.count, 6)
        let line = LiveShape.line(start: p(1, 2), end: p(30, 40))
        XCTAssertFalse(ShapePrimitives.path(line).subpaths[0].closed)
        for s in [star, hex, line] {
            let e = try enginePrimitivePath(s)
            XCTAssertEqual(e.subpaths.count, 1)
            for (a, b) in zip(ShapePrimitives.path(s).subpaths[0].anchors, e.subpaths[0].anchors) {
                XCTAssertEqual(a.point.distance(to: b.point), 0, accuracy: 1e-9)
            }
        }
    }

    func testEllipseOverlayLiesOnTheEnginesEllipse() throws {
        let live = LiveShape.ellipse(center: p(40, 30), radii: p(25, 10))
        let overlay = ShapePrimitives.path(live)
        let engine = try enginePrimitivePath(live)
        // Every flattened overlay point is (nearly) on the ellipse, and both enclose the same area.
        for (pts, _) in overlay.flattened(tolerance: 0.01) {
            for q in pts {
                let v = pow((q.x - 40) / 25, 2) + pow((q.y - 30) / 10, 2)
                XCTAssertEqual(v, 1, accuracy: 0.003)
            }
        }
        for q in [p(40, 30), p(60, 30), p(40, 38)] { XCTAssertEqual(overlay.contains(q), engine.contains(q)) }
        XCTAssertFalse(overlay.contains(p(66, 30)))
        let b = try XCTUnwrap(overlay.bounds)
        XCTAssertEqual(b.minX, 15, accuracy: 0.01); XCTAssertEqual(b.maxY, 40, accuracy: 0.01)
    }

    // MARK: Containment and hits

    func testEvenOddHolesAndNonzeroContainment() {
        var path = ShapePath(subpaths: [square(10, 10, 90, 90), square(30, 30, 70, 70)], fillRule: .evenOdd)
        XCTAssertTrue(path.contains(p(20, 20)))
        XCTAssertFalse(path.contains(p(50, 50)), "even-odd hole")
        XCTAssertFalse(path.contains(p(95, 50)))
        path.fillRule = .nonZero
        XCTAssertTrue(path.contains(p(50, 50)), "same orientation: nonzero fills")
        // Opposite orientation makes a nonzero hole too (booleans produce these).
        var reversed = square(30, 30, 70, 70)
        reversed.anchors.reverse()
        let hole = ShapePath(subpaths: [square(10, 10, 90, 90), reversed], fillRule: .nonZero)
        XCTAssertFalse(hole.contains(p(50, 50)))
        XCTAssertEqual(hole.winding(p(20, 20)), path.winding(p(20, 20)))
        // Open subpaths fill as if closed.
        let open = ShapePath.polyline([p(0, 0), p(10, 0), p(10, 10)], closed: false)
        XCTAssertTrue(open.contains(p(8, 2)))
    }

    func testInverseAffineHitsWithSkewAndTranslation() throws {
        let t = AffineTransform2D(a: 1.5, b: 0.6, c: 40, d: -0.2, e: 1.2, f: 30)
        let local = ShapePrimitives.path(.rectangle(rect: ShapeRect(x0: 0, y0: 0, x1: 40, y1: 30), radii: [0, 0, 0, 0]))
        let inv = try XCTUnwrap(t.inverse)
        for (lx, ly, inside) in [(1.0, 1.0, true), (20, 15, true), (39, 29, true), (-2, 15, false), (20, 32, false)] {
            let doc = t.apply(p(lx, ly))
            let back = inv.apply(doc)
            XCTAssertEqual(back.x, lx, accuracy: 1e-9)
            XCTAssertEqual(local.contains(back), inside, "(\(lx), \(ly))")
        }
        // The document-space path (overlay geometry) agrees with the inverse-mapped test.
        let docPath = local.transformed(t)
        for q in [p(70, 40), p(45, 32), p(120, 80), p(30, 60)] {
            XCTAssertEqual(docPath.contains(q), local.contains(inv.apply(q)), "\(q)")
        }
    }

    // MARK: Anchors and handles

    func testSplitIsExactAndHandlesMirrorOrStayIndependent() throws {
        let curve = ShapePath(subpaths: [ShapeSubpath(anchors: [
            ShapeAnchor(point: p(10, 50), incoming: p(10, 50), outgoing: p(30, 10)),
            ShapeAnchor(point: p(90, 50), incoming: p(70, 10), outgoing: p(90, 50)),
        ], closed: false)])
        let split = try XCTUnwrap(curve.splitting(subpath: 0, segment: 0, t: 0.5))
        let a = split.subpaths[0].anchors
        XCTAssertEqual(a.count, 3)
        XCTAssertEqual(a[1].point, p(50, 20))
        XCTAssertEqual(a[0].outgoing, p(20, 30))
        XCTAssertEqual(a[2].incoming, p(80, 30))
        // The split curve traces the original.
        let original = CubicSegment(p(10, 50), p(30, 10), p(70, 10), p(90, 50))
        let left = split.segments(of: 0)[0]
        XCTAssertEqual(left.point(0.5).distance(to: original.point(0.25)), 0, accuracy: 1e-12)
        XCTAssertNil(curve.splitting(subpath: 0, segment: 0, t: 1))
        XCTAssertNil(curve.splitting(subpath: 0, segment: 1, t: 0.5), "open path: no closing segment")
        let r = AnchorRef(subpath: 0, anchor: 1)
        let mirrored = split.settingHandle(r, .outgoing, to: p(60, 10), mirror: true)
        XCTAssertEqual(mirrored.anchor(r)?.incoming, p(40, 30))
        let independent = mirrored.settingHandle(r, .incoming, to: p(35, 20), mirror: false)
        XCTAssertEqual(independent.anchor(r)?.outgoing, p(60, 10))
        let moved = independent.movingAnchor(r, to: p(55, 25))
        XCTAssertEqual(moved.anchor(r)?.incoming, p(40, 25), "handles travel with the anchor")
    }

    func testCoincidentAnchorsMergeLikeTheEngine() {
        let rect = ShapePrimitives.rectangle(ShapeRect(x0: 10, y0: 10, x1: 50, y1: 50), radii: [0, 0, 0, 0])
        XCTAssertEqual(rect.subpaths[0].anchors.count, 8, "generated square corners repeat anchors")
        let merged = rect.mergingCoincidentAnchors
        XCTAssertEqual(merged.subpaths[0].anchors.map(\.point), [p(10, 10), p(50, 10), p(50, 50), p(10, 50)])
        XCTAssertTrue(merged.subpaths[0].anchors.allSatisfy(\.isCorner))
        XCTAssertEqual(merged.mergingCoincidentAnchors, merged, "idempotent")
        let rounded = ShapePrimitives.rectangle(ShapeRect(x0: 0, y0: 0, x1: 40, y1: 40), radii: [5, 5, 5, 5])
        XCTAssertEqual(rounded.mergingCoincidentAnchors, rounded, "distinct anchors stay")
    }

    func testTargetsPreferSelectedHandlesThenAnchorsThenSegments() {
        let path = ShapePath(subpaths: [ShapeSubpath(anchors: [
            ShapeAnchor(point: p(0, 0), incoming: p(0, 0), outgoing: p(20, 0)),
            ShapeAnchor(point: p(100, 0), incoming: p(80, 0), outgoing: p(100, 0)),
        ], closed: false)])
        let first = AnchorRef(subpath: 0, anchor: 0)
        XCTAssertEqual(path.target(at: p(1, 1), radius: 4), .anchor(first))
        XCTAssertEqual(path.target(at: p(20, 1), radius: 4, selected: [first]), .handle(first, .outgoing))
        if case .segment(_, _, _)? = path.target(at: p(20, 1), radius: 4) {} else { XCTFail("unselected handles are not targets") }
        guard case .segment(_, _, let t)? = path.target(at: p(50, 2), radius: 4) else { return XCTFail("segment") }
        XCTAssertEqual(t, 0.5, accuracy: 1e-3)
        XCTAssertNil(path.target(at: p(50, 20), radius: 4))
        XCTAssertNil(path.anchor(AnchorRef(subpath: 3, anchor: 0)))
    }

    func testPathCommandJSONMatchesTheEngineSchema() throws {
        let r = AnchorRef(subpath: 0, anchor: 2)
        XCTAssertEqual(PathCommand.moveAnchor(r, to: p(70, 80)).json, #"{"anchor":2,"op":"move_anchor","subpath":0,"x":70,"y":80}"#)
        XCTAssertEqual(PathCommand.setHandle(r, .outgoing, to: p(1, 2), mirror: true).json,
                       #"{"anchor":2,"handle":"outgoing","mirror":true,"op":"set_handle","subpath":0,"x":1,"y":2}"#)
        XCTAssertEqual(PathCommand.insertAnchor(subpath: 0, segment: 1, t: 0.5).json, #"{"op":"insert_anchor","segment":1,"subpath":0,"t":0.5}"#)
        XCTAssertEqual(PathCommand.deleteAnchor(r).json, #"{"anchor":2,"op":"delete_anchor","subpath":0}"#)
        XCTAssertEqual(PathCommand.setClosed(subpath: 1, closed: true).json, #"{"closed":true,"op":"set_closed","subpath":1}"#)
        XCTAssertEqual(PathCommand.setFillRule(.evenOdd).json, #"{"op":"set_fill_rule","rule":"EvenOdd"}"#)
        let many = PathCommand.json([.deleteAnchor(r), .setFillRule(.nonZero)])
        XCTAssertTrue(many.hasPrefix("[") && many.contains("delete_anchor") && many.contains("NonZero"))
        XCTAssertTrue(PathCommand.addSubpath(square(0, 0, 1, 1)).json.contains(#""anchors":["#))
    }

    // MARK: Pen and tools

    func testPenDraftBuildsCubicHandlesAndCloses() {
        var pen = PenDraft()
        XCTAssertFalse(pen.click(p(10, 10), radius: 4))
        XCTAssertFalse(pen.click(p(60, 10), radius: 4))
        pen.drag(to: p(80, 30), independent: false)
        XCTAssertEqual(pen.anchors[1].outgoing, p(80, 30))
        XCTAssertEqual(pen.anchors[1].incoming, p(40, -10), "mirrored handle")
        XCTAssertFalse(pen.click(p(60, 70), radius: 4))
        pen.drag(to: p(40, 90), independent: true)
        XCTAssertEqual(pen.anchors[2].incoming, p(60, 70), "⌥ keeps the incoming handle")
        XCTAssertFalse(pen.closes(at: p(30, 30), radius: 4))
        XCTAssertTrue(pen.click(p(11, 11), radius: 4), "clicking the first anchor closes")
        XCTAssertTrue(pen.path.subpaths[0].closed)
        XCTAssertEqual(pen.path.anchorCount, 3)
        XCTAssertTrue(pen.isCommittable)
        pen.removeLast()
        XCTAssertFalse(pen.closed)
        pen.removeLast()
        XCTAssertEqual(pen.anchors.count, 2)
        var single = PenDraft()
        single.click(p(0, 0), radius: 4)
        XCTAssertFalse(single.isCommittable)
        XCTAssertFalse(single.click(p(1, 1), radius: 4), "one anchor cannot close")
    }

    func testShapeToolConstraints() {
        let box = ShapeToolMath.box(start: CGPoint(x: 10, y: 10), current: CGPoint(x: 40, y: 20), square: true, fromCenter: false)
        XCTAssertEqual(box, ShapeRect(x0: 10, y0: 10, x1: 40, y1: 40))
        let centred = ShapeToolMath.box(start: CGPoint(x: 50, y: 50), current: CGPoint(x: 60, y: 55), square: false, fromCenter: true)
        XCTAssertEqual(centred, ShapeRect(x0: 40, y0: 45, x1: 60, y1: 55))
        XCTAssertEqual(ShapeToolMath.ellipse(centred), .ellipse(center: p(50, 50), radii: p(10, 5)))
        guard case .polygon(let c, let r, let n, let rot, let inner) =
            ShapeToolMath.polygon(center: CGPoint(x: 0, y: 0), current: CGPoint(x: 10, y: 1), sides: 5, star: true, inset: 0.4, snap: true)
        else { return XCTFail("polygon") }
        XCTAssertEqual(c, p(0, 0)); XCTAssertEqual(n, 5); XCTAssertEqual(rot, 0, accuracy: 1e-12)
        XCTAssertEqual(r, hypot(10, 1), accuracy: 1e-12); XCTAssertEqual(inner ?? 0, r * 0.4, accuracy: 1e-12)
        guard case .line(_, let end) = ShapeToolMath.line(start: .zero, current: CGPoint(x: 10, y: 9), snap: true) else { return XCTFail() }
        XCTAssertEqual(end.x, end.y, accuracy: 1e-9)
        XCTAssertFalse(ShapeToolMath.isShape(.rectangle(rect: ShapeRect(x0: 0, y0: 0, x1: 0.5, y1: 20), radii: [0, 0, 0, 0])))
        XCTAssertTrue(ShapeToolMath.isShape(.line(start: p(0, 0), end: p(3, 0))))
        // Inside / Outside strokes on open paths are explained before the engine rejects them.
        var line = ShapeSource(live: .line(start: p(0, 0), end: p(10, 0)), fill: nil, stroke: (ShapeStroke(alignment: .outside), .solid([0, 0, 0, 1])))
        XCTAssertNotNil(line.strokeAlignmentProblem)
        line.stroke?.0.alignment = .center
        XCTAssertNil(line.strokeAlignmentProblem)
        XCTAssertFalse(ShapeStroke(width: 2, dashes: [0, 3]).isValid)
    }

    func testToolRoutingKeysAndGroups() {
        XCTAssertEqual(ToolKeyMap.action(keyCode: 32, characters: "u", mods: [], current: .move), .tool(.rectangleShape))
        XCTAssertEqual(ToolKeyMap.action(keyCode: 32, characters: "u", mods: .shift, current: .rectangleShape), .tool(.ellipseShape))
        XCTAssertEqual(ToolKeyMap.action(keyCode: 32, characters: "u", mods: .shift, current: .lineShape), .tool(.rectangleShape))
        XCTAssertEqual(ToolKeyMap.action(keyCode: 35, characters: "p", mods: [], current: .move), .tool(.pen))
        XCTAssertEqual(ToolKeyMap.action(keyCode: 0, characters: "a", mods: [], current: .move), .tool(.pathSelect))
        XCTAssertEqual(ToolKeyMap.action(keyCode: 0, characters: "a", mods: .shift, current: .pathSelect), .tool(.directSelect))
        for t in DocumentTool.allCases where t.isVector {
            XCTAssertTrue(DocumentTool.paletteSlots.contains { $0.group.contains(t) }, "\(t) has a palette slot")
            XCTAssertFalse(t.isPlaceholder)
        }
        XCTAssertEqual(LayerKindTag.shape.title, "Shape")
        XCTAssertEqual(LayerKindTag(DocLayerKind.shape), .shape)
        XCTAssertEqual(LayerKindTag.shape.ffi, .shape)
    }

    // MARK: Engine

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vector-doc-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testEngineShapesDraftsMasksBooleansAndConversion() throws {
        let dir = try temp()
        let docs = EngineDocumentEngine.for(try Engine.open(appSupportDir: dir.appendingPathComponent("support").path))
        let doc = try docs.newDocument(width: 120, height: 90, depth: .u8, profile: nil)
        defer { doc.close() }
        let v = try XCTUnwrap(doc as? any DocumentVectorBackend)
        let red: [Double] = [1, 0, 0, 1]
        let rect = ShapeSource(live: .rectangle(rect: ShapeRect(x0: 10, y0: 10, x1: 60, y1: 50), radii: [0, 0, 0, 0]),
                               fill: .solid(red), stroke: nil)
        let added = try v.addShapeLayer(name: "", parent: nil, index: nil, source: rect, transform: .identity)
        let id = try XCTUnwrap(added.created.first)
        XCTAssertEqual(try doc.layer(id: id).kind, .shape, "a Shape row, not Fill")
        var info = try v.shapeLayer(id)
        XCTAssertEqual(info.source, rect)
        XCTAssertEqual(info.liveKind, "rectangle")
        XCTAssertEqual(info.transform, .identity)
        let base = try doc.historyItems().count
        // A drag previews; Esc leaves history and source unchanged.
        for x in [5.0, 10, 15] { _ = try v.setShapeLayer(id, source: rect, transform: .translation(x, 0), interactive: true) }
        XCTAssertEqual(try v.shapeLayer(id).transform, .translation(15, 0))
        _ = try v.cancelSourcePreview()
        XCTAssertEqual(try v.shapeLayer(id).transform, .identity)
        XCTAssertEqual(try doc.historyItems().count, base)
        // The final value is one node; a skewed transform hits through its inverse.
        let skew = AffineTransform2D(a: 1, b: 0.5, c: 4, d: 0, e: 1, f: 2)
        _ = try v.setShapeLayer(id, source: rect, transform: skew, interactive: false)
        XCTAssertEqual(try doc.historyItems().count, base + 1)
        let inside = skew.apply(CGPoint(x: 30, y: 40))
        XCTAssertEqual(try v.shapeHitTest(inside, includeStroke: true, tolerance: 0)?.layer, id)
        XCTAssertNil(try v.shapeHitTest(CGPoint(x: 30, y: 40 + 20), includeStroke: true, tolerance: 0))
        // Direct Selection on the merged corners; the primitive is dropped.
        let merged = info.source.path.mergingCoincidentAnchors
        XCTAssertEqual(merged.anchorCount, 4)
        _ = try v.editShapePath(id, commands: [.moveAnchor(AnchorRef(subpath: 0, anchor: 2), to: p(70, 60))], interactive: false)
        info = try v.shapeLayer(id)
        XCTAssertNil(info.liveKind)
        XCTAssertEqual(info.source.path.subpaths[0].anchors[2].point, p(70, 60))
        // A vector mask next to a raster mask; density and disable.
        _ = try doc.addMask(id: id, mask: .revealAll)
        let mask = VectorMaskInfo(path: ShapePath(subpaths: [square(0, 0, 40, 90)]), feather: 2, density: 0.5)
        _ = try v.setVectorMask(id, mask: mask, interactive: false)
        XCTAssertEqual(try v.vectorMask(id), mask)
        XCTAssertTrue(try doc.layer(id: id).hasMask)
        XCTAssertThrowsError(try v.setVectorMask(id, mask: VectorMaskInfo(path: mask.path, density: 2), interactive: false))
        // Linked move: shape and mask together, one undo.
        let n = try doc.historyItems().count
        _ = try v.transformShapeWithMask(id, transform: .translation(10, 0).concatenating(after: skew), interactive: false)
        XCTAssertEqual(try doc.historyItems().count, n + 1)
        XCTAssertEqual(try v.vectorMask(id)?.path.bounds?.minX ?? -1, 10, accuracy: 1e-9)
        _ = try doc.undo()
        XCTAssertEqual(try v.vectorMask(id), mask)
        XCTAssertEqual(try v.shapeLayer(id).transform, skew)
        // Boolean: one node, operand removed.
        let other = try XCTUnwrap(try v.addShapeLayer(name: "", parent: nil, index: nil, source: rect, transform: .translation(30, 0)).created.first)
        let before = try doc.historyItems().count
        _ = try v.booleanShapes(id, operands: [other], operation: .subtract)
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        XCTAssertThrowsError(try doc.layer(id: other))
        // Invalid source and convert / undo.
        var bad = rect
        bad.stroke = (ShapeStroke(alignment: .inside), .solid(red))
        bad.liveShape = .line(start: p(0, 0), end: p(5, 5))
        XCTAssertThrowsError(try v.setShapeLayer(id, source: bad, transform: skew, interactive: false)) { e in
            XCTAssertTrue(e.localizedDescription.contains("closed path"), e.localizedDescription)
        }
        let source = try v.shapeLayer(id).source
        _ = try v.convertToPixels(id: id)
        XCTAssertEqual(try doc.layer(id: id).kind, .pixel)
        XCTAssertNotNil(try v.vectorMask(id), "masks survive conversion")
        _ = try doc.undo()
        XCTAssertEqual(try v.shapeLayer(id).source, source)
    }
}
