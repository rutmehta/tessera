import Foundation

/// A JSON value that keeps every field it was read with (WP B5-07). Layer style settings are edited
/// through it rather than a closed Swift struct so fields the inspector does not show (contour and
/// jitter metadata, pattern pixels, future engine fields) survive a read-modify-write untouched.
public enum StyleJSON: Equatable, Sendable, Codable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([StyleJSON])
    case object([String: StyleJSON])

    public init(from decoder: any Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let b = try? c.decode(Bool.self) { self = .bool(b) }
        else if let d = try? c.decode(Double.self) { self = .number(d) }
        else if let s = try? c.decode(String.self) { self = .string(s) }
        else if let a = try? c.decode([StyleJSON].self) { self = .array(a) }
        else { self = .object(try c.decode([String: StyleJSON].self)) }
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .number(let d):
            // Whole numbers go out as integers: serde reads `u32` fields (pattern sizes) only from those.
            if d.rounded() == d, abs(d) < 9e15 { try c.encode(Int64(d)) } else { try c.encode(d) }
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }

    /// Parses JSON text; nil when it is not JSON.
    public static func parse(_ text: String?) -> StyleJSON? {
        guard let text, let data = text.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(StyleJSON.self, from: data)
    }

    /// Compact JSON with sorted keys.
    public var text: String {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return (try? e.encode(self)).map { String(decoding: $0, as: UTF8.self) } ?? "null"
    }

    public subscript(key: String) -> StyleJSON? {
        get { if case .object(let o) = self { o[key] } else { nil } }
        set {
            var o: [String: StyleJSON] = if case .object(let o) = self { o } else { [:] }
            o[key] = newValue
            self = .object(o)
        }
    }

    public var double: Double? { if case .number(let d) = self { d } else { nil } }
    public var bool: Bool? { if case .bool(let b) = self { b } else { nil } }
    public var string: String? { if case .string(let s) = self { s } else { nil } }
    public var array: [StyleJSON]? { if case .array(let a) = self { a } else { nil } }
    public var object: [String: StyleJSON]? { if case .object(let o) = self { o } else { nil } }
    /// Numbers of an array (`[0.2, 0.1, 0.6, 1]`).
    public var doubles: [Double]? { array?.compactMap(\.double) }

    public init(_ values: [Double]) { self = .array(values.map { .number($0) }) }
}
