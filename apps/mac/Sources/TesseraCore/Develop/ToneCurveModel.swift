import Foundation

/// One knot of a point curve, both coordinates on the engine's log tone axis in `[0, 1]`.
public struct CurveKnot: Equatable, Sendable {
    public var x: Double
    public var y: Double
    public init(_ x: Double, _ y: Double) { self.x = x; self.y = y }
}

/// Point-curve editing model. The engine accepts knots with strictly increasing x and
/// nondecreasing y in [0, 1] (pipeline-cpu TONE_M2.md) and evaluates a Fritsch–Carlson
/// monotone cubic; every edit here keeps those invariants, so the editor can never produce a
/// curve the renderer refuses. Endpoints are always present and keep x = 0 / x = 1.
public struct PointCurve: Equatable, Sendable {
    public private(set) var knots: [CurveKnot]
    /// Minimum horizontal distance between neighbouring knots.
    public static let minGap = 0.01
    public static let identity = PointCurve()

    public init() { knots = [CurveKnot(0, 0), CurveKnot(1, 1)] }

    /// From the engine JSON array (`[{"x":…,"y":…}]`); missing endpoints are added like the engine does.
    public init(json: Any?) {
        var k = ((json as? [[String: Any]]) ?? []).compactMap { d -> CurveKnot? in
            guard let x = (d["x"] as? NSNumber)?.doubleValue, let y = (d["y"] as? NSNumber)?.doubleValue,
                  x.isFinite, y.isFinite else { return nil }
            return CurveKnot(min(max(x, 0), 1), min(max(y, 0), 1))
        }.sorted { $0.x < $1.x }
        if k.first.map({ $0.x > 0 }) ?? true { k.insert(CurveKnot(0, 0), at: 0) }
        if k.last!.x < 1 { k.append(CurveKnot(1, 1)) }
        knots = []
        for p in k where knots.last.map({ p.x - $0.x >= Self.minGap / 2 }) ?? true {
            knots.append(CurveKnot(p.x, max(p.y, knots.last?.y ?? 0)))
        }
        if knots.count < 2 { self = .identity }
    }

    public var isIdentity: Bool { knots.allSatisfy { $0.x == $0.y } }

    /// Engine JSON: empty for identity (the renderable default keeps the fast path).
    public var json: [[String: Double]] {
        isIdentity ? [] : knots.map { ["x": Self.round($0.x), "y": Self.round($0.y)] }
    }

    private static func round(_ v: Double) -> Double { (v * 10000).rounded() / 10000 }

    /// Every invariant the engine checks.
    public var isValid: Bool {
        knots.count >= 2 && knots.first!.x == 0 && knots.last!.x == 1
            && knots.allSatisfy { (0...1).contains($0.x) && (0...1).contains($0.y) }
            && zip(knots, knots.dropFirst()).allSatisfy { $0.x < $1.x && $0.y <= $1.y }
    }

    /// Adds a knot on or near the curve at `x`, returning its index (nil if too close to one).
    @discardableResult
    public mutating func insert(x: Double, y: Double? = nil) -> Int? {
        let x = min(max(x, Self.minGap), 1 - Self.minGap)
        guard let i = knots.firstIndex(where: { $0.x > x }), x - knots[i - 1].x >= Self.minGap,
              knots[i].x - x >= Self.minGap else { return nil }
        let target = y ?? evaluate(x)
        knots.insert(CurveKnot(x, min(max(target, knots[i - 1].y), knots[i].y)), at: i)
        return i
    }

    /// Moves knot `i`, clamped between its neighbours (x strictly, y monotone). Endpoints move vertically only.
    public mutating func move(_ i: Int, to p: CurveKnot) {
        guard knots.indices.contains(i) else { return }
        var x = p.x
        if i == 0 { x = 0 } else if i == knots.count - 1 { x = 1 } else {
            x = min(max(x, knots[i - 1].x + Self.minGap), knots[i + 1].x - Self.minGap)
        }
        let lo = i > 0 ? knots[i - 1].y : 0
        let hi = i < knots.count - 1 ? knots[i + 1].y : 1
        knots[i] = CurveKnot(x, min(max(p.y, lo), hi))
    }

    /// Keyboard nudge.
    public mutating func nudge(_ i: Int, dx: Double, dy: Double) {
        guard knots.indices.contains(i) else { return }
        move(i, to: CurveKnot(knots[i].x + dx, knots[i].y + dy))
    }

    /// Removes an interior knot.
    public mutating func remove(_ i: Int) {
        guard i > 0, i < knots.count - 1 else { return }
        knots.remove(at: i)
    }

