import Foundation
import simd

/// CPU compositor of the stub backend: the layer tree compiled once per render (JSON parsed,
/// LUTs built), then evaluated per pixel. Accumulates premultiplied RGBA (COMPOSITOR.md §2.1).
struct StubCompositor: Sendable {
    indirect enum Node: Sendable {
        case pixel(StubContent)
        case adjustment(Adjust)
        case fill(FillEval)
        case group(isolated: Bool, children: [Compiled])
        /// Pixel content that is itself a composite (merge down, flatten).
        case merged([Compiled])
        case empty
    }

    struct Compiled: Sendable {
        var node: Node
        var visible: Bool
        var opacity: Float
        var fill: Float
        var mode: DocBlendMode
        var clipped: Bool
        var mask: StubMask?
    }

    let root: [Compiled]
    let width: Float
    let height: Float

    init(_ state: StubState) {
        width = Float(state.width)
        height = Float(state.height)
        root = state.root.map(Self.compile)
    }

    init(layers: [StubLayer], width: Float, height: Float) {
        self.width = width
        self.height = height
        root = layers.map(Self.compile)
    }

    static func compile(_ l: StubLayer) -> Compiled {
        let node: Node
        switch l.kind {
        case .pixel(.merged(let layers)): node = .merged(layers.map(compile))
        case .pixel(let c): node = .pixel(c)
        case .adjustment(let json): node = AdjustmentModel(json: json).map { .adjustment(Adjust($0)) } ?? .empty
        case .fill(let json): node = FillModel(json: json).map { .fill(FillEval($0)) } ?? .empty
        case .group(let mode, let kids): node = .group(isolated: mode == .isolated, children: kids.map(compile))
        case .text: node = .empty
        }
        return Compiled(node: node, visible: l.props.visible, opacity: l.props.opacity, fill: l.props.fillOpacity,
                        mode: DocBlendMode(backendName: l.props.blendMode) ?? .normal, clipped: l.props.clipped,
                        mask: l.mask?.enabled == true ? l.mask : nil)
    }

    /// Straight RGBA of the composite at a canvas point.
    func sample(_ x: Float, _ y: Float) -> RGBA {
        let acc = composite(root, over: .zero, x, y)
        return acc.w > 0 ? RGBA(acc.x / acc.w, acc.y / acc.w, acc.z / acc.w, acc.w) : .zero
    }

    /// `acc` is premultiplied.
    func composite(_ list: [Compiled], over start: RGBA, _ x: Float, _ y: Float) -> RGBA {
        var acc = start
        var clipBase: Float = 1
        var baseVisible = true
        for l in list {
            if !l.clipped {
                baseVisible = l.visible
                clipBase = 1
            }
            guard l.visible, !(l.clipped && !baseVisible) else { continue }
            let mask = l.mask?.value(x, y) ?? 1
            switch l.node {
            case .empty: continue
            case .pixel, .merged:
                let s: RGBA
                if case .pixel(let content) = l.node { s = pixel(content, x, y) } else if case .merged(let kids) = l.node {
                    let inner = composite(kids, over: .zero, x, y)
                    s = inner.w > 0 ? RGBA(inner.x / inner.w, inner.y / inner.w, inner.z / inner.w, inner.w) : .zero
                } else { s = .zero }
                let shape = s.w * mask
                if !l.clipped { clipBase = shape }
                let a = shape * l.opacity * l.fill * (l.clipped ? clipBase : 1)
                acc = Self.blend(acc, RGBA(s.x, s.y, s.z, 1), alpha: a, mode: l.mode)
            case .fill(let f):
                let s = f.eval(x, y)
                let shape = s.w * mask
                if !l.clipped { clipBase = shape }
                acc = Self.blend(acc, RGBA(s.x, s.y, s.z, 1), alpha: shape * l.opacity * l.fill * (l.clipped ? clipBase : 1), mode: l.mode)
            case .adjustment(let adj):
                guard acc.w > 0 else { continue }
                let cb = SIMD3(acc.x, acc.y, acc.z) / acc.w
                let out = adj.apply(cb)
                if !l.clipped { clipBase = 1 }
                let a = mask * l.opacity * l.fill * (l.clipped ? clipBase : 1)
                // An adjustment's source is the adjusted backdrop, only where there is a backdrop.
                let blended = Self.blendColor(cb, out, mode: l.mode)
                let mixed = cb + (blended - cb) * a
                acc = RGBA(mixed.x * acc.w, mixed.y * acc.w, mixed.z * acc.w, acc.w)
            case .group(let isolated, let children):
                let a = mask * l.opacity * (l.clipped ? clipBase : 1)
                if isolated {
                    let inner = composite(children, over: .zero, x, y)
                    if !l.clipped { clipBase = inner.w * mask }
                    guard inner.w > 0 else { continue }
                    let s = SIMD3(inner.x, inner.y, inner.z) / inner.w
                    acc = Self.blend(acc, RGBA(s.x, s.y, s.z, 1), alpha: inner.w * a, mode: l.mode)
                } else {
                    let after = composite(children, over: acc, x, y)
                    acc = acc + (after - acc) * a
                }
            }
        }
        return acc
    }

