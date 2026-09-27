import CoreGraphics
import Foundation

// Warp, Perspective Warp, Puppet Warp and Content-Aware Scale (WP B5-12): host models of the engine's
// versioned `transform::TransformOp` JSON (crates/transform), in the smart object's CHILD level-0
// pixels (pixel edges; the first pixel centre is (0.5, 0.5)). Everything here is UI-free and testable:
// Bézier evaluation for the warp net, the perspective grid with linked vertices, puppet pins, the
// content-aware scale box, and the child ↔ document mapping. The engine validates every preview; these
// models only pre-check what a drag can break so a rejected edit never replaces the previous preview.

/// `transform::Kernel` serde names.
public enum TransformKernel: String, CaseIterable, Codable, Sendable, Identifiable {
    case nearest = "Nearest", bilinear = "Bilinear", bicubic = "Bicubic", lanczos3 = "Lanczos3", automatic = "Automatic"
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .nearest: "Nearest Neighbor"
        case .bilinear: "Bilinear"
        case .bicubic: "Bicubic"
        case .lanczos3: "Lanczos-3"
        case .automatic: "Automatic"
        }
    }
}

/// Which advanced transform a session edits.
public enum AdvancedTransformTag: String, CaseIterable, Sendable, Identifiable {
    case warp, perspective, puppet, contentAwareScale, free, displacement
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .warp: "Warp"
        case .perspective: "Perspective Warp"
        case .puppet: "Puppet Warp"
        case .contentAwareScale: "Content-Aware Scale"
        case .free: "Transform"
        case .displacement: "Displacement"
        }
    }
    public var symbol: String {
        switch self {
        case .warp: "square.grid.3x3"
        case .perspective: "perspective"
        case .puppet: "figure.walk"
        case .contentAwareScale: "arrow.left.and.right.square"
        case .free: "arrow.up.left.and.arrow.down.right"
        case .displacement: "circle.grid.cross"
        }
    }
    public var editable: Bool { [.warp, .perspective, .puppet, .contentAwareScale].contains(self) }
}

// MARK: - JSON points

private struct PointPair: Codable, Equatable {
    var x: Double, y: Double
    init(_ p: CGPoint) { x = Double(p.x); y = Double(p.y) }
    var point: CGPoint { CGPoint(x: CGFloat(x), y: CGFloat(y)) }
    init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        x = try c.decode(Double.self)
        y = try c.decode(Double.self)
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        try c.encode(x)
        try c.encode(y)
    }
}

private func encoder() -> JSONEncoder {
    let e = JSONEncoder()
    e.outputFormatting = [.sortedKeys]
    return e
}

// MARK: - Warp

/// `transform::warp::WarpMesh`: a piecewise bicubic Bézier surface. `controlPoints` is row-major
/// (`3·(vSplits.count − 1) + 1` rows of `3·(uSplits.count − 1) + 1` points); rows/columns at multiples of
/// three are patch anchors, the others tangent handles. The source domain is `[0, width] × [0, height]`.
public struct WarpMeshModel: Codable, Equatable, Sendable {
    public var width: Double
    public var height: Double
    public var controlPoints: [[CGPoint]]
    public var uSplits: [Double]
    public var vSplits: [Double]

    enum CodingKeys: String, CodingKey {
        case width, height, controlPoints = "control_points", uSplits = "u_splits", vSplits = "v_splits"
    }

    public init(width: Double, height: Double, controlPoints: [[CGPoint]], uSplits: [Double], vSplits: [Double]) {
        self.width = width; self.height = height; self.controlPoints = controlPoints
        self.uSplits = uSplits; self.vSplits = vSplits
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        width = try c.decode(Double.self, forKey: .width)
        height = try c.decode(Double.self, forKey: .height)
        controlPoints = try c.decode([[PointPair]].self, forKey: .controlPoints).map { $0.map(\.point) }
        uSplits = try c.decode([Double].self, forKey: .uSplits)
        vSplits = try c.decode([Double].self, forKey: .vSplits)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(width, forKey: .width)
        try c.encode(height, forKey: .height)
        try c.encode(controlPoints.map { $0.map(PointPair.init) }, forKey: .controlPoints)
        try c.encode(uSplits, forKey: .uSplits)
        try c.encode(vSplits, forKey: .vSplits)
    }

