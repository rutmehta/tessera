import Foundation
import IOSurface

/// Image-dependent adjustment parameters (WP B5-06). The engine stores Equalize, Auto, Match Color, Color
/// Lookup and HDR Toning's Equalize Histogram as frozen parameters (COMPOSITOR.md §4.1) and has no FFI
/// constructor for them, so the app analyses the pixels it can read (backend thumbnails, RGBA8) with Swift
/// ports of adjust/statistics.rs, adjust/hdr.rs and adjust/lookup.rs and sends the resulting JSON.
public enum AdjustmentAnalysis {
    /// Straight RGB samples in [0, 1] of a backend thumbnail surface (RGBA8, straight alpha); transparent
    /// pixels are skipped.
    public static func samples(surfaceID: UInt32) -> [SIMD3<Float>] {
        guard let s = IOSurfaceLookup(surfaceID) else { return [] }
        let w = IOSurfaceGetWidth(s), h = IOSurfaceGetHeight(s), stride = IOSurfaceGetBytesPerRow(s)
        IOSurfaceLock(s, .readOnly, nil)
        defer { IOSurfaceUnlock(s, .readOnly, nil) }
        let base = IOSurfaceGetBaseAddress(s).assumingMemoryBound(to: UInt8.self)
        var out: [SIMD3<Float>] = []
        out.reserveCapacity(w * h)
        for y in 0..<h {
            let row = base + y * stride
            for x in 0..<w where row[x * 4 + 3] > 0 {
                out.append(SIMD3(Float(row[x * 4]), Float(row[x * 4 + 1]), Float(row[x * 4 + 2])) / 255)
            }
        }
        return out
    }

    /// Per-channel `bins`-entry histograms over [0, 1].
    public static func histograms(_ pixels: [SIMD3<Float>], bins: Int = 256) -> [[UInt64]] {
        var h = Array(repeating: Array(repeating: UInt64(0), count: bins), count: 3)
        let top = Float(bins - 1)
        for p in pixels {
            for c in 0..<3 { h[c][Int((min(max(p[c], 0), 1) * top).rounded())] += 1 }
        }
        return h
    }

    // MARK: Auto (adjust/statistics.rs `auto_from_histogram`)

    private static func endpoints(_ h: [Double], shadow: Double, highlight: Double) -> (Double, Double) {
        let total = h.reduce(0, +)
        guard total > 0 else { return (0, 1) }
        var sum = 0.0, lo = 0, hi = h.count - 1
        for (i, v) in h.enumerated() {
            sum += v
            if sum > total * shadow { lo = i; break }
        }
        sum = 0
        for (i, v) in h.enumerated().reversed() {
            sum += v
            if sum > total * highlight { hi = i; break }
        }
        guard hi > lo else { return (0, 1) }
        let n = Double(h.count - 1)
        return (Double(Float(Double(lo) / n)), Double(Float(Double(hi) / n)))
    }

    /// `clip` is the fraction clipped from each tail, [0, 0.5).
    public static func auto(_ mode: AutoModeModel, histograms h: [[UInt64]], clip: Double) -> AutoAdjustmentModel {
        auto(mode, histograms: h, shadowClip: clip * 100, highlightClip: clip * 100)
    }

    /// M5-32: `shadowClip` / `highlightClip` are the percent clipped from the dark and light tails (their sum
    /// below 100); the result stores them, so the frozen endpoints and the persisted clips agree.
    public static func auto(_ mode: AutoModeModel, histograms h: [[UInt64]], shadowClip: Double,
                            highlightClip: Double) -> AutoAdjustmentModel {
        let hs = h.map { $0.map(Double.init) }
        let sc = min(max(shadowClip.isFinite ? shadowClip : 0, 0), 49.99)
        let hc = min(max(highlightClip.isFinite ? highlightClip : 0, 0), 49.99)
        let shadow = sc / 100, highlight = hc / 100
        var m = AutoAdjustmentModel(mode: mode, black: [0, 0, 0], white: [1, 1, 1], gamma: [1, 1, 1],
                                    shadowClip: sc, highlightClip: hc)
        guard hs.count == 3, hs.allSatisfy({ $0.count >= 2 && $0.count == hs[0].count }) else { return m }
        if mode == .contrast {
            let pooled = (0..<hs[0].count).map { hs[0][$0] + hs[1][$0] + hs[2][$0] }
            let (b, w) = endpoints(pooled, shadow: shadow, highlight: highlight)
            m.black = [b, b, b]; m.white = [w, w, w]
            return m
        }
        for i in 0..<3 {
            (m.black[i], m.white[i]) = endpoints(hs[i], shadow: shadow, highlight: highlight)
            guard mode == .color else { continue }
            let total = hs[i].reduce(0, +), n = Double(hs[i].count - 1)
            guard total > 0 else { continue }
            let mean = hs[i].enumerated().reduce(0.0) { acc, e in
                acc + e.element * min(max((Double(e.offset) / n - m.black[i]) / (m.white[i] - m.black[i]), 0), 1)
            } / total
            if mean > 0, mean < 1 { m.gamma[i] = Double(Float(min(max(log(mean) / log(0.5), 0.1), 10))) }
        }
        return m
    }