    private func pixel(_ c: StubContent, _ x: Float, _ y: Float) -> RGBA {
        switch c {
        case .empty: return .zero
        case .pattern(let name, _, _): return StubPatterns.sample(name, x, y, width, height)
        case .image(let path): return StubImageCache.shared.raster(path)?.sample(x, y) ?? .zero
        case .merged: return .zero   // compiled into `Node.merged`
        }
    }

    // MARK: Blending

    /// Source-over with the blend function `B` (COMPOSITOR.md §2.1); `acc` premultiplied, `s` straight.
    static func blend(_ acc: RGBA, _ s: RGBA, alpha a: Float, mode: DocBlendMode) -> RGBA {
        guard a > 0 else { return acc }
        let ab = acc.w
        let cs = SIMD3(s.x, s.y, s.z)
        let cb = ab > 0 ? SIMD3(acc.x, acc.y, acc.z) / ab : .zero
        let b = ab > 0 ? blendColor(cb, cs, mode: mode) : cs
        let co = a * (1 - ab) * cs + a * ab * b + (1 - a) * SIMD3(acc.x, acc.y, acc.z)
        return RGBA(co.x, co.y, co.z, a + (1 - a) * ab)
    }

    static func blendColor(_ b: SIMD3<Float>, _ s: SIMD3<Float>, mode: DocBlendMode) -> SIMD3<Float> {
        func sep(_ f: (Float, Float) -> Float) -> SIMD3<Float> { SIMD3(f(b.x, s.x), f(b.y, s.y), f(b.z, s.z)) }
        switch mode {
        case .normal, .dissolve: return s
        case .darken: return sep { min($0, $1) }
        case .multiply: return b * s
        case .colorBurn: return sep { b, s in b >= 1 ? 1 : s <= 0 ? 0 : 1 - min(1, (1 - b) / s) }
        case .linearBurn: return sep { max(0, $0 + $1 - 1) }
        case .darkerColor: return (b.x + b.y + b.z) <= (s.x + s.y + s.z) ? b : s
        case .lighten: return sep { max($0, $1) }
        case .screen: return sep { $0 + $1 - $0 * $1 }
        case .colorDodge: return sep { b, s in b <= 0 ? 0 : s >= 1 ? 1 : min(1, b / (1 - s)) }
        case .linearDodge: return sep { min(1, $0 + $1) }
        case .lighterColor: return (b.x + b.y + b.z) > (s.x + s.y + s.z) ? b : s
        case .overlay: return sep { b, s in hardLight(s, b) }
        case .softLight:
            return sep { b, s in s <= 0.5 ? 2 * b * s + b * b * (1 - 2 * s) : 2 * b * (1 - s) + b.squareRoot() * (2 * s - 1) }
        case .hardLight: return sep { hardLight($0, $1) }
        case .vividLight:
            return sep { b, s in
                s <= 0.5 ? (b >= 1 ? 1 : 2 * s <= 0 ? 0 : 1 - min(1, (1 - b) / (2 * s)))
                    : (b <= 0 ? 0 : 2 * s - 1 >= 1 ? 1 : min(1, b / (1 - (2 * s - 1))))
            }
        case .linearLight: return sep { min(max($0 + 2 * $1 - 1, 0), 1) }
        case .pinLight: return sep { b, s in s <= 0.5 ? min(b, 2 * s) : max(b, 2 * s - 1) }
        case .hardMix: return sep { $0 + $1 >= 1 ? 1 : 0 }
        case .difference: return sep { abs($0 - $1) }
        case .exclusion: return sep { $0 + $1 - 2 * $0 * $1 }
        case .subtract: return sep { max(0, $0 - $1) }
        case .divide: return sep { b, s in s <= 0 ? (b <= 0 ? 0 : 1) : min(1, b / s) }
        case .hue: return setLum(setSat(s, sat(b)), lum(b))
        case .saturation: return setLum(setSat(b, sat(s)), lum(b))
        case .color: return setLum(s, lum(b))
        case .luminosity: return setLum(b, lum(s))
        }
    }

