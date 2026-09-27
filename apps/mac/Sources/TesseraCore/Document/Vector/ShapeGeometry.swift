import Foundation

// Geometry for the shape tools, Pen and Direct Selection (WP B5-11): live primitive paths (the same
// construction as `vector::Shape::path`, so draft overlays match what the engine will add), explicit
// affine layout conversions, cubic evaluation / flattening / splitting, fill-rule containment, and
// anchor / handle / segment hit testing in local shape pixels. Unit tested (DocumentVectorTests).
// Engine-side hit testing of a layer (stroke outlines, dashes) is `shape_hit_test`; this file only
// serves the host's own draft and handle interaction.

// MARK: - Affine layouts

/// Three layouts meet here and are converted explicitly, never reinterpreted:
/// * `AffineTransform2D` / the engine's `TransformMatrix`: row-major `[a, b, c, d, e, f]` mapping
///   `(a·x + b·y + c, d·x + e·y + f)`;
/// * `CGAffineTransform(a, b, c, d, tx, ty)`: `(a·x + c·y + tx, b·x + d·y + ty)`;
/// * `kurbo::Affine([a, b, c, d, e, f])`: `(a·x + c·y + e, b·x + d·y + f)`.
/// (`cgAffineTransform`, `init(_: CGAffineTransform)` and `isFiniteAndInvertible` are B5-10's, in
/// Text/TextSourceModel.swift; this file adds the kurbo layout.)
extension AffineTransform2D {
    /// `kurbo::Affine` coefficients (the vector crate's own layout).
    public var kurboCoefficients: [Double] { [a, d, b, e, c, f] }

    public init?(kurbo k: [Double]) {
        guard k.count == 6 else { return nil }
        self.init(a: k[0], b: k[2], c: k[4], d: k[1], e: k[3], f: k[5])
    }

    public func apply(_ p: ShapePoint) -> ShapePoint { ShapePoint(x: a * p.x + b * p.y + c, y: d * p.x + e * p.y + f) }

    /// The linear part's scale along x and y (column lengths) and whether it is a similarity
    /// (uniform scale, no skew) — a nonuniform shape transform changes how other apps redraw PSD strokes.
    public var axisScales: (x: Double, y: Double) { (hypot(a, d), hypot(b, e)) }
    public var isSimilarity: Bool {
        let (sx, sy) = axisScales
        let m = max(sx, sy, 1)
        return abs(sx - sy) <= 1e-9 * m && abs(a * b + d * e) <= 1e-9 * m * m
    }
    /// Rotation of the x axis in degrees (clockwise on the y-down canvas).
    public var rotationDegrees: Double { atan2(d, a) * 180 / .pi }
}

// MARK: - Primitive paths

/// `vector::Shape::path` in Swift, for draft overlays. Rectangles, polygons, stars and lines are
/// the same construction as the engine (anchor for anchor); ellipses use four quarter-arc cubics,
/// which the engine refines further (it approximates to 1e-6 px), so an ellipse overlay is within a
/// fraction of a pixel of the engine's path.
public enum ShapePrimitives {
    /// Quarter-circle cubic handle ratio (the engine's `k`).
    public static let kappa = 0.5522847498307936

    public static func path(_ shape: LiveShape) -> ShapePath {
        switch shape {
        case .rectangle(let rect, let radii): rectangle(rect, radii: radii)
        case .ellipse(let c, let r): ellipse(center: c, radii: r)
        case .polygon(let c, let radius, let sides, let rotation, let inner):
            polygon(center: c, radius: radius, sides: sides, rotation: rotation, innerRadius: inner)
        case .line(let s, let e): ShapePath.polyline([s, e], closed: false)
        case .custom(let p): p
        }
    }

