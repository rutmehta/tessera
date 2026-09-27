import CoreText
import Foundation

/// An export watermark (WP M2-46), mirroring the engine's `export::Watermark` JSON: internally
/// tagged by `kind`, every field of the chosen kind present, nothing else. Sizes, scale and inset
/// are fractions of the output picture's short edge; rotation is in degrees; the text colour is
/// RGB 0–1 in the output colour space. The engine composites after resizing and sharpening, with an
/// explicit font file (no installed-font lookup), so an export is the same on every Mac.
///
/// One value keeps both kinds' fields so the sheet can switch Text ↔ Graphic without losing either;
/// only the chosen kind's fields are written.
public struct ExportWatermark: Codable, Equatable, Sendable {
    public enum Kind: String, Codable, CaseIterable, Sendable, Identifiable {
        case text, graphic
        public var id: String { rawValue }
        public var title: String { self == .text ? "Text" : "Graphic" }
    }

    /// The engine's 3 × 3 placement grid.
    public enum Anchor: String, Codable, CaseIterable, Sendable, Identifiable {
        case topLeft = "top_left", top, topRight = "top_right"
        case left, center, right
        case bottomLeft = "bottom_left", bottom, bottomRight = "bottom_right"
        public var id: String { rawValue }
        /// Column and row, 0…2 each (left/top = 0).
        public var axes: (column: Int, row: Int) {
            let i = Self.allCases.firstIndex(of: self)!
            return (i % 3, i / 3)
        }
        public var title: String {
            switch self {
            case .topLeft: "Top left"
            case .top: "Top"
            case .topRight: "Top right"
            case .left: "Left"
            case .center: "Centre"
            case .right: "Right"
            case .bottomLeft: "Bottom left"
            case .bottom: "Bottom"
            case .bottomRight: "Bottom right"
            }
        }
    }

    public var kind: Kind = .text
    // Text
    public var text = "© "
    /// A TrueType / OpenType font file (single face: .ttf or .otf).
    public var font = ""
    /// Text height as a fraction of the short edge, (0, 1].
    public var size = 0.04
    public var color: [Double] = [1, 1, 1]
    /// Degrees, −360…360.
    public var rotation = 0.0
    // Graphic
    /// A PNG (straight alpha).
    public var path = ""
    /// Graphic width as a fraction of the short edge, (0, 1].
    public var scale = 0.15
    // Both
    public var opacity = 0.6
    public var anchor: Anchor = .bottomRight
    /// Distance from the anchored edges as a fraction of the short edge, 0…0.5.
    public var inset = 0.03

    public init(kind: Kind = .text) { self.kind = kind }

    enum CodingKeys: String, CodingKey {
        case kind, text, font, size, color, opacity, anchor, inset, rotation, path, scale
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = ExportWatermark()
        kind = try c.decode(Kind.self, forKey: .kind)
        text = try c.decodeIfPresent(String.self, forKey: .text) ?? d.text
        font = try c.decodeIfPresent(String.self, forKey: .font) ?? d.font
        size = try c.decodeIfPresent(Double.self, forKey: .size) ?? d.size
        color = try c.decodeIfPresent([Double].self, forKey: .color) ?? d.color
        rotation = try c.decodeIfPresent(Double.self, forKey: .rotation) ?? d.rotation
        path = try c.decodeIfPresent(String.self, forKey: .path) ?? d.path
        scale = try c.decodeIfPresent(Double.self, forKey: .scale) ?? d.scale
        opacity = try c.decodeIfPresent(Double.self, forKey: .opacity) ?? d.opacity
        anchor = try c.decodeIfPresent(Anchor.self, forKey: .anchor) ?? d.anchor
        inset = try c.decodeIfPresent(Double.self, forKey: .inset) ?? d.inset
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(kind, forKey: .kind)
        switch kind {
        case .text:
            try c.encode(text, forKey: .text)
            try c.encode(font, forKey: .font)
            try c.encode(size, forKey: .size)
            try c.encode(color, forKey: .color)
            try c.encode(rotation, forKey: .rotation)
        case .graphic:
            try c.encode(path, forKey: .path)
            try c.encode(scale, forKey: .scale)
        }
        try c.encode(opacity, forKey: .opacity)
        try c.encode(anchor, forKey: .anchor)
        try c.encode(inset, forKey: .inset)
    }

    /// What stops an export before the engine sees it (missing files, empty text), or nil. Range
    /// checks are the engine's (`normalize_export_settings`).
    public var problem: String? {
        let fm = FileManager.default
        switch kind {
        case .text:
            if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "Type the watermark text" }
            if font.isEmpty { return "Choose a watermark font" }
            if !fm.fileExists(atPath: font) { return "Watermark font not found: \(font)" }
        case .graphic:
            if path.isEmpty { return "Choose a PNG for the watermark" }
            if !fm.fileExists(atPath: path) { return "Watermark graphic not found: \(path)" }
            if URL(fileURLWithPath: path).pathExtension.lowercased() != "png" { return "The watermark graphic must be a PNG" }
        }
        return nil
    }

    // MARK: Fonts

    /// An installed font the engine can read: a single-face TrueType / OpenType file.
    public struct FontChoice: Identifiable, Equatable, Sendable {
        public let name: String
        public let path: String
        public var id: String { path }
    }

    /// Installed single-face fonts (.ttf / .otf), by name. Collections (.ttc) are left out: the
    /// engine reads the first face of a file, which may not be the one chosen.
    public static let installedFonts: [FontChoice] = {
        let collection = CTFontCollectionCreateFromAvailableFonts(nil)
        let descriptors = (CTFontCollectionCreateMatchingFontDescriptors(collection) as? [CTFontDescriptor]) ?? []
        var seen = Set<String>()
        var fonts: [FontChoice] = []
        for d in descriptors {
            guard let url = CTFontDescriptorCopyAttribute(d, kCTFontURLAttribute) as? URL,
                  ["ttf", "otf"].contains(url.pathExtension.lowercased()),
                  let name = CTFontDescriptorCopyAttribute(d, kCTFontDisplayNameAttribute) as? String,
                  !name.hasPrefix("."), seen.insert(url.path).inserted else { continue }
            fonts.append(FontChoice(name: name, path: url.path))
        }
        return fonts.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }()

    /// The font a new text watermark starts with (Arial when installed).
    public static var defaultFontPath: String {
        let fonts = installedFonts
        for preferred in ["Arial", "Georgia", "Verdana", "Helvetica Neue"] {
            if let f = fonts.first(where: { $0.name == preferred }) { return f.path }
        }
        return fonts.first?.path ?? ""
    }

    /// The name shown for a font file (its display name, or the file name).
    public static func fontName(path: String) -> String {
        if let f = installedFonts.first(where: { $0.path == path }) { return f.name }
        return URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent
    }

    // MARK: Placement (the sheet's static preview; the same arithmetic as `apply_watermark`)

    /// Top-left origin of a `markWidth × markHeight` box (already rotated) in a `width × height`
    /// picture, as the engine places it.
    public func origin(markWidth: Double, markHeight: Double, width: Double, height: Double) -> (x: Double, y: Double) {
        let short = min(width, height)
        let (column, row) = anchor.axes
        func position(_ extent: Double, _ size: Double, _ axis: Int) -> Double {
            switch axis {
            case 0: short * inset
            case 1: (extent - size) / 2
            default: extent - size - short * inset
            }
        }
        return (position(width, markWidth, column), position(height, markHeight, row))
    }
}
