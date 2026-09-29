import Foundation

// Filter ▸ Camera Raw Filter… in document mode (WP B5-18): the settings the sheet edits and the filter JSON
// it sends. The engine's `camera_raw` filter (crates/filters/src/camera_raw.rs) takes
// `{"settings": DevelopSettings, "amount": 0…1}` and validates every value against the engine's own
// domains; the sliders are the Develop panels' `DevelopControl`s (DevelopControls.swift), so each one
// writes the same `DevelopSettings` path it writes in Develop.

public enum CameraRawFilter {
    public static let id = "camera_raw"
    public static let title = "Camera Raw Filter"

    /// Why the sheet cannot open on a layer of `kind` (nil: it can). Pixel layers are filtered inside the
    /// selection; on a smart object the filter is a smart filter over the whole layer, and the engine's
    /// retouch smart filters take no selection mask.
    public static func refusal(kind: LayerKindTag?, hasSelection: Bool) -> String? {
        switch kind {
        case .pixel?: nil
        case .smartObject?:
            hasSelection ? "\(title): Deselect first (⌘D). On a smart object it is a smart filter over the whole layer and cannot be limited to a selection." : nil
        default: "\(title): select a pixel layer or a smart object"
        }
    }

    /// `MaskKind` tags the engine computes with a model (`MaskKind::is_ai`).
    public static let aiMaskKinds: Set<String> = ["subject", "sky", "background", "person", "object", "landscape", "depth"]
}

/// The sheet's Basic sliders. Paths are `DevelopParameter`'s; ranges are the engine's `CrsKey` domains.
public enum CameraRawControls {
    /// Rendered pixels have no as-shot white: Custom white balance adapts from this source white to D65,
    /// so 6500 K / 0 is (close to) no change.
    public static let neutralTemperature = 6500.0
    public static let temperature = DevelopControl("Temp", DevelopParameter.temperature.path, 2000...25000,
                                                   default: neutralTemperature, step: 50, format: "%.0f K", history: "Temperature")
    public static let tint = DevelopControl("Tint", DevelopParameter.tint.path, -150...150)
    public static let exposure = DevelopControl("Exposure", DevelopParameter.exposure.path, -5...5, step: 0.01, format: "%+.2f")
    public static let contrast = DevelopControl("Contrast", DevelopParameter.contrast.path, -100...100)
    public static let highlights = DevelopControl("Highlights", DevelopParameter.highlights.path, -100...100)
    public static let shadows = DevelopControl("Shadows", DevelopParameter.shadows.path, -100...100)
    public static let whites = DevelopControl("Whites", DevelopParameter.whites.path, -100...100)
    public static let blacks = DevelopControl("Blacks", DevelopParameter.blacks.path, -100...100)
    public static let texture = DevelopControl("Texture", ["tone", "texture"], -100...100)
    public static let clarity = DevelopControl("Clarity", ["tone", "clarity"], -100...100)
    public static let dehaze = DevelopControl("Dehaze", ["tone", "dehaze"], -100...100)
    public static let vibrance = DevelopControl("Vibrance", ["color", "vibrance"], -100...100)
    public static let saturation = DevelopControl("Saturation", ["color", "saturation"], -100...100)

    /// A parametric curve split point (0…100, kept in shadow < midtone < highlight order by the draft).
    public static func split(_ s: SplitPoint) -> DevelopControl {
        let title = switch s { case .shadow: "Shadow Split"; case .midtone: "Midtone Split"; case .highlight: "Highlight Split" }
        return DevelopControl(title, s.path, 0...100, default: s.defaultValue)
    }
}

public struct CameraRawSection: Identifiable, Sendable {
    public let title: String
    public let controls: [DevelopControl]
    public var id: String { title }
}

