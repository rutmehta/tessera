import CoreGraphics
import Foundation

// Filter ▸ Adaptive Wide Angle… (WP B5-20): the protocol document backends adopt, mirroring the session calls in
// crates/tessera-ffi/src/document/adaptive.rs, and the UI-free recipe model the sheet edits. The engine stores the
// recipe (`transform::adaptive::Adaptive`) verbatim as the `adaptive_wide_angle` smart filter's params, so a smart
// object's filter re-opens with its constraints. Camera: Manual rectilinear ("Perspective") or equidistant
// ("Fisheye") with a focal length; no lens profiles. Unit tested in DocumentAdaptiveWideAngleTests.

public enum AdaptiveWideAngleFilter {
    public static let id = "adaptive_wide_angle"
    public static let title = "Adaptive Wide Angle"
    public static let menuTitle = title + "…"
    /// Focal length, millimetres (35 mm equivalent).
    public static let focalRange: ClosedRange<Double> = 4...400
    /// Scale, percent.
    public static let scaleRange: ClosedRange<Double> = 50...150

    /// The engine names this filter by its id (it is not in the menu catalogue).
    public static func displayName(_ engineName: String) -> String { engineName == id ? title : engineName }

    /// Why the workspace cannot open on a layer of `kind` (nil: it can).
    public static func refusal(kind: LayerKindTag?) -> String? {
        switch kind {
        case .pixel?, .smartObject?: nil
        default: "\(title): select a pixel layer or a smart object"
        }
    }
}

/// The camera model (engine `Projection`).
public enum AdaptiveProjection: String, CaseIterable, Identifiable, Sendable {
    case rectilinear = "Rectilinear"
    case equidistant = "Equidistant"
    public var id: String { rawValue }
    /// Photoshop's Correction names.
    public var title: String { self == .rectilinear ? "Perspective" : "Fisheye" }
}

/// What a constraint line becomes (engine `LineOrientation`).
public enum AdaptiveLineOrientation: String, CaseIterable, Identifiable, Sendable {
    case straight = "Straight"
    case horizontal = "Horizontal"
    case vertical = "Vertical"
    public var id: String { rawValue }
    public var title: String { rawValue }

    /// A new line: Straight, or with Shift (Photoshop's modifier) horizontal / vertical by its dominant direction.
    public static func forDrag(from a: CGPoint, to b: CGPoint, constrain: Bool) -> Self {
        guard constrain else { return .straight }
        return abs(Double(b.x - a.x)) >= abs(Double(b.y - a.y)) ? .horizontal : .vertical
    }
}

/// One constraint: its two ends, and the curve the engine traces between them under the camera model.
public struct AdaptiveConstraint: Equatable, Sendable {
    public var from: CGPoint
    public var to: CGPoint
    public var orientation: AdaptiveLineOrientation
    public var weight: Double
    /// Source-pixel samples, both ends included (the camera model's image of the straight edge).
    public var points: [CGPoint]

    public init(from: CGPoint, to: CGPoint, orientation: AdaptiveLineOrientation, weight: Double = 1, points: [CGPoint]? = nil) {
        self.from = from; self.to = to; self.orientation = orientation; self.weight = weight
        self.points = points ?? [from, to]
    }

    /// Shortest distance from `p` to the curve, source pixels.
    public func distance(to p: CGPoint) -> Double {
        guard points.count > 1 else { return points.first.map { hypot(Double($0.x - p.x), Double($0.y - p.y)) } ?? .infinity }
        return zip(points, points.dropFirst()).map { Self.segment(p, $0, $1) }.min() ?? .infinity
    }

    static func segment(_ p: CGPoint, _ a: CGPoint, _ b: CGPoint) -> Double {
        let (dx, dy) = (Double(b.x - a.x), Double(b.y - a.y))
        let l2 = dx * dx + dy * dy
        let t = l2 > 0 ? min(max((Double(p.x - a.x) * dx + Double(p.y - a.y) * dy) / l2, 0), 1) : 0
        return hypot(Double(a.x) + t * dx - Double(p.x), Double(a.y) + t * dy - Double(p.y))
    }
}

