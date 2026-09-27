import Foundation

// Editable vector shapes (WP B5-11): Codable mirrors of the vector crate's serde JSON
// (crates/vector: `ShapeModel`, `Path`, `Shape`, `Fill`, `Stroke`), exactly as the engine reads and
// writes it (`model_json` of `shape_layer`, `path_json` of vector masks). Rust enums are externally
// tagged (`{"Solid":[r,g,b,a]}`), the stroke is a two-element array `[stroke, paint]`, points are
// `{"x":…,"y":…}` and rectangles `{"x0","y0","x1","y1"}`. Pattern paints are carried opaquely so an
// edit of anything else preserves them byte-for-byte in meaning. Shape geometry is in local level-0
// pixels; vector masks and paint coordinates (gradient ends) are in document pixels.

/// A 2-D point (`kurbo::Point` / `kurbo::Vec2`).
public struct ShapePoint: Codable, Equatable, Hashable, Sendable {
    public var x: Double
    public var y: Double
    public init(x: Double, y: Double) { self.x = x; self.y = y }
    public init(_ p: CGPoint) { self.init(x: Double(p.x), y: Double(p.y)) }
    public var cgPoint: CGPoint { CGPoint(x: x, y: y) }
    public static func + (a: ShapePoint, b: ShapePoint) -> ShapePoint { ShapePoint(x: a.x + b.x, y: a.y + b.y) }
    public static func - (a: ShapePoint, b: ShapePoint) -> ShapePoint { ShapePoint(x: a.x - b.x, y: a.y - b.y) }
    public static func * (a: ShapePoint, k: Double) -> ShapePoint { ShapePoint(x: a.x * k, y: a.y * k) }
    public func distance(to o: ShapePoint) -> Double { hypot(x - o.x, y - o.y) }
    public func lerp(_ o: ShapePoint, _ t: Double) -> ShapePoint { ShapePoint(x: x + (o.x - x) * t, y: y + (o.y - y) * t) }
    public var isFinite: Bool { x.isFinite && y.isFinite }
}

/// `kurbo::Rect`.
public struct ShapeRect: Codable, Equatable, Sendable {
    public var x0: Double, y0: Double, x1: Double, y1: Double
    public init(x0: Double, y0: Double, x1: Double, y1: Double) { self.x0 = x0; self.y0 = y0; self.x1 = x1; self.y1 = y1 }
    public init(_ r: CGRect) { self.init(x0: Double(r.minX), y0: Double(r.minY), x1: Double(r.maxX), y1: Double(r.maxY)) }
    public var cgRect: CGRect { CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0) }
    public var width: Double { x1 - x0 }
    public var height: Double { y1 - y0 }
}

/// `vector::Anchor`: a point with its incoming and outgoing cubic control points (straight
/// segments have handles on the point).
public struct ShapeAnchor: Codable, Equatable, Sendable {
    public var point: ShapePoint
    public var incoming: ShapePoint
    public var outgoing: ShapePoint
    public init(point: ShapePoint, incoming: ShapePoint, outgoing: ShapePoint) {
        self.point = point; self.incoming = incoming; self.outgoing = outgoing
    }
    public static func corner(_ p: ShapePoint) -> ShapeAnchor { ShapeAnchor(point: p, incoming: p, outgoing: p) }
    public var isCorner: Bool { incoming == point && outgoing == point }
}

public struct ShapeSubpath: Codable, Equatable, Sendable {
    public var anchors: [ShapeAnchor]
    public var closed: Bool
    public init(anchors: [ShapeAnchor], closed: Bool) { self.anchors = anchors; self.closed = closed }
}

public enum ShapeFillRule: String, Codable, CaseIterable, Sendable {
    case evenOdd = "EvenOdd"
    case nonZero = "NonZero"
    public var title: String { self == .evenOdd ? "Even-odd" : "Nonzero" }
}

/// `vector::Path`.
public struct ShapePath: Codable, Equatable, Sendable {
    public var subpaths: [ShapeSubpath]
    public var fillRule: ShapeFillRule
    enum CodingKeys: String, CodingKey { case subpaths, fillRule = "fill_rule" }
    public init(subpaths: [ShapeSubpath] = [], fillRule: ShapeFillRule = .nonZero) {
        self.subpaths = subpaths; self.fillRule = fillRule
    }
    /// A closed or open polyline of corner anchors.
    public static func polyline(_ points: [ShapePoint], closed: Bool) -> ShapePath {
        ShapePath(subpaths: [ShapeSubpath(anchors: points.map(ShapeAnchor.corner), closed: closed)])
    }
    public var anchorCount: Int { subpaths.reduce(0) { $0 + $1.anchors.count } }
    public var hasOpenSubpaths: Bool { subpaths.contains { !$0.closed } }
}

