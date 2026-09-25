import Foundation

/// One numeric develop control: where it lives in the engine's `DevelopSettings` JSON, its range,
/// neutral value and display format. Every panel slider is one of these, so each panel's JSON
/// merge patch is defined (and tested) here rather than in view code.
public struct DevelopControl: Hashable, Sendable, Identifiable {
    public let title: String
    public let path: [String]
    public let range: ClosedRange<Double>
    public let defaultValue: Double
    public let step: Double
    public let format: String
    /// Name in history entries when the title alone is ambiguous ("Vignette Amount").
    public let historyName: String

    public init(_ title: String, _ path: [String], _ range: ClosedRange<Double>, default defaultValue: Double = 0,
                step: Double = 1, format: String? = nil, history: String? = nil) {
        self.title = title
        self.historyName = history ?? title
        self.path = path
        self.range = range
        self.defaultValue = defaultValue
        self.step = step
        self.format = format ?? (range.lowerBound < 0 ? "%+.0f" : "%.0f")
    }

    public var id: String { path.joined(separator: ".") }

    /// The engine merge patch that sets this control, e.g. `{"color":{"hsl":{"hue":{"red":-10}}}}`.
    public func patch(_ value: Double) -> [String: Any] {
        DevelopController.patch(path, clamp(value))
    }

    public func clamp(_ v: Double) -> Double { min(max(v, range.lowerBound), range.upperBound) }

    /// History entry label, e.g. "Orange Hue −10".
    public func historyLabel(_ value: Double) -> String {
        historyName + " " + String(format: format, value)
    }
}

// MARK: - Tone curve

public enum ParametricRegion: String, CaseIterable, Sendable {
    case highlights, lights, darks, shadows
    public var title: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }
    public var control: DevelopControl {
        DevelopControl(title, ["tone", "curves", "parametric", rawValue], -100...100, history: "Curve \(title)")
    }
}

public enum SplitPoint: String, CaseIterable, Sendable {
    case shadow = "shadow_split", midtone = "midtone_split", highlight = "highlight_split"
    public var defaultValue: Double { switch self { case .shadow: 25; case .midtone: 50; case .highlight: 75 } }
    public var path: [String] { ["tone", "curves", "parametric", rawValue] }
}

/// The five point curves of `ToneCurves`.
public enum CurveChannel: String, CaseIterable, Sendable, Identifiable {
    case rgb, red, green, blue, luminance
    public var id: String { rawValue }
    public var title: String {
        switch self { case .rgb: "RGB"; case .luminance: "Luminance"; default: rawValue.capitalized }
    }
    public var path: [String] { ["tone", "curves", rawValue] }
}

// MARK: - HSL

/// The eight OkLCh hue bands of the HSL mixer, in engine order.
public enum HueBand: String, CaseIterable, Sendable, Identifiable {
    case red, orange, yellow, green, aqua, blue, purple, magenta
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    /// Band centre, OkLCh hue degrees (pipeline-cpu COLOR_DETAIL_M2.md).
    public var center: Double { [25, 55, 95, 145, 195, 255, 295, 335][index] }
    public var index: Int { Self.allCases.firstIndex(of: self)! }
}

public enum HSLProperty: String, CaseIterable, Sendable, Identifiable {
    case hue, saturation, luminance
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    public func control(_ band: HueBand) -> DevelopControl {
        DevelopControl(band.title, ["color", "hsl", rawValue, band.rawValue], -100...100, history: "\(band.title) \(title)")
    }
}

// MARK: - Colour grading

public enum GradeRange: String, CaseIterable, Sendable, Identifiable {
    case shadows, midtones, highlights, global
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    public var hue: DevelopControl {
        DevelopControl("Hue", ["color", "grading", rawValue, "hue"], 0...360, format: "%.0f°", history: "\(title) Hue")
    }
    public var saturation: DevelopControl {
        DevelopControl("Saturation", ["color", "grading", rawValue, "saturation"], 0...100, history: "\(title) Saturation")
    }
    public var luminance: DevelopControl {
        DevelopControl("Luminance", ["color", "grading", rawValue, "luminance"], -100...100, history: "\(title) Luminance")
    }

    public static let blending = DevelopControl("Blending", ["color", "grading", "blending"], 0...100, default: 50,
                                                history: "Grading Blending")
    public static let balance = DevelopControl("Balance", ["color", "grading", "balance"], -100...100,
                                               history: "Grading Balance")

