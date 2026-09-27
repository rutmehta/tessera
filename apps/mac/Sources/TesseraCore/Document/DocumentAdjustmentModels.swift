import Foundation

// Payloads of the M5-26 / M5-28 `compositor::adjust::Adjustment` variants (WP B5-06). Field names and
// defaults follow crates/compositor/src/adjust.rs (and adjust/shadows.rs, adjust/hdr.rs) exactly; the
// JSON is the engine's serde form, checked against crates/tessera-ffi/tests/fixtures/adjustments.json.

/// Number helpers for `JSONSerialization` objects.
public enum JSONNumbers {
    /// The engine's parameters are f32: numbers are rounded to f32, so the shortest decimal the engine prints
    /// ("0.9254902") and the value the app computed (`Double(Float(236 / 255))`) decode to the same model.
    public static func double(_ v: Any?) -> Double? {
        guard let n = v as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID() else { return nil }
        return Double(Float(n.doubleValue))
    }

    /// A number array (optionally of exactly `count` entries).
    public static func array(_ v: Any?, count: Int? = nil) -> [Double]? {
        guard let a = v as? [Any] else { return nil }
        let d = a.compactMap(double)
        guard d.count == a.count, count == nil || d.count == count else { return nil }
        return d
    }

    public static func matrix(_ v: Any?, rows: Int, columns: Int) -> [[Double]]? {
        guard let a = v as? [Any], a.count == rows else { return nil }
        let m = a.compactMap { array($0, count: columns) }
        return m.count == rows ? m : nil
    }

    /// Curve points `[[x, y], …]`, skipping malformed entries.
    public static func points(_ v: Any?) -> [[Double]] {
        ((v as? [Any]) ?? []).compactMap { array($0, count: 2) }
    }
}

/// `Adjustment::ColorBalance`: percent RGB offsets per tonal range.
public struct ColorBalanceModel: Equatable, Sendable {
    public var shadows: [Double] = [0, 0, 0]
    public var midtones: [Double] = [0, 0, 0]
    public var highlights: [Double] = [0, 0, 0]
    public var preserveLuminosity = true
    public init() {}
    public init(shadows: [Double], midtones: [Double], highlights: [Double], preserveLuminosity: Bool) {
        self.shadows = shadows; self.midtones = midtones; self.highlights = highlights
        self.preserveLuminosity = preserveLuminosity
    }

    init(object o: [String: Any]) {
        shadows = JSONNumbers.array(o["shadows"], count: 3) ?? [0, 0, 0]
        midtones = JSONNumbers.array(o["midtones"], count: 3) ?? [0, 0, 0]
        highlights = JSONNumbers.array(o["highlights"], count: 3) ?? [0, 0, 0]
        preserveLuminosity = (o["preserve_luminosity"] as? Bool) ?? true
    }

    var jsonObject: [String: Any] {
        ["kind": "color_balance", "shadows": shadows, "midtones": midtones, "highlights": highlights,
         "preserve_luminosity": preserveLuminosity]
    }

    /// Tonal range 0 shadows, 1 midtones, 2 highlights.
    public subscript(range: Int) -> [Double] {
        get { [shadows, midtones, highlights][range] }
        set {
            switch range {
            case 0: shadows = newValue
            case 1: midtones = newValue
            default: highlights = newValue
            }
        }
    }
}

/// `adjust::GradientMethod`.
public enum GradientMethodModel: String, CaseIterable, Sendable, Identifiable {
    case perceptual, linear, classic
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .perceptual: "Perceptual"
        case .linear: "Linear"
        case .classic: "Classic"
        }
    }
}

/// `Adjustment::GradientMap`: stops `[position, r, g, b]` in document encoding.
public struct GradientMapModel: Equatable, Sendable {
    public var stops: [[Double]] = [[0, 0, 0, 0], [1, 1, 1, 1]]
    public var dither = false
    public var reverse = false
    public var method: GradientMethodModel = .perceptual
    public init() {}
    public init(stops: [[Double]], dither: Bool, reverse: Bool, method: GradientMethodModel) {
        self.stops = stops; self.dither = dither; self.reverse = reverse; self.method = method
    }