/// `vector::Shape`: live construction parameters.
public enum LiveShape: Equatable, Sendable {
    /// `radii` are top-left, top-right, bottom-right, bottom-left.
    case rectangle(rect: ShapeRect, radii: [Double])
    case ellipse(center: ShapePoint, radii: ShapePoint)
    /// `innerRadius` makes a star (2 × sides points alternating radius and inner radius).
    case polygon(center: ShapePoint, radius: Double, sides: UInt32, rotation: Double, innerRadius: Double?)
    case line(start: ShapePoint, end: ShapePoint)
    case custom(ShapePath)

    /// The engine's `live_kind` string.
    public var kind: String {
        switch self {
        case .rectangle: "rectangle"
        case .ellipse: "ellipse"
        case .polygon: "polygon"
        case .line: "line"
        case .custom: "custom"
        }
    }
}

extension LiveShape: Codable {
    private struct Key: CodingKey {
        var stringValue: String
        var intValue: Int? { nil }
        init(_ s: String) { stringValue = s }
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { nil }
    }
    private struct Rectangle: Codable { var rect: ShapeRect; var radii: [Double] }
    private struct Ellipse: Codable { var center: ShapePoint; var radii: ShapePoint }
    private struct Polygon: Codable {
        var center: ShapePoint; var radius: Double; var sides: UInt32; var rotation: Double; var inner_radius: Double?
    }
    private struct Line: Codable { var start: ShapePoint; var end: ShapePoint }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        guard let k = c.allKeys.first, c.allKeys.count == 1 else {
            throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "one shape variant expected"))
        }
        switch k.stringValue {
        case "Rectangle":
            let r = try c.decode(Rectangle.self, forKey: k)
            self = .rectangle(rect: r.rect, radii: r.radii)
        case "Ellipse":
            let e = try c.decode(Ellipse.self, forKey: k)
            self = .ellipse(center: e.center, radii: e.radii)
        case "Polygon":
            let p = try c.decode(Polygon.self, forKey: k)
            self = .polygon(center: p.center, radius: p.radius, sides: p.sides, rotation: p.rotation, innerRadius: p.inner_radius)
        case "Line":
            let l = try c.decode(Line.self, forKey: k)
            self = .line(start: l.start, end: l.end)
        case "Custom":
            self = .custom(try c.decode(ShapePath.self, forKey: k))
        default:
            throw DecodingError.dataCorruptedError(forKey: k, in: c, debugDescription: "unknown shape \(k.stringValue)")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Key.self)
        switch self {
        case .rectangle(let rect, let radii): try c.encode(Rectangle(rect: rect, radii: radii), forKey: Key("Rectangle"))
        case .ellipse(let center, let radii): try c.encode(Ellipse(center: center, radii: radii), forKey: Key("Ellipse"))
        case .polygon(let center, let radius, let sides, let rotation, let inner):
            try c.encode(Polygon(center: center, radius: radius, sides: sides, rotation: rotation, inner_radius: inner),
                         forKey: Key("Polygon"))
        case .line(let start, let end): try c.encode(Line(start: start, end: end), forKey: Key("Line"))
        case .custom(let p): try c.encode(p, forKey: Key("Custom"))
        }
    }
}

/// Any JSON value, kept opaque (imported pattern paints).
public indirect enum JSONValue: Codable, Equatable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let b = try? c.decode(Bool.self) { self = .bool(b) }
        else if let n = try? c.decode(Double.self) { self = .number(n) }
        else if let s = try? c.decode(String.self) { self = .string(s) }
        else if let a = try? c.decode([JSONValue].self) { self = .array(a) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .number(let n):
            // Integral values stay integral (`u32` fields such as a pattern's width).
            if n == n.rounded(), abs(n) < 1e15 { try c.encode(Int64(n)) } else { try c.encode(n) }
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }
}

public enum ShapeGradientKind: String, Codable, CaseIterable, Sendable {
    case linear = "Linear", radial = "Radial", angle = "Angle", reflected = "Reflected", diamond = "Diamond"
    public var title: String { rawValue }
}

