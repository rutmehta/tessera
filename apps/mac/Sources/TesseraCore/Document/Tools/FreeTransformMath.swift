import Foundation

/// `x' = a·x + b·y + c`, `y' = d·x + e·y + f` in canvas pixels (the engine's `TransformMatrix`).
public struct AffineTransform2D: Equatable, Sendable {
    public var a: Double, b: Double, c: Double, d: Double, e: Double, f: Double

    public init(a: Double, b: Double, c: Double, d: Double, e: Double, f: Double) {
        self.a = a; self.b = b; self.c = c; self.d = d; self.e = e; self.f = f
    }

    public static let identity = AffineTransform2D(a: 1, b: 0, c: 0, d: 0, e: 1, f: 0)
    public static func translation(_ tx: Double, _ ty: Double) -> Self { .init(a: 1, b: 0, c: tx, d: 0, e: 1, f: ty) }
    public static func scale(_ sx: Double, _ sy: Double) -> Self { .init(a: sx, b: 0, c: 0, d: 0, e: sy, f: 0) }
    /// Counter-clockwise on screen for positive degrees in a y-down space is clockwise; this is the
    /// y-down convention of the canvas: positive angles turn clockwise, as Photoshop's field does.
    public static func rotation(degrees: Double) -> Self {
        let r = degrees * .pi / 180
        return .init(a: cos(r), b: -sin(r), c: 0, d: sin(r), e: cos(r), f: 0)
    }
    /// Horizontal and vertical skew, degrees.
    public static func skew(x: Double, y: Double) -> Self {
        .init(a: 1, b: tan(x * .pi / 180), c: 0, d: tan(y * .pi / 180), e: 1, f: 0)
    }

    /// `self ∘ other`: apply `other`, then `self`.
    public func concatenating(after other: AffineTransform2D) -> AffineTransform2D {
        AffineTransform2D(a: a * other.a + b * other.d, b: a * other.b + b * other.e, c: a * other.c + b * other.f + c,
                          d: d * other.a + e * other.d, e: d * other.b + e * other.e, f: d * other.c + e * other.f + f)
    }

    public func apply(_ p: CGPoint) -> CGPoint { CGPoint(x: a * p.x + b * p.y + c, y: d * p.x + e * p.y + f) }

    public var determinant: Double { a * e - b * d }

    public var inverse: AffineTransform2D? {
        let det = determinant
        guard det.isFinite, abs(det) > 1e-12 else { return nil }
        let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det)
        return AffineTransform2D(a: ia, b: ib, c: -(ia * c + ib * f), d: id, e: ie, f: -(id * c + ie * f))
    }

    public var isIdentity: Bool { self == .identity }
}

/// Free Transform (⌘T): the numeric parameters of the options bar and the on-canvas handle
/// gestures, composed into one matrix about the reference point:
/// `M = T(ref + (tx, ty)) · R(angle) · Skew(skewX, skewY) · S(sx, sy) · T(−ref)`.
public struct FreeTransformModel: Equatable, Sendable {
    /// The layers' bounds when the transform began (canvas pixels).
    public var bounds: CGRect
    /// Reference point in canvas pixels (the bounds' centre by default).
    public var reference: CGPoint
    public var tx: Double = 0
    public var ty: Double = 0
    public var sx: Double = 1
    public var sy: Double = 1
    /// Degrees, clockwise.
    public var angle: Double = 0
    public var skewX: Double = 0
    public var skewY: Double = 0

    public init(bounds: CGRect) {
        self.bounds = bounds
        reference = CGPoint(x: bounds.midX, y: bounds.midY)
    }

    public var matrix: AffineTransform2D {
        let toOrigin = AffineTransform2D.translation(-reference.x, -reference.y)
        let back = AffineTransform2D.translation(reference.x + tx, reference.y + ty)
        return back.concatenating(after: .rotation(degrees: angle))
            .concatenating(after: .skew(x: skewX, y: skewY))
            .concatenating(after: .scale(sx, sy))
            .concatenating(after: toOrigin)
    }

    public var isIdentity: Bool { tx == 0 && ty == 0 && sx == 1 && sy == 1 && angle == 0 && skewX == 0 && skewY == 0 }