    public init?(json: String) {
        guard let m = try? JSONDecoder().decode(WarpMeshModel.self, from: Data(json.utf8)) else { return nil }
        self = m
    }

    public var json: String { (try? String(decoding: encoder().encode(self), as: UTF8.self)) ?? "{}" }

    /// The identity mesh over `width × height` (one patch).
    public static func identity(width: Double, height: Double) -> WarpMeshModel {
        let pts = (0..<4).map { y in
            (0..<4).map { x in CGPoint(x: CGFloat(width * Double(x) / 3), y: CGFloat(height * Double(y) / 3)) }
        }
        return WarpMeshModel(width: width, height: height, controlPoints: pts, uSplits: [0, 1], vSplits: [0, 1])
    }

    public var rows: Int { controlPoints.count }
    public var columns: Int { controlPoints.first?.count ?? 0 }
    public var isStructurallyValid: Bool {
        uSplits.count >= 2 && vSplits.count >= 2 && rows == 3 * (vSplits.count - 1) + 1
            && controlPoints.allSatisfy { $0.count == 3 * (uSplits.count - 1) + 1 }
    }

    public static func isAnchor(row: Int, column: Int) -> Bool { row % 3 == 0 && column % 3 == 0 }

    /// The surface point at normalized `(u, v)` (destination child pixels).
    public func point(u: Double, v: Double) -> CGPoint {
        func cell(_ s: [Double], _ t: Double) -> (Int, Double) {
            var i = 0
            while i < s.count - 2, t > s[i + 1] { i += 1 }
            let span = s[i + 1] - s[i]
            return (i, span > 0 ? min(max((t - s[i]) / span, 0), 1) : 0)
        }
        let (i, s) = cell(uSplits, u)
        let (j, t) = cell(vSplits, v)
        func b(_ t: Double) -> [Double] {
            let m = 1 - t
            return [m * m * m, 3 * t * m * m, 3 * t * t * m, t * t * t]
        }
        let bu = b(s), bv = b(t)
        var x = 0.0, y = 0.0
        for r in 0..<4 {
            for c in 0..<4 {
                let p = controlPoints[3 * j + r][3 * i + c]
                let w = bv[r] * bu[c]
                x += w * Double(p.x)
                y += w * Double(p.y)
            }
        }
        return CGPoint(x: CGFloat(x), y: CGFloat(y))
    }

    /// The net's iso-curves (every split line, both directions), `samples` points per patch edge.
    public func gridLines(samples: Int = 16) -> [[CGPoint]] {
        func along(_ knots: [Double]) -> [Double] {
            var out: [Double] = []
            for k in 0..<(knots.count - 1) {
                for s in 0..<samples { out.append(knots[k] + (knots[k + 1] - knots[k]) * Double(s) / Double(samples)) }
            }
            out.append(1)
            return out
        }
        let us = along(uSplits), vs = along(vSplits)
        var lines: [[CGPoint]] = []
        for v in vSplits { lines.append(us.map { point(u: $0, v: v) }) }
        for u in uSplits { lines.append(vs.map { point(u: u, v: $0) }) }
        return lines
    }

    /// Handle segments (anchor → its tangent handles).
    public func handleSegments() -> [(CGPoint, CGPoint)] {
        var out: [(CGPoint, CGPoint)] = []
        for r in 0..<rows {
            for c in 0..<columns where !Self.isAnchor(row: r, column: c) {
                // A handle belongs to the nearest anchor along its row or column.
                if r % 3 == 0 {
                    let a = c % 3 == 1 ? c - 1 : c + 1
                    out.append((controlPoints[r][a], controlPoints[r][c]))
                } else if c % 3 == 0 {
                    let a = r % 3 == 1 ? r - 1 : r + 1
                    out.append((controlPoints[a][c], controlPoints[r][c]))
                }
            }
        }
        return out
    }