public struct ShapeGradientStop: Codable, Equatable, Sendable {
    public var position: Double
    /// Straight RGBA 0…1 in the document's sample space.
    public var color: [Double]
    public init(position: Double, color: [Double]) { self.position = position; self.color = color }
}

/// `vector::Gradient`: `start` / `end` are document pixels (the paint does not move with the shape).
public struct ShapeGradient: Codable, Equatable, Sendable {
    public var kind: ShapeGradientKind
    public var start: ShapePoint
    public var end: ShapePoint
    public var stops: [ShapeGradientStop]
    public var dither: Bool
    public init(kind: ShapeGradientKind, start: ShapePoint, end: ShapePoint, stops: [ShapeGradientStop], dither: Bool = false) {
        self.kind = kind; self.start = start; self.end = end; self.stops = stops; self.dither = dither
    }
    /// The engine's validation: distinct ends, ≥ 2 strictly increasing stops in 0…1, colours in 0…1.
    public var isValid: Bool {
        start.isFinite && end.isFinite && start.distance(to: end) > .ulpOfOne && stops.count >= 2
            && stops.allSatisfy { (0...1).contains($0.position) && $0.color.count == 4 && $0.color.allSatisfy { (0...1).contains($0) } }
            && zip(stops, stops.dropFirst()).allSatisfy { $0.position < $1.position }
    }
}

/// `vector::Fill`.
public enum ShapePaint: Equatable, Sendable {
    case solid([Double])
    case gradient(ShapeGradient)
    /// Imported pattern, kept as-is (PSD export of pattern shape fills is an engine limitation).
    case pattern(JSONValue)

    public var title: String {
        switch self {
        case .solid: "Solid"
        case .gradient: "Gradient"
        case .pattern: "Pattern"
        }
    }
    public var isPattern: Bool { if case .pattern = self { true } else { false } }
    /// The colour a solid paint uses, or the first gradient stop's.
    public var representativeColor: [Double] {
        switch self {
        case .solid(let c): c
        case .gradient(let g): g.stops.first?.color ?? [0, 0, 0, 1]
        case .pattern: [0.5, 0.5, 0.5, 1]
        }
    }
}

extension ShapePaint: Codable {
    private enum Key: String, CodingKey { case solid = "Solid", gradient = "Gradient", pattern = "Pattern" }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        if let v = try c.decodeIfPresent([Double].self, forKey: .solid) { self = .solid(v) }
        else if let g = try c.decodeIfPresent(ShapeGradient.self, forKey: .gradient) { self = .gradient(g) }
        else if let p = try c.decodeIfPresent(JSONValue.self, forKey: .pattern) { self = .pattern(p) }
        else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "unknown paint")) }
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Key.self)
        switch self {
        case .solid(let v): try c.encode(v, forKey: .solid)
        case .gradient(let g): try c.encode(g, forKey: .gradient)
        case .pattern(let p): try c.encode(p, forKey: .pattern)
        }
    }
}

public enum ShapeStrokeAlignment: String, Codable, CaseIterable, Sendable {
    case inside = "Inside", center = "Center", outside = "Outside"
    public var title: String { rawValue }
}

public enum ShapeLineCap: String, Codable, CaseIterable, Sendable {
    case butt = "Butt", square = "Square", round = "Round"
    public var title: String { rawValue }
}

public enum ShapeLineJoin: String, Codable, CaseIterable, Sendable {
    case miter = "Miter", miterClip = "MiterClip", round = "Round", bevel = "Bevel"
    public var title: String { self == .miterClip ? "Miter clip" : rawValue }
}

/// `vector::Stroke`: width and dashes in local shape pixels.
public struct ShapeStroke: Codable, Equatable, Sendable {
    public var width: Double = 1
    public var alignment: ShapeStrokeAlignment = .center
    public var dashes: [Double] = []
    public var dashOffset: Double = 0
    public var cap: ShapeLineCap = .butt
    public var join: ShapeLineJoin = .miter
    public var miterLimit: Double = 4
    enum CodingKeys: String, CodingKey {
        case width, alignment, dashes, dashOffset = "dash_offset", cap, join, miterLimit = "miter_limit"
    }
    public init(width: Double = 1, alignment: ShapeStrokeAlignment = .center, dashes: [Double] = [], dashOffset: Double = 0,
                cap: ShapeLineCap = .butt, join: ShapeLineJoin = .miter, miterLimit: Double = 4) {
        self.width = width; self.alignment = alignment; self.dashes = dashes; self.dashOffset = dashOffset
        self.cap = cap; self.join = join; self.miterLimit = miterLimit
    }
    /// The engine's validation (`Stroke::validate`).
    public var isValid: Bool {
        width.isFinite && width >= 0 && miterLimit.isFinite && miterLimit >= 1 && dashOffset.isFinite
            && dashes.allSatisfy { $0.isFinite && $0 > 0 }
    }
}