    static func hardLight(_ b: Float, _ s: Float) -> Float {
        s <= 0.5 ? 2 * b * s : { let t = 2 * s - 1; return b + t - b * t }()
    }

    static func lum(_ c: SIMD3<Float>) -> Float { 0.3 * c.x + 0.59 * c.y + 0.11 * c.z }
    static func sat(_ c: SIMD3<Float>) -> Float { max(c.x, c.y, c.z) - min(c.x, c.y, c.z) }
    static func clipColor(_ c: SIMD3<Float>) -> SIMD3<Float> {
        let l = lum(c), n = min(c.x, c.y, c.z), x = max(c.x, c.y, c.z)
        var c = c
        if n < 0 { c = SIMD3(repeating: l) + (c - SIMD3(repeating: l)) * l / (l - n) }
        if x > 1 { c = SIMD3(repeating: l) + (c - SIMD3(repeating: l)) * (1 - l) / (x - l) }
        return c
    }
    static func setLum(_ c: SIMD3<Float>, _ l: Float) -> SIMD3<Float> { clipColor(c + SIMD3(repeating: l - lum(c))) }
    static func setSat(_ c: SIMD3<Float>, _ s: Float) -> SIMD3<Float> {
        let mn = min(c.x, c.y, c.z), mx = max(c.x, c.y, c.z)
        guard mx > mn else { return .zero }
        return (c - SIMD3(repeating: mn)) * s / (mx - mn)
    }

    // MARK: Adjustments

    struct Adjust: Sendable {
        let model: AdjustmentModel
        /// Levels / curves: per-channel then master, as 256-entry LUTs.
        let luts: [[Float]]?

        init(_ m: AdjustmentModel) {
            model = m
            switch m {
            case .levels(let master, let rgb):
                luts = (0..<3).map { c in (0..<256).map { i in
                    Float(master.apply(rgb[c].apply(Double(i) / 255)))
                } }
            case .curves(let master, let rgb):
                luts = (0..<3).map { c in (0..<256).map { i in
                    Float(Self.curve(master, Self.curve(rgb[c], Double(i) / 255)))
                } }
            default: luts = nil
            }
        }

        /// Piecewise-linear through the points (the engine uses a monotone cubic).
        static func curve(_ pts: [[Double]], _ v: Double) -> Double {
            let p = pts.filter { $0.count == 2 }.sorted { $0[0] < $1[0] }
            guard p.count >= 2 else { return v }
            if v <= p[0][0] { return p[0][1] }
            for i in 1..<p.count where v <= p[i][0] {
                let t = (v - p[i - 1][0]) / max(p[i][0] - p[i - 1][0], 1e-9)
                return min(max(p[i - 1][1] + (p[i][1] - p[i - 1][1]) * t, 0), 1)
            }
            return p[p.count - 1][1]
        }

