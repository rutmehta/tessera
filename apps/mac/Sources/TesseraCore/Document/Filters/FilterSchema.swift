import Foundation

// The Filter menu's data model (WP B5-05): the engine's `list_filters()` catalogue
// (crates/filters/src/registry.rs) decoded into controls, the values a filter dialog edits, the
// filter JSON the session takes, and the memory behind Filter ▸ Last Filter (⌃F).

/// One control of a filter dialog, from the `params_schema_json` of a catalogue entry.
public enum FilterControl: Equatable, Sendable {
    /// A number: `unit` is `px`, `%`, `levels` or empty; `spatial` values scale with the preview level.
    case slider(min: Double, max: Double, defaultValue: Double, step: Double, unit: String, spatial: Bool, integer: Bool)
    /// Degrees.
    case angle(min: Double, max: Double, defaultValue: Double)
    /// `(value, label)` options.
    case choice(options: [FilterChoice], defaultValue: String)
    /// A point in normalized canvas coordinates.
    case point(defaultX: Double, defaultY: Double)
    case toggle(defaultValue: Bool)

    public var defaultValue: FilterValue {
        switch self {
        case .slider(_, _, let d, _, _, _, _), .angle(_, _, let d): .number(d)
        case .choice(_, let d): .text(d)
        case .point(let x, let y): .point(x, y)
        case .toggle(let d): .bool(d)
        }
    }

    /// The printf format a slider shows its value with (`%.1f px`, `%.0f °`).
    public var format: String {
        switch self {
        case .slider(_, _, _, let step, let unit, _, let integer):
            let digits = integer || step >= 1 ? 0 : (step >= 0.1 ? 1 : 2)
            let suffix = switch unit {
            case "": ""
            case "%": " %%"
            case "levels": ""
            default: " " + unit
            }
            return "%.\(digits)f" + suffix
        case .angle: return "%.0f°"
        default: return "%.0f"
        }
    }

    /// `value` clamped (and rounded, for integer sliders) into the control's range.
    public func clamped(_ value: FilterValue) -> FilterValue {
        switch (self, value) {
        case (.slider(let lo, let hi, _, _, _, _, let integer), .number(let v)):
            let c = min(max(v, lo), hi)
            return .number(integer ? c.rounded() : c)
        case (.angle(let lo, let hi, _), .number(let v)):
            return .number(min(max(v, lo), hi))
        case (.choice(let options, let d), .text(let t)):
            return .text(options.contains { $0.value == t } ? t : d)
        case (.point, .point(let x, let y)):
            return .point(min(max(x, 0), 1), min(max(y, 0), 1))
        case (.toggle, .bool):
            return value
        default:
            return defaultValue
        }
    }
}

public struct FilterChoice: Equatable, Sendable, Hashable {
    public var value: String
    public var label: String
    public init(value: String, label: String) { self.value = value; self.label = label }
}

public struct FilterParam: Equatable, Sendable, Identifiable {
    public var key: String
    public var label: String
    public var control: FilterControl
    public var id: String { key }
    public init(key: String, label: String, control: FilterControl) { self.key = key; self.label = label; self.control = control }
}

/// One parameter value (the JSON types of filter parameters).
public enum FilterValue: Equatable, Sendable, Hashable {
    case number(Double)
    case bool(Bool)
    case text(String)
    case point(Double, Double)

    public var number: Double? { if case .number(let v) = self { v } else { nil } }
    public var bool: Bool? { if case .bool(let v) = self { v } else { nil } }
    public var text: String? { if case .text(let v) = self { v } else { nil } }

    var json: Any {
        switch self {
        case .number(let v): v
        case .bool(let v): v
        case .text(let v): v
        case .point(let x, let y): [x, y]
        }
    }

    init?(json: Any) {
        switch json {
        case let b as Bool where type(of: json) == type(of: NSNumber(value: true)): self = .bool(b)
        case let n as NSNumber: self = .number(n.doubleValue)
        case let s as String: self = .text(s)
        case let a as [NSNumber] where a.count == 2: self = .point(a[0].doubleValue, a[1].doubleValue)
        default: return nil
        }
    }
}

/// A catalogue entry (`list_filters()` / FFI `FilterInfo`).
public struct FilterCatalogEntry: Equatable, Sendable, Identifiable {
    public var id: String
    public var group: String
    public var name: String
    public var params: [FilterParam]

    public init(id: String, group: String, name: String, params: [FilterParam]) {
        self.id = id; self.group = group; self.name = name; self.params = params
    }

    /// Decodes `params_schema_json`; unknown control kinds are skipped.
    public init(id: String, group: String, name: String, schemaJson: String) {
        self.init(id: id, group: group, name: name, params: Self.decode(schemaJson))
    }