    /// Radii scaled together so adjacent radii fit each edge (the engine's rule).
    public static func fittedRadii(_ rect: ShapeRect, _ radii: [Double]) -> [Double] {
        var r = (0..<4).map { $0 < radii.count ? max(radii[$0], 0) : 0 }
        var scale = 1.0
        for (size, sum) in [(rect.width, r[0] + r[1]), (rect.width, r[2] + r[3]), (rect.height, r[0] + r[3]), (rect.height, r[1] + r[2])]
        where sum > 0 { scale = min(scale, size / sum) }
        r = r.map { $0 * scale }
        return r
    }

    public static func rectangle(_ rect: ShapeRect, radii: [Double]) -> ShapePath {
        let r = fittedRadii(rect, radii)
        let (tl, tr, br, bl) = (r[0], r[1], r[2], r[3])
        let k = kappa
        let (x, y, u, v) = (rect.x0, rect.y0, rect.x1, rect.y1)
        var b = BezBuilder()
        b.move(ShapePoint(x: x + tl, y: y))
        b.line(ShapePoint(x: u - tr, y: y))
        b.curve(ShapePoint(x: u - tr + k * tr, y: y), ShapePoint(x: u, y: y + tr - k * tr), ShapePoint(x: u, y: y + tr))
        b.line(ShapePoint(x: u, y: v - br))
        b.curve(ShapePoint(x: u, y: v - br + k * br), ShapePoint(x: u - br + k * br, y: v), ShapePoint(x: u - br, y: v))
        b.line(ShapePoint(x: x + bl, y: v))
        b.curve(ShapePoint(x: x + bl - k * bl, y: v), ShapePoint(x: x, y: v - bl + k * bl), ShapePoint(x: x, y: v - bl))
        b.line(ShapePoint(x: x, y: y + tl))
        b.curve(ShapePoint(x: x, y: y + tl - k * tl), ShapePoint(x: x + tl - k * tl, y: y), ShapePoint(x: x + tl, y: y))
        b.close()
        return b.path
    }

    public static func ellipse(center c: ShapePoint, radii r: ShapePoint) -> ShapePath {
        let (kx, ky) = (kappa * r.x, kappa * r.y)
        let right = ShapePoint(x: c.x + r.x, y: c.y), bottom = ShapePoint(x: c.x, y: c.y + r.y)
        let left = ShapePoint(x: c.x - r.x, y: c.y), top = ShapePoint(x: c.x, y: c.y - r.y)
        return ShapePath(subpaths: [ShapeSubpath(anchors: [
            ShapeAnchor(point: right, incoming: ShapePoint(x: right.x, y: c.y - ky), outgoing: ShapePoint(x: right.x, y: c.y + ky)),
            ShapeAnchor(point: bottom, incoming: ShapePoint(x: c.x + kx, y: bottom.y), outgoing: ShapePoint(x: c.x - kx, y: bottom.y)),
            ShapeAnchor(point: left, incoming: ShapePoint(x: left.x, y: c.y + ky), outgoing: ShapePoint(x: left.x, y: c.y - ky)),
            ShapeAnchor(point: top, incoming: ShapePoint(x: c.x - kx, y: top.y), outgoing: ShapePoint(x: c.x + kx, y: top.y)),
        ], closed: true)])
    }

    public static func polygon(center: ShapePoint, radius: Double, sides: UInt32, rotation: Double, innerRadius: Double?) -> ShapePath {
        let n = innerRadius == nil ? Int(sides) : Int(sides) * 2
        guard n >= 3 else { return ShapePath() }
        let points = (0..<n).map { i -> ShapePoint in
            let a = rotation + 2 * Double.pi * Double(i) / Double(n)
            let r = i % 2 == 1 ? (innerRadius ?? radius) : radius
            return ShapePoint(x: center.x + cos(a) * r, y: center.y + sin(a) * r)
        }
        return ShapePath.polyline(points, closed: true)
    }