        func apply(_ c: SIMD3<Float>) -> SIMD3<Float> {
            if let luts {
                let lut = { (ch: Int, v: Float) -> Float in
                    let x = min(max(v, 0), 1) * 255
                    let i = min(Int(x), 254), f = x - Float(i)
                    return luts[ch][i] + (luts[ch][i + 1] - luts[ch][i]) * f
                }
                return SIMD3(lut(0, c.x), lut(1, c.y), lut(2, c.z))
            }
            switch model {
            case .hueSaturation(let hue, let saturation, let lightness, let colorize):
                var hsl = Self.hsl(c)
                if colorize {
                    hsl.x = Float((hue + 360).truncatingRemainder(dividingBy: 360) / 360)
                    hsl.y = Float(max(saturation, 0) / 100)
                } else {
                    hsl.x = (hsl.x + Float(hue / 360)).truncatingRemainder(dividingBy: 1)
                    if hsl.x < 0 { hsl.x += 1 }
                    hsl.y = min(max(hsl.y * Float(1 + saturation / 100), 0), 1)
                }
                let l = Float(lightness / 100)
                hsl.z = l >= 0 ? hsl.z + (1 - hsl.z) * l : hsl.z * (1 + l)
                return Self.rgb(hsl)
            case .exposure(let e, let o, let g):
                let gain = Float(pow(2, e)), gamma = Float(g > 0 ? g : 1)
                func f(_ v: Float) -> Float { powf(max(v * gain + Float(o), 0), 1 / gamma) }
                return SIMD3(f(c.x), f(c.y), f(c.z))
            case .invert: return SIMD3(repeating: 1) - c
            case .posterize(let n):
                let k = Float(max(min(n, 255), 2))
                func f(_ v: Float) -> Float { min(floor(min(max(v, 0), 1) * k), k - 1) / (k - 1) }
                return SIMD3(f(c.x), f(c.y), f(c.z))
            case .threshold(let level):
                let l = 0.299 * c.x + 0.587 * c.y + 0.114 * c.z
                return SIMD3(repeating: l >= Float(level) ? 1 : 0)
            case .channelMixer(let m, let k, let mono):
                func row(_ r: Int) -> Float {
                    Float(m[r][0]) * c.x + Float(m[r][1]) * c.y + Float(m[r][2]) * c.z + Float(k[r])
                }
                return mono ? SIMD3(repeating: row(0)) : SIMD3(row(0), row(1), row(2))
            case .levels, .curves: return c
            // WP B5-06: the pointwise M5-26 kinds follow adjust.rs; the stub leaves Vibrance, Selective Color, Match
            // Color and the neighbourhood / HDR operators as the identity (the engine renders them).
            case .brightnessContrast(let b0, let k0, let legacy):
                let b = Float(min(max(b0, -150), 150) / 150), k = Float(min(max(k0, -100), 100) / 100)
                func f(_ v0: Float) -> Float {
                    if legacy { return min(max((v0 - 0.5) * (1 + k) + 0.5 + b, 0), 1) }
                    let v = min(max(v0, 0), 1), t = min(max(v + b * v * (1 - v), 0), 1), p = powf(2, k)
                    let x = powf(t, p), y = powf(1 - t, p)
                    return x + y > 0 ? x / (x + y) : t
                }
                return SIMD3(f(c.x), f(c.y), f(c.z))
            case .colorBalance(let m):
                let y = min(max(Self.luma(c), 0), 1)
                let w = SIMD3<Float>((1 - y) * (1 - y), 2 * y * (1 - y), y * y)
                func off(_ i: Int) -> Float { (w.x * Float(m.shadows[i]) + w.y * Float(m.midtones[i]) + w.z * Float(m.highlights[i])) / 100 }
                let o = SIMD3(c.x + off(0), c.y + off(1), c.z + off(2))
                return m.preserveLuminosity ? Self.preserveLuma(o, y) : simd_clamp(o, .zero, .one)
            case .blackWhite(let sliders, let tint):
                let h = Self.hsl(c).x * 6, i = Int(h) % 6, f = h - floorf(h)
                let mx = max(c.x, c.y, c.z), mn = min(c.x, c.y, c.z)
                let y = min(max(mn + (mx - mn) * (Float(sliders[i]) * (1 - f) + Float(sliders[(i + 1) % 6]) * f) / 100, 0), 1)
                guard let tint, tint.count == 3 else { return SIMD3(repeating: y) }
                let t = Self.hsl(SIMD3(Float(tint[0]), Float(tint[1]), Float(tint[2])))
                return Self.rgb(SIMD3(t.x, t.y, y))
            case .photoFilter(let color, let density, let preserve):
                let d = Float(min(max(density / 100, 0), 1))
                let fc = SIMD3(Float(color[0]), Float(color[1]), Float(color[2]))
                let o = c * (SIMD3(repeating: 1 - d) + d * simd_clamp(fc, .zero, .one))
                return preserve ? Self.preserveLuma(o, Self.luma(c)) : o
            case .gradientMap(let m):
                var t = min(max(Self.luma(c), 0), 1)
                if m.reverse { t = 1 - t }
                let stops = m.stops.filter { $0.count == 4 }.sorted { $0[0] < $1[0] }
                guard let first = stops.first, let last = stops.last else { return SIMD3(repeating: t) }
                func rgb(_ s: [Double]) -> SIMD3<Float> { SIMD3(Float(s[1]), Float(s[2]), Float(s[3])) }
                if Double(t) <= first[0] { return rgb(first) }
                for i in 1..<stops.count where Double(t) <= stops[i][0] {
                    let f = Float((Double(t) - stops[i - 1][0]) / max(stops[i][0] - stops[i - 1][0], 1e-9))
                    return rgb(stops[i - 1]) + (rgb(stops[i]) - rgb(stops[i - 1])) * f
                }
                return rgb(last)
            case .desaturate: return SIMD3(repeating: Self.hsl(c).z)
            case .equalize(let maps):
                func f(_ m: [Double], _ v: Float) -> Float {
                    guard m.count >= 2 else { return v }
                    let x = min(max(v, 0), 1) * Float(m.count - 1), i = min(Int(x), m.count - 2), t = x - Float(i)
                    return Float(m[i]) + Float(m[i + 1] - m[i]) * t
                }
                return SIMD3(f(maps[0], c.x), f(maps[1], c.y), f(maps[2], c.z))
            case .auto(let m):
                func f(_ i: Int, _ v: Float) -> Float {
                    var l = LevelsChannelModel()
                    l.inBlack = m.black[i]; l.inWhite = m.white[i]; l.gamma = m.gamma[i]
                    return Float(l.apply(Double(v)))
                }
                return SIMD3(f(0, c.x), f(1, c.y), f(2, c.z))
            case .replaceColor(let color, let fuzz, let hue, let sat, let light):
                let sel = SIMD3(Float(color[0]), Float(color[1]), Float(color[2]))
                let dist = simd_length(c - sel) / Float(3).squareRoot()
                let radius = Float(min(max(fuzz / 200, 0), 1))
                let w: Float = radius <= 0 ? (dist <= 1e-7 ? 1 : 0) : min(max(1 - dist / radius, 0), 1)
                let o = Adjust(.hueSaturation(hue: hue, saturation: sat, lightness: light, colorize: false)).apply(c)
                return c + w * (o - c)
            case .colorLookup(let size, let data, _, _):
                guard size >= 2, data.count == 3 * size * size * size else { return c }
                let n = size, x = simd_clamp(c, .zero, .one) * Float(n - 1)
                let lo = SIMD3<Int>(min(Int(x.x), n - 2), min(Int(x.y), n - 2), min(Int(x.z), n - 2))
                let f = x - SIMD3<Float>(Float(lo.x), Float(lo.y), Float(lo.z))
                var o = SIMD3<Float>.zero
                for b in 0..<2 { for g in 0..<2 { for r in 0..<2 {
                    let w = (r == 0 ? 1 - f.x : f.x) * (g == 0 ? 1 - f.y : f.y) * (b == 0 ? 1 - f.z : f.z)
                    let k = 3 * (lo.x + r + n * (lo.y + g + n * (lo.z + b)))
                    o += w * SIMD3(Float(data[k]), Float(data[k + 1]), Float(data[k + 2]))
                } } }
                return o
            case .vibrance, .selectiveColor, .matchColor, .shadowsHighlights, .hdrToning: return c
            }
        }