    /// Control points shown as handles: anchors and edge tangents (interior twist points are hidden,
    /// as in Photoshop's net, and move with their anchors).
    public func visibleControls() -> [(row: Int, column: Int)] {
        var out: [(Int, Int)] = []
        for r in 0..<rows {
            for c in 0..<columns where r % 3 == 0 || c % 3 == 0 { out.append((r, c)) }
        }
        return out
    }

    /// Moves control `(row, column)` to `p`. Moving an anchor carries its tangent handles and the
    /// interior points of the patches around it, so the surface follows smoothly.
    public mutating func move(row: Int, column: Int, to p: CGPoint) {
        guard row >= 0, row < rows, column >= 0, column < columns else { return }
        let old = controlPoints[row][column]
        let d = CGSize(width: p.x - old.x, height: p.y - old.y)
        controlPoints[row][column] = p
        guard Self.isAnchor(row: row, column: column) else { return }
        for dr in -1...1 {
            for dc in -1...1 where !(dr == 0 && dc == 0) {
                let r = row + dr, c = column + dc
                guard r >= 0, r < rows, c >= 0, c < columns else { continue }
                controlPoints[r][c].x += d.width
                controlPoints[r][c].y += d.height
            }
        }
    }

    /// The control nearest to `p` within `radius` (same units as `p`), visible controls only.
    public func hit(_ p: CGPoint, radius: Double, map: (CGPoint) -> CGPoint = { $0 }) -> (row: Int, column: Int)? {
        var best: (Int, Int, Double)?
        for (r, c) in visibleControls() {
            let q = map(controlPoints[r][c])
            let d = hypot(Double(q.x - p.x), Double(q.y - p.y))
            if d <= radius, d < (best?.2 ?? .infinity) { best = (r, c, d) }
        }
        return best.map { ($0.0, $0.1) }
    }
}

/// Photoshop's warp presets (engine names, menu titles).
public enum WarpPresetInfo {
    public static let titles: [String: String] = [
        "Arc": "Arc", "ArcLower": "Arc Lower", "ArcUpper": "Arc Upper", "Arch": "Arch", "Bulge": "Bulge", "Shell": "Shell",
        "Flag": "Flag", "Wave": "Wave", "Fish": "Fish", "Rise": "Rise", "Fisheye": "Fisheye", "Inflate": "Inflate",
        "Squeeze": "Squeeze", "Twist": "Twist",
    ]
    public static func title(_ name: String) -> String { titles[name] ?? name }
}

// MARK: - Perspective

/// `transform::perspective::PerspectiveWarp` as a grid of linked planes: `(rows + 1) × (columns + 1)`
/// vertices in layout (source) and warp (destination) space; every cell is one quad, and neighbouring
/// quads share their vertices, so a shared edge moves in both and never cracks.
public struct PerspectiveModel: Equatable, Sendable {
    public var source: [[CGPoint]]
    public var destination: [[CGPoint]]

    public init(source: [[CGPoint]], destination: [[CGPoint]]) {
        self.source = source; self.destination = destination
    }

