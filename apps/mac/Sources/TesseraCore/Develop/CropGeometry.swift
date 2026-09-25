import Foundation

/// Aspect presets of the crop tool.
public enum CropAspect: Hashable, Sendable, Identifiable {
    case free
    case original
    case ratio(Int, Int)

    public var id: String { title }
    public var title: String {
        switch self {
        case .free: "Free"
        case .original: "Original"
        case .ratio(let w, let h): "\(w) : \(h)"
        }
    }

    /// Ratios are stored long side first; `portrait` turns them.
    public static let presets: [CropAspect] = [.free, .original, .ratio(1, 1), .ratio(5, 4), .ratio(7, 5),
                                               .ratio(3, 2), .ratio(4, 3), .ratio(16, 9)]

    /// Width / height for an image of `imageAspect` (landscape-normalised ratios follow the crop's orientation).
    public func value(imageAspect: Double, portrait: Bool) -> Double? {
        switch self {
        case .free: return nil
        case .original: return portrait == (imageAspect < 1) ? imageAspect : 1 / imageAspect
        case .ratio(let w, let h):
            let r = Double(max(w, h)) / Double(min(w, h))
            return portrait ? 1 / r : r
        }
    }

    /// The engine's aspect hint (`[w, h]` in the displayed orientation), nil for free.
    public func hint(imageWidth: Int, imageHeight: Int, portrait: Bool) -> [Int]? {
        let pair: (Int, Int)
        switch self {
        case .free: return nil
        case .original: pair = (imageWidth, imageHeight)
        case .ratio(let w, let h): pair = (w, h)
        }
        let (lo, hi) = (min(pair.0, pair.1), max(pair.0, pair.1))
        return portrait ? [lo, hi] : [hi, lo]
    }

    /// The preset a stored hint names (`.original` when it matches the image's own ratio).
    public init?(hint: [Int]?, imageWidth: Int, imageHeight: Int) {
        guard let h = hint, h.count == 2, h[0] > 0, h[1] > 0 else { return nil }
        let (hi, lo) = (max(h[0], h[1]), min(h[0], h[1]))
        if hi * min(imageWidth, imageHeight) == lo * max(imageWidth, imageHeight) {
            self = .original
        } else {
            self = .ratio(hi, lo)
        }
    }
}

/// Overlay drawn inside the crop box (cycled with O).
public enum CropOverlay: String, CaseIterable, Sendable {
    case thirds, grid, goldenRatio, diagonals, none
    public var title: String {
        switch self {
        case .thirds: "Thirds"; case .grid: "Grid"; case .goldenRatio: "Golden Ratio"
        case .diagonals: "Diagonals"; case .none: "None"
        }
    }
    public var next: CropOverlay {
        let all = Self.allCases
        return all[(all.firstIndex(of: self)! + 1) % all.count]
    }
}

/// Crop and straighten in the *displayed* orientation, in full-resolution pixels of the displayed
/// image (`width × height`). The engine stores the crop in sensor orientation; `engineValues` and
/// `init(engine…)` convert through the EXIF orientation. Geometry follows pipeline-cpu
/// GEOMETRY_EFFECTS_M2.md: an image point is `center + R(angle)·q` for crop-frame offset `q`, with
/// `R = [cos sin; −sin cos]` in y-down coordinates (positive angle turns the content clockwise).
public struct CropGeometry: Equatable, Sendable {
    public var width: Double
    public var height: Double
    /// Crop centre and size, displayed-image pixels.
    public var centerX: Double
    public var centerY: Double
    public var cropWidth: Double
    public var cropHeight: Double
    /// Degrees, displayed orientation, within ±45.
    public var angle: Double

    public static let maxAngle = 45.0

    public init(width: Double, height: Double) {
        self.width = width
        self.height = height
        centerX = width / 2
        centerY = height / 2
        cropWidth = width
        cropHeight = height
        angle = 0
    }

    public var isIdentity: Bool {
        angle == 0 && abs(cropWidth - width) < 0.5 && abs(cropHeight - height) < 0.5
            && abs(centerX - width / 2) < 0.5 && abs(centerY - height / 2) < 0.5
    }

    public var aspect: Double { cropWidth / max(cropHeight, 1e-9) }

    // MARK: Mapping

    /// Image point of crop-frame offset `(qx, qy)` from the crop centre.
    public func imagePoint(qx: Double, qy: Double) -> (x: Double, y: Double) {
        let r = angle * .pi / 180
        return (centerX + cos(r) * qx + sin(r) * qy, centerY - sin(r) * qx + cos(r) * qy)
    }