    // MARK: Equalize (`equalize_from_histogram`)

    public static func equalizeMaps(_ h: [[UInt64]]) -> [[Double]] {
        h.map { cdfMap($0) }
    }

    /// CDF-min normalization; empty or constant populations map linearly.
    static func cdfMap(_ h: [UInt64]) -> [Double] {
        let total = h.reduce(0.0) { $0 + Double($1) }
        let first = Double(h.first { $0 > 0 } ?? 0)
        var sum = 0.0
        return h.enumerated().map { j, v in
            sum += Double(v)
            let x = total <= first ? Double(j) / Double(h.count - 1) : min(max((sum - first) / (total - first), 0), 1)
            return Double(Float(x))
        }
    }

    // MARK: HDR Toning, Equalize Histogram (`HdrToning::equalize_from_histogram`)

    public static func hdrEqualize(_ pixels: [SIMD3<Float>], bins: Int = 256, base: HDRToningModel = .init()) -> HDRToningModel {
        let lum = pixels.map { 0.2126 * $0.x + 0.7152 * $0.y + 0.0722 * $0.z }
        let maxL = Double(lum.max() ?? 1) > 0 ? Double(lum.max() ?? 1) : 1
        var h = Array(repeating: UInt64(0), count: bins)
        for l in lum { h[min(max(Int((Double(l) / maxL * Double(bins - 1)).rounded()), 0), bins - 1)] += 1 }
        var m = base
        m.method = .equalizeHistogram
        m.equalizeMap = cdfMap(h)
        m.equalizeMax = Double(Float(maxL))
        return m
    }

    // MARK: Match Color (`match_color_from_pixels`, CIE Lab D65)

    static func decode(_ v: Float) -> Double {
        let v = Double(v)
        return v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
    }

    public static func lab(_ c: SIMD3<Float>) -> [Double] {
        let (r, g, b) = (decode(c.x), decode(c.y), decode(c.z))
        func f(_ v: Double) -> Double { v > 216.0 / 24389.0 ? cbrt(v) : (24389.0 / 27.0 * v + 16) / 116 }
        let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047)
        let y = f(0.2126729 * r + 0.7151522 * g + 0.0721750 * b)
        let z = f((0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883)
        return [116 * y - 16, 500 * (x - y), 200 * (y - z)]
    }

    /// Population mean and standard deviation in Lab (Welford).
    public static func labStats(_ pixels: [SIMD3<Float>]) -> (mean: [Double], std: [Double])? {
        guard !pixels.isEmpty else { return nil }
        var mean = [0.0, 0, 0], m2 = [0.0, 0, 0]
        for (j, p) in pixels.enumerated() {
            let l = lab(p)
            for i in 0..<3 {
                let d = l[i] - mean[i]
                mean[i] += d / Double(j + 1)
                m2[i] += d * (l[i] - mean[i])
            }
        }
        return (mean.map { Double(Float($0)) }, m2.map { Double(Float((max($0, 0) / Double(pixels.count)).squareRoot())) })
    }

    /// Frozen statistics of `source` (layer `sourceLayer`) and `target`, keeping the model's sliders.
    public static func matchColor(sourceLayer: UInt64, source: [SIMD3<Float>], target: [SIMD3<Float>],
                                  neutralize: Bool, keeping base: MatchColorModel = .init()) -> MatchColorModel? {
        guard sourceLayer != 0, let s = labStats(source), let t = labStats(target) else { return nil }
        var m = base
        m.sourceLayer = sourceLayer
        m.sourceMean = s.mean
        m.neutralize = neutralize   // M5-32: the engine removes the mean chroma (was: a zeroed source mean)
        m.sourceStd = s.std
        m.targetMean = t.mean
        m.targetStd = t.std
        return m
    }

