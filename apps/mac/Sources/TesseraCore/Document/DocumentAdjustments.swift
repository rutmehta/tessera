import Foundation

/// Swift mirror of `compositor::adjust::Adjustment` (serde: internally tagged by `kind`,
/// snake_case), edited by the Properties panel and sent back through `set_adjustment_json`.
public struct LevelsChannelModel: Codable, Equatable, Sendable {
    public var inBlack: Double = 0
    public var inWhite: Double = 1
    public var gamma: Double = 1
    public var outBlack: Double = 0
    public var outWhite: Double = 1
    public init() {}
    enum CodingKeys: String, CodingKey {
        case inBlack = "in_black", inWhite = "in_white", gamma, outBlack = "out_black", outWhite = "out_white"
    }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        inBlack = try c.decodeIfPresent(Double.self, forKey: .inBlack) ?? 0
        inWhite = try c.decodeIfPresent(Double.self, forKey: .inWhite) ?? 1
        gamma = try c.decodeIfPresent(Double.self, forKey: .gamma) ?? 1
        outBlack = try c.decodeIfPresent(Double.self, forKey: .outBlack) ?? 0
        outWhite = try c.decodeIfPresent(Double.self, forKey: .outWhite) ?? 1
    }
    /// `ob + (ow − ob)·clamp((v − ib)/(iw − ib), 0, 1)^(1/γ)` (COMPOSITOR.md §4).
    public func apply(_ v: Double) -> Double {
        let span = inWhite - inBlack
        var t = abs(span) < 1e-9 ? (v >= inWhite ? 1 : 0) : min(max((v - inBlack) / span, 0), 1)
        let g = gamma > 0 ? gamma : 1
        if g != 1 { t = pow(t, 1 / g) }
        return outBlack + (outWhite - outBlack) * t
    }
}

public enum AdjustmentModel: Equatable, Sendable {
    case levels(master: LevelsChannelModel, rgb: [LevelsChannelModel])
    /// Points `(input, output)` in [0, 1]; empty = identity.
    case curves(master: [[Double]], rgb: [[[Double]]])
    case hueSaturation(hue: Double, saturation: Double, lightness: Double, colorize: Bool)
    case exposure(exposure: Double, offset: Double, gamma: Double)
    case invert
    case posterize(levels: Int)
    case threshold(level: Double)
    case channelMixer(matrix: [[Double]], constant: [Double], monochrome: Bool)
    // M5-26 / M5-28 (WP B5-06). Payloads mirror adjust.rs field for field; see DocumentAdjustmentModels.swift.
    /// Brightness −150…150, contrast −100…100; `legacy` = the affine formula.
    case brightnessContrast(brightness: Double, contrast: Double, legacy: Bool)
    /// Percent, −100…100.
    case vibrance(vibrance: Double, saturation: Double)
    case colorBalance(ColorBalanceModel)
    /// R, Y, G, C, B, M percent weights; optional encoded RGB tint.
    case blackWhite(sliders: [Double], tint: [Double]?)
    /// Encoded RGB filter colour, density 0…100.
    case photoFilter(color: [Double], density: Double, preserveLuminosity: Bool)
    case gradientMap(GradientMapModel)
    /// Nine rows (R, Y, G, C, B, M, whites, neutrals, blacks) of C, M, Y, K percent.
    case selectiveColor(colors: [[Double]], absolute: Bool)
    case desaturate
    /// Frozen per-channel maps over [0, 1].
    case equalize(maps: [[Double]])
    case auto(AutoAdjustmentModel)
    case matchColor(MatchColorModel)
    /// Encoded RGB colour; fuzziness 0…200; hue degrees; saturation / lightness percent.
    case replaceColor(color: [Double], fuzziness: Double, hue: Double, saturation: Double, lightness: Double)
    /// Red-fastest cube of `size`³ RGB samples, flattened (`data.count == 3·size³`).
    case colorLookup(size: Int, data: [Double])
    case shadowsHighlights(ShadowsHighlightsModel)
    case hdrToning(HDRToningModel)