/// `vector::ShapeModel`: `path` is the rendered geometry; `liveShape` regenerates it whenever present.
public struct ShapeSource: Equatable, Sendable {
    public var path: ShapePath
    public var fill: ShapePaint?
    public var stroke: (ShapeStroke, ShapePaint)?
    public var liveShape: LiveShape?

    public init(path: ShapePath, fill: ShapePaint?, stroke: (ShapeStroke, ShapePaint)?, liveShape: LiveShape?) {
        self.path = path; self.fill = fill; self.stroke = stroke; self.liveShape = liveShape
    }

    /// A live source with its generated path (what the engine regenerates on add / edit).
    public init(live: LiveShape, fill: ShapePaint?, stroke: (ShapeStroke, ShapePaint)?) {
        self.init(path: ShapePrimitives.path(live), fill: fill, stroke: stroke, liveShape: live)
    }

    public static func == (a: ShapeSource, b: ShapeSource) -> Bool {
        a.path == b.path && a.fill == b.fill && a.liveShape == b.liveShape
            && a.stroke?.0 == b.stroke?.0 && a.stroke?.1 == b.stroke?.1 && (a.stroke == nil) == (b.stroke == nil)
    }

    /// Inside / Outside strokes need closed paths (the engine rejects them on open paths).
    public var strokeAlignmentProblem: String? {
        guard let s = stroke?.0, s.alignment != .center else { return nil }
        let geometry = liveShape.map(ShapePrimitives.path) ?? path
        return geometry.hasOpenSubpaths
            ? "Inside and Outside alignment need a closed path; this path is open (use Center for lines and open paths)"
            : nil
    }

    /// The same source after an arbitrary path edit: the primitive is dropped so it cannot
    /// regenerate over the custom geometry.
    public func withCustomPath(_ p: ShapePath) -> ShapeSource {
        ShapeSource(path: p, fill: fill, stroke: stroke, liveShape: nil)
    }

    public var json: String { (try? String(data: VectorJSON.encoder.encode(self), encoding: .utf8)) ?? "{}" }
    public init(json: String) throws { self = try VectorJSON.decoder.decode(ShapeSource.self, from: Data(json.utf8)) }
}

extension ShapeSource: Codable {
    enum CodingKeys: String, CodingKey { case path, fill, stroke, liveShape = "live_shape" }
    private struct StrokePair: Codable {
        var stroke: ShapeStroke
        var paint: ShapePaint
        init(_ s: ShapeStroke, _ p: ShapePaint) { stroke = s; paint = p }
        init(from decoder: Decoder) throws {
            var c = try decoder.unkeyedContainer()
            stroke = try c.decode(ShapeStroke.self)
            paint = try c.decode(ShapePaint.self)
        }
        func encode(to encoder: Encoder) throws {
            var c = encoder.unkeyedContainer()
            try c.encode(stroke)
            try c.encode(paint)
        }
    }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        path = try c.decode(ShapePath.self, forKey: .path)
        fill = try c.decodeIfPresent(ShapePaint.self, forKey: .fill)
        stroke = try c.decodeIfPresent(StrokePair.self, forKey: .stroke).map { ($0.stroke, $0.paint) }
        liveShape = try c.decodeIfPresent(LiveShape.self, forKey: .liveShape)
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(path, forKey: .path)
        if let fill { try c.encode(fill, forKey: .fill) } else { try c.encodeNil(forKey: .fill) }
        if let stroke { try c.encode(StrokePair(stroke.0, stroke.1), forKey: .stroke) } else { try c.encodeNil(forKey: .stroke) }
        if let liveShape { try c.encode(liveShape, forKey: .liveShape) } else { try c.encodeNil(forKey: .liveShape) }
    }
}

/// Shared coders: sorted keys keep the JSON deterministic for tests and history comparisons.
public enum VectorJSON {
    public static var encoder: JSONEncoder {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys]
        return e
    }
    public static var decoder: JSONDecoder { JSONDecoder() }
    public static func string<T: Encodable>(_ v: T) -> String {
        (try? String(data: encoder.encode(v), encoding: .utf8)) ?? "null"
    }
}