    /// One plane over `rect` (child pixels).
    public init(rect: CGRect) {
        let g = [[CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY)],
                 [CGPoint(x: rect.minX, y: rect.maxY), CGPoint(x: rect.maxX, y: rect.maxY)]]
        source = g; destination = g
    }

    public var rows: Int { source.count - 1 }
    public var columns: Int { (source.first?.count ?? 1) - 1 }

    /// Perimeter-ordered corners of cell `(r, c)`.
    public func quad(_ grid: [[CGPoint]], _ r: Int, _ c: Int) -> [CGPoint] {
        [grid[r][c], grid[r][c + 1], grid[r + 1][c + 1], grid[r + 1][c]]
    }

    public var quads: [(row: Int, column: Int)] {
        (0..<rows).flatMap { r in (0..<columns).map { (r, $0) } }
    }

    /// `PerspectiveWarp` JSON body: `{"source_quads":…,"destination_quads":…}`.
    public var jsonObject: [String: Any] {
        func q(_ g: [[CGPoint]]) -> [[[Double]]] {
            quads.map { cell in quad(g, cell.row, cell.column).map { [Double($0.x), Double($0.y)] } }
        }
        return ["source_quads": q(source), "destination_quads": q(destination)]
    }

    /// Strictly convex, consistently wound, finite quads in both spaces (the engine additionally
    /// rejects overlaps; this pre-check catches what a vertex drag breaks).
    public var isValid: Bool {
        quads.allSatisfy { Self.convex(quad(source, $0.row, $0.column)) && Self.convex(quad(destination, $0.row, $0.column)) }
    }

    public static func convex(_ q: [CGPoint]) -> Bool {
        guard q.count == 4, q.allSatisfy({ $0.x.isFinite && $0.y.isFinite }) else { return false }
        var sign = 0.0
        for i in 0..<4 {
            let a = q[i], b = q[(i + 1) % 4], c = q[(i + 2) % 4]
            let cross = Double((b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x))
            if abs(cross) < 1e-6 { return false }
            if sign == 0 { sign = cross } else if (cross > 0) != (sign > 0) { return false }
        }
        return true
    }

    /// Moves vertex `(r, c)`: in layout mode source and destination together (fitting the planes to
    /// the image), in warp mode the destination only.
    public mutating func move(row r: Int, column c: Int, to p: CGPoint, layout: Bool) {
        if layout {
            let d = CGSize(width: p.x - source[r][c].x, height: p.y - source[r][c].y)
            source[r][c] = p
            destination[r][c].x += d.width
            destination[r][c].y += d.height
        } else {
            destination[r][c] = p
        }
    }

    /// Splits column `c` in two (vertically) at its middle across every row: new source vertices at
    /// the edge midpoints, destination vertices mapped through each cell's homography, so the image
    /// does not move.
    public mutating func splitColumn(_ c: Int) {
        guard c >= 0, c < columns else { return }
        for r in 0...rows {
            let cellRow = min(r, rows - 1)
            let s = CGPoint(x: (source[r][c].x + source[r][c + 1].x) / 2, y: (source[r][c].y + source[r][c + 1].y) / 2)
            let h = Homography(from: quad(source, cellRow, c), to: quad(destination, cellRow, c))
            source[r].insert(s, at: c + 1)
            destination[r].insert(h?.map(s) ?? s, at: c + 1)
        }
    }

    /// Splits row `r` in two (horizontally), as `splitColumn`.
    public mutating func splitRow(_ r: Int) {
        guard r >= 0, r < rows else { return }
        var ns: [CGPoint] = [], nd: [CGPoint] = []
        for c in 0...columns {
            let cellCol = min(c, columns - 1)
            let s = CGPoint(x: (source[r][c].x + source[r + 1][c].x) / 2, y: (source[r][c].y + source[r + 1][c].y) / 2)
            let h = Homography(from: quad(source, r, cellCol), to: quad(destination, r, cellCol))
            ns.append(s)
            nd.append(h?.map(s) ?? s)
        }
        source.insert(ns, at: r + 1)
        destination.insert(nd, at: r + 1)
    }

    /// The vertex nearest `p` within `radius` in `space` (mapped by `map`).
    public func hit(_ p: CGPoint, radius: Double, layout: Bool, map: (CGPoint) -> CGPoint = { $0 }) -> (row: Int, column: Int)? {
        let g = layout ? source : destination
        var best: (Int, Int, Double)?
        for r in 0...rows {
            for c in 0...columns {
                let q = map(g[r][c])
                let d = hypot(Double(q.x - p.x), Double(q.y - p.y))
                if d <= radius, d < (best?.2 ?? .infinity) { best = (r, c, d) }
            }
        }
        return best.map { ($0.0, $0.1) }
    }

    /// The cell containing `p` (source space), for splitting where the user clicks.
    public func cell(containing p: CGPoint) -> (row: Int, column: Int)? {
        quads.first { Self.contains(quad(source, $0.row, $0.column), p) }
    }

    static func contains(_ q: [CGPoint], _ p: CGPoint) -> Bool {
        var sign = 0.0
        for i in 0..<4 {
            let a = q[i], b = q[(i + 1) % 4]
            let cross = Double((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x))
            if cross == 0 { continue }
            if sign == 0 { sign = cross } else if (cross > 0) != (sign > 0) { return false }
        }
        return true
    }
}