        static func luma(_ c: SIMD3<Float>) -> Float { 0.299 * c.x + 0.587 * c.y + 0.114 * c.z }

        /// adjust.rs `preserve_luma`: scale to luma `y`, then contract towards gray to fit the gamut.
        static func preserveLuma(_ c0: SIMD3<Float>, _ y0: Float) -> SIMD3<Float> {
            let y = min(max(y0, 0), 1), old = luma(c0)
            guard abs(old) >= 1e-8 else { return SIMD3(repeating: y) }
            let c = c0 * (y / old)
            var scale: Float = 1
            for v in [c.x, c.y, c.z] {
                if v > 1 { scale = min(scale, (1 - y) / (v - y)) } else if v < 0 { scale = min(scale, y / (y - v)) }
            }
            return SIMD3(repeating: y) + (c - SIMD3(repeating: y)) * scale
        }

        static func hsl(_ c: SIMD3<Float>) -> SIMD3<Float> {
            let mx = max(c.x, c.y, c.z), mn = min(c.x, c.y, c.z)
            let l = (mx + mn) / 2
            guard mx > mn else { return SIMD3(0, 0, l) }
            let d = mx - mn
            let s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn)
            var h: Float
            if mx == c.x { h = (c.y - c.z) / d + (c.y < c.z ? 6 : 0) }
            else if mx == c.y { h = (c.z - c.x) / d + 2 }
            else { h = (c.x - c.y) / d + 4 }
            return SIMD3(h / 6, s, l)
        }

