import Foundation

/// The 27 Photoshop layer blend modes (spec 02 §1.2, COMPOSITOR.md §2) in menu order, with the
/// stable backend strings of `compositor::BlendMode` (serde snake_case). Groups add Pass Through
/// through `LayerGroupMode`, not here.
public enum DocBlendMode: String, CaseIterable, Sendable, Identifiable {
    case normal, dissolve
    case darken, multiply, colorBurn = "color_burn", linearBurn = "linear_burn", darkerColor = "darker_color"
    case lighten, screen, colorDodge = "color_dodge", linearDodge = "linear_dodge", lighterColor = "lighter_color"
    case overlay, softLight = "soft_light", hardLight = "hard_light", vividLight = "vivid_light",
         linearLight = "linear_light", pinLight = "pin_light", hardMix = "hard_mix"
    case difference, exclusion, subtract, divide
    case hue, saturation, color, luminosity

    public var id: String { rawValue }
    /// The backend string (`LayerRecord.blendMode`, `set_blend_mode`).
    public var backendName: String { rawValue }
    public init?(backendName: String) { self.init(rawValue: backendName) }

    public var title: String {
        switch self {
        case .normal: "Normal"
        case .dissolve: "Dissolve"
        case .darken: "Darken"
        case .multiply: "Multiply"
        case .colorBurn: "Color Burn"
        case .linearBurn: "Linear Burn"
        case .darkerColor: "Darker Color"
        case .lighten: "Lighten"
        case .screen: "Screen"
        case .colorDodge: "Color Dodge"
        case .linearDodge: "Linear Dodge (Add)"
        case .lighterColor: "Lighter Color"
        case .overlay: "Overlay"
        case .softLight: "Soft Light"
        case .hardLight: "Hard Light"
        case .vividLight: "Vivid Light"
        case .linearLight: "Linear Light"
        case .pinLight: "Pin Light"
        case .hardMix: "Hard Mix"
        case .difference: "Difference"
        case .exclusion: "Exclusion"
        case .subtract: "Subtract"
        case .divide: "Divide"
        case .hue: "Hue"
        case .saturation: "Saturation"
        case .color: "Color"
        case .luminosity: "Luminosity"
        }
    }

    /// Photoshop's menu groups, separated by dividers in the pop-up.
    public enum Group: String, CaseIterable, Sendable {
        case normal = "Normal", darken = "Darken", lighten = "Lighten", contrast = "Contrast",
             inversion = "Inversion", component = "Component"
    }

    public var group: Group {
        switch self {
        case .normal, .dissolve: .normal
        case .darken, .multiply, .colorBurn, .linearBurn, .darkerColor: .darken
        case .lighten, .screen, .colorDodge, .linearDodge, .lighterColor: .lighten
        case .overlay, .softLight, .hardLight, .vividLight, .linearLight, .pinLight, .hardMix: .contrast
        case .difference, .exclusion, .subtract, .divide: .inversion
        case .hue, .saturation, .color, .luminosity: .component
        }
    }

    /// Menu sections in order.
    public static var grouped: [(Group, [DocBlendMode])] {
        Group.allCases.map { g in (g, allCases.filter { $0.group == g }) }
    }

    /// Stable index (menu order), also the compositor's GPU mode code.
    public var index: Int { Self.allCases.firstIndex(of: self)! }

    /// Title for a layer's mode string; groups in pass-through show "Pass Through".
    public static func title(backendName: String, groupMode: LayerGroupMode?) -> String {
        if groupMode == .passThrough { return "Pass Through" }
        return DocBlendMode(backendName: backendName)?.title ?? backendName
    }
}