/// A projective map of four point correspondences (for splitting planes without a jump).
public struct Homography: Sendable {
    public var m: [Double]   // row-major 3 × 3

    public init?(from s: [CGPoint], to d: [CGPoint]) {
        guard s.count == 4, d.count == 4 else { return nil }
        // Solve the 8 × 8 system A h = b (h33 = 1) by Gaussian elimination with partial pivoting.
        var a = [[Double]](repeating: [Double](repeating: 0, count: 9), count: 8)
        for i in 0..<4 {
            let (x, y, u, v) = (Double(s[i].x), Double(s[i].y), Double(d[i].x), Double(d[i].y))
            a[2 * i] = [x, y, 1, 0, 0, 0, -u * x, -u * y, u]
            a[2 * i + 1] = [0, 0, 0, x, y, 1, -v * x, -v * y, v]
        }
        for col in 0..<8 {
            guard let pivot = (col..<8).max(by: { abs(a[$0][col]) < abs(a[$1][col]) }), abs(a[pivot][col]) > 1e-12 else { return nil }
            a.swapAt(col, pivot)
            for r in 0..<8 where r != col {
                let f = a[r][col] / a[col][col]
                if f == 0 { continue }
                for k in col..<9 { a[r][k] -= f * a[col][k] }
            }
        }
        var h = (0..<8).map { a[$0][8] / a[$0][$0] }
        h.append(1)
        m = h
    }

    public func map(_ p: CGPoint) -> CGPoint {
        let (x, y) = (Double(p.x), Double(p.y))
        let w = m[6] * x + m[7] * y + m[8]
        return CGPoint(x: CGFloat((m[0] * x + m[1] * y + m[2]) / w), y: CGFloat((m[3] * x + m[4] * y + m[5]) / w))
    }
}

// MARK: - Puppet

public enum PuppetDensityTag: String, CaseIterable, Sendable, Identifiable {
    case sparse = "Sparse", normal = "Normal", dense = "Dense"
    public var id: String { rawValue }
    public var title: String { rawValue }
}

/// A pin: a mesh vertex held at `target` (child pixels), optionally rotated (radians, rest-relative).
public struct PuppetPinModel: Codable, Equatable, Sendable {
    public var vertex: Int
    public var target: CGPoint
    public var rotation: Double?

    public init(vertex: Int, target: CGPoint, rotation: Double? = nil) {
        self.vertex = vertex; self.target = target; self.rotation = rotation
    }

    enum CodingKeys: String, CodingKey { case vertex, target, rotation }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        vertex = try c.decode(Int.self, forKey: .vertex)
        target = try c.decode(PointPair.self, forKey: .target).point
        rotation = try c.decodeIfPresent(Double.self, forKey: .rotation)
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(vertex, forKey: .vertex)
        try c.encode(PointPair(target), forKey: .target)
        try c.encode(rotation, forKey: .rotation)
    }
}

/// `transform::puppet::PuppetWarp`.
public struct PuppetModel: Codable, Equatable, Sendable {
    public var restVertices: [CGPoint]
    public var triangles: [[Int]]
    public var pins: [PuppetPinModel]
    public var density: String
    public var expansion: UInt32
    /// `Normal` or `Rigid`.
    public var mode: String
    public var iterations: Int