    /// Crop-frame offset of an image point.
    public func cropPoint(x: Double, y: Double) -> (qx: Double, qy: Double) {
        let r = angle * .pi / 180
        let (dx, dy) = (x - centerX, y - centerY)
        return (cos(r) * dx - sin(r) * dy, sin(r) * dx + cos(r) * dy)
    }

    /// Image point (normalised to the displayed image) of normalised crop-output coordinates.
    public func imageUV(fromCropUV u: Double, _ v: Double) -> (u: Double, v: Double) {
        let p = imagePoint(qx: (u - 0.5) * cropWidth, qy: (v - 0.5) * cropHeight)
        return (p.x / width, p.y / height)
    }

    public var corners: [(x: Double, y: Double)] {
        let (w, h) = (cropWidth / 2, cropHeight / 2)
        return [(-w, -h), (w, -h), (w, h), (-w, h)].map { imagePoint(qx: $0.0, qy: $0.1) }
    }

    /// All four corners inside the image (with a tolerance for rounding), and the stored
    /// rectangle (centre ± half size, which the engine rotates about) inside it too.
    public var fitsImage: Bool {
        let inside = { (x: Double, y: Double) in x >= -0.01 && y >= -0.01 && x <= width + 0.01 && y <= height + 0.01 }
        return corners.allSatisfy { inside($0.x, $0.y) }
            && inside(centerX - cropWidth / 2, centerY - cropHeight / 2)
            && inside(centerX + cropWidth / 2, centerY + cropHeight / 2)
    }

    // MARK: Constraints

    /// Shrinks the crop about its centre (keeping its aspect) until it lies inside the image;
    /// the centre is first pulled inside the image.
    public mutating func constrainToImage() {
        centerX = min(max(centerX, 1), width - 1)
        centerY = min(max(centerY, 1), height - 1)
        if fitsImage { return }
        let (w0, h0) = (cropWidth, cropHeight)
        var lo = 0.0, hi = 1.0
        for _ in 0..<40 {
            let mid = (lo + hi) / 2
            cropWidth = w0 * mid; cropHeight = h0 * mid
            if fitsImage { lo = mid } else { hi = mid }
        }
        cropWidth = max(w0 * lo, 1); cropHeight = max(h0 * lo, 1)
    }

    /// The largest crop of `aspect` (width / height; nil keeps the current one) centred on the
    /// current centre that fits the image at the current angle.
    public mutating func fitLargest(aspect: Double? = nil) {
        let a = aspect ?? self.aspect
        let diag = hypot(width, height) * 2
        cropWidth = a >= 1 ? diag : diag * a
        cropHeight = cropWidth / a
        constrainToImage()
    }

    /// Applies an aspect ratio keeping the crop's area roughly, then fits it inside the image.
    public mutating func setAspect(_ a: Double) {
        let area = cropWidth * cropHeight
        cropWidth = sqrt(area * a)
        cropHeight = cropWidth / a
        constrainToImage()
    }

    /// Resizes by dragging an edge/corner. `dx, dy` are crop-frame deltas of the handle;
    /// `sx, sy` ∈ {−1, 0, 1} say which edges move. With `aspect` the ratio is kept.
    public mutating func resize(sx: Int, sy: Int, dx: Double, dy: Double, aspect: Double?, constrain: Bool) {
        var w = cropWidth + Double(sx) * dx
        var h = cropHeight + Double(sy) * dy
        w = max(w, 16); h = max(h, 16)
        if let a = aspect {
            if sx != 0 && sy != 0 {
                if w / h > a { h = w / a } else { w = h * a }
            } else if sx != 0 { h = w / a } else { w = h * a }
        }
        // The fixed edge stays put: move the centre by half the change along the moving axes.
        let qx = sx == 0 ? 0 : Double(sx) * (w - cropWidth) / 2
        let qy = sy == 0 ? 0 : Double(sy) * (h - cropHeight) / 2
        let c = imagePoint(qx: qx, qy: qy)
        let previous = self
        centerX = c.x; centerY = c.y; cropWidth = w; cropHeight = h
        if constrain, !fitsImage { self = previous }
    }

    /// Moves the crop by an image-space offset, keeping it inside the image when constrained.
    public mutating func move(dx: Double, dy: Double, constrain: Bool) {
        let previous = self
        centerX += dx; centerY += dy
        guard constrain, !fitsImage else { return }
        // Try each axis alone so the box slides along an edge.
        self = previous; centerX += dx
        if !fitsImage { centerX = previous.centerX }
        centerY += dy
        if !fitsImage { centerY = previous.centerY }
    }

    /// Sets the straighten angle; when constrained the crop shrinks to stay inside the image.
    public mutating func rotate(to degrees: Double, constrain: Bool) {
        angle = min(max(degrees, -Self.maxAngle), Self.maxAngle)
        if constrain { constrainToImage() }
    }

