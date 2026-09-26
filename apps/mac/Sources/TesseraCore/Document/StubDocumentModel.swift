import CoreGraphics
import Foundation
import ImageIO

// The stub document's data and its CPU compositor (WP M5-10). Everything is a pure per-pixel
// function of the layer tree, so the stub renders any viewport rectangle at any level without
// storing rasters. Pixel content is procedural (the sample document), a decoded image file, or a
// "merged" snapshot of other layers (merge down, flatten). Formulas follow COMPOSITOR.md §2–4
// closely enough to look right; the engine (M5-09) is the reference.

/// Straight RGBA in the document's encoding, 0…1.
typealias RGBA = SIMD4<Float>

struct StubProps: Codable, Equatable, Sendable {
    var name: String
    var visible = true
    var opacity: Float = 1
    var fillOpacity: Float = 1
    var blendMode = "normal"
    var clipped = false
    var locks = LayerLockFlags()
    var background = false
    var colorTag: String?
}

struct StubMask: Codable, Equatable, Sendable {
    enum Shape: Codable, Equatable, Sendable {
        case reveal
        case hide
        /// Revealed inside the rectangle (from a marquee selection), with a feather in pixels.
        case rect(CanvasRect, feather: Float)
        /// Revealed inside an ellipse inscribed in the rectangle (the sample document).
        case ellipse(CanvasRect)
    }
    var shape: Shape
    var enabled = true
    var linked = true
    /// Effective mask `1 − d·(1 − m)`.
    var density: Float = 1

    func value(_ x: Float, _ y: Float) -> Float { 1 - density * (1 - raw(x, y)) }

    func raw(_ x: Float, _ y: Float) -> Float {
        switch shape {
        case .reveal: return 1
        case .hide: return 0
        case .rect(let r, let feather):
            let dx = min(x - Float(r.x), Float(r.x) + Float(r.width) - x)
            let dy = min(y - Float(r.y), Float(r.y) + Float(r.height) - y)
            let d = min(dx, dy)
            if feather <= 0 { return d >= 0 ? 1 : 0 }
            return min(max(d / feather + 0.5, 0), 1)
        case .ellipse(let r):
            let cx = Float(r.x) + Float(r.width) / 2, cy = Float(r.y) + Float(r.height) / 2
            let nx = (x - cx) / max(Float(r.width) / 2, 1), ny = (y - cy) / max(Float(r.height) / 2, 1)
            let d = (nx * nx + ny * ny).squareRoot()
            return min(max((1.05 - d) / 0.25, 0), 1)
        }
    }
}

/// Procedural and file-backed pixel content.
indirect enum StubContent: Codable, Equatable, Sendable {
    case empty
    /// Built-in sample artwork by name ("paper", "landscape") over the canvas.
    case pattern(String, canvasWidth: Float, canvasHeight: Float)
    /// A decoded image file placed at the canvas origin.
    case image(path: String)
    /// The composite of these layers (bottom first), isolated.
    case merged([StubLayer])
}

indirect enum StubKind: Codable, Equatable, Sendable {
    case pixel(StubContent)
    case adjustment(json: String)
    case fill(json: String)
    case group(mode: LayerGroupMode, children: [StubLayer])
    case text(String)

    var tag: LayerKindTag {
        switch self {
        case .pixel: .pixel
        case .adjustment: .adjustment
        case .fill: .fill
        case .group: .group
        case .text: .text
        }
    }
}

struct StubLayer: Codable, Equatable, Sendable {
    var id: DocLayerID
    var props: StubProps
    var kind: StubKind
    var mask: StubMask?
    var revision: UInt64 = 1
}

/// The whole editable state: one history entry holds one of these.
struct StubState: Codable, Equatable, Sendable {
    var width: UInt32
    var height: UInt32
    var depth: DocBitDepth
    var profile: String
    /// Root children, bottom first (compositor order).
    var root: [StubLayer]
    var nextID: DocLayerID
    var revision: UInt64 = 1
    /// The marquee (part of the history, as in Photoshop).
    var selection: CanvasRect?
}

// MARK: - Tree helpers

extension StubState {
    /// Path of child indices to `id`.
    func path(of id: DocLayerID) -> [Int]? {
        func find(_ list: [StubLayer]) -> [Int]? {
            for (i, l) in list.enumerated() {
                if l.id == id { return [i] }
                if case .group(_, let kids) = l.kind, let p = find(kids) { return [i] + p }
            }
            return nil
        }
        return find(root)
    }

    func layer(_ id: DocLayerID) -> StubLayer? {
        guard let p = path(of: id) else { return nil }
        return layer(at: p)
    }

    func layer(at path: [Int]) -> StubLayer {
        var list = root
        var l = list[path[0]]
        for i in path.dropFirst() {
            guard case .group(_, let kids) = l.kind else { break }
            list = kids
            l = list[i]
        }
        return l
    }

    /// Children list (bottom first) of `parent` (nil = root).
    func children(of parent: DocLayerID?) -> [StubLayer]? {
        guard let parent else { return root }
        guard let l = layer(parent), case .group(_, let kids) = l.kind else { return nil }
        return kids
    }