    static func decode(_ json: String) -> [FilterParam] {
        guard let root = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any],
              let params = root["params"] as? [[String: Any]] else { return [] }
        return params.compactMap { p in
            guard let key = p["key"] as? String, let kind = p["kind"] as? String else { return nil }
            let label = p["label"] as? String ?? key
            let num = { (k: String) in (p[k] as? NSNumber)?.doubleValue ?? 0 }
            let control: FilterControl
            switch kind {
            case "slider":
                control = .slider(min: num("min"), max: num("max"), defaultValue: num("default"),
                                  step: (p["step"] as? NSNumber)?.doubleValue ?? 1, unit: p["unit"] as? String ?? "",
                                  spatial: p["spatial"] as? Bool ?? false, integer: p["integer"] as? Bool ?? false)
            case "angle":
                control = .angle(min: num("min"), max: num("max"), defaultValue: num("default"))
            case "choice":
                let options = (p["options"] as? [[String: Any]] ?? []).compactMap { o -> FilterChoice? in
                    guard let v = o["value"] as? String else { return nil }
                    return FilterChoice(value: v, label: o["label"] as? String ?? v)
                }
                guard !options.isEmpty else { return nil }
                control = .choice(options: options, defaultValue: p["default"] as? String ?? options[0].value)
            case "point":
                let d = (p["default"] as? [NSNumber])?.map(\.doubleValue) ?? [0.5, 0.5]
                control = .point(defaultX: d.first ?? 0.5, defaultY: d.count > 1 ? d[1] : 0.5)
            case "toggle":
                control = .toggle(defaultValue: p["default"] as? Bool ?? false)
            default:
                return nil
            }
            return FilterParam(key: key, label: label, control: control)
        }
    }

    public var defaults: [String: FilterValue] {
        Dictionary(uniqueKeysWithValues: params.map { ($0.key, $0.control.defaultValue) })
    }

    /// Menu title with an ellipsis when the filter opens a dialog.
    public var menuTitle: String { params.isEmpty ? name : name + "…" }

    /// Menu groups in Photoshop order (`filters::registry::GROUPS`).
    public static let groupOrder = ["Blur", "Sharpen", "Noise", "Distort", "Stylize", "Render", "Other"]

    /// Entries grouped for the Filter menu, groups in `groupOrder`, entries in catalogue order.
    public static func grouped(_ entries: [FilterCatalogEntry]) -> [(group: String, entries: [FilterCatalogEntry])] {
        var order = groupOrder
        for e in entries where !order.contains(e.group) { order.append(e.group) }
        return order.compactMap { g in
            let es = entries.filter { $0.group == g }
            return es.isEmpty ? nil : (g, es)
        }
    }
}

/// A filter with its values: what `preview_filter` / `apply_filter` take as JSON.
public struct FilterSettings: Equatable, Sendable {
    public var id: String
    public var values: [String: FilterValue]

    public init(id: String, values: [String: FilterValue] = [:]) { self.id = id; self.values = values }

    /// Defaults of `entry`, overridden by `values` (clamped to each control).
    public init(_ entry: FilterCatalogEntry, values: [String: FilterValue] = [:]) {
        var v = entry.defaults
        for p in entry.params { if let given = values[p.key] { v[p.key] = p.control.clamped(given) } }
        self.init(id: entry.id, values: v)
    }

    /// `{"id":…,"params":{…}}`, keys sorted.
    public var json: String {
        let params = values.mapValues(\.json)
        let obj: [String: Any] = ["id": id, "params": params]
        let data = (try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }

    /// Parses filter JSON (smart filter records).
    public init?(json: String) {
        guard let root = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any],
              let id = root["id"] as? String else { return nil }
        var values: [String: FilterValue] = [:]
        for (k, v) in root["params"] as? [String: Any] ?? [:] { if let fv = FilterValue(json: v) { values[k] = fv } }
        self.init(id: id, values: values)
    }

    public func number(_ key: String) -> Double? { values[key]?.number }
}

/// Filter ▸ Last Filter (⌃F) and the values each filter dialog reopens with (Photoshop keeps the
/// last-used settings per filter for the session). Optionally persisted in user defaults.
public final class FilterMemory: @unchecked Sendable {
    private let lock = NSLock()
    private var lastID: String?
    private var perFilter: [String: [String: FilterValue]] = [:]
    private let defaults: UserDefaults?
    private let key: String

    public init(defaults: UserDefaults? = nil, key: String = "documentFilterMemory") {
        self.defaults = defaults
        self.key = key
        load()
    }

    /// The filter applied last, with its values (nil before the first filter).
    public var last: FilterSettings? {
        lock.lock(); defer { lock.unlock() }
        return lastID.map { FilterSettings(id: $0, values: perFilter[$0] ?? [:]) }
    }

    /// The values a filter's dialog opens with: the last applied ones, else the defaults.
    public func settings(for entry: FilterCatalogEntry) -> FilterSettings {
        lock.lock(); defer { lock.unlock() }
        return FilterSettings(entry, values: perFilter[entry.id] ?? [:])
    }

    /// Records an applied filter (OK, or ⌃F).
    public func record(_ s: FilterSettings) {
        lock.lock()
        lastID = s.id
        perFilter[s.id] = s.values
        lock.unlock()
        save()
    }

    private func load() {
        guard let d = defaults?.string(forKey: key),
              let root = (try? JSONSerialization.jsonObject(with: Data(d.utf8))) as? [String: Any] else { return }
        lastID = root["last"] as? String
        for (id, v) in root["filters"] as? [String: String] ?? [:] {
            if let s = FilterSettings(json: v) { perFilter[id] = s.values }
        }
    }

    private func save() {
        guard let defaults else { return }
        lock.lock()
        let filters = perFilter.mapValues { FilterSettings(id: "", values: $0).json }
        let root: [String: Any] = ["last": lastID as Any, "filters": filters]
        lock.unlock()
        if let data = try? JSONSerialization.data(withJSONObject: root, options: [.sortedKeys]) {
            defaults.set(String(decoding: data, as: UTF8.self), forKey: key)
        }
    }
}
