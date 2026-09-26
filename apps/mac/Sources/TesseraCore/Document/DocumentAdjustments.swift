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

    public enum Kind: String, CaseIterable, Sendable, Identifiable {
        case levels, curves, hueSaturation = "hue_saturation", exposure, invert, posterize, threshold
        case channelMixer = "channel_mixer"
        public var id: String { rawValue }
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
            }
        }
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
            }
        }
        /// The adjustment with neutral parameters (a new adjustment layer).
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
        }
    }

    // MARK: JSON

    public init?(json: String?) {
        guard let data = json?.data(using: .utf8),
              let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let kind = o["kind"] as? String else { return nil }
        let num = { (k: String, d: Double) in (o[k] as? NSNumber)?.doubleValue ?? d }
        func levels(_ v: Any?) -> LevelsChannelModel {
            guard let v, let d = try? JSONSerialization.data(withJSONObject: v) else { return .init() }
            return (try? JSONDecoder().decode(LevelsChannelModel.self, from: d)) ?? .init()
        }
        func points(_ v: Any?) -> [[Double]] {
            ((v as? [[Any]]) ?? []).compactMap { p in
                let xy = p.compactMap { ($0 as? NSNumber)?.doubleValue }
                return xy.count == 2 ? xy : nil
            }
        }
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
            let m = ((o["matrix"] as? [[Any]]) ?? []).map { $0.compactMap { ($0 as? NSNumber)?.doubleValue } }
            let c = ((o["constant"] as? [Any]) ?? []).compactMap { ($0 as? NSNumber)?.doubleValue }
            self = .channelMixer(matrix: m.count == 3 && m.allSatisfy { $0.count == 3 } ? m : [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
                                 constant: c.count == 3 ? c : [0, 0, 0], monochrome: (o["monochrome"] as? Bool) ?? false)
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