    /// `kurbo::BezPath` → `vector::Path::from_bez` (move / line / curve / close).
    struct BezBuilder {
        var path = ShapePath()
        private var current: ShapeSubpath?
        mutating func move(_ p: ShapePoint) {
            if let c = current, !c.anchors.isEmpty { path.subpaths.append(c) }
            current = ShapeSubpath(anchors: [.corner(p)], closed: false)
        }
        mutating func line(_ p: ShapePoint) { current?.anchors.append(.corner(p)) }
        mutating func curve(_ a: ShapePoint, _ b: ShapePoint, _ p: ShapePoint) {
            guard current != nil else { return }
            if !current!.anchors.isEmpty { current!.anchors[current!.anchors.count - 1].outgoing = a }
            current!.anchors.append(ShapeAnchor(point: p, incoming: b, outgoing: p))
        }
        mutating func close() {
            guard var s = current else { return }
            s.closed = true
            if s.anchors.count > 1, s.anchors[s.anchors.count - 1].point == s.anchors[0].point {
                let last = s.anchors.removeLast()
                s.anchors[0].incoming = last.incoming
            }
            path.subpaths.append(s)
            current = nil
        }
    }
}

// MARK: - Path maths

public struct CubicSegment: Equatable, Sendable {
    public var p0: ShapePoint, p1: ShapePoint, p2: ShapePoint, p3: ShapePoint
    public init(_ p0: ShapePoint, _ p1: ShapePoint, _ p2: ShapePoint, _ p3: ShapePoint) {
        self.p0 = p0; self.p1 = p1; self.p2 = p2; self.p3 = p3
    }
    public func point(_ t: Double) -> ShapePoint {
        let u = 1 - t
        let a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t
        return ShapePoint(x: a * p0.x + b * p1.x + c * p2.x + d * p3.x, y: a * p0.y + b * p1.y + c * p2.y + d * p3.y)
    }
    /// de Casteljau split at `t`: the left and right halves.
    public func split(_ t: Double) -> (CubicSegment, CubicSegment) {
        let a = p0.lerp(p1, t), b = p1.lerp(p2, t), c = p2.lerp(p3, t)
        let d = a.lerp(b, t), e = b.lerp(c, t)
        let m = d.lerp(e, t)
        return (CubicSegment(p0, a, d, m), CubicSegment(m, e, c, p3))
    }
    public var isLine: Bool { p1 == p0 && p2 == p3 }
    /// Steps so that consecutive chords stay within about `tolerance` of the curve.
    func steps(_ tolerance: Double) -> Int {
        if isLine { return 1 }
        let dd = max((p0 - p1 * 2 + p2).distance(to: ShapePoint(x: 0, y: 0)),
                     (p1 - p2 * 2 + p3).distance(to: ShapePoint(x: 0, y: 0)))
        let n = Int(ceil(sqrt(max(dd, 0) * 0.75 / max(tolerance, 1e-6))))
        return min(max(n, 1), 512)
    }
    /// Nearest parameter and distance to `p` (sampling, then a bracketed refinement).
    public func nearest(_ p: ShapePoint) -> (t: Double, distance: Double) {
        let n = 48
        var best = (t: 0.0, d: Double.infinity)
        for i in 0...n {
            let t = Double(i) / Double(n)
            let d = point(t).distance(to: p)
            if d < best.d { best = (t, d) }
        }
        var lo = max(best.t - 1.0 / Double(n), 0), hi = min(best.t + 1.0 / Double(n), 1)
        for _ in 0..<40 {
            let m1 = lo + (hi - lo) / 3, m2 = hi - (hi - lo) / 3
            if point(m1).distance(to: p) < point(m2).distance(to: p) { hi = m2 } else { lo = m1 }
        }
        let t = (lo + hi) / 2
        return (t, point(t).distance(to: p))
    }
}

/// An anchor of a path: subpath and anchor index.
public struct AnchorRef: Hashable, Sendable, Codable {
    public var subpath: Int
    public var anchor: Int
    public init(subpath: Int, anchor: Int) { self.subpath = subpath; self.anchor = anchor }
}

public enum PathHandle: String, Codable, Sendable { case incoming, outgoing }

