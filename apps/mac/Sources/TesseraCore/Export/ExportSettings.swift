import Foundation

/// File ▸ Export… settings (WP M2-20). Mirrors the engine's `ExportOptions` JSON exactly (snake_case
/// keys, unknown keys rejected by the engine), so a preset file, the sheet and `export_batch` share
/// one document.
public struct ExportSettings: Codable, Equatable, Sendable {
    public enum FileFormat: String, Codable, CaseIterable, Sendable, Identifiable {
        case jpeg, png, tiff
        public var id: String { rawValue }
        public var title: String { switch self { case .jpeg: "JPEG"; case .png: "PNG"; case .tiff: "TIFF" } }
        public var fileExtension: String { switch self { case .jpeg: "jpg"; case .png: "png"; case .tiff: "tif" } }
    }
    public enum ColorSpace: String, Codable, CaseIterable, Sendable, Identifiable {
        case srgb, displayP3 = "display_p3", rec2020, prophoto
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .srgb: "sRGB"
            case .displayP3: "Display P3"
            case .rec2020: "Rec. 2020"
            case .prophoto: "ProPhoto RGB"
            }
        }
    }
    public enum ResizeMode: String, Codable, CaseIterable, Sendable, Identifiable {
        case none, longEdge = "long_edge", fit, percent
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .none: "Original size"
            case .longEdge: "Long edge"
            case .fit: "Fit within"
            case .percent: "Percentage"
            }
        }
    }
    public enum SizeUnit: String, Codable, CaseIterable, Sendable, Identifiable {
        case px, `in`, cm
        public var id: String { rawValue }
        public var title: String { switch self { case .px: "pixels"; case .in: "inches"; case .cm: "cm" } }
    }
    public enum Sharpening: String, Codable, CaseIterable, Sendable, Identifiable {
        case none, screen, matte, glossy
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .none: "None"
            case .screen: "Screen"
            case .matte: "Matte paper"
            case .glossy: "Glossy paper"
            }
        }
    }
    public enum MetadataPolicy: String, Codable, CaseIterable, Sendable, Identifiable {
        case all, copyright, none
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .all: "All metadata"
            case .copyright: "Copyright only"
            case .none: "None"
            }
        }
    }
    public enum OnConflict: String, Codable, CaseIterable, Sendable, Identifiable {
        case unique, skip
        public var id: String { rawValue }
        public var title: String { switch self { case .unique: "Add a number"; case .skip: "Skip the photo" } }
    }

    public struct Resize: Codable, Equatable, Sendable {
        public var mode: ResizeMode = .none
        public var unit: SizeUnit = .px
        public var longEdge: Double = 2048
        public var width: Double = 2048
        public var height: Double = 2048
        public var percent: Double = 100
        public init() {}
        enum CodingKeys: String, CodingKey {
            case mode, unit, longEdge = "long_edge", width, height, percent
        }
        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let d = Resize.init()
            mode = try c.decodeIfPresent(ResizeMode.self, forKey: .mode) ?? d.mode
            unit = try c.decodeIfPresent(SizeUnit.self, forKey: .unit) ?? d.unit
            longEdge = try c.decodeIfPresent(Double.self, forKey: .longEdge) ?? d.longEdge
            width = try c.decodeIfPresent(Double.self, forKey: .width) ?? d.width
            height = try c.decodeIfPresent(Double.self, forKey: .height) ?? d.height
            percent = try c.decodeIfPresent(Double.self, forKey: .percent) ?? d.percent
        }
    }

    public var format: FileFormat = .jpeg
    public var quality: Int = 90
    public var bitDepth: Int = 8
    public var colorSpace: ColorSpace = .srgb
    public var resize = Resize()
    public var dpi: Int = 72
    public var sharpening: Sharpening = .none
    public var metadata: MetadataPolicy = .all
    public var naming: String = "{name}"
    public var upscale: Int = 1
    public var destination: String = ""
    public var onConflict: OnConflict = .unique
    public var openInFinder = false

    public init() {}

    enum CodingKeys: String, CodingKey {
        case format, quality, bitDepth = "bit_depth", colorSpace = "color_space", resize, dpi, sharpening,
             metadata, naming, upscale, destination, onConflict = "on_conflict", openInFinder = "open_in_finder"
    }

    /// Missing keys take the defaults (like the engine's `#[serde(default)]`).
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = ExportSettings()
        format = try c.decodeIfPresent(FileFormat.self, forKey: .format) ?? d.format
        quality = try c.decodeIfPresent(Int.self, forKey: .quality) ?? d.quality
        bitDepth = try c.decodeIfPresent(Int.self, forKey: .bitDepth) ?? d.bitDepth
        colorSpace = try c.decodeIfPresent(ColorSpace.self, forKey: .colorSpace) ?? d.colorSpace
        resize = try c.decodeIfPresent(Resize.self, forKey: .resize) ?? d.resize
        dpi = try c.decodeIfPresent(Int.self, forKey: .dpi) ?? d.dpi
        sharpening = try c.decodeIfPresent(Sharpening.self, forKey: .sharpening) ?? d.sharpening
        metadata = try c.decodeIfPresent(MetadataPolicy.self, forKey: .metadata) ?? d.metadata
        naming = try c.decodeIfPresent(String.self, forKey: .naming) ?? d.naming
        upscale = try c.decodeIfPresent(Int.self, forKey: .upscale) ?? d.upscale
        destination = try c.decodeIfPresent(String.self, forKey: .destination) ?? d.destination
        onConflict = try c.decodeIfPresent(OnConflict.self, forKey: .onConflict) ?? d.onConflict
        openInFinder = try c.decodeIfPresent(Bool.self, forKey: .openInFinder) ?? d.openInFinder
    }

    public init(json: String) throws {
        self = try JSONDecoder().decode(ExportSettings.self, from: Data(json.utf8))
    }

    public var json: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return String(decoding: (try? encoder.encode(self)) ?? Data("{}".utf8), as: UTF8.self)
    }

    // MARK: Derived

    /// Format constraints the sheet enforces as the user switches formats.
    public mutating func normalizeForFormat() {
        if format != .tiff { bitDepth = 8 }
        quality = min(max(quality, 1), 100)
        if ![1, 2, 4].contains(upscale) { upscale = 1 }
    }

    private func pixels(_ value: Double) -> Double {
        switch resize.unit {
        case .px: value
        case .in: value * Double(dpi)
        case .cm: value / 2.54 * Double(dpi)
        }
    }

    /// Output pixel size for a rendered picture of `width × height` (after any upscale), or nil for
    /// an invalid size. Matches `export::Resize::dimensions` (rounding, never zero).
    public func outputSize(width: Int, height: Int) -> (width: Int, height: Int)? {
        guard width > 0, height > 0 else { return nil }
        let w = Double(width * upscale), h = Double(height * upscale)
        let scale: Double
        switch resize.mode {
        case .none: scale = 1
        case .longEdge: scale = pixels(resize.longEdge).rounded() / max(w, h)
        case .fit: scale = min(pixels(resize.width).rounded() / w, pixels(resize.height).rounded() / h)
        case .percent: scale = resize.percent / 100
        }
        guard scale.isFinite, scale > 0 else { return nil }
        return (max(Int((w * scale).rounded()), 1), max(Int((h * scale).rounded()), 1))
    }

    /// "2048 px long edge · JPEG 85 · sRGB", for the sheet and preset tooltips.
    public var summary: String {
        var parts: [String] = []
        let size: String = switch resize.mode {
        case .none: "Full size"
        case .longEdge: "\(Self.number(resize.longEdge)) \(resize.unit.rawValue) long edge"
        case .fit: "Fit \(Self.number(resize.width)) × \(Self.number(resize.height)) \(resize.unit.rawValue)"
        case .percent: "\(Self.number(resize.percent))%"
        }
        parts.append(size)
        let file: String = switch format {
        case .jpeg: "JPEG \(quality)"
        case .png: "PNG"
        case .tiff: "TIFF \(bitDepth)-bit"
        }
        parts.append(file)
        parts.append(colorSpace.title)
        if resize.unit != .px || format != .png { parts.append("\(dpi) dpi") }
        if upscale > 1 { parts.append("\(upscale)× upscale") }
        return parts.joined(separator: " · ")
    }

    static func number(_ v: Double) -> String {
        v == v.rounded() ? String(Int(v)) : String(format: "%.2f", v)
    }
}