    /// Index of the knot within `radius` of `p`, if any.
    public func hit(_ p: CurveKnot, radius: Double) -> Int? {
        knots.indices.min { hypot(knots[$0].x - p.x, knots[$0].y - p.y) < hypot(knots[$1].x - p.x, knots[$1].y - p.y) }
            .flatMap { hypot(knots[$0].x - p.x, knots[$0].y - p.y) <= radius ? $0 : nil }
    }

    /// The engine's monotone spline (pipeline-cpu `Spline::eval`) at `x` in [0, 1].
    public func evaluate(_ x: Double) -> Double {
        if isIdentity { return x }
        let p = knots
        let d = zip(p, p.dropFirst()).map { ($1.y - $0.y) / ($1.x - $0.x) }
        var m = [Double](repeating: 0, count: p.count)
        m[0] = d[0]
        m[p.count - 1] = d[d.count - 1]
        for i in 1..<(p.count - 1) { m[i] = 0.5 * d[i - 1] + 0.5 * d[i] }
        for i in d.indices {
            if d[i] == 0 { m[i] = 0; m[i + 1] = 0; continue }
            m[i] = min(m[i], 3 * d[i]); m[i + 1] = min(m[i + 1], 3 * d[i])
            let a = m[i] / d[i], b = m[i + 1] / d[i], r = hypot(a, b)
            if r > 3 { m[i] = 3 * a / r * d[i]; m[i + 1] = 3 * b / r * d[i] }
        }
        if x >= 1 { return x + p[p.count - 1].y - 1 }
        if x <= 0 { return p[0].y + x }
        let i = max((p.firstIndex { $0.x > x } ?? p.count) - 1, 0)
        let (x0, y0, x1, y1) = (p[i].x, p[i].y, p[i + 1].x, p[i + 1].y)
        if y0 == y1 { return y0 }
        let h = x1 - x0, t = (x - x0) / h
        let s = t * t * (3 - 2 * t)
        let v = y0 + (y1 - y0) * s + t * (1 - t) * (1 - t) * h * m[i] - t * t * (1 - t) * h * m[i + 1]
        return min(max(v, y0), y1)
    }

    /// Named starting points (docs/01 §2.3 curve presets).
    public static let presets: [(String, PointCurve)] = [
        ("Linear", .identity),
        ("Medium Contrast", PointCurve(json: [["x": 0.25, "y": 0.21], ["x": 0.75, "y": 0.80]])),
        ("Strong Contrast", PointCurve(json: [["x": 0.25, "y": 0.17], ["x": 0.75, "y": 0.84]])),
    ]
}

/// The engine's native parametric curve on the log tone axis (pipeline-cpu `Parametric::eval`):
/// in each split interval `[a,b]`, `z' = z + 3 s (b−a) u² (1−u)²`.
public struct ParametricCurveModel: Equatable, Sendable {
    /// Region amounts in −100…100: shadows, darks, lights, highlights.
    public var amounts: [Double]
    /// Split points in (0, 100), strictly increasing.
    public var splits: [Double]

    public init(amounts: [Double] = [0, 0, 0, 0], splits: [Double] = [25, 50, 75]) {
        self.amounts = amounts
        self.splits = splits
    }

    public func evaluate(_ z: Double) -> Double {
        guard (0..<1).contains(z), amounts.contains(where: { $0 != 0 }) else { return z }
        let edges = [0] + splits.map { $0 / 100 } + [1]
        let i = min(max((edges.firstIndex { $0 > z } ?? edges.count) - 1, 0), 3)
        let w = edges[i + 1] - edges[i], u = (z - edges[i]) / w
        return z + 3 * min(max(amounts[i], -100), 100) / 100 * w * u * u * (1 - u) * (1 - u)
    }

    /// Moves split `index` (0…2) keeping `0 < s0 < s1 < s2 < 100` with a small gap.
    public mutating func setSplit(_ index: Int, _ value: Double) {
        let lo = index == 0 ? 1 : splits[index - 1] + 1
        let hi = index == 2 ? 99 : splits[index + 1] - 1
        splits[index] = min(max(value.rounded(), lo), hi)
    }
}

/// The engine's log tone axis: `E(Y) = ln(1 + Y/.18) / ln(1 + 1/.18)`.
public enum ToneAxis {
    private static let k = log(1 + 1 / 0.18)
    public static func encode(_ y: Double) -> Double { log(1 + max(y, 0) / 0.18) / k }
    public static func decode(_ z: Double) -> Double { 0.18 * expm1(z * k) }
}