/// What a Direct Selection / Pen click landed on (local shape pixels).
public enum PathTarget: Equatable, Sendable {
    case handle(AnchorRef, PathHandle)
    case anchor(AnchorRef)
    /// Segment from anchor `segment` to the next, at cubic parameter `t`.
    case segment(subpath: Int, segment: Int, t: Double)
}

extension ShapePath {
    /// Segments of a subpath (closed subpaths include the closing segment).
    public func segments(of s: Int) -> [CubicSegment] {
        let sub = subpaths[s]
        let n = sub.anchors.count
        guard n >= 2 else { return [] }
        let count = sub.closed ? n : n - 1
        return (0..<count).map { i in
            let a = sub.anchors[i], b = sub.anchors[(i + 1) % n]
            return CubicSegment(a.point, a.outgoing, b.incoming, b.point)
        }
    }

    /// Polylines of every subpath at `tolerance` (closed flag kept).
    public func flattened(tolerance: Double = 0.25) -> [(points: [ShapePoint], closed: Bool)] {
        subpaths.indices.compactMap { s in
            guard let first = subpaths[s].anchors.first else { return nil }
            var pts = [first.point]
            for seg in segments(of: s) {
                let n = seg.steps(tolerance)
                for i in 1...n { pts.append(seg.point(Double(i) / Double(n))) }
            }
            if subpaths[s].closed, pts.count > 1, pts.last == pts.first { pts.removeLast() }
            return (pts, subpaths[s].closed)
        }
    }

    /// Winding number of `p` over every subpath, open ones implicitly closed (as fills treat them).
    public func winding(_ p: ShapePoint, tolerance: Double = 0.05) -> Int {
        var w = 0
        for (pts, _) in flattened(tolerance: tolerance) where pts.count >= 2 {
            for i in pts.indices {
                let a = pts[i], b = pts[(i + 1) % pts.count]
                if a.y <= p.y {
                    if b.y > p.y, (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y) > 0 { w += 1 }
                } else if b.y <= p.y, (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y) < 0 {
                    w -= 1
                }
            }
        }
        return w
    }

    /// Whether the fill covers `p` under the path's fill rule.
    public func contains(_ p: ShapePoint) -> Bool {
        let w = winding(p)
        return fillRule == .evenOdd ? w % 2 != 0 : w != 0
    }

    /// Bounds of the curves (flattened), nil for an empty path.
    public var bounds: CGRect? {
        let pts = flattened(tolerance: 0.05).flatMap(\.points)
        guard let f = pts.first else { return nil }
        var r = CGRect(origin: f.cgPoint, size: .zero)
        for p in pts.dropFirst() { r = r.union(CGRect(origin: p.cgPoint, size: .zero)) }
        return r
    }

    /// The path mapped through `t` (points and handles; affine maps keep cubics exact).
    public func transformed(_ t: AffineTransform2D) -> ShapePath {
        ShapePath(subpaths: subpaths.map { s in
            ShapeSubpath(anchors: s.anchors.map {
                ShapeAnchor(point: t.apply($0.point), incoming: t.apply($0.incoming), outgoing: t.apply($0.outgoing))
            }, closed: s.closed)
        }, fillRule: fillRule)
    }

    /// Consecutive anchors at exactly the same point merged (a square-cornered generated rectangle
    /// has two anchors per corner). `edit_shape_path` addresses anchors of this merged path.
    public var mergingCoincidentAnchors: ShapePath {
        var p = self
        for i in p.subpaths.indices {
            var out: [ShapeAnchor] = []
            for a in p.subpaths[i].anchors {
                if let last = out.last, last.point == a.point { out[out.count - 1].outgoing = a.outgoing } else { out.append(a) }
            }
            if p.subpaths[i].closed, out.count > 1, out[0].point == out[out.count - 1].point {
                let last = out.removeLast()
                out[0].incoming = last.incoming
            }
            p.subpaths[i].anchors = out
        }
        return p
    }