    enum CodingKeys: String, CodingKey {
        case restVertices = "rest_vertices", triangles, pins, density, expansion, mode, iterations
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        restVertices = try c.decode([PointPair].self, forKey: .restVertices).map(\.point)
        triangles = try c.decode([[Int]].self, forKey: .triangles)
        pins = try c.decode([PuppetPinModel].self, forKey: .pins)
        density = try c.decode(String.self, forKey: .density)
        expansion = try c.decode(UInt32.self, forKey: .expansion)
        mode = try c.decode(String.self, forKey: .mode)
        iterations = try c.decode(Int.self, forKey: .iterations)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(restVertices.map(PointPair.init), forKey: .restVertices)
        try c.encode(triangles, forKey: .triangles)
        try c.encode(pins, forKey: .pins)
        try c.encode(density, forKey: .density)
        try c.encode(expansion, forKey: .expansion)
        try c.encode(mode, forKey: .mode)
        try c.encode(iterations, forKey: .iterations)
    }

    public init?(json: String) {
        guard let m = try? JSONDecoder().decode(PuppetModel.self, from: Data(json.utf8)) else { return nil }
        self = m
    }

    public var json: String { (try? String(decoding: encoder().encode(self), as: UTF8.self)) ?? "{}" }

    /// Unique mesh edges (for drawing).
    public var edges: [(Int, Int)] {
        var seen = Set<Int>()
        var out: [(Int, Int)] = []
        let n = restVertices.count
        for t in triangles where t.count == 3 {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let key = min(a, b) * n + max(a, b)
                if seen.insert(key).inserted { out.append((a, b)) }
            }
        }
        return out
    }

    /// The pin index at vertex positions `positions` (deformed) nearest `p` within `radius`.
    public func pinHit(_ p: CGPoint, radius: Double, map: (CGPoint) -> CGPoint = { $0 }) -> Int? {
        var best: (Int, Double)?
        for (i, pin) in pins.enumerated() {
            let q = map(pin.target)
            let d = hypot(Double(q.x - p.x), Double(q.y - p.y))
            if d <= radius, d < (best?.1 ?? .infinity) { best = (i, d) }
        }
        return best?.0
    }

    /// Adds a pin at the vertex nearest `p` in `positions` (the current, deformed mesh) within
    /// `radius`, held where it is now, so adding a pin never moves the image. Returns its index.
    public mutating func addPin(near p: CGPoint, positions: [CGPoint], radius: Double) -> Int? {
        var best: (Int, Double)?
        for (i, q) in positions.enumerated() {
            let d = hypot(Double(q.x - p.x), Double(q.y - p.y))
            if d <= radius, d < (best?.1 ?? .infinity) { best = (i, d) }
        }
        guard let (v, _) = best else { return nil }
        if let existing = pins.firstIndex(where: { $0.vertex == v }) { return existing }
        pins.append(PuppetPinModel(vertex: v, target: positions[v]))
        return pins.count - 1
    }

    /// Re-attaches pins to `mesh` (new density / expansion): each pin moves to the new rest vertex
    /// nearest its old rest position, keeping target and rotation; duplicates are dropped.
    public func rebased(onto mesh: PuppetModel) -> PuppetModel {
        var out = mesh
        out.mode = mode
        out.pins = []
        for pin in pins where pin.vertex < restVertices.count {
            let rest = restVertices[pin.vertex]
            guard let v = mesh.restVertices.indices.min(by: {
                hypot(Double(mesh.restVertices[$0].x - rest.x), Double(mesh.restVertices[$0].y - rest.y))
                    < hypot(Double(mesh.restVertices[$1].x - rest.x), Double(mesh.restVertices[$1].y - rest.y))
            }), !out.pins.contains(where: { $0.vertex == v }) else { continue }
            out.pins.append(PuppetPinModel(vertex: v, target: pin.target, rotation: pin.rotation))
        }
        return out
    }
}