    init(object o: [String: Any]) {
        let s = ((o["stops"] as? [Any]) ?? []).compactMap { JSONNumbers.array($0, count: 4) }
        stops = s.isEmpty ? [[0, 0, 0, 0], [1, 1, 1, 1]] : s
        dither = (o["dither"] as? Bool) ?? false
        reverse = (o["reverse"] as? Bool) ?? false
        method = (o["method"] as? String).flatMap(GradientMethodModel.init(rawValue:)) ?? .perceptual
    }

    var jsonObject: [String: Any] {
        ["kind": "gradient_map", "stops": stops, "dither": dither, "reverse": reverse, "method": method.rawValue]
    }
}

/// `adjust::AutoMode`.
public enum AutoModeModel: String, CaseIterable, Sendable, Identifiable {
    case tone, contrast, color
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .tone: "Auto Tone"
        case .contrast: "Auto Contrast"
        case .color: "Auto Color"
        }
    }
}

/// `Adjustment::Auto`: frozen per-channel black / white / gamma.
public struct AutoAdjustmentModel: Equatable, Sendable {
    public var mode: AutoModeModel = .tone
    public var black: [Double] = [0, 0, 0]
    public var white: [Double] = [1, 1, 1]
    public var gamma: [Double] = [1, 1, 1]
    public init() {}
    public init(mode: AutoModeModel, black: [Double], white: [Double], gamma: [Double]) {
        self.mode = mode; self.black = black; self.white = white; self.gamma = gamma
    }

    init(object o: [String: Any]) {
        mode = (o["mode"] as? String).flatMap(AutoModeModel.init(rawValue:)) ?? .tone
        black = JSONNumbers.array(o["black"], count: 3) ?? [0, 0, 0]
        white = JSONNumbers.array(o["white"], count: 3) ?? [1, 1, 1]
        gamma = JSONNumbers.array(o["gamma"], count: 3) ?? [1, 1, 1]
    }

    var jsonObject: [String: Any] {
        ["kind": "auto", "mode": mode.rawValue, "black": black, "white": white, "gamma": gamma]
    }
}

/// `Adjustment::MatchColor`: frozen CIE Lab D65 statistics of a source layer and the target.
public struct MatchColorModel: Equatable, Sendable {
    /// Layer id whose statistics were frozen (never 0: the engine rejects the root).
    public var sourceLayer: UInt64 = 1
    public var sourceMean: [Double] = [0, 0, 0]
    public var sourceStd: [Double] = [1, 1, 1]
    public var targetMean: [Double] = [0, 0, 0]
    public var targetStd: [Double] = [1, 1, 1]
    /// Neutral at 100.
    public var luminance: Double = 100
    /// Neutral at 100.
    public var colorIntensity: Double = 100
    /// 0…100.
    public var fade: Double = 0
    public init() {}

    init(object o: [String: Any]) {
        sourceLayer = (o["source_layer"] as? NSNumber)?.uint64Value ?? 1
        sourceMean = JSONNumbers.array(o["source_mean"], count: 3) ?? [0, 0, 0]
        sourceStd = JSONNumbers.array(o["source_std"], count: 3) ?? [1, 1, 1]
        targetMean = JSONNumbers.array(o["target_mean"], count: 3) ?? [0, 0, 0]
        targetStd = JSONNumbers.array(o["target_std"], count: 3) ?? [1, 1, 1]
        luminance = JSONNumbers.double(o["luminance"]) ?? 100
        colorIntensity = JSONNumbers.double(o["color_intensity"]) ?? 100
        fade = JSONNumbers.double(o["fade"]) ?? 0
    }

    var jsonObject: [String: Any] {
        ["kind": "match_color", "source_layer": NSNumber(value: sourceLayer), "source_mean": sourceMean,
         "source_std": sourceStd, "target_mean": targetMean, "target_std": targetStd, "luminance": luminance,
         "color_intensity": colorIntensity, "fade": fade]
    }