        static func rgb(_ hsl: SIMD3<Float>) -> SIMD3<Float> {
            let (h, s, l) = (hsl.x, hsl.y, hsl.z)
            guard s > 0 else { return SIMD3(repeating: l) }
            let q = l < 0.5 ? l * (1 + s) : l + s - l * s
            let p = 2 * l - q
            func f(_ t0: Float) -> Float {
                var t = t0.truncatingRemainder(dividingBy: 1)
                if t < 0 { t += 1 }
                if t < 1.0 / 6 { return p + (q - p) * 6 * t }
                if t < 0.5 { return q }
                if t < 2.0 / 3 { return p + (q - p) * (2.0 / 3 - t) * 6 }
                return p
            }
            return SIMD3(f(h + 1.0 / 3), f(h), f(h - 1.0 / 3))
        }
    }

    // MARK: Fills

    struct FillEval: Sendable {
        let model: FillModel
        init(_ m: FillModel) { model = m }

        func eval(_ x: Float, _ y: Float) -> RGBA {
            switch model {
            case .solid(let c): return RGBA(Float(c[0]), Float(c[1]), Float(c[2]), 1)
            case .gradient(let radial, let s, let e, let stops):
                let (sx, sy, ex, ey) = (Float(s[0]), Float(s[1]), Float(e[0]), Float(e[1]))
                let dx = ex - sx, dy = ey - sy
                let len2 = max(dx * dx + dy * dy, 1e-6)
                let t: Float = radial
                    ? ((x - sx) * (x - sx) + (y - sy) * (y - sy)).squareRoot() / len2.squareRoot()
                    : ((x - sx) * dx + (y - sy) * dy) / len2
                let tt = min(max(t, 0), 1)
                let sorted = stops.sorted { $0.position < $1.position }
                guard let first = sorted.first else { return .zero }
                func rgba(_ st: FillModel.Stop) -> RGBA { RGBA(Float(st.color[0]), Float(st.color[1]), Float(st.color[2]), Float(st.color[3])) }
                if tt <= Float(first.position) { return rgba(first) }
                for i in 1..<max(sorted.count, 1) where tt <= Float(sorted[i].position) {
                    let a = sorted[i - 1], b = sorted[i]
                    let f = (tt - Float(a.position)) / max(Float(b.position - a.position), 1e-6)
                    return rgba(a) + (rgba(b) - rgba(a)) * f
                }
                return rgba(sorted[sorted.count - 1])
            case .pattern(let w, let h, _):
                // Placeholder checker in the pattern's cell size.
                let cell = Float(max(w, h, 1)) * 8
                let on = (Int(x / cell) + Int(y / cell)) % 2 == 0
                return on ? RGBA(0.85, 0.85, 0.85, 1) : RGBA(0.7, 0.7, 0.7, 1)
            }
        }
    }
}
