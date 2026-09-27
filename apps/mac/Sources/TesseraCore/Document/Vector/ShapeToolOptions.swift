import Foundation

/// Options of the shape tools and the Pen (WP B5-11): the paint a new shape gets and the live
/// parameters the options bar shows. Colours are straight RGBA 0…1 in the document's samples.
public struct ShapeToolOptions: Equatable, Sendable {
    public var fillEnabled = true
    public var fillColor: [Double] = [0.2, 0.45, 0.8, 1]
    public var strokeEnabled = false
    public var strokeColor: [Double] = [0, 0, 0, 1]
    public var strokeWidth: Double = 4
    public var strokeAlignment: ShapeStrokeAlignment = .center
    /// Rectangle corner radius (all four; the inspector edits them independently afterwards).
    public var cornerRadius: Double = 0
    public var sides = 5
    public var star = false
    /// Star inner radius as a fraction of the outer radius.
    public var starInset = 0.5
    public var lineWeight: Double = 6
    public init() {}

    public static let sidesRange = 3...100
    public static let widthRange: ClosedRange<Double> = 0...500

    /// The source a new shape gets. Lines are stroke-only (their fill has no area) with the line weight.
    public func source(for live: LiveShape) -> ShapeSource {
        switch live {
        case .line:
            return ShapeSource(live: live, fill: nil,
                               stroke: (ShapeStroke(width: max(lineWeight, 0.5), alignment: .center, cap: .butt),
                                        .solid(strokeEnabled ? strokeColor : fillColor)))
        default:
            let alignment: ShapeStrokeAlignment = ShapePrimitives.path(live).hasOpenSubpaths ? .center : strokeAlignment
            return ShapeSource(live: live, fill: fillEnabled ? .solid(fillColor) : nil,
                               stroke: strokeEnabled ? (ShapeStroke(width: strokeWidth, alignment: alignment), .solid(strokeColor)) : nil)
        }
    }

    /// A Pen path as a custom shape (no live primitive).
    public func source(for path: ShapePath) -> ShapeSource {
        let open = path.hasOpenSubpaths
        let stroke: (ShapeStroke, ShapePaint)? = strokeEnabled || (open && !fillEnabled)
            ? (ShapeStroke(width: strokeWidth, alignment: open ? .center : strokeAlignment), .solid(strokeColor))
            : nil
        return ShapeSource(path: path, fill: fillEnabled ? .solid(fillColor) : nil, stroke: stroke, liveShape: nil)
    }

    public mutating func setSides(_ n: Int) { sides = min(max(n, Self.sidesRange.lowerBound), Self.sidesRange.upperBound) }
}