    /// Serde tags. Declared in Photoshop's Layer ▸ New Adjustment Layer order (Brightness/Contrast to Selective
    /// Color), then the native-only kinds in Image ▸ Adjustments order.
    public enum Kind: String, CaseIterable, Sendable, Identifiable {
        case brightnessContrast = "brightness_contrast", levels, curves, exposure
        case vibrance, hueSaturation = "hue_saturation", colorBalance = "color_balance", blackWhite = "black_white"
        case photoFilter = "photo_filter", channelMixer = "channel_mixer", colorLookup = "color_lookup"
        case invert, posterize, threshold, gradientMap = "gradient_map", selectiveColor = "selective_color"
        case shadowsHighlights = "shadows_highlights", hdrToning = "hdr_toning"
        case desaturate, matchColor = "match_color", replaceColor = "replace_color", equalize, auto
        public var id: String { rawValue }
        /// Photoshop's names (the engine's `adjustment_title`; new layers are numbered after them).
        public var title: String {
            switch self {
            case .levels: "Levels"
            case .curves: "Curves"
            case .hueSaturation: "Hue/Saturation"
            case .exposure: "Exposure"
            case .invert: "Invert"
            case .posterize: "Posterize"
            case .threshold: "Threshold"
            case .channelMixer: "Channel Mixer"
            case .brightnessContrast: "Brightness/Contrast"
            case .vibrance: "Vibrance"
            case .colorBalance: "Color Balance"
            case .blackWhite: "Black & White"
            case .photoFilter: "Photo Filter"
            case .gradientMap: "Gradient Map"
            case .selectiveColor: "Selective Color"
            case .desaturate: "Desaturate"
            case .equalize: "Equalize"
            case .auto: "Auto"
            case .matchColor: "Match Color"
            case .replaceColor: "Replace Color"
            case .colorLookup: "Color Lookup"
            case .shadowsHighlights: "Shadows/Highlights"
            case .hdrToning: "HDR Toning"
            }
        }
        /// Applied at once from Image ▸ Adjustments (no settings, or settings the image determines).
        public var appliesDirectly: Bool { [.invert, .desaturate, .equalize, .auto].contains(self) }
        /// Glyph for the Layers panel and menus.
        public var symbol: String {
            switch self {
            case .levels: "chart.bar.xaxis"
            case .curves: "point.topleft.down.to.point.bottomright.curvepath"
            case .hueSaturation: "paintpalette"
            case .exposure: "plusminus.circle"
            case .invert: "circle.lefthalf.filled"
            case .posterize: "square.stack.3d.down.right"
            case .threshold: "circle.righthalf.filled"
            case .channelMixer: "slider.horizontal.3"
            case .brightnessContrast: "sun.max"
            case .vibrance: "drop.triangle"
            case .colorBalance: "scalemass"
            case .blackWhite: "circle.bottomhalf.filled"
            case .photoFilter: "camera.filters"
            case .gradientMap: "rectangle.righthalf.inset.filled"
            case .selectiveColor: "eyedropper.halffull"
            case .desaturate: "circle.dotted"
            case .equalize: "chart.bar"
            case .auto: "wand.and.stars"
            case .matchColor: "arrow.left.arrow.right.circle"
            case .replaceColor: "arrow.triangle.swap"
            case .colorLookup: "cube"
            case .shadowsHighlights: "circle.lefthalf.striped.horizontal"
            case .hdrToning: "camera.aperture"
            }
        }
        /// Layer ▸ New Adjustment Layer and the Layers panel's footer menu: Photoshop's groups, then the
        /// adjustments Photoshop only offers as Image ▸ Adjustments commands (native adjustment layers here).
        public static let layerMenuSections: [[Kind]] = [
            [.brightnessContrast, .levels, .curves, .exposure],
            [.vibrance, .hueSaturation, .colorBalance, .blackWhite, .photoFilter, .channelMixer, .colorLookup],
            [.invert, .posterize, .threshold, .gradientMap, .selectiveColor],
            [.shadowsHighlights, .hdrToning, .desaturate, .matchColor, .replaceColor, .equalize, .auto],
        ]
        /// Image ▸ Adjustments, as in Photoshop (Auto Tone / Contrast / Color are Image menu commands).
        public static let imageMenuSections: [[Kind]] = [
            [.brightnessContrast, .levels, .curves, .exposure],
            [.vibrance, .hueSaturation, .colorBalance, .blackWhite, .photoFilter, .channelMixer, .colorLookup],
            [.invert, .posterize, .threshold, .gradientMap, .selectiveColor],
            [.shadowsHighlights, .hdrToning],
            [.desaturate, .matchColor, .replaceColor, .equalize],
        ]
        /// The adjustment with neutral parameters (a new adjustment layer). Image-dependent kinds (Equalize, Auto,
        /// Match Color) start as the identity until analysed (`AdjustmentAnalysis`).
        public var neutral: AdjustmentModel {
            switch self {
            case .levels: .levels(master: .init(), rgb: [.init(), .init(), .init()])
            case .curves: .curves(master: [], rgb: [[], [], []])
            case .hueSaturation: .hueSaturation(hue: 0, saturation: 0, lightness: 0, colorize: false)
            case .exposure: .exposure(exposure: 0, offset: 0, gamma: 1)
            case .invert: .invert
            case .posterize: .posterize(levels: 4)
            case .threshold: .threshold(level: 0.5)
            case .channelMixer: .channelMixer(matrix: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], constant: [0, 0, 0], monochrome: false)
            case .brightnessContrast: .brightnessContrast(brightness: 0, contrast: 0, legacy: false)
            case .vibrance: .vibrance(vibrance: 0, saturation: 0)
            case .colorBalance: .colorBalance(.init())
            case .blackWhite: .blackWhite(sliders: BlackWhitePresets.default, tint: nil)
            case .photoFilter:
                .photoFilter(color: PhotoFilterPreset.warming85.color, density: 25, preserveLuminosity: true)
            case .gradientMap: .gradientMap(.init())
            case .selectiveColor: .selectiveColor(colors: Array(repeating: [0, 0, 0, 0], count: 9), absolute: false)
            case .desaturate: .desaturate
            case .equalize: .equalize(maps: [[0, 1], [0, 1], [0, 1]])
            case .auto: .auto(.init())
            case .matchColor: .matchColor(.init())
            case .replaceColor: .replaceColor(color: [1, 0, 0], fuzziness: 40, hue: 0, saturation: 0, lightness: 0)
            case .colorLookup: .colorLookup(size: 2, data: ColorLookupFile.identity(size: 2))
            case .shadowsHighlights: .shadowsHighlights(.init())
            case .hdrToning: .hdrToning(.init())
            }
        }
    }

    public var kind: Kind {
        switch self {
        case .levels: .levels
        case .curves: .curves
        case .hueSaturation: .hueSaturation
        case .exposure: .exposure
        case .invert: .invert
        case .posterize: .posterize
        case .threshold: .threshold
        case .channelMixer: .channelMixer
        case .brightnessContrast: .brightnessContrast
        case .vibrance: .vibrance
        case .colorBalance: .colorBalance
        case .blackWhite: .blackWhite
        case .photoFilter: .photoFilter
        case .gradientMap: .gradientMap
        case .selectiveColor: .selectiveColor
        case .desaturate: .desaturate
        case .equalize: .equalize
        case .auto: .auto
        case .matchColor: .matchColor
        case .replaceColor: .replaceColor
        case .colorLookup: .colorLookup
        case .shadowsHighlights: .shadowsHighlights
        case .hdrToning: .hdrToning
        }
    }

    // MARK: JSON

    public init?(json: String?) {
        guard let data = json?.data(using: .utf8),
              let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return nil }
        self.init(object: o)
    }

    /// Decodes the engine's serde object (`{"kind": …}`); missing fields take serde's defaults where adjust.rs
    /// declares them (`LevelsChannel`, `ShadowsHighlights`, `HdrToning`) and the neutral value otherwise.
    public init?(object o: [String: Any]) {
        guard let kind = o["kind"] as? String else { return nil }
        let num = { (k: String, d: Double) in JSONNumbers.double(o[k]) ?? d }
        func levels(_ v: Any?) -> LevelsChannelModel {
            guard let v, let d = try? JSONSerialization.data(withJSONObject: v) else { return .init() }
            return (try? JSONDecoder().decode(LevelsChannelModel.self, from: d)) ?? .init()
        }
        func points(_ v: Any?) -> [[Double]] { JSONNumbers.points(v) }
        func triple(_ k: String, _ d: [Double]) -> [Double] { JSONNumbers.array(o[k], count: 3) ?? d }
        switch kind {
        case "levels":
            let rgb = (o["rgb"] as? [Any]) ?? []
            self = .levels(master: levels(o["master"]), rgb: (0..<3).map { $0 < rgb.count ? levels(rgb[$0]) : .init() })
        case "curves":
            let rgb = (o["rgb"] as? [Any]) ?? []
            self = .curves(master: points(o["master"]), rgb: (0..<3).map { $0 < rgb.count ? points(rgb[$0]) : [] })
        case "hue_saturation":
            self = .hueSaturation(hue: num("hue", 0), saturation: num("saturation", 0), lightness: num("lightness", 0),
                                  colorize: (o["colorize"] as? Bool) ?? false)
        case "exposure":
            self = .exposure(exposure: num("exposure", 0), offset: num("offset", 0), gamma: num("gamma", 1))
        case "invert": self = .invert
        case "posterize": self = .posterize(levels: Int(num("levels", 4)))
        case "threshold": self = .threshold(level: num("level", 0.5))
        case "channel_mixer":
            let m = JSONNumbers.matrix(o["matrix"], rows: 3, columns: 3) ?? [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
            self = .channelMixer(matrix: m, constant: triple("constant", [0, 0, 0]), monochrome: (o["monochrome"] as? Bool) ?? false)
        case "brightness_contrast":
            self = .brightnessContrast(brightness: num("brightness", 0), contrast: num("contrast", 0),
                                       legacy: (o["legacy"] as? Bool) ?? false)
        case "vibrance": self = .vibrance(vibrance: num("vibrance", 0), saturation: num("saturation", 0))
        case "color_balance": self = .colorBalance(ColorBalanceModel(object: o))
        case "black_white":
            self = .blackWhite(sliders: JSONNumbers.array(o["sliders"], count: 6) ?? BlackWhitePresets.default,
                               tint: JSONNumbers.array(o["tint"], count: 3))
        case "photo_filter":
            self = .photoFilter(color: triple("color", PhotoFilterPreset.warming85.color), density: num("density", 25),
                                preserveLuminosity: (o["preserve_luminosity"] as? Bool) ?? true)
        case "gradient_map": self = .gradientMap(GradientMapModel(object: o))
        case "selective_color":
            self = .selectiveColor(colors: JSONNumbers.matrix(o["colors"], rows: 9, columns: 4)
                                       ?? Array(repeating: [0, 0, 0, 0], count: 9),
                                   absolute: (o["absolute"] as? Bool) ?? false)
        case "desaturate": self = .desaturate
        case "equalize":
            let maps = ((o["maps"] as? [Any]) ?? []).map { JSONNumbers.array($0) ?? [] }
            self = .equalize(maps: maps.count == 3 ? maps : [[0, 1], [0, 1], [0, 1]])
        case "auto": self = .auto(AutoAdjustmentModel(object: o))
        case "match_color": self = .matchColor(MatchColorModel(object: o))
        case "replace_color":
            self = .replaceColor(color: triple("color", [1, 0, 0]), fuzziness: num("fuzziness", 40), hue: num("hue", 0),
                                 saturation: num("saturation", 0), lightness: num("lightness", 0))
        case "color_lookup":
            let size = Int(num("size", 2))
            let data = ((o["data"] as? [Any]) ?? []).flatMap { JSONNumbers.array($0, count: 3) ?? [] }
            guard data.count == 3 * size * size * size else { return nil }
            self = .colorLookup(size: size, data: data)
        case "shadows_highlights":
            self = .shadowsHighlights(ShadowsHighlightsModel(object: (o["settings"] as? [String: Any]) ?? [:]))
        case "hdr_toning":
            self = .hdrToning(HDRToningModel(object: (o["settings"] as? [String: Any]) ?? [:]))
        default: return nil
        }
    }

    public var jsonObject: [String: Any] {
        func lv(_ l: LevelsChannelModel) -> [String: Any] {
            ["in_black": l.inBlack, "in_white": l.inWhite, "gamma": l.gamma, "out_black": l.outBlack, "out_white": l.outWhite]
        }
        switch self {
        case .levels(let m, let rgb): return ["kind": "levels", "master": lv(m), "rgb": rgb.map(lv)]
        case .curves(let m, let rgb): return ["kind": "curves", "master": m, "rgb": rgb]
        case .hueSaturation(let h, let s, let l, let c):
            return ["kind": "hue_saturation", "hue": h, "saturation": s, "lightness": l, "colorize": c]
        case .exposure(let e, let o, let g): return ["kind": "exposure", "exposure": e, "offset": o, "gamma": g]
        case .invert: return ["kind": "invert"]
        case .posterize(let n): return ["kind": "posterize", "levels": n]
        case .threshold(let l): return ["kind": "threshold", "level": l]
        case .channelMixer(let m, let c, let mono):
            return ["kind": "channel_mixer", "matrix": m, "constant": c, "monochrome": mono]
        case .brightnessContrast(let b, let c, let legacy):
            return ["kind": "brightness_contrast", "brightness": b, "contrast": c, "legacy": legacy]
        case .vibrance(let v, let s): return ["kind": "vibrance", "vibrance": v, "saturation": s]
        case .colorBalance(let m): return m.jsonObject
        case .blackWhite(let sliders, let tint):
            return ["kind": "black_white", "sliders": sliders, "tint": tint.map { $0 as Any } ?? NSNull()]
        case .photoFilter(let c, let d, let p):
            return ["kind": "photo_filter", "color": c, "density": d, "preserve_luminosity": p]
        case .gradientMap(let m): return m.jsonObject
        case .selectiveColor(let colors, let absolute):
            return ["kind": "selective_color", "colors": colors, "absolute": absolute]
        case .desaturate: return ["kind": "desaturate"]
        case .equalize(let maps): return ["kind": "equalize", "maps": maps]
        case .auto(let m): return m.jsonObject
        case .matchColor(let m): return m.jsonObject
        case .replaceColor(let c, let f, let h, let s, let l):
            return ["kind": "replace_color", "color": c, "fuzziness": f, "hue": h, "saturation": s, "lightness": l]
        case .colorLookup(let size, let data):
            let rows: [[Double]] = stride(from: 0, to: data.count - 2, by: 3).map { (i: Int) -> [Double] in
                [data[i], data[i + 1], data[i + 2]]
            }
            return ["kind": "color_lookup", "size": size, "data": rows]
        case .shadowsHighlights(let m): return ["kind": "shadows_highlights", "settings": m.jsonObject]
        case .hdrToning(let m): return ["kind": "hdr_toning", "settings": m.jsonObject]
        }
    }

    public var json: String {
        let data = (try? JSONSerialization.data(withJSONObject: jsonObject, options: [.sortedKeys])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }
}

/// Swift mirror of `compositor::Fill` (serde tag `kind`).
public enum FillModel: Equatable, Sendable {
    public struct Stop: Equatable, Sendable {
        public var position: Double
        /// Straight RGBA.
        public var color: [Double]
        public init(position: Double, color: [Double]) { self.position = position; self.color = color }
    }
    case solid(color: [Double])
    /// `radial` false = linear. Points in canvas pixels.
    case gradient(radial: Bool, start: [Double], end: [Double], stops: [Stop])
    /// Pattern contents are not editable in B5-02 (placeholder); the JSON is kept as is.
    case pattern(width: Int, height: Int, json: String)

    public enum Kind: String, CaseIterable, Sendable, Identifiable {
        case solid, gradient, pattern
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .solid: "Solid Color"
            case .gradient: "Gradient"
            case .pattern: "Pattern"
            }
        }
    }

    public var kind: Kind {
        switch self {
        case .solid: .solid
        case .gradient: .gradient
        case .pattern: .pattern
        }
    }

    /// A default fill of `kind` over a `width × height` canvas.
    public static func neutral(_ kind: Kind, width: Double, height: Double) -> FillModel {
        switch kind {
        case .solid: .solid(color: [0.5, 0.5, 0.5])
        case .gradient: .gradient(radial: false, start: [0, 0], end: [width, 0],
                                  stops: [Stop(position: 0, color: [0, 0, 0, 1]), Stop(position: 1, color: [1, 1, 1, 1])])
        case .pattern:
            .pattern(width: 2, height: 2, json: #"{"kind":"pattern","width":2,"height":2,"rgba":[1,1,1,1,0.8,0.8,0.8,1,0.8,0.8,0.8,1,1,1,1,1],"origin":[0,0]}"#)
        }
    }

    public init?(json: String?) {
        guard let json, let data = json.data(using: .utf8),
              let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let kind = o["kind"] as? String else { return nil }
        let nums = { (v: Any?) in ((v as? [Any]) ?? []).compactMap { ($0 as? NSNumber)?.doubleValue } }
        switch kind {
        case "solid":
            let c = nums(o["color"])
            self = .solid(color: c.count == 3 ? c : [0.5, 0.5, 0.5])
        case "gradient":
            let stops = ((o["stops"] as? [[String: Any]]) ?? []).compactMap { s -> Stop? in
                guard let p = (s["position"] as? NSNumber)?.doubleValue else { return nil }
                let c = nums(s["color"])
                return Stop(position: p, color: c.count == 4 ? c : [0, 0, 0, 1])
            }
            let s = nums(o["start"]), e = nums(o["end"])
            self = .gradient(radial: (o["gradient"] as? String) == "radial", start: s.count == 2 ? s : [0, 0],
                             end: e.count == 2 ? e : [1, 0], stops: stops.isEmpty ? [Stop(position: 0, color: [0, 0, 0, 1])] : stops)
        case "pattern":
            self = .pattern(width: (o["width"] as? NSNumber)?.intValue ?? 0, height: (o["height"] as? NSNumber)?.intValue ?? 0,
                            json: json)
        default: return nil
        }
    }

    public var json: String {
        let o: [String: Any]
        switch self {
        case .solid(let c): o = ["kind": "solid", "color": c]
        case .gradient(let radial, let s, let e, let stops):
            o = ["kind": "gradient", "gradient": radial ? "radial" : "linear", "start": s, "end": e,
                 "stops": stops.sorted { $0.position < $1.position }.map { ["position": $0.position, "color": $0.color] }]
        case .pattern(_, _, let json): return json
        }
        let data = (try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }
}