/// What the Adaptive Wide Angle sheet edits. Fields the sheet has no control for (mesh size, smoothness, tolerance,
/// crop, output size) are kept verbatim from the engine's recipe.
public struct AdaptiveWideAngleDraft: Equatable, @unchecked Sendable {
    public let width: Int
    public let height: Int
    public var projection: AdaptiveProjection
    /// Millimetres, 35 mm equivalent (`focal_px = mm / 36 × long edge`).
    public private(set) var focal35: Double
    /// Percent.
    public private(set) var scalePercent: Double
    public private(set) var lines: [AdaptiveConstraint]
    private let base: [String: Any]
    private let center: [Double]

    /// Parses an engine recipe. Throws for a recipe this sheet cannot edit (a lens profile camera, a malformed one).
    public init(recipeJson: String) throws {
        guard let root = (try? JSONSerialization.jsonObject(with: Data(recipeJson.utf8))) as? [String: Any],
              let w = (root["source_width"] as? NSNumber)?.intValue, let h = (root["source_height"] as? NSNumber)?.intValue,
              w > 0, h > 0 else { throw DocumentError.invalid("\(AdaptiveWideAngleFilter.title): the recipe could not be read") }
        guard let camera = root["camera"] as? [String: Any], let manual = camera["Manual"] as? [String: Any] else {
            throw DocumentError.unsupported("\(AdaptiveWideAngleFilter.title): this recipe uses a lens profile, which this build cannot edit")
        }
        width = w
        height = h
        base = root
        projection = AdaptiveProjection(rawValue: manual["projection"] as? String ?? "") ?? .rectilinear
        center = (manual["center"] as? [NSNumber])?.map(\.doubleValue) ?? [Double(w) / 2, Double(h) / 2]
        let focalPx = (manual["focal_px"] as? NSNumber)?.doubleValue ?? Double(max(w, h))
        focal35 = 0
        scalePercent = 100
        lines = []
        focal35 = clampFocal(focalPx / Double(max(w, h)) * 36)
        setScalePercent(((root["scale"] as? NSNumber)?.doubleValue ?? 1) * 100)
        lines = (root["lines"] as? [[String: Any]] ?? []).compactMap { l in
            let pts = (l["points"] as? [[NSNumber]] ?? []).compactMap { $0.count == 2 ? CGPoint(x: $0[0].doubleValue, y: $0[1].doubleValue) : nil }
            guard let a = pts.first, let b = pts.last, pts.count >= 2 else { return nil }
            return AdaptiveConstraint(from: a, to: b,
                                      orientation: AdaptiveLineOrientation(rawValue: l["orientation"] as? String ?? "") ?? .straight,
                                      weight: (l["weight"] as? NSNumber)?.doubleValue ?? 1, points: pts)
        }
    }

    public static func == (a: Self, b: Self) -> Bool { a.recipeJson == b.recipeJson }

    private var longEdge: Double { Double(max(width, height)) }
    public var focalPx: Double { focal35 / 36 * longEdge }

    private func clampFocal(_ v: Double) -> Double {
        let r = AdaptiveWideAngleFilter.focalRange
        return v.isFinite ? min(max(v, r.lowerBound), r.upperBound) : 24
    }

    public mutating func setFocal35(_ mm: Double) { focal35 = clampFocal(mm) }

    public mutating func setScalePercent(_ v: Double) {
        let r = AdaptiveWideAngleFilter.scaleRange
        if v.isFinite { scalePercent = min(max(v, r.lowerBound), r.upperBound) }
    }

    /// Changes whenever the constraint curves must be traced again (camera model or focal length).
    public var curveKey: String { "\(projection.rawValue)|\(focalPx)" }

    // MARK: Lines

    /// Clamped into the source.
    public func clamp(_ p: CGPoint) -> CGPoint {
        CGPoint(x: min(max(Double(p.x), 0), Double(width)), y: min(max(Double(p.y), 0), Double(height)))
    }

    /// Adds a line unless its ends are (nearly) the same point; returns its index.
    @discardableResult
    public mutating func addLine(from a: CGPoint, to b: CGPoint, orientation: AdaptiveLineOrientation) -> Int? {
        let (a, b) = (clamp(a), clamp(b))
        guard hypot(Double(b.x - a.x), Double(b.y - a.y)) >= 2 else { return nil }
        lines.append(AdaptiveConstraint(from: a, to: b, orientation: orientation))
        return lines.count - 1
    }

