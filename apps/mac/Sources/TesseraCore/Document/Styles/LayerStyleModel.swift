import Foundation

// Layer styles in the app (WP B5-07): the Swift side of `compositor::render::styles::LayerStyles`
// JSON (`{"effects":[{"kind":"drop_shadow","settings":{…}}],"scale":1}`) as the inspector edits it,
// the effect kinds in the engine's stacking order and the document's Global Light. Settings stay
// `StyleJSON` so fields the inspector does not edit (contour, jitter, pattern pixels) round-trip.

/// The effect kinds of `StyleEffect` (serde names). `allCases` is the engine's stacking order top
/// first, the order the Layer Style inspector and the Layers panel list effects in.
public enum StyleEffectKind: String, CaseIterable, Sendable, Identifiable, Codable {
    case bevel
    case stroke
    case innerShadow = "inner_shadow"
    case innerGlow = "inner_glow"
    case satin
    case colorOverlay = "color_overlay"
    /// Generic fill overlay (composited at the color-overlay rank); PSD files and agents may use it.
    case overlay
    case gradientOverlay = "gradient_overlay"
    case patternOverlay = "pattern_overlay"
    case outerGlow = "outer_glow"
    case dropShadow = "drop_shadow"

    public var id: String { rawValue }

    /// Stacking rank (0 = bottom) as `render::styles` composites the kinds.
    public var rank: Int {
        switch self {
        case .dropShadow: 0
        case .outerGlow: 1
        case .patternOverlay: 2
        case .gradientOverlay: 3
        case .colorOverlay, .overlay: 4
        case .satin: 5
        case .innerGlow: 6
        case .innerShadow: 7
        case .stroke: 8
        case .bevel: 9
        }
    }

    public var title: String {
        switch self {
        case .bevel: "Bevel & Emboss"
        case .stroke: "Stroke"
        case .innerShadow: "Inner Shadow"
        case .innerGlow: "Inner Glow"
        case .satin: "Satin"
        case .colorOverlay: "Color Overlay"
        case .gradientOverlay: "Gradient Overlay"
        case .patternOverlay: "Pattern Overlay"
        case .overlay: "Overlay"
        case .outerGlow: "Outer Glow"
        case .dropShadow: "Drop Shadow"
        }
    }

    /// Kinds that may appear more than once (Photoshop's "+" kinds: strokes, overlays, shadows).
    public var isRepeatable: Bool {
        switch self {
        case .stroke, .innerShadow, .dropShadow, .colorOverlay, .gradientOverlay, .patternOverlay, .overlay: true
        default: false
        }
    }

    /// Kinds with a Use Global Light switch (their light angle, and a bevel's altitude, can follow it).
    public var usesGlobalLight: Bool { self == .dropShadow || self == .innerShadow || self == .bevel }

    /// Kinds offered in menus and the inspector (the generic overlay is shown only when present).
    public static var menuKinds: [StyleEffectKind] { allCases.filter { $0 != .overlay } }
}

/// One effect: its kind and its settings object.
public struct StyleEffect: Equatable, Sendable {
    public var kind: StyleEffectKind
    public var settings: StyleJSON

    public init(kind: StyleEffectKind, settings: StyleJSON = .object([:])) {
        self.kind = kind
        self.settings = settings.object == nil ? .object([:]) : settings
    }

    /// serde defaults `enabled` to true.
    public var enabled: Bool {
        get { settings["enabled"]?.bool ?? true }
        set { settings["enabled"] = .bool(newValue) }
    }

    public func number(_ key: String) -> Double? { settings[key]?.double }
    public mutating func setNumber(_ key: String, _ value: Double) { settings[key] = .number(value) }
    public func flag(_ key: String) -> Bool? { settings[key]?.bool }
    public mutating func setFlag(_ key: String, _ value: Bool) { settings[key] = .bool(value) }
    public func text(_ key: String) -> String? { settings[key]?.string }
    public mutating func setText(_ key: String, _ value: String) { settings[key] = .string(value) }
    /// Straight RGBA (`[r, g, b, a]`).
    public func color(_ key: String) -> [Double]? { settings[key]?.doubles.flatMap { $0.count == 4 ? $0 : nil } }
    public mutating func setColor(_ key: String, _ rgba: [Double]) { settings[key] = StyleJSON(rgba) }
    /// The effect's `compositor::Fill` (overlays and strokes).
    public var fill: FillModel? {
        get { settings["fill"].flatMap { FillModel(json: $0.text) } }
        set { if let f = newValue, let v = StyleJSON.parse(f.json) { settings["fill"] = v } }
    }