    // MARK: Black & White Auto

    /// Slider values whose gray mix reproduces the image's Rec. 601 luma as closely as possible (least squares
    /// over the engine's hue-band interpolation, lightly pulled towards Photoshop's defaults so bands the
    /// image lacks keep them), rounded to whole percent in −200…300.
    public static func blackWhiteAuto(_ pixels: [SIMD3<Float>]) -> [Double] {
        var ata = Array(repeating: Array(repeating: 0.0, count: 6), count: 6)
        var atb = Array(repeating: 0.0, count: 6)
        let step = max(1, pixels.count / 40_000)
        var n = 0.0
        for k in stride(from: 0, to: pixels.count, by: step) {
            let c = pixels[k]
            let mx = Double(max(c.x, c.y, c.z)), mn = Double(min(c.x, c.y, c.z))
            let chroma = mx - mn
            guard chroma > 1e-3 else { continue }
            let h = Double(hue(c)) * 6
            let i = Int(h.rounded(.down)) % 6, f = h - h.rounded(.down)
            var a = Array(repeating: 0.0, count: 6)
            a[i] += chroma * (1 - f) / 100
            a[(i + 1) % 6] += chroma * f / 100
            let y = 0.299 * Double(c.x) + 0.587 * Double(c.y) + 0.114 * Double(c.z)
            let target = y - mn
            for r in 0..<6 {
                atb[r] += a[r] * target
                for q in 0..<6 { ata[r][q] += a[r] * a[q] }
            }
            n += 1
        }
        guard n > 0 else { return BlackWhitePresets.default }
        let lambda = 1e-6 * n
        for r in 0..<6 {
            ata[r][r] += lambda
            atb[r] += lambda * BlackWhitePresets.default[r]
        }
        guard let s = solve(ata, atb) else { return BlackWhitePresets.default }
        return s.map { min(max($0.rounded(), -200), 300) }
    }

    /// HSL hue in [0, 1) (adjust.rs `rgb_to_hsl`).
    static func hue(_ c: SIMD3<Float>) -> Float {
        let mx = max(c.x, c.y, c.z), mn = min(c.x, c.y, c.z), d = mx - mn
        guard d > 0 else { return 0 }
        var h: Float
        if mx == c.x { h = (c.y - c.z) / d + (c.y < c.z ? 6 : 0) } else if mx == c.y { h = (c.z - c.x) / d + 2 } else { h = (c.x - c.y) / d + 4 }
        return (h / 6).truncatingRemainder(dividingBy: 1)
    }

    /// Gaussian elimination with partial pivoting.
    static func solve(_ a0: [[Double]], _ b0: [Double]) -> [Double]? {
        var a = a0, b = b0
        let n = b.count
        for c in 0..<n {
            guard let p = (c..<n).max(by: { abs(a[$0][c]) < abs(a[$1][c]) }), abs(a[p][c]) > 1e-12 else { return nil }
            a.swapAt(c, p); b.swapAt(c, p)
            for r in (c + 1)..<n {
                let f = a[r][c] / a[c][c]
                for q in c..<n { a[r][q] -= f * a[c][q] }
                b[r] -= f * b[c]
            }
        }
        var x = Array(repeating: 0.0, count: n)
        for r in (0..<n).reversed() {
            x[r] = (b[r] - ((r + 1)..<n).reduce(0) { $0 + a[r][$1] * x[$1] }) / a[r][r]
        }
        return x
    }
}

/// `.cube` / `.3dl` loaders (Swift ports of adjust/lookup.rs): red-fastest RGB samples in [0, 1].
public enum ColorLookupFile {
    public struct Failure: LocalizedError, Equatable {
        public let message: String
        public var errorDescription: String? { message }
    }

    /// The identity cube of `size` knots per axis, flattened.
    public static func identity(size: Int) -> [Double] {
        var out: [Double] = []
        out.reserveCapacity(3 * size * size * size)
        let n = Double(size - 1)
        for b in 0..<size { for g in 0..<size { for r in 0..<size {
            out += [Double(Float(Double(r) / n)), Double(Float(Double(g) / n)), Double(Float(Double(b) / n))]
        } } }
        return out
    }

