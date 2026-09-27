import Foundation

/// The engine's description of the effect kinds (`style_effects_schema_json()`, WP B5-07): fields,
/// UI ranges, defaults and flags. The Layer Style inspector generates each effect's editor from it.
public struct StyleSchema: Equatable, Sendable {
    public struct Option: Equatable, Sendable {
        public var value: StyleJSON
        public var title: String
    }

    public enum FieldType: String, Sendable {
        case number, angle, bool, color
        case blendMode = "blend_mode"
        case choice = "enum"
        case fill
    }

    public struct Field: Equatable, Sendable, Identifiable {
        /// Key in the effect's `settings`.
        public var key: String
        public var title: String
        public var type: FieldType
        /// Range in stored units (a slider's range, not the engine's hard limit).
        public var min: Double
        public var max: Double
        /// Unit shown after the value; the value is shown × `displayScale` (opacity 0…1 as 0…100 %).
        public var unit: String
        public var displayScale: Double
        public var options: [Option]
        public var id: String { key }
    }

    /// A stored field that is kept but not rendered (contour, jitter, texture).
    public struct Metadata: Equatable, Sendable {
        public var key: String
        public var title: String
        public var note: String
    }

    public struct Effect: Equatable, Sendable {
        public var kind: StyleEffectKind
        public var title: String
        public var repeatable: Bool
        public var globalLight: Bool
        /// Survives a PSD save.
        public var psd: Bool
        /// Composited behind the layer.
        public var behind: Bool
        public var defaults: StyleJSON
        public var fields: [Field]
        public var metadata: [Metadata]
    }

    /// Top first (the engine's stacking order).
    public var effects: [Effect]
    public var scale: Field
    public var maxEffects: Int
    public var maxPerKind: Int

    public func effect(_ kind: StyleEffectKind) -> Effect? { effects.first { $0.kind == kind } }

    public init?(json: String) {
        guard let v = StyleJSON.parse(json), let list = v["effects"]?.array, let scale = v["scale"].flatMap(Self.field) else {
            return nil
        }
        effects = list.compactMap { e in
            guard let k = e["kind"]?.string, let kind = StyleEffectKind(rawValue: k) else { return nil }
            return Effect(kind: kind, title: e["title"]?.string ?? kind.title, repeatable: e["repeatable"]?.bool ?? false,
                          globalLight: e["global_light"]?.bool ?? false, psd: e["psd"]?.bool ?? false,
                          behind: e["behind"]?.bool ?? false, defaults: e["defaults"] ?? .object([:]),
                          fields: (e["fields"]?.array ?? []).compactMap(Self.field),
                          metadata: (e["metadata"]?.array ?? []).compactMap { m in
                              guard let key = m["key"]?.string else { return nil }
                              return Metadata(key: key, title: m["title"]?.string ?? key, note: m["note"]?.string ?? "")
                          })
        }
        self.scale = scale
        maxEffects = Int(v["max_effects"]?.double ?? 64)
        maxPerKind = Int(v["max_per_kind"]?.double ?? 10)
    }

    private static func field(_ f: StyleJSON) -> Field? {
        guard let key = f["key"]?.string, let t = f["type"]?.string, let type = FieldType(rawValue: t) else { return nil }
        return Field(key: key, title: f["title"]?.string ?? key, type: type, min: f["min"]?.double ?? 0,
                     max: f["max"]?.double ?? 1, unit: f["unit"]?.string ?? "", displayScale: f["display_scale"]?.double ?? 1,
                     options: (f["options"]?.array ?? []).compactMap { o in
                         guard let v = o["value"] else { return nil }
                         return Option(value: v, title: o["title"]?.string ?? "")
                     })
    }

    /// Default settings for a new effect of `kind` on a canvas `width` pixels wide (gradient
    /// overlays span it).
    public func defaults(_ kind: StyleEffectKind, width: Double) -> StyleJSON {
        var d = effect(kind)?.defaults ?? .object([:])
        if kind == .gradientOverlay, d["fill"]?["kind"]?.string == "gradient" {
            d["fill"]?["end"] = StyleJSON([max(width, 1), 0])
        }
        return d
    }
}