    public var usesGlobalLight: Bool {
        get { kind.usesGlobalLight && (flag("use_global_light") ?? true) }
        set { if kind.usesGlobalLight { setFlag("use_global_light", newValue) } }
    }

    var json: StyleJSON { .object(["kind": .string(kind.rawValue), "settings": settings]) }

    init?(json: StyleJSON) {
        guard let k = json["kind"]?.string, let kind = StyleEffectKind(rawValue: k) else { return nil }
        self.init(kind: kind, settings: json["settings"] ?? .object([:]))
    }
}

/// The document's light (`GlobalLightRecord`): angle in degrees (0 from the right, 90 from above)
/// and altitude (elevation) 0…90 degrees.
public struct DocGlobalLight: Equatable, Sendable {
    public var angle: Double
    public var altitude: Double
    public init(angle: Double = 120, altitude: Double = 30) { self.angle = angle; self.altitude = altitude }
    public static let `default` = DocGlobalLight()
}

/// A layer's styles (`LayerStyles` JSON).
public struct LayerStyleModel: Equatable, Sendable {
    /// Effects in the engine's vector order (equal kinds composite in this order, later on top).
    public var effects: [StyleEffect]
    /// Scale Effects as a multiplier (1 = 100 %).
    public var scale: Double
    /// Top-level fields this model does not know, kept as read.
    private var extra: [String: StyleJSON]

    public static let maxEffects = 64
    public static let maxPerKind = 10

    public init(effects: [StyleEffect] = [], scale: Double = 1) {
        self.effects = effects; self.scale = scale; extra = [:]
    }

    /// Nil when `json` is not `LayerStyles` JSON or names an unknown effect kind.
    public init?(json: String?) {
        guard let v = StyleJSON.parse(json), var o = v.object else { return nil }
        var effects: [StyleEffect] = []
        for e in o["effects"]?.array ?? [] {
            guard let effect = StyleEffect(json: e) else { return nil }
            effects.append(effect)
        }
        self.effects = effects
        scale = o["scale"]?.double ?? 1
        o["effects"] = nil
        o["scale"] = nil
        extra = o
    }

    public var json: String {
        var o = extra
        o["effects"] = .array(effects.map(\.json))
        o["scale"] = .number(scale)
        return StyleJSON.object(o).text
    }

    public var isEmpty: Bool { effects.isEmpty }

    /// Effect indices top first: descending stacking rank, and within a kind the later entry
    /// (composited above) first. The engine's order; the inspector never reorders across kinds.
    public var displayOrder: [Int] {
        effects.indices.sorted { a, b in
            let (ra, rb) = (effects[a].kind.rank, effects[b].kind.rank)
            return ra != rb ? ra > rb : a > b
        }
    }

    public func indices(of kind: StyleEffectKind) -> [Int] { displayOrder.filter { effects[$0].kind == kind } }

    public func count(of kind: StyleEffectKind) -> Int { effects.lazy.filter { $0.kind == kind }.count }

    /// Whether another effect of `kind` may be added.
    public func canAdd(_ kind: StyleEffectKind) -> Bool {
        guard effects.count < Self.maxEffects else { return false }
        let n = count(of: kind)
        return n == 0 || (kind.isRepeatable && n < Self.maxPerKind)
    }

    /// Adds an effect of `kind` from `settings` directly above effect `above` when given (the "+"
    /// of a repeatable effect), else above every effect of its kind. Returns its index, or nil when
    /// the kind cannot take another.
    @discardableResult
    public mutating func add(_ kind: StyleEffectKind, settings: StyleJSON, above: Int? = nil) -> Int? {
        guard canAdd(kind) else { return nil }
        let at: Int
        if let above, effects.indices.contains(above), effects[above].kind == kind {
            at = above + 1
        } else {
            at = (effects.lastIndex { $0.kind == kind }).map { $0 + 1 } ?? effects.count
        }
        effects.insert(StyleEffect(kind: kind, settings: settings), at: at)
        return at
    }

    public mutating func remove(at index: Int) {
        guard effects.indices.contains(index) else { return }
        effects.remove(at: index)
    }

    // MARK: Global Light