    /// Angle that levels a line drawn on screen from `a` to `b` (screen y-down coordinates,
    /// drawn over the image as currently displayed at `angle`): horizontal or vertical,
    /// whichever is closer.
    public func straightenAngle(from a: (x: Double, y: Double), to b: (x: Double, y: Double)) -> Double? {
        let (dx, dy) = (b.x - a.x, b.y - a.y)
        guard hypot(dx, dy) > 4 else { return nil }
        var alpha = atan2(dy, dx) * 180 / .pi          // screen angle of the drawn line
        while alpha > 45 { alpha -= 90 }                // nearest horizontal/vertical
        while alpha < -45 { alpha += 90 }
        return min(max(angle - alpha, -Self.maxAngle), Self.maxAngle)
    }

    // MARK: Engine conversion

    /// Displayed uv → stored (sensor) uv for EXIF orientation `o` (same mapping as the loupe shader).
    public static func orient(_ u: Double, _ v: Double, _ o: Int) -> (Double, Double) {
        switch o {
        case 2: (1 - u, v)
        case 3: (1 - u, 1 - v)
        case 4: (u, 1 - v)
        case 5: (v, u)
        case 6: (v, 1 - u)
        case 7: (1 - v, 1 - u)
        case 8: (1 - v, u)
        default: (u, v)
        }
    }

    /// Inverse of `orient`.
    public static func unorient(_ s: Double, _ t: Double, _ o: Int) -> (Double, Double) {
        switch o {
        case 2: (1 - s, t)
        case 3: (1 - s, 1 - t)
        case 4: (s, 1 - t)
        case 5: (t, s)
        case 6: (1 - t, s)
        case 7: (1 - t, 1 - s)
        case 8: (t, 1 - s)
        default: (s, t)
        }
    }

    static func mirrored(_ o: Int) -> Bool { [2, 4, 5, 7].contains(o) }

    /// From the engine's crop (`rect` normalised in sensor orientation, `angle`) for an image whose
    /// displayed size is `width × height` and EXIF orientation `orientation`.
    public init(engineRect r: (left: Double, top: Double, right: Double, bottom: Double), angle: Double,
                width: Double, height: Double, orientation: Int) {
        self.init(width: width, height: height)
        let a = Self.unorient(r.left, r.top, orientation), b = Self.unorient(r.right, r.bottom, orientation)
        let (l, rr) = (min(a.0, b.0), max(a.0, b.0)), (t, bb) = (min(a.1, b.1), max(a.1, b.1))
        centerX = (l + rr) / 2 * width
        centerY = (t + bb) / 2 * height
        // The rectangle's centre and size are orientation-independent in pixels; for 90° turns
        // the normalised extents swap axes, which unorient already did.
        cropWidth = (rr - l) * width
        cropHeight = (bb - t) * height
        self.angle = Self.mirrored(orientation) ? -angle : angle
    }

    /// The engine crop: rect normalised in sensor orientation, angle in the sensor convention.
    /// The rect is the crop's centre ± half size (the engine rotates about the rect's centre).
    public func engineValues(orientation: Int) -> (left: Double, top: Double, right: Double, bottom: Double, angle: Double) {
        let l = (centerX - cropWidth / 2) / width, r = (centerX + cropWidth / 2) / width
        let t = (centerY - cropHeight / 2) / height, b = (centerY + cropHeight / 2) / height
        let p = Self.orient(l, t, orientation), q = Self.orient(r, b, orientation)
        // Clamped into [0, 1] and rounded to 1e-5 (readable recipes); strictly ordered for the engine.
        let round = { (v: Double) in (min(max(v, 0), 1) * 100_000).rounded() / 100_000 }
        let a = (angle * 100).rounded() / 100
        var out = (left: round(min(p.0, q.0)), top: round(min(p.1, q.1)), right: round(max(p.0, q.0)),
                   bottom: round(max(p.1, q.1)), angle: Self.mirrored(orientation) ? -a : a)
        if out.right <= out.left { out.right = min(out.left + 0.001, 1); out.left = out.right - 0.001 }
        if out.bottom <= out.top { out.bottom = min(out.top + 0.001, 1); out.top = out.bottom - 0.001 }
        return out
    }

    /// Engine merge patch for this crop (the aspect hint included when locked).
    public func patch(orientation: Int, aspectHint: [Int]?) -> [String: Any] {
        let e = engineValues(orientation: orientation)
        var crop: [String: Any] = [
            "rect": ["left": e.left, "top": e.top, "right": e.right, "bottom": e.bottom],
            "angle": e.angle,
        ]
        crop["aspect"] = aspectHint.map { $0 as Any } ?? NSNull()
        return ["geometry": ["crop": crop]]
    }
}