    mutating func withChildren<T>(of parent: DocLayerID?, _ body: (inout [StubLayer]) throws -> T) throws -> T {
        guard let parent else { return try body(&root) }
        guard let p = path(of: parent) else { throw DocumentError.notFound("layer \(parent)") }
        return try Self.modify(&root, p) { layer in
            guard case .group(let mode, var kids) = layer.kind else { throw DocumentError.invalid("Not a group") }
            let r = try body(&kids)
            layer.kind = .group(mode: mode, children: kids)
            return r
        }
    }

    mutating func modify<T>(_ id: DocLayerID, _ body: (inout StubLayer) throws -> T) throws -> T {
        guard let p = path(of: id) else { throw DocumentError.notFound("layer \(id)") }
        revision += 1
        let rev = revision
        return try Self.modify(&root, p) { l in
            let r = try body(&l)
            l.revision = rev
            return r
        }
    }

    private static func modify<T>(_ list: inout [StubLayer], _ path: [Int], _ body: (inout StubLayer) throws -> T) throws -> T {
        if path.count == 1 { return try body(&list[path[0]]) }
        guard case .group(let mode, var kids) = list[path[0]].kind else { throw DocumentError.invalid("Bad path") }
        let r = try modify(&kids, Array(path.dropFirst()), body)
        list[path[0]].kind = .group(mode: mode, children: kids)
        return r
    }

    func parent(of id: DocLayerID) -> DocLayerID?? {
        guard let p = path(of: id) else { return nil }
        if p.count == 1 { return .some(nil) }
        return .some(layer(at: Array(p.dropLast())).id)
    }

    /// Removes and returns `id` with its subtree.
    mutating func take(_ id: DocLayerID) throws -> StubLayer {
        guard let parent = parent(of: id) else { throw DocumentError.notFound("layer \(id)") }
        return try withChildren(of: parent) { kids in kids.remove(at: kids.firstIndex { $0.id == id }!) }
    }

    /// Fresh ids for a copied subtree.
    mutating func renumbered(_ l: StubLayer) -> StubLayer {
        var c = l
        c.id = nextID
        nextID += 1
        if case .group(let mode, let kids) = c.kind { c.kind = .group(mode: mode, children: kids.map { renumbered($0) }) }
        return c
    }

    /// Flat pre-order, siblings top first (the `layers()` contract).
    var nodes: [LayerRecord] {
        var out: [LayerRecord] = []
        func maxRevision(_ l: StubLayer) -> UInt64 {
            if case .group(_, let kids) = l.kind { return kids.map(maxRevision).max().map { max($0, l.revision) } ?? l.revision }
            return l.revision
        }
        func walk(_ list: [StubLayer], parent: DocLayerID?, depth: UInt32) {
            for (i, l) in list.enumerated().reversed() {
                var n = LayerRecord(id: l.id, parent: parent, index: UInt32(i), depth: depth, kind: l.kind.tag, name: l.props.name,
                                    visible: l.props.visible, opacity: l.props.opacity, fillOpacity: l.props.fillOpacity,
                                    blendMode: l.props.blendMode, clipped: l.props.clipped, locks: l.props.locks,
                                    background: l.props.background,
                                    hasMask: l.mask != nil, maskEnabled: l.mask?.enabled ?? true, maskLinked: l.mask?.linked ?? true,
                                    maskDensity: l.mask?.density ?? 1, revision: maxRevision(l))
                switch l.kind {
                case .adjustment(let json): n.adjustmentJson = json
                case .fill(let json): n.fillJson = json
                case .group(let mode, _):
                    n.groupMode = mode
                    if mode == .passThrough { n.blendMode = "pass_through" }
                case .pixel(let c): n.bounds = c.bounds(width: width, height: height)
                case .text: break
                }
                out.append(n)
                if case .group(_, let kids) = l.kind { walk(kids, parent: l.id, depth: depth + 1) }
            }
        }
        walk(root, parent: nil, depth: 0)
        return out
    }
}

extension StubContent {
    func bounds(width: UInt32, height: UInt32) -> CanvasRect? {
        switch self {
        case .empty: return nil
        case .pattern(let name, _, _):
            if name == "paper" || name == "landscape" {
                let m = StubPatterns.inset(width: Float(width), height: Float(height))
                return CanvasRect(x: Int64(m.minX), y: Int64(m.minY), width: Int64(m.width), height: Int64(m.height))
            }
            return CanvasRect(x: 0, y: 0, width: Int64(width), height: Int64(height))
        case .image(let path):
            guard let r = StubImageCache.shared.raster(path) else { return nil }
            return CanvasRect(x: 0, y: 0, width: Int64(r.width), height: Int64(r.height))
        case .merged: return CanvasRect(x: 0, y: 0, width: Int64(width), height: Int64(height))
        }
    }
}

// MARK: - Sample artwork

enum StubPatterns {
    /// The artwork's inset rectangle: a transparent margin shows the checkerboard.
    static func inset(width: Float, height: Float) -> CGRect {
        let m = min(width, height) * 0.06
        return CGRect(x: CGFloat(m), y: CGFloat(m), width: CGFloat(width - 2 * m), height: CGFloat(height - 2 * m))
    }