    /// Neutralize (Photoshop's option): the source's mean chroma is zero, so the match removes the target's cast.
    public var neutralized: Bool { sourceMean.count == 3 && sourceMean[1] == 0 && sourceMean[2] == 0 }
}

/// `adjust::shadows::ShadowsHighlights` (serde default: the identity).
public struct ShadowsHighlightsModel: Equatable, Sendable {
    public var shadowsAmount: Double = 0
    public var shadowsTone: Double = 0.5
    public var shadowsRadius: Double = 30
    public var highlightsAmount: Double = 0
    public var highlightsTone: Double = 0.5
    public var highlightsRadius: Double = 30
    public var color: Double = 0
    public var midtone: Double = 0
    public var blackClip: Double = 0
    public var whiteClip: Double = 0
    public init() {}

    static let keys = ["shadows_amount", "shadows_tone", "shadows_radius", "highlights_amount", "highlights_tone",
                       "highlights_radius", "color", "midtone", "black_clip", "white_clip"]
    private var values: [Double] {
        [shadowsAmount, shadowsTone, shadowsRadius, highlightsAmount, highlightsTone, highlightsRadius, color, midtone,
         blackClip, whiteClip]
    }

    init(object o: [String: Any]) {
        let d = ShadowsHighlightsModel()
        let v = zip(Self.keys, d.values).map { JSONNumbers.double(o[$0.0]) ?? $0.1 }
        (shadowsAmount, shadowsTone, shadowsRadius) = (v[0], v[1], v[2])
        (highlightsAmount, highlightsTone, highlightsRadius) = (v[3], v[4], v[5])
        (color, midtone, blackClip, whiteClip) = (v[6], v[7], v[8], v[9])
    }

    var jsonObject: [String: Any] { Dictionary(uniqueKeysWithValues: zip(Self.keys, values.map { $0 as Any })) }
}

/// `adjust::hdr::HdrMethod`.
public enum HDRMethodModel: String, CaseIterable, Sendable, Identifiable {
    case localAdaptation = "local_adaptation", equalizeHistogram = "equalize_histogram"
    case exposureGamma = "exposure_gamma", highlightCompression = "highlight_compression"
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .localAdaptation: "Local Adaptation"
        case .equalizeHistogram: "Equalize Histogram"
        case .exposureGamma: "Exposure and Gamma"
        case .highlightCompression: "Highlight Compression"
        }
    }
}

/// `adjust::hdr::HdrToning` (serde default: neutral Local Adaptation).
public struct HDRToningModel: Equatable, Sendable {
    public var method: HDRMethodModel = .localAdaptation
    /// Level-zero pixels, 0…250.
    public var radius: Double = 30
    /// 0…1.
    public var strength: Double = 0
    /// 0.1…10.
    public var gamma: Double = 1
    /// Stops, −20…20.
    public var exposure: Double = 0
    /// −1…1 each.
    public var detail: Double = 0
    public var shadows: Double = 0
    public var highlights: Double = 0
    public var vibrance: Double = 0
    public var saturation: Double = 0
    /// Output luminance curve points; empty = identity.
    public var curve: [[Double]] = []
    /// Frozen luminance CDF (Equalize Histogram), empty for identity.
    public var equalizeMap: [Double] = []
    public var equalizeMax: Double = 1
    public init() {}

    init(object o: [String: Any]) {
        let d = HDRToningModel()
        let n = { (k: String, v: Double) in JSONNumbers.double(o[k]) ?? v }
        method = (o["method"] as? String).flatMap(HDRMethodModel.init(rawValue:)) ?? .localAdaptation
        radius = n("radius", d.radius); strength = n("strength", d.strength); gamma = n("gamma", d.gamma)
        exposure = n("exposure", d.exposure); detail = n("detail", d.detail); shadows = n("shadows", d.shadows)
        highlights = n("highlights", d.highlights); vibrance = n("vibrance", d.vibrance)
        saturation = n("saturation", d.saturation)
        curve = JSONNumbers.points(o["curve"])
        equalizeMap = JSONNumbers.array(o["equalize_map"]) ?? []
        equalizeMax = n("equalize_max", d.equalizeMax)
    }

