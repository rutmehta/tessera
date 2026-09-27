import CoreGraphics
import Foundation

// Editable text source (WP B5-10): a Codable mirror of `typography::TextModel`
// (crates/typography/src/model.rs) exchanged with the engine as JSON. The Rust side rejects unknown
// fields, so exactly the known fields are encoded; `path` (text on a path) is carried as opaque JSON
// so its commands round-trip untouched. Distances are local level-0 pixels, colours straight sRGBA8.

/// Opaque JSON value (text-on-path data the host never edits).
public indirect enum TextJSONValue: Codable, Equatable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([TextJSONValue])
    case object([String: TextJSONValue])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let b = try? c.decode(Bool.self) { self = .bool(b) }
        else if let n = try? c.decode(Double.self) { self = .number(n) }
        else if let s = try? c.decode(String.self) { self = .string(s) }
        else if let a = try? c.decode([TextJSONValue].self) { self = .array(a) }
        else { self = .object(try c.decode([String: TextJSONValue].self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .number(let n): try c.encode(n)
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }
}

/// One styled run: the Character panel's values.
public struct TextRunModel: Codable, Equatable, Sendable {
    public var text: String
    public var family: String
    public var weight: UInt16
    public var italic: Bool
    /// Pixels.
    public var size: Float
    /// Pixels added after each shaped cluster.
    public var tracking: Float
    public var kerning: Bool
    /// Baseline-to-baseline distance in pixels; 0 = automatic (1.2 × size).
    public var leading: Float
    /// Pixels; positive lifts the glyphs.
    public var baselineShift: Float
    /// Straight sRGBA8.
    public var color: [UInt8]
    public var features: [String: UInt32]
    public var axes: [String: Float]

    public init(text: String = "", family: String = "sans-serif", weight: UInt16 = 400, italic: Bool = false,
                size: Float = 24, tracking: Float = 0, kerning: Bool = true, leading: Float = 0,
                baselineShift: Float = 0, color: [UInt8] = [0, 0, 0, 255], features: [String: UInt32] = [:],
                axes: [String: Float] = [:]) {
        self.text = text; self.family = family; self.weight = weight; self.italic = italic; self.size = size
        self.tracking = tracking; self.kerning = kerning; self.leading = leading; self.baselineShift = baselineShift
        self.color = color; self.features = features; self.axes = axes
    }

    enum CodingKeys: String, CodingKey {
        case text, family, weight, italic, size, tracking, kerning, leading, color, features, axes
        case baselineShift = "baseline_shift"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = TextRunModel()
        text = try c.decodeIfPresent(String.self, forKey: .text) ?? d.text
        family = try c.decodeIfPresent(String.self, forKey: .family) ?? d.family
        weight = try c.decodeIfPresent(UInt16.self, forKey: .weight) ?? d.weight
        italic = try c.decodeIfPresent(Bool.self, forKey: .italic) ?? d.italic
        size = try c.decodeIfPresent(Float.self, forKey: .size) ?? d.size
        tracking = try c.decodeIfPresent(Float.self, forKey: .tracking) ?? d.tracking
        kerning = try c.decodeIfPresent(Bool.self, forKey: .kerning) ?? d.kerning
        leading = try c.decodeIfPresent(Float.self, forKey: .leading) ?? d.leading
        baselineShift = try c.decodeIfPresent(Float.self, forKey: .baselineShift) ?? d.baselineShift
        color = try c.decodeIfPresent([UInt8].self, forKey: .color) ?? d.color
        features = try c.decodeIfPresent([String: UInt32].self, forKey: .features) ?? d.features
        axes = try c.decodeIfPresent([String: Float].self, forKey: .axes) ?? d.axes
    }

    /// Everything but the text: runs with equal styles can share text.
    public var style: TextRunModel {
        var s = self
        s.text = ""
        return s
    }

    public func sameStyle(_ other: TextRunModel) -> Bool { style == other.style }

    /// The run's colour as a tool colour (0…1).
    public var toolColor: ToolColor {
        ToolColor(r: Float(color[textSafe: 0] ?? 0) / 255, g: Float(color[textSafe: 1] ?? 0) / 255, b: Float(color[textSafe: 2] ?? 0) / 255)
    }

    public mutating func setColor(_ c: ToolColor) {
        func q(_ v: Float) -> UInt8 { UInt8(max(0, min(255, (v * 255).rounded()))) }
        color = [q(c.r), q(c.g), q(c.b), color[textSafe: 3] ?? 255]
    }
}

extension Array {
    subscript(textSafe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}

public enum TextParagraphAlignment: String, Codable, CaseIterable, Sendable {
    case left, center, right, justify
    public var title: String {
        switch self {
        case .left: "Left"
        case .center: "Center"
        case .right: "Right"
        case .justify: "Justify"
        }
    }
    public var symbol: String {
        switch self {
        case .left: "text.alignleft"
        case .center: "text.aligncenter"
        case .right: "text.alignright"
        case .justify: "text.justify"
        }
    }
}

/// The Paragraph panel's values (shared by all paragraphs of a layer).
public struct TextParagraphModel: Codable, Equatable, Sendable {
    public var alignment: TextParagraphAlignment = .left
    /// Stored, not applied (no dictionary hyphenation).
    public var hyphenation = false
    public var leftIndent: Float = 0
    public var rightIndent: Float = 0
    public var firstLineIndent: Float = 0
    public var spaceBefore: Float = 0
    public var spaceAfter: Float = 0

    public init() {}

    enum CodingKeys: String, CodingKey {
        case alignment, hyphenation
        case leftIndent = "left_indent", rightIndent = "right_indent", firstLineIndent = "first_line_indent"
        case spaceBefore = "space_before", spaceAfter = "space_after"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        alignment = try c.decodeIfPresent(TextParagraphAlignment.self, forKey: .alignment) ?? .left
        hyphenation = try c.decodeIfPresent(Bool.self, forKey: .hyphenation) ?? false
        leftIndent = try c.decodeIfPresent(Float.self, forKey: .leftIndent) ?? 0
        rightIndent = try c.decodeIfPresent(Float.self, forKey: .rightIndent) ?? 0
        firstLineIndent = try c.decodeIfPresent(Float.self, forKey: .firstLineIndent) ?? 0
        spaceBefore = try c.decodeIfPresent(Float.self, forKey: .spaceBefore) ?? 0
        spaceAfter = try c.decodeIfPresent(Float.self, forKey: .spaceAfter) ?? 0
    }
}

/// Point text (unbounded lines from the anchor) or paragraph (area) text wrapping in a box.
public enum TextBoxModel: Codable, Equatable, Sendable {
    case point
    case paragraph(width: Float, height: Float)

    enum CodingKeys: String, CodingKey { case kind, width, height }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "paragraph":
            self = .paragraph(width: try c.decode(Float.self, forKey: .width), height: try c.decode(Float.self, forKey: .height))
        default: self = .point
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .point: try c.encode("point", forKey: .kind)
        case .paragraph(let w, let h):
            try c.encode("paragraph", forKey: .kind)
            try c.encode(w, forKey: .width)
            try c.encode(h, forKey: .height)
        }
    }

    public var isParagraph: Bool { if case .paragraph = self { true } else { false } }
    public var size: CGSize? {
        if case .paragraph(let w, let h) = self { CGSize(width: Double(w), height: Double(h)) } else { nil }
    }
}