    static func sample(_ name: String, _ x: Float, _ y: Float, _ w: Float, _ h: Float) -> RGBA {
        let r = inset(width: w, height: h)
        let (x0, y0, x1, y1) = (Float(r.minX), Float(r.minY), Float(r.maxX), Float(r.maxY))
        guard x >= x0, x < x1, y >= y0, y < y1 else { return .zero }
        let u = (x - x0) / (x1 - x0), v = (y - y0) / (y1 - y0)
        switch name {
        case "paper":
            // Warm paper with a faint fibre texture.
            let n = hash(Int(x / 3), Int(y / 3)) * 0.03
            return RGBA(0.93 - n, 0.90 - n, 0.84 - n, 1)
        case "landscape":
            // Dusk sky, a low sun, three ranges of hills and a lake.
            let horizon: Float = 0.58
            var c: SIMD3<Float>
            if v < horizon {
                let t = v / horizon
                c = mix(SIMD3(0.24, 0.32, 0.55), SIMD3(0.97, 0.66, 0.42), t * t)
                let sd = ((u - 0.68) * (u - 0.68) * 2.2 + (v - 0.47) * (v - 0.47) * 4).squareRoot()
                c = mix(c, SIMD3(1.0, 0.93, 0.74), max(0, min(1, (0.09 - sd) / 0.02)))
            } else {
                let t = (v - horizon) / (1 - horizon)
                c = mix(SIMD3(0.55, 0.45, 0.45), SIMD3(0.13, 0.16, 0.22), t)
                // Sun reflection streaks.
                if abs(u - 0.68) < 0.05 * (1 - t) { c = mix(c, SIMD3(0.98, 0.78, 0.55), 0.35 * (1 - t)) }
            }
            let ridges: [(Float, Float, Float, SIMD3<Float>)] = [
                (0.50, 0.05, 9, SIMD3(0.36, 0.33, 0.45)), (0.55, 0.06, 5, SIMD3(0.23, 0.25, 0.32)),
                (0.60, 0.07, 3, SIMD3(0.14, 0.17, 0.20)),
            ]
            for (base, amp, freq, col) in ridges {
                let ridge = base - amp * (0.6 * sin(u * freq + base * 11) + 0.4 * sin(u * freq * 2.3 + 1.7))
                if v > ridge, v < horizon { c = col }
            }
            return RGBA(c.x, c.y, c.z, 1)
        default:
            return .zero
        }
    }

    static func mix(_ a: SIMD3<Float>, _ b: SIMD3<Float>, _ t: Float) -> SIMD3<Float> { a + (b - a) * min(max(t, 0), 1) }

    static func hash(_ x: Int, _ y: Int) -> Float {
        var h = UInt32(truncatingIfNeeded: x &* 374_761_393 &+ y &* 668_265_263)
        h = (h ^ (h >> 13)) &* 1_274_126_177
        return Float(h & 0xFFFF) / 65535
    }
}

// MARK: - Image files

/// Decoded RGBA8 rasters of image files, shared by all stub documents.
final class StubImageCache: @unchecked Sendable {
    static let shared = StubImageCache()
    struct Raster: Sendable {
        let width: Int
        let height: Int
        let rgba: [UInt8]
        func sample(_ x: Float, _ y: Float) -> RGBA {
            let ix = Int(x), iy = Int(y)
            guard ix >= 0, iy >= 0, ix < width, iy < height else { return .zero }
            let o = (iy * width + ix) * 4
            let a = Float(rgba[o + 3]) / 255
            guard a > 0 else { return .zero }
            // Stored premultiplied by CoreGraphics: unpremultiply.
            return RGBA(Float(rgba[o]) / 255 / a, Float(rgba[o + 1]) / 255 / a, Float(rgba[o + 2]) / 255 / a, a)
        }
    }
    private var rasters: [String: Raster] = [:]
    private let lock = NSLock()
    /// Long edge of decoded images (the stub keeps documents light).
    static let maxEdge = 3000

    func raster(_ path: String) -> Raster? {
        lock.lock()
        if let r = rasters[path] { lock.unlock(); return r }
        lock.unlock()
        guard let r = Self.decode(URL(fileURLWithPath: path)) else { return nil }
        lock.lock()
        rasters[path] = r
        lock.unlock()
        return r
    }

    static func size(_ url: URL) -> (Int, Int)? {
        decode(url).map { ($0.width, $0.height) }
    }

    static func decode(_ url: URL) -> Raster? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let opts: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                                     kCGImageSourceCreateThumbnailWithTransform: true,
                                     kCGImageSourceThumbnailMaxPixelSize: maxEdge]
        guard let image = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary)
                ?? CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
        let w = image.width, h = image.height
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        let ok = bytes.withUnsafeMutableBytes { buf -> Bool in
            guard let ctx = CGContext(data: buf.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                      space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        return ok ? Raster(width: w, height: h, rgba: bytes) : nil
    }
}