/// The sheet's tabs, in Camera Raw's order.
public enum CameraRawPanel: String, CaseIterable, Identifiable, Sendable {
    case basic, curve, hsl, colorGrading = "color_grading", detail, effects
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .basic: "Basic"
        case .curve: "Curve"
        case .hsl: "HSL"
        case .colorGrading: "Color Grading"
        case .detail: "Detail"
        case .effects: "Effects"
        }
    }

    public var sections: [CameraRawSection] {
        typealias C = CameraRawControls
        switch self {
        case .basic:
            return [CameraRawSection(title: "White Balance", controls: [C.temperature, C.tint]),
                    CameraRawSection(title: "Tone", controls: [C.exposure, C.contrast, C.highlights, C.shadows, C.whites, C.blacks]),
                    CameraRawSection(title: "Presence", controls: [C.texture, C.clarity, C.dehaze, C.vibrance, C.saturation])]
        case .curve:
            return [CameraRawSection(title: "Parametric Curve", controls: ParametricRegion.allCases.map(\.control)),
                    CameraRawSection(title: "Split Points", controls: SplitPoint.allCases.map(C.split))]
        case .hsl:
            return HSLProperty.allCases.map { p in CameraRawSection(title: p.title, controls: HueBand.allCases.map(p.control)) }
        case .colorGrading:
            return GradeRange.allCases.map { g in CameraRawSection(title: g.title, controls: [g.hue, g.saturation, g.luminance]) }
                + [CameraRawSection(title: "Blending", controls: [GradeRange.blending, GradeRange.balance])]
        case .detail:
            return [CameraRawSection(title: "Sharpening", controls: DetailControls.sharpening),
                    CameraRawSection(title: "Noise Reduction", controls: DetailControls.luminanceNoise),
                    CameraRawSection(title: "Color Noise Reduction", controls: DetailControls.colorNoise)]
        case .effects:
            return [CameraRawSection(title: "Vignette", controls: EffectsControls.vignette),
                    CameraRawSection(title: "Grain", controls: EffectsControls.grain)]
        }
    }
}

/// What the Camera Raw Filter sheet edits: `DevelopSettings` as a JSON object plus the amount.
/// Settings the sheet has no control for (curve points, locals from a recipe) are kept verbatim.
public struct CameraRawDraft: Equatable, @unchecked Sendable {
    /// `DevelopSettings` JSON (engine `#[serde(default)]`: missing members are engine defaults).
    public private(set) var settings: [String: Any]
    /// 0…1.
    public private(set) var amount: Double

    /// Neutral on rendered pixels: `DevelopSettings::default` sharpens (40), colour-denoises (25) and applies
    /// lens profile / chromatic aberration corrections for raw files; Camera Raw's defaults for non-raw images
    /// turn them off (the sheet has no Optics panel). Without whole-image lens analysis the engine previews
    /// only the visible region (B5-18b).
    public static var neutralSettings: [String: Any] {
        ["detail": ["sharpening": ["amount": 0.0], "noise_reduction": ["color": 0.0]],
         "lens": ["profile": ["kind": "none"], "remove_chromatic_aberration": false]]
    }

    public init() {
        settings = Self.neutralSettings
        amount = 1
    }

    /// Re-edit: parses a smart filter's `{"id":"camera_raw","params":{…}}` (nil for another filter).
    public init?(filterJson: String) {
        guard let root = (try? JSONSerialization.jsonObject(with: Data(filterJson.utf8))) as? [String: Any],
              root["id"] as? String == CameraRawFilter.id else { return nil }
        let params = root["params"] as? [String: Any] ?? [:]
        settings = params["settings"] as? [String: Any] ?? [:]
        amount = min(max((params["amount"] as? NSNumber)?.doubleValue ?? 1, 0), 1)
    }

    public static func == (a: Self, b: Self) -> Bool { a.filterJson == b.filterJson }

    // MARK: Values

    public var amountPercent: Double {
        get { amount * 100 }
        set { amount = min(max(newValue, 0), 100) / 100 }
    }

    /// White balance is Custom (Temp/Tint moved); otherwise as shot, i.e. unchanged.
    public var customWhiteBalance: Bool { Self.lookup(settings, ["white_balance", "mode"]) as? String == "custom" }

    /// The control's value: set, else the engine default (the neutral base's for sharpening / colour NR).
    public func value(_ c: DevelopControl) -> Double {
        if c.path.first == "white_balance" && !customWhiteBalance { return c.defaultValue }
        return (Self.lookup(settings, c.path) as? NSNumber)?.doubleValue ?? c.defaultValue
    }