    /// The angle effect `index` is lit from: the document's when it uses the Global Light.
    public func angle(of index: Int, global: DocGlobalLight) -> Double {
        let e = effects[index]
        return e.usesGlobalLight ? global.angle : (e.number("angle") ?? 120)
    }

    /// A bevel's altitude (elevation), following the Global Light like its angle.
    public func altitude(of index: Int, global: DocGlobalLight) -> Double {
        let e = effects[index]
        return e.usesGlobalLight ? global.altitude : (e.number("elevation") ?? 30)
    }

    /// Sets the light of effect `index` as Photoshop does: when it uses the Global Light the
    /// document's light changes (every layer following it moves); otherwise only its own angle.
    /// Returns the new Global Light when it changed.
    public mutating func setLight(of index: Int, angle: Double, altitude: Double? = nil,
                                  global: DocGlobalLight) -> DocGlobalLight? {
        guard effects.indices.contains(index) else { return nil }
        if effects[index].usesGlobalLight {
            var g = global
            g.angle = angle
            if let altitude { g.altitude = altitude }
            return g == global ? nil : g
        }
        effects[index].setNumber("angle", angle)
        if let altitude, effects[index].kind == .bevel { effects[index].setNumber("elevation", altitude) }
        return nil
    }

    /// Use Global Light on or off. Turning it off keeps the light where it is (the document's
    /// angle, and altitude for bevels, become the effect's own).
    public mutating func setUsesGlobalLight(_ on: Bool, of index: Int, global: DocGlobalLight) {
        guard effects.indices.contains(index), effects[index].kind.usesGlobalLight else { return }
        if !on, effects[index].usesGlobalLight {
            effects[index].setNumber("angle", global.angle)
            if effects[index].kind == .bevel { effects[index].setNumber("elevation", global.altitude) }
        }
        effects[index].usesGlobalLight = on
    }

    /// Whether any effect follows the Global Light.
    public var followsGlobalLight: Bool { effects.contains { $0.usesGlobalLight } }
}

/// The effects of one styled layer, top first (FFI `LayerStyleSummary`), for the Layers panel's
/// fx glyph and effect rows.
public struct LayerStyleRow: Equatable, Sendable {
    public struct Effect: Equatable, Sendable {
        /// Index into the layer's `effects`.
        public var index: Int
        public var kind: StyleEffectKind
        public var enabled: Bool
        public init(index: Int, kind: StyleEffectKind, enabled: Bool) { self.index = index; self.kind = kind; self.enabled = enabled }
    }
    public var layer: DocLayerID
    public var effects: [Effect]
    public init(layer: DocLayerID, effects: [Effect]) { self.layer = layer; self.effects = effects }

    /// The row a model would produce (the stub computes rows this way).
    public init(layer: DocLayerID, model: LayerStyleModel) {
        self.init(layer: layer, effects: model.displayOrder.map {
            Effect(index: $0, kind: model.effects[$0].kind, enabled: model.effects[$0].enabled)
        })
    }
}

/// Global Light across the open document's styled layers, as the inspector shows it: which layers
/// follow it and the angle each of their effects is drawn with. The Swift-side check that editing
/// one layer's global angle moves every layer that follows it (and no local one).
public struct StyleLightModel: Equatable, Sendable {
    public var global: DocGlobalLight
    public var layers: [DocLayerID: LayerStyleModel]

    public init(global: DocGlobalLight, layers: [DocLayerID: LayerStyleModel]) { self.global = global; self.layers = layers }

    /// Layers with at least one effect following the Global Light.
    public var followers: [DocLayerID] { layers.filter { $0.value.followsGlobalLight }.map(\.key).sorted() }

    /// The angle effect `index` of `layer` is drawn with.
    public func angle(_ layer: DocLayerID, _ index: Int) -> Double? {
        guard let m = layers[layer], m.effects.indices.contains(index) else { return nil }
        return m.angle(of: index, global: global)
    }

    /// Sets the angle of one effect; a Global Light change applies to every follower.
    /// Returns true when the Global Light changed.
    @discardableResult
    public mutating func setAngle(_ angle: Double, layer: DocLayerID, index: Int) -> Bool {
        guard var m = layers[layer] else { return false }
        let changed = m.setLight(of: index, angle: angle, global: global)
        layers[layer] = m
        if let changed { global = changed }
        return changed != nil
    }
}