    public static func load(_ url: URL) throws -> (size: Int, data: [Double]) {
        let text = try String(contentsOf: url, encoding: .utf8)
        switch url.pathExtension.lowercased() {
        case "cube": return try cube(text)
        case "3dl": return try threeDL(text)
        default: throw Failure(message: "Color Lookup reads .cube and .3dl files")
        }
    }

    private static func fields(_ line: Substring) -> [Substring] {
        (line.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false).first ?? "").split(whereSeparator: \.isWhitespace)
    }

    private static func triple(_ f: [Substring]) throws -> [Double] {
        guard f.count == 3 else { throw Failure(message: "expected three numeric components") }
        return try f.map {
            guard let v = Float($0), v.isFinite else { throw Failure(message: "invalid number “\($0)”") }
            return Double(v)
        }
    }

    /// Unit-domain 3D CUBE (1D / shaper LUTs and other domains are rejected, as in the engine).
    public static func cube(_ text: String) throws -> (size: Int, data: [Double]) {
        var size: Int?
        var data: [Double] = []
        for line in text.split(whereSeparator: \.isNewline) {
            let f = fields(line)
            guard let head = f.first else { continue }
            switch head {
            case "TITLE": continue
            case "LUT_3D_SIZE":
                guard size == nil, f.count == 2, let n = Int(f[1]), (2...256).contains(n) else {
                    throw Failure(message: "invalid LUT_3D_SIZE (2…256)")
                }
                size = n
            case "DOMAIN_MIN", "DOMAIN_MAX":
                let d = try triple(Array(f.dropFirst()))
                guard d == Array(repeating: head == "DOMAIN_MIN" ? 0 : 1, count: 3) else {
                    throw Failure(message: "non-unit CUBE domains are not supported")
                }
            default:
                if head.hasPrefix("LUT_") { throw Failure(message: "only 3D CUBE LUTs are supported") }
                guard let n = size else { throw Failure(message: "LUT_3D_SIZE must precede the data") }
                guard data.count < 3 * n * n * n else { throw Failure(message: "too many LUT entries") }
                data += try triple(f)
            }
        }
        guard let n = size else { throw Failure(message: "missing LUT_3D_SIZE") }
        guard data.count == 3 * n * n * n else { throw Failure(message: "entry count does not match \(n)³") }
        return (n, data)
    }

    /// Blue-fastest integer 3DL with a uniform input row; the output scale is the smallest of 1, 1023, 4095
    /// and 65535 that holds every value (the engine takes it explicitly).
    public static func threeDL(_ text: String) throws -> (size: Int, data: [Double]) {
        var lines = text.split(whereSeparator: \.isNewline).map(fields).filter { !$0.isEmpty }.makeIterator()
        guard let first = lines.next() else { throw Failure(message: "missing 3DL input knots") }
        let knots = try first.map { s -> Float in
            guard let v = Float(s), v.isFinite else { throw Failure(message: "invalid 3DL knot") }
            return v
        }
        let n = knots.count
        guard (2...256).contains(n), knots[0] == 0, knots[n - 1] > 0, zip(knots, knots.dropFirst()).allSatisfy({ $0 < $1 }) else {
            throw Failure(message: "invalid 3DL input grid")
        }
        let top = knots[n - 1]
        guard knots.enumerated().allSatisfy({ abs($0.element - Float($0.offset) * top / Float(n - 1)) <= 1 }) else {
            throw Failure(message: "nonuniform 3DL shapers are not supported")
        }
        var raw: [[Double]] = []
        while let f = lines.next() {
            guard raw.count < n * n * n else { throw Failure(message: "too many 3DL entries") }
            raw.append(try triple(f))
        }
        guard raw.count == n * n * n else { throw Failure(message: "3DL entry count does not match the grid") }
        let peak = raw.joined().max() ?? 1
        guard raw.joined().allSatisfy({ $0 >= 0 }) else { throw Failure(message: "negative 3DL output") }
        guard let scale = [1.0, 1023, 4095, 65535].first(where: { peak <= $0 }) else {
            throw Failure(message: "3DL output above 16-bit")
        }
        var out = Array(repeating: 0.0, count: 3 * n * n * n)
        for r in 0..<n { for g in 0..<n { for b in 0..<n {
            let src = raw[b + n * (g + n * r)], dst = 3 * (r + n * (g + n * b))
            for c in 0..<3 { out[dst + c] = Double(Float(src[c] / scale)) }
        } } }
        return (n, out)
    }
}