    public mutating func removeLine(at i: Int) { if lines.indices.contains(i) { lines.remove(at: i) } }
    public mutating func removeAllLines() { lines.removeAll() }

    public mutating func setOrientation(_ o: AdaptiveLineOrientation, at i: Int) {
        if lines.indices.contains(i) { lines[i].orientation = o }
    }

    /// Replaces line `i`'s traced curve (ends kept exactly).
    public mutating func setCurve(_ points: [CGPoint], at i: Int) {
        guard lines.indices.contains(i), points.count >= 2 else { return }
        var p = points
        p[0] = lines[i].from
        p[p.count - 1] = lines[i].to
        lines[i].points = p
    }

    /// The line nearest `p` within `tolerance` source pixels.
    public func line(near p: CGPoint, tolerance: Double) -> Int? {
        lines.indices.map { ($0, lines[$0].distance(to: p)) }.filter { $0.1 <= tolerance }.min { $0.1 < $1.1 }?.0
    }

    // MARK: JSON

    /// The recipe for the engine (keys sorted), with the edited camera, scale and lines over the base recipe.
    public var recipeJson: String {
        var r = base
        r["camera"] = ["Manual": ["focal_px": focalPx, "center": center, "projection": projection.rawValue]]
        r["output_focal_px"] = focalPx
        r["scale"] = scalePercent / 100
        r["lines"] = lines.map { l in
            ["points": l.points.map { [Double($0.x), Double($0.y)] }, "orientation": l.orientation.rawValue, "weight": l.weight] as [String: Any]
        }
        guard let d = try? JSONSerialization.data(withJSONObject: r, options: [.sortedKeys]) else { return "{}" }
        return String(decoding: d, as: UTF8.self)
    }
}

// MARK: - Backend

public struct AdaptiveWideAngleWorkspaceInfo: Equatable, Sendable {
    public var token: UInt64
    public var layer: DocLayerID
    public var stageIndex: UInt32?
    public var smartObject: Bool
    public var width: UInt32
    public var height: UInt32
    public var previewWidth: UInt32
    public var previewHeight: UInt32
    public var previewFactor: UInt32
    public var recipeJson: String
    public var exifFocal35mm: Double?
    public init(token: UInt64, layer: DocLayerID, stageIndex: UInt32?, smartObject: Bool, width: UInt32, height: UInt32,
                previewWidth: UInt32, previewHeight: UInt32, previewFactor: UInt32, recipeJson: String, exifFocal35mm: Double?) {
        self.token = token; self.layer = layer; self.stageIndex = stageIndex; self.smartObject = smartObject
        self.width = width; self.height = height; self.previewWidth = previewWidth; self.previewHeight = previewHeight
        self.previewFactor = previewFactor; self.recipeJson = recipeJson; self.exifFocal35mm = exifFocal35mm
    }
    public var sourceSize: CGSize { CGSize(width: Double(width), height: Double(height)) }
}

public struct AdaptiveWideAnglePreviewFrame: Equatable, Sendable {
    public var surfaceId: UInt32
    public var width: UInt32
    public var height: UInt32
    public var original: Bool
    public var millis: Double
    public init(surfaceId: UInt32, width: UInt32, height: UInt32, original: Bool, millis: Double) {
        self.surfaceId = surfaceId; self.width = width; self.height = height; self.original = original; self.millis = millis
    }
}

public protocol DocumentAdaptiveWideAngleBackend: AnyObject, Sendable {
    func beginAdaptiveWideAngle(layer: DocLayerID, stageIndex: UInt32?) throws -> AdaptiveWideAngleWorkspaceInfo
    /// Blocking proxy solve (`nil` recipe: the untouched source).
    func previewAdaptiveWideAngle(token: UInt64, recipeJson: String?) throws -> AdaptiveWideAnglePreviewFrame
    /// Blocking full-resolution render and one history node.
    func commitAdaptiveWideAngle(token: UInt64, recipeJson: String) throws -> DocumentChange
    func cancelAdaptiveWideAngle(token: UInt64)
    /// The source-pixel curve of the straight edge between `from` and `to` under the recipe's camera.
    func adaptiveWideAngleCurve(recipeJson: String, from: CGPoint, to: CGPoint) throws -> [CGPoint]
}