    public func anchor(_ r: AnchorRef) -> ShapeAnchor? {
        guard subpaths.indices.contains(r.subpath), subpaths[r.subpath].anchors.indices.contains(r.anchor) else { return nil }
        return subpaths[r.subpath].anchors[r.anchor]
    }

    /// The nearest segment within `radius`.
    public func nearestSegment(to p: ShapePoint, radius: Double) -> (subpath: Int, segment: Int, t: Double, distance: Double)? {
        var best: (Int, Int, Double, Double)?
        for s in subpaths.indices {
            for (i, seg) in segments(of: s).enumerated() {
                let n = seg.nearest(p)
                if n.distance <= radius, n.distance < (best?.3 ?? .infinity) { best = (s, i, n.t, n.distance) }
            }
        }
        return best.map { (subpath: $0.0, segment: $0.1, t: $0.2, distance: $0.3) }
    }

    /// Direct Selection / Pen target at `p`: handles of `selected` anchors first, then anchors, then
    /// segments (all within `radius`, local pixels).
    public func target(at p: ShapePoint, radius: Double, selected: Set<AnchorRef> = []) -> PathTarget? {
        var best: (PathTarget, Double)?
        for r in selected.sorted(by: { ($0.subpath, $0.anchor) < ($1.subpath, $1.anchor) }) {
            guard let a = anchor(r) else { continue }
            for (h, q) in [(PathHandle.incoming, a.incoming), (.outgoing, a.outgoing)] where q != a.point {
                let d = q.distance(to: p)
                if d <= radius, d < (best?.1 ?? .infinity) { best = (.handle(r, h), d) }
            }
        }
        if let best { return best.0 }
        for s in subpaths.indices {
            for (i, a) in subpaths[s].anchors.enumerated() {
                let d = a.point.distance(to: p)
                if d <= radius, d < (best?.1 ?? .infinity) { best = (.anchor(AnchorRef(subpath: s, anchor: i)), d) }
            }
        }
        if let best { return best.0 }
        return nearestSegment(to: p, radius: radius).map { .segment(subpath: $0.subpath, segment: $0.segment, t: $0.t) }
    }

    /// The anchor that splitting segment `segment` at `t` inserts (the engine's `insert_anchor`).
    public func splitting(subpath s: Int, segment i: Int, t: Double) -> ShapePath? {
        guard subpaths.indices.contains(s), t > 0, t < 1 else { return nil }
        let segs = segments(of: s)
        guard segs.indices.contains(i) else { return nil }
        let (l, r) = segs[i].split(t)
        var p = self
        let n = p.subpaths[s].anchors.count
        p.subpaths[s].anchors[i].outgoing = l.p1
        p.subpaths[s].anchors[(i + 1) % n].incoming = r.p2
        p.subpaths[s].anchors.insert(ShapeAnchor(point: l.p3, incoming: l.p2, outgoing: r.p1), at: i + 1)
        return p
    }

    /// Moves an anchor with its handles (the engine's `move_anchor`).
    public func movingAnchor(_ r: AnchorRef, to q: ShapePoint) -> ShapePath {
        guard let a = anchor(r) else { return self }
        var p = self
        let d = q - a.point
        p.subpaths[r.subpath].anchors[r.anchor] = ShapeAnchor(point: q, incoming: a.incoming + d, outgoing: a.outgoing + d)
        return p
    }

    /// Moves a handle; `mirror` keeps the opposite handle symmetric (the engine's `set_handle`).
    public func settingHandle(_ r: AnchorRef, _ h: PathHandle, to q: ShapePoint, mirror: Bool) -> ShapePath {
        guard var a = anchor(r) else { return self }
        let opposite = a.point * 2 - q
        switch h {
        case .incoming:
            a.incoming = q
            if mirror { a.outgoing = opposite }
        case .outgoing:
            a.outgoing = q
            if mirror { a.incoming = opposite }
        }
        var p = self
        p.subpaths[r.subpath].anchors[r.anchor] = a
        return p
    }
}