    var jsonObject: [String: Any] {
        ["method": method.rawValue, "radius": radius, "strength": strength, "gamma": gamma, "exposure": exposure,
         "detail": detail, "shadows": shadows, "highlights": highlights, "vibrance": vibrance, "saturation": saturation,
         "curve": curve, "equalize_map": equalizeMap, "equalize_max": equalizeMax]
    }
}

/// Black & White slider values (percent, R Y G C B M).
public enum BlackWhitePresets {
    /// Photoshop's Default.
    public static let `default`: [Double] = [40, 60, 40, 60, 20, 80]
    /// Default tint: hue 42°, saturation 20 %, brightness 88 % (HSB), as in Photoshop's Tint option.
    public static let tint: [Double] = [0.882, 0.827, 0.706]
}

/// `adjust::PhotoFilterPreset`: the engine's approximate sRGB swatches (bytes / 255) and Custom.
public enum PhotoFilterPreset: String, CaseIterable, Sendable, Identifiable {
    case warming85 = "warming85", warmingLBA = "warming_lba", warming81 = "warming81"
    case cooling80 = "cooling80", coolingLBB = "cooling_lbb", cooling82 = "cooling82"
    case red, orange, yellow, green, cyan, blue, violet, magenta, sepia
    case deepRed = "deep_red", deepBlue = "deep_blue", deepEmerald = "deep_emerald", deepYellow = "deep_yellow"
    case underwater
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .warming85: "Warming Filter (85)"
        case .warmingLBA: "Warming Filter (LBA)"
        case .warming81: "Warming Filter (81)"
        case .cooling80: "Cooling Filter (80)"
        case .coolingLBB: "Cooling Filter (LBB)"
        case .cooling82: "Cooling Filter (82)"
        case .red: "Red"
        case .orange: "Orange"
        case .yellow: "Yellow"
        case .green: "Green"
        case .cyan: "Cyan"
        case .blue: "Blue"
        case .violet: "Violet"
        case .magenta: "Magenta"
        case .sepia: "Sepia"
        case .deepRed: "Deep Red"
        case .deepBlue: "Deep Blue"
        case .deepEmerald: "Deep Emerald"
        case .deepYellow: "Deep Yellow"
        case .underwater: "Underwater"
        }
    }

    /// adjust/presets.rs bytes.
    public var bytes: [Int] {
        switch self {
        case .warming85: [236, 138, 0]
        case .warmingLBA: [250, 150, 0]
        case .warming81: [235, 177, 19]
        case .cooling80: [0, 109, 255]
        case .coolingLBB: [0, 93, 255]
        case .cooling82: [0, 181, 255]
        case .red: [234, 26, 26]
        case .orange: [243, 132, 23]
        case .yellow: [249, 227, 28]
        case .green: [25, 201, 25]
        case .cyan: [29, 201, 201]
        case .blue: [29, 53, 234]
        case .violet: [111, 29, 234]
        case .magenta: [201, 29, 201]
        case .sepia: [172, 122, 51]
        case .deepRed: [158, 0, 0]
        case .deepBlue: [0, 0, 158]
        case .deepEmerald: [0, 102, 51]
        case .deepYellow: [255, 204, 0]
        case .underwater: [0, 194, 177]
        }
    }

    public var color: [Double] { bytes.map { Double(Float(Double($0) / 255)) } }

    /// The preset whose swatch `color` is (within f32 round-off), nil = Custom.
    public static func matching(_ color: [Double]) -> PhotoFilterPreset? {
        allCases.first { p in zip(p.color, color).allSatisfy { abs($0 - $1) < 1e-5 } && color.count == 3 }
    }
}