// MARK: - Content-aware scale

/// The content-aware scale box: target child pixels anchored at the child origin (the engine pads or
/// clips at the origin inside the fixed canvas), amount and the chosen protection channel.
public struct ContentAwareScaleModel: Equatable, Sendable {
    public var width: UInt32
    public var height: UInt32
    public var amount: Float = 1
    public var protectChannel: UInt64?
    public let canvasWidth: UInt32
    public let canvasHeight: UInt32

    public init(canvasWidth: UInt32, canvasHeight: UInt32) {
        self.canvasWidth = canvasWidth; self.canvasHeight = canvasHeight
        width = canvasWidth; height = canvasHeight
    }

    public enum Handle: CaseIterable, Sendable { case right, bottom, corner }

    public var rect: CGRect { CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)) }

    public func point(_ h: Handle) -> CGPoint {
        switch h {
        case .right: CGPoint(x: CGFloat(width), y: CGFloat(height) / 2)
        case .bottom: CGPoint(x: CGFloat(width) / 2, y: CGFloat(height))
        case .corner: CGPoint(x: CGFloat(width), y: CGFloat(height))
        }
    }

    /// Drags a handle to child point `p`; ⇧ (`proportional`) keeps the aspect ratio. Dimensions stay
    /// 1…4 × the canvas (enlargements beyond the canvas are clipped by the engine).
    public mutating func drag(_ h: Handle, to p: CGPoint, proportional: Bool) {
        let maxW = Double(canvasWidth) * 4, maxH = Double(canvasHeight) * 4
        var w = Double(width), hh = Double(height)
        switch h {
        case .right: w = Double(p.x)
        case .bottom: hh = Double(p.y)
        case .corner: w = Double(p.x); hh = Double(p.y)
        }
        if proportional {
            let aspect = Double(canvasWidth) / Double(canvasHeight)
            if h == .bottom { w = hh * aspect } else { hh = w / aspect }
        }
        width = UInt32(min(max(w.rounded(), 1), maxW))
        height = UInt32(min(max(hh.rounded(), 1), maxH))
    }

    public var percent: (w: Double, h: Double) {
        (Double(width) / Double(canvasWidth) * 100, Double(height) / Double(canvasHeight) * 100)
    }
}

// MARK: - TransformOp

/// A complete `transform::TransformOp` (version 1).
public enum TransformOperationModel: Equatable, Sendable {
    case warp(WarpMeshModel)
    case perspective(PerspectiveModel)
    case puppet(PuppetModel)
    case contentAwareScale(ContentAwareScaleModel)

    public var tag: AdvancedTransformTag {
        switch self {
        case .warp: .warp
        case .perspective: .perspective
        case .puppet: .puppet
        case .contentAwareScale: .contentAwareScale
        }
    }

    /// The engine JSON. Content-aware protection is not sent: the engine samples the chosen channel.
    public func json(kernel: TransformKernel) -> String {
        let operation: Any
        switch self {
        case .warp(let m):
            operation = ["Warp": (try? JSONSerialization.jsonObject(with: Data(m.json.utf8))) ?? [:]]
        case .perspective(let p):
            operation = ["Perspective": p.jsonObject]
        case .puppet(let p):
            operation = ["Puppet": (try? JSONSerialization.jsonObject(with: Data(p.json.utf8))) ?? [:]]
        case .contentAwareScale(let c):
            operation = ["ContentAwareScale": ["target_width": c.width, "target_height": c.height,
                                               "amount": Double(c.amount), "protect": NSNull()] as [String: Any]]
        }
        let op: [String: Any] = ["version": 1, "operation": operation, "kernel": kernel.rawValue]
        guard let d = try? JSONSerialization.data(withJSONObject: op, options: [.sortedKeys]) else { return "{}" }
        return String(decoding: d, as: UTF8.self)
    }