// MARK: - Path commands (`edit_shape_path`)

/// One `edit_shape_path` command; positions are absolute local pixels.
public enum PathCommand: Equatable, Sendable {
    case moveAnchor(AnchorRef, to: ShapePoint)
    case setHandle(AnchorRef, PathHandle, to: ShapePoint, mirror: Bool)
    case insertAnchor(subpath: Int, segment: Int, t: Double)
    case deleteAnchor(AnchorRef)
    case addSubpath(ShapeSubpath)
    case setClosed(subpath: Int, closed: Bool)
    case setFillRule(ShapeFillRule)
    case setPath(ShapePath)

    /// The command JSON (`{"op":"move_anchor",…}`).
    public var json: String { VectorJSON.string(Wire(self)) }
    public static func json(_ commands: [PathCommand]) -> String { VectorJSON.string(commands.map(Wire.init)) }

    private struct Wire: Encodable {
        let c: PathCommand
        init(_ c: PathCommand) { self.c = c }
        enum K: String, CodingKey { case op, subpath, anchor, x, y, handle, mirror, segment, t, closed, rule, path }
        func encode(to encoder: Encoder) throws {
            var k = encoder.container(keyedBy: K.self)
            switch c {
            case .moveAnchor(let r, let p):
                try k.encode("move_anchor", forKey: .op)
                try k.encode(r.subpath, forKey: .subpath); try k.encode(r.anchor, forKey: .anchor)
                try k.encode(p.x, forKey: .x); try k.encode(p.y, forKey: .y)
            case .setHandle(let r, let h, let p, let mirror):
                try k.encode("set_handle", forKey: .op)
                try k.encode(r.subpath, forKey: .subpath); try k.encode(r.anchor, forKey: .anchor)
                try k.encode(h.rawValue, forKey: .handle)
                try k.encode(p.x, forKey: .x); try k.encode(p.y, forKey: .y); try k.encode(mirror, forKey: .mirror)
            case .insertAnchor(let s, let i, let t):
                try k.encode("insert_anchor", forKey: .op)
                try k.encode(s, forKey: .subpath); try k.encode(i, forKey: .segment); try k.encode(t, forKey: .t)
            case .deleteAnchor(let r):
                try k.encode("delete_anchor", forKey: .op)
                try k.encode(r.subpath, forKey: .subpath); try k.encode(r.anchor, forKey: .anchor)
            case .addSubpath(let s):
                try k.encode("add_subpath", forKey: .op)
                try k.encode(s, forKey: .subpath)
            case .setClosed(let s, let closed):
                try k.encode("set_closed", forKey: .op)
                try k.encode(s, forKey: .subpath); try k.encode(closed, forKey: .closed)
            case .setFillRule(let r):
                try k.encode("set_fill_rule", forKey: .op)
                try k.encode(r, forKey: .rule)
            case .setPath(let p):
                try k.encode("set_path", forKey: .op)
                try k.encode(p, forKey: .path)
            }
        }
    }
}

// MARK: - Tool gestures

/// Live shapes from a canvas drag (document pixels; the new layer's transform is the identity, so
/// local = document at creation).
public enum ShapeToolMath {
    /// Rectangle / ellipse box: ⇧ square, ⌥ from the centre (the marquee rules).
    public static func box(start: CGPoint, current: CGPoint, square: Bool, fromCenter: Bool) -> ShapeRect {
        ShapeRect(SelectionModifiers.marqueeRect(start: start, current: current, square: square, fromCenter: fromCenter))
    }

    public static func rectangle(_ r: ShapeRect, cornerRadius: Double) -> LiveShape {
        .rectangle(rect: r, radii: Array(repeating: max(cornerRadius, 0), count: 4))
    }