    /// The eight handles (corners and edge midpoints) of the original bounds, before the transform.
    public enum Handle: Int, CaseIterable, Sendable {
        case topLeft, top, topRight, right, bottomRight, bottom, bottomLeft, left
        /// Unit position in the bounds (0…1).
        public var unit: CGPoint {
            switch self {
            case .topLeft: CGPoint(x: 0, y: 0)
            case .top: CGPoint(x: 0.5, y: 0)
            case .topRight: CGPoint(x: 1, y: 0)
            case .right: CGPoint(x: 1, y: 0.5)
            case .bottomRight: CGPoint(x: 1, y: 1)
            case .bottom: CGPoint(x: 0.5, y: 1)
            case .bottomLeft: CGPoint(x: 0, y: 1)
            case .left: CGPoint(x: 0, y: 0.5)
            }
        }
        public var isCorner: Bool { rawValue % 2 == 0 }
        public var opposite: Handle { Handle(rawValue: (rawValue + 4) % 8)! }
    }

    public func point(_ h: Handle) -> CGPoint {
        CGPoint(x: bounds.minX + bounds.width * h.unit.x, y: bounds.minY + bounds.height * h.unit.y)
    }

    /// Where a handle is on the canvas now.
    public func transformed(_ h: Handle) -> CGPoint { matrix.apply(point(h)) }

    /// The four transformed corners (top-left, top-right, bottom-right, bottom-left).
    public var corners: [CGPoint] { [.topLeft, .topRight, .bottomRight, .bottomLeft].map(transformed) }

    /// A drag of handle `h` to canvas point `p`. Scales about the opposite handle, or about the
    /// reference point with ⌥ (`fromCenter`); `constrain` (⇧) keeps the aspect ratio for corners.
    public mutating func drag(_ h: Handle, to p: CGPoint, constrain: Bool, fromCenter: Bool) {
        // Work in the transform's local frame (rotation and skew removed).
        let local = AffineTransform2D.rotation(degrees: angle).concatenating(after: .skew(x: skewX, y: skewY))
        guard let toLocal = local.inverse else { return }
        let anchorPoint = fromCenter ? reference : point(h.opposite)
        let anchorNow = matrix.apply(anchorPoint)
        let dragged = toLocal.apply(CGPoint(x: p.x - anchorNow.x, y: p.y - anchorNow.y))
        let original = CGPoint(x: point(h).x - anchorPoint.x, y: point(h).y - anchorPoint.y)
        var nsx = sx, nsy = sy
        if abs(original.x) > 1e-9, h != .top, h != .bottom { nsx = dragged.x / original.x }
        if abs(original.y) > 1e-9, h != .left, h != .right { nsy = dragged.y / original.y }
        if constrain {
            if h.isCorner {
                // Keep sy / sx; the axis that moved more leads.
                let ratio = sy / sx
                if abs(nsx - sx) >= abs(nsy - sy) { nsy = nsx * ratio } else { nsx = nsy / ratio }
            } else if h == .top || h == .bottom {
                nsx = sx * (nsy / sy)
            } else {
                nsy = sy * (nsx / sx)
            }
        }
        // Avoid a singular matrix.
        if abs(nsx) < 1e-4 { nsx = nsx < 0 ? -1e-4 : 1e-4 }
        if abs(nsy) < 1e-4 { nsy = nsy < 0 ? -1e-4 : 1e-4 }
        sx = nsx
        sy = nsy
        // Keep the anchor where it was on the canvas.
        let moved = matrix.apply(anchorPoint)
        tx += anchorNow.x - moved.x
        ty += anchorNow.y - moved.y
    }

    /// A rotation drag outside the box: the angle follows the pointer around the reference point;
    /// ⇧ snaps to 15°.
    public mutating func rotate(from start: CGPoint, to p: CGPoint, startAngle: Double, snap: Bool) {
        let c = matrix.apply(reference)
        let a0 = atan2(start.y - c.y, start.x - c.x), a1 = atan2(p.y - c.y, p.x - c.x)
        var deg = startAngle + (a1 - a0) * 180 / .pi
        if snap { deg = (deg / 15).rounded() * 15 }
        deg = deg.truncatingRemainder(dividingBy: 360)
        if deg > 180 { deg -= 360 }
        if deg <= -180 { deg += 360 }
        let before = matrix.apply(reference)
        angle = deg
        let after = matrix.apply(reference)
        tx += before.x - after.x
        ty += before.y - after.y
    }

    /// Moving the whole box.
    public mutating func move(by d: CGSize) {
        tx += d.width
        ty += d.height
    }

    /// Whether canvas point `p` lies inside the transformed box.
    public func contains(_ p: CGPoint) -> Bool {
        guard let inv = matrix.inverse else { return false }
        return bounds.contains(inv.apply(p))
    }

    /// Photoshop's W / H percentages and X / Y of the reference point for the options bar.
    public var widthPercent: Double { sx * 100 }
    public var heightPercent: Double { sy * 100 }
    public var referenceNow: CGPoint { matrix.apply(reference) }
}