    /// Parses an engine stage (`transform_json`); `canvas` sizes a content-aware box.
    public static func parse(_ json: String, canvasWidth: UInt32, canvasHeight: UInt32) -> (TransformOperationModel, TransformKernel)? {
        guard let root = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
              let operation = root["operation"] as? [String: Any], let (key, body) = operation.first else { return nil }
        let kernel = (root["kernel"] as? String).flatMap(TransformKernel.init(rawValue:)) ?? .bicubic
        guard let data = try? JSONSerialization.data(withJSONObject: body) else { return nil }
        let text = String(decoding: data, as: UTF8.self)
        switch key {
        case "Warp":
            return WarpMeshModel(json: text).map { (.warp($0), kernel) }
        case "Puppet":
            return PuppetModel(json: text).map { (.puppet($0), kernel) }
        case "Perspective":
            guard let b = body as? [String: Any], let src = b["source_quads"] as? [[[Double]]],
                  let dst = b["destination_quads"] as? [[[Double]]],
                  let model = PerspectiveModel(quads: src, destination: dst) else { return nil }
            return (.perspective(model), kernel)
        case "ContentAwareScale":
            guard let b = body as? [String: Any] else { return nil }
            var c = ContentAwareScaleModel(canvasWidth: canvasWidth, canvasHeight: canvasHeight)
            c.width = (b["target_width"] as? NSNumber)?.uint32Value ?? canvasWidth
            c.height = (b["target_height"] as? NSNumber)?.uint32Value ?? canvasHeight
            c.amount = (b["amount"] as? NSNumber)?.floatValue ?? 1
            return (.contentAwareScale(c), kernel)
        default:
            return nil
        }
    }
}

extension PerspectiveModel {
    /// Rebuilds a grid from engine quads when they form one (row-major cells of a lattice, as this
    /// editor writes them); other quad sets are not editable here.
    public init?(quads src: [[[Double]]], destination dst: [[[Double]]]) {
        func pt(_ v: [Double]) -> CGPoint { CGPoint(x: CGFloat(v.first ?? 0), y: CGFloat(v.count > 1 ? v[1] : 0)) }
        let n = src.count
        guard n > 0, dst.count == n, src.allSatisfy({ $0.count == 4 }), dst.allSatisfy({ $0.count == 4 }) else { return nil }
        // Columns: consecutive cells sharing the right edge of the first one.
        var columns = 1
        while columns < n, pt(src[columns][0]) == pt(src[columns - 1][1]) { columns += 1 }
        guard n % columns == 0 else { return nil }
        let rows = n / columns
        var s = [[CGPoint]](repeating: [CGPoint](repeating: .zero, count: columns + 1), count: rows + 1)
        var d = s
        for r in 0..<rows {
            for c in 0..<columns {
                let i = r * columns + c
                let (qs, qd) = (src[i].map(pt), dst[i].map(pt))
                s[r][c] = qs[0]; s[r][c + 1] = qs[1]; s[r + 1][c + 1] = qs[2]; s[r + 1][c] = qs[3]
                d[r][c] = qd[0]; d[r][c + 1] = qd[1]; d[r + 1][c + 1] = qd[2]; d[r + 1][c] = qd[3]
            }
        }
        let model = PerspectiveModel(source: s, destination: d)
        // Round trip: the lattice must reproduce every quad exactly.
        for r in 0..<rows {
            for c in 0..<columns where model.quad(s, r, c) != src[r * columns + c].map(pt) { return nil }
        }
        self = model
    }
}

// MARK: - Coordinates

/// Child level-0 pixels ↔ document pixels through the smart object's placement.
public struct ChildMapping: Equatable, Sendable {
    public var childToDocument: AffineTransform2D
    public init(_ m: AffineTransform2D) { childToDocument = m }
    public func document(_ child: CGPoint) -> CGPoint { childToDocument.apply(child) }
    public func child(_ document: CGPoint) -> CGPoint { (childToDocument.inverse ?? .identity).apply(document) }
}