    public static func ellipse(_ r: ShapeRect) -> LiveShape {
        .ellipse(center: ShapePoint(x: (r.x0 + r.x1) / 2, y: (r.y0 + r.y1) / 2), radii: ShapePoint(x: r.width / 2, y: r.height / 2))
    }

    /// Polygon / star dragged out from its centre: the pointer sets the radius and the direction of
    /// the first point (⇧ snaps the angle to 15°). `inset` is the star's inner radius as a fraction.
    public static func polygon(center: CGPoint, current: CGPoint, sides: Int, star: Bool, inset: Double, snap: Bool) -> LiveShape {
        let dx = Double(current.x - center.x), dy = Double(current.y - center.y)
        var angle = atan2(dy, dx)
        if snap { angle = (angle / (.pi / 12)).rounded() * (.pi / 12) }
        let radius = hypot(dx, dy)
        let n = UInt32(min(max(sides, 3), 100))
        return .polygon(center: ShapePoint(center), radius: radius, sides: n, rotation: angle,
                        innerRadius: star ? radius * min(max(inset, 0.01), 1) : nil)
    }

    /// A line; ⇧ snaps to 45°.
    public static func line(start: CGPoint, current: CGPoint, snap: Bool) -> LiveShape {
        var end = ShapePoint(current)
        if snap {
            let dx = Double(current.x - start.x), dy = Double(current.y - start.y)
            let a = (atan2(dy, dx) / (.pi / 4)).rounded() * (.pi / 4)
            let len = hypot(dx, dy)
            end = ShapePoint(x: Double(start.x) + cos(a) * len, y: Double(start.y) + sin(a) * len)
        }
        return .line(start: ShapePoint(start), end: end)
    }

    /// Whether a drag is big enough to make a shape (a click is not).
    public static func isShape(_ s: LiveShape) -> Bool {
        switch s {
        case .rectangle(let r, _): r.width >= 1 && r.height >= 1
        case .ellipse(_, let r): r.x >= 0.5 && r.y >= 0.5
        case .polygon(_, let radius, _, _, _): radius >= 1
        case .line(let a, let b): a.distance(to: b) >= 1
        case .custom(let p): p.anchorCount >= 2
        }
    }
}

/// The Pen's path under construction (host state until it is committed as one Add Shape): clicks
/// add corner anchors, a drag pulls symmetric handles (⌥ breaks them), a click on the first anchor
/// closes the path.
public struct PenDraft: Equatable, Sendable {
    public private(set) var anchors: [ShapeAnchor] = []
    public private(set) var closed = false
    public init() {}

    public var isEmpty: Bool { anchors.isEmpty }
    /// Whether `p` would close the path (on the first anchor, with two or more anchors).
    public func closes(at p: ShapePoint, radius: Double) -> Bool {
        anchors.count >= 2 && anchors[0].point.distance(to: p) <= radius
    }

    /// Mouse-down. Returns true when the click closed the path.
    @discardableResult
    public mutating func click(_ p: ShapePoint, radius: Double) -> Bool {
        guard !closed else { return true }
        if closes(at: p, radius: radius) { closed = true; return true }
        anchors.append(.corner(p))
        return false
    }

    /// Mouse-dragged after a click: the outgoing handle follows the pointer; the incoming one mirrors
    /// it unless `independent` (⌥). On a closing click the first anchor's handles are pulled.
    public mutating func drag(to q: ShapePoint, independent: Bool) {
        guard !anchors.isEmpty else { return }
        let i = closed ? 0 : anchors.count - 1
        var a = anchors[i]
        a.outgoing = q
        if !independent { a.incoming = a.point * 2 - q }
        anchors[i] = a
    }

    public mutating func removeLast() {
        if closed { closed = false } else if !anchors.isEmpty { anchors.removeLast() }
    }

    public var path: ShapePath { ShapePath(subpaths: anchors.isEmpty ? [] : [ShapeSubpath(anchors: anchors, closed: closed)]) }
    /// Enough to become a shape layer.
    public var isCommittable: Bool { anchors.count >= 2 }
}