    public mutating func set(_ c: DevelopControl, _ v: Double) {
        var v = c.clamp(v)
        if let order = Self.splitOrder.firstIndex(of: c.path) {
            // Parametric splits stay strictly ordered (the engine rejects crossed splits).
            let splits = SplitPoint.allCases.map(CameraRawControls.split)
            if order > 0 { v = max(v, value(splits[order - 1]) + 1) }
            if order < splits.count - 1 { v = min(v, value(splits[order + 1]) - 1) }
        }
        if c.path.first == "white_balance" && !customWhiteBalance {
            Self.put(&settings, ["white_balance", "mode"], "custom")
            Self.put(&settings, CameraRawControls.temperature.path, CameraRawControls.neutralTemperature)
            Self.put(&settings, CameraRawControls.tint.path, 0.0)
        }
        Self.put(&settings, c.path, v)
    }

    public mutating func reset() { self = CameraRawDraft() }

    public var isNeutral: Bool { self == CameraRawDraft() }

    // MARK: JSON

    /// `{"amount":…,"settings":{…}}`: the `params` of the filter.
    public var paramsJson: String {
        DevelopController.encode(["settings": settings, "amount": amount]) ?? "{}"
    }

    /// `{"id":"camera_raw","params":{…}}`, keys sorted: what preview / apply / set_smart_filter take.
    public var filterJson: String {
        DevelopController.encode(["id": CameraRawFilter.id, "params": ["settings": settings, "amount": amount]]) ?? "{}"
    }

    // MARK: Zoomed-out preview (B5-18b)

    /// Effects whose pixel radii are full-resolution (Sharpening, Noise Reduction, Texture, Clarity). When the
    /// engine previews a smaller pyramid level (level > 0: zoom at or below 50 %), where they would look too wide,
    /// it leaves them out of the canvas preview (like Camera Raw); the 1:1 pane and OK always include them.
    public static let zoomedOutOmitted: [DevelopControl] = [DetailControls.amount, DetailControls.luminance,
                                                             DetailControls.color, CameraRawControls.texture,
                                                             CameraRawControls.clarity]
    public static let detailPreviewNote = "Detail effects preview at 100 %"

    /// One of `zoomedOutOmitted` is active (0 is off for each of them).
    public var hasDetailEffects: Bool { Self.zoomedOutOmitted.contains { value($0) != 0 } }

    /// The sheet's note for a preview the engine renders at pyramid `previewLevel` (`filterPreviewLevel`): nil
    /// unless the level is above 0 (the engine omits the detail effects) and a detail effect is active.
    public func detailPreviewNote(previewLevel: Int) -> String? {
        previewLevel > 0 && hasDetailEffects ? Self.detailPreviewNote : nil
    }

    // MARK: AI masks

    /// AI mask component kinds in the settings' local adjustments.
    public var aiMaskKinds: [String] {
        let groups = Self.lookup(settings, ["locals", "adjustments"]) as? [[String: Any]] ?? []
        let kinds = groups.flatMap { ($0["components"] as? [[String: Any]] ?? []).compactMap { $0["kind"] as? String } }
        return kinds.filter(CameraRawFilter.aiMaskKinds.contains)
    }

    /// Why this recipe cannot be edited here (nil: it can).
    public var aiMaskRefusal: String? {
        let kinds = aiMaskKinds
        guard !kinds.isEmpty else { return nil }
        return "\(CameraRawFilter.title): this recipe uses AI masks (\(Array(Set(kinds)).sorted().joined(separator: ", "))); no AI mask provider is installed, so it cannot be previewed or edited here"
    }

    // MARK: Helpers

    private static let splitOrder = SplitPoint.allCases.map(\.path)

    static func lookup(_ obj: [String: Any], _ path: [String]) -> Any? {
        var cur: Any? = obj
        for k in path { cur = (cur as? [String: Any])?[k] }
        return cur
    }

    static func put(_ obj: inout [String: Any], _ path: [String], _ value: Any) {
        guard let first = path.first else { return }
        if path.count == 1 { obj[first] = value; return }
        var child = obj[first] as? [String: Any] ?? [:]
        put(&child, Array(path.dropFirst()), value)
        obj[first] = child
    }
}