public struct TextWarpModel: Codable, Equatable, Sendable {
    public enum Kind: String, Codable, Sendable { case arc, flag, wave }
    public var kind: Kind = .arc
    public var amount: Float = 0
    public init() {}
}

/// `typography::TextModel`.
public struct TextSourceModel: Codable, Equatable, Sendable {
    public var runs: [TextRunModel]
    public var paragraph = TextParagraphModel()
    public var textBox: TextBoxModel = .point
    /// Stored; vertical composition is unsupported.
    public var vertical = false
    public var warp = TextWarpModel()
    /// Text-on-path commands, carried opaquely.
    public var path: TextJSONValue?

    public init(runs: [TextRunModel], paragraph: TextParagraphModel = TextParagraphModel(), textBox: TextBoxModel = .point) {
        self.runs = runs; self.paragraph = paragraph; self.textBox = textBox
    }

    enum CodingKeys: String, CodingKey {
        case runs, paragraph, vertical, warp, path
        case textBox = "text_box"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        runs = try c.decodeIfPresent([TextRunModel].self, forKey: .runs) ?? []
        paragraph = try c.decodeIfPresent(TextParagraphModel.self, forKey: .paragraph) ?? TextParagraphModel()
        textBox = try c.decodeIfPresent(TextBoxModel.self, forKey: .textBox) ?? .point
        vertical = try c.decodeIfPresent(Bool.self, forKey: .vertical) ?? false
        warp = try c.decodeIfPresent(TextWarpModel.self, forKey: .warp) ?? TextWarpModel()
        let p = try c.decodeIfPresent(TextJSONValue.self, forKey: .path)
        path = p == .null ? nil : p
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(runs, forKey: .runs)
        try c.encode(paragraph, forKey: .paragraph)
        try c.encode(textBox, forKey: .textBox)
        try c.encode(vertical, forKey: .vertical)
        try c.encode(warp, forKey: .warp)
        if let path { try c.encode(path, forKey: .path) } else { try c.encodeNil(forKey: .path) }
    }

    /// The concatenated source (layout clusters index its UTF-8 bytes).
    public var text: String { runs.map(\.text).joined() }
    /// UTF-8 length of `text`.
    public var utf8Count: Int { runs.reduce(0) { $0 + $1.text.utf8.count } }

    /// Point text with one run.
    public static func point(_ text: String, family: String, size: Float, color: [UInt8] = [0, 0, 0, 255]) -> TextSourceModel {
        TextSourceModel(runs: [TextRunModel(text: text, family: family, size: size, color: color)])
    }

    /// Warp, path or vertical data: no canvas caret (source editor only).
    public var needsSourceEditor: Bool { vertical || path != nil || warp.amount != 0 }

    public var json: String {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys]
        return (try? String(decoding: e.encode(self), as: UTF8.self)) ?? "{}"
    }

    public init?(json: String?) {
        guard let json, let m = try? JSONDecoder().decode(TextSourceModel.self, from: Data(json.utf8)) else { return nil }
        self = m
    }
}

// MARK: - Affine layouts

extension AffineTransform2D {
    /// CoreGraphics' layout: `x' = a·x + c·y + tx`, `y' = b·x + d·y + ty` — NOT the row-major
    /// `[a, b, c, d, e, f]` of the engine (`x' = a·x + b·y + c`). Convert explicitly.
    public var cgAffineTransform: CGAffineTransform {
        CGAffineTransform(a: a, b: d, c: b, d: e, tx: c, ty: f)
    }

    public init(_ t: CGAffineTransform) {
        self.init(a: t.a, b: t.c, c: t.tx, d: t.b, e: t.d, f: t.ty)
    }

    public var isFiniteAndInvertible: Bool {
        [a, b, c, d, e, f].allSatisfy(\.isFinite) && inverse != nil
    }
}