    /// Hue and saturation of one wheel in a single patch (a wheel drag moves both).
    public func wheelPatch(hue: Double, saturation: Double) -> [String: Any] {
        ["color": ["grading": [rawValue: ["hue": self.hue.clamp(hue), "saturation": self.saturation.clamp(saturation)]]]]
    }
}

// MARK: - Detail

public enum DetailControls {
    public static let amount = DevelopControl("Amount", ["detail", "sharpening", "amount"], 0...150, default: 40,
                                              history: "Sharpening Amount")
    public static let radius = DevelopControl("Radius", ["detail", "sharpening", "radius"], 0.5...3, default: 1,
                                              step: 0.1, format: "%.1f", history: "Sharpening Radius")
    public static let detail = DevelopControl("Detail", ["detail", "sharpening", "detail"], 0...100, default: 25,
                                              history: "Sharpening Detail")
    public static let masking = DevelopControl("Masking", ["detail", "sharpening", "masking"], 0...100,
                                               history: "Sharpening Masking")
    public static let luminance = DevelopControl("Luminance", ["detail", "noise_reduction", "luminance"], 0...100,
                                                 history: "Luminance NR")
    public static let luminanceDetail = DevelopControl("Detail", ["detail", "noise_reduction", "luminance_detail"],
                                                       0...100, default: 50, history: "Luminance NR Detail")
    public static let luminanceContrast = DevelopControl("Contrast", ["detail", "noise_reduction", "luminance_contrast"],
                                                         0...100, history: "Luminance NR Contrast")
    public static let color = DevelopControl("Color", ["detail", "noise_reduction", "color"], 0...100, default: 25,
                                             history: "Color NR")
    public static let colorDetail = DevelopControl("Color Detail", ["detail", "noise_reduction", "color_detail"], 0...100,
                                                   default: 50, history: "Color NR Detail")
    public static let colorSmoothness = DevelopControl("Smoothness", ["detail", "noise_reduction", "color_smoothness"],
                                                       0...100, default: 50, history: "Color NR Smoothness")
    public static let sharpening = [amount, radius, detail, masking]
    public static let luminanceNoise = [luminance, luminanceDetail, luminanceContrast]
    public static let colorNoise = [color, colorDetail, colorSmoothness]
}

// MARK: - Effects

public enum VignetteStyle: String, CaseIterable, Sendable, Identifiable {
    case highlightPriority = "highlight_priority", colorPriority = "color_priority", paintOverlay = "paint_overlay"
    public var id: String { rawValue }
    public var title: String {
        switch self { case .highlightPriority: "Highlight Priority"; case .colorPriority: "Color Priority"; case .paintOverlay: "Paint Overlay" }
    }
    public static let path = ["effects", "vignette", "style"]
}

public enum EffectsControls {
    public static let amount = DevelopControl("Amount", ["effects", "vignette", "amount"], -100...100, history: "Vignette Amount")
    public static let midpoint = DevelopControl("Midpoint", ["effects", "vignette", "midpoint"], 0...100, default: 50,
                                                history: "Vignette Midpoint")
    public static let roundness = DevelopControl("Roundness", ["effects", "vignette", "roundness"], -100...100,
                                                 history: "Vignette Roundness")
    public static let feather = DevelopControl("Feather", ["effects", "vignette", "feather"], 0...100, default: 50,
                                               history: "Vignette Feather")
    public static let highlights = DevelopControl("Highlights", ["effects", "vignette", "highlights"], 0...100,
                                                  history: "Vignette Highlights")
    public static let grainAmount = DevelopControl("Amount", ["effects", "grain", "amount"], 0...100, history: "Grain Amount")
    public static let grainSize = DevelopControl("Size", ["effects", "grain", "size"], 0...100, default: 25, history: "Grain Size")
    public static let grainRoughness = DevelopControl("Roughness", ["effects", "grain", "roughness"], 0...100, default: 50,
                                                      history: "Grain Roughness")
    public static let vignette = [amount, midpoint, roundness, feather, highlights]
    public static let grain = [grainAmount, grainSize, grainRoughness]
}

// MARK: - Crop

public enum CropControls {
    /// Straighten angle in the engine (sensor-orientation) convention; the crop tool converts.
    public static let anglePath = ["geometry", "crop", "angle"]
    public static let rectPath = ["geometry", "crop", "rect"]
    public static let aspectPath = ["geometry", "crop", "aspect"]
}
