import Foundation

/// Deliver (redesign SPEC §5, WP UX-15): What · How · Where. The two choices, the settings each
/// How row stands for, the size estimate and the agent field's phrases. Pure values so the
/// mapping is testable without an engine; `AppModel+Deliver.swift` applies them.

/// What gets written: the keeps of the shoot, the selection, or every photo.
public enum DeliverWhat: String, CaseIterable, Identifiable, Sendable {
    case keeps, selection, shoot
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .keeps: "Keeps"
        case .selection: "Selection"
        case .shoot: "Whole shoot"
        }
    }
}

/// How it is written: the four rows of SPEC §5. Web, Instagram and Full size are export settings
/// (the shipped presets where one exists, else built in); Print opens the print flow.
public enum DeliverHow: String, CaseIterable, Identifiable, Sendable {
    case web, instagram, fullSize, print
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .web: "Web JPEG"
        case .instagram: "Instagram"
        case .fullSize: "Full size"
        case .print: "Print"
        }
    }

    /// The row's detail, right-aligned.
    public var subtitle: String {
        switch self {
        case .web: "2048 px · sRGB"
        case .instagram: "4:5 · 1350 px · sRGB"
        case .fullSize: "TIFF 16-bit · ProPhoto"
        case .print: "300 dpi · 12 in"
        }
    }

    /// The shipped preset this row prefers (`default_export_presets` in the engine), by name.
    public var presetName: String? {
        switch self {
        case .web: "Web 2048 sRGB"
        case .instagram: nil
        case .fullSize: "16-bit TIFF ProPhoto"
        case .print: "Print 300 dpi"
        }
    }

    /// The settings of this row: the named preset when `presets` has it, else the same values
    /// built in. Instagram fits inside 1080 × 1350 (the 4:5 portrait frame; a landscape photo
    /// lands at 1080 wide, since export does not crop). The destination is left empty.
    public func settings(presets: [ExportPresetEntry]) -> ExportSettings {
        if let name = presetName, let preset = presets.first(where: { $0.name == name }) { return preset.settings }
        var s = ExportSettings()
        switch self {
        case .web:
            s.quality = 85
            s.resize.mode = .longEdge
            s.resize.longEdge = 2048
            s.sharpening = .screen
        case .instagram:
            s.quality = 90
            s.resize.mode = .fit
            s.resize.width = 1080
            s.resize.height = 1350
            s.sharpening = .screen
        case .fullSize:
            s.format = .tiff
            s.bitDepth = 16
            s.colorSpace = .prophoto
            s.dpi = 300
        case .print:
            s.quality = 95
            s.dpi = 300
            s.resize.mode = .longEdge
            s.resize.unit = .in
            s.resize.longEdge = 12
            s.sharpening = .glossy
        }
        return s
    }
}

extension ExportSettings {
    /// A typical file size for a picture written at `width × height` with these settings: the
    /// output size times the format's usual bytes per megapixel (JPEG by quality, TIFF and
    /// JPEG XL by bit depth and no compression, PNG and AVIF by experience, DNG as float). An
    /// estimate, labelled "about" wherever it is shown.
    public func estimatedBytes(width: Int, height: Int) -> Int {
        guard let size = outputSize(width: width, height: height) else { return 0 }
        let megapixels = Double(size.width * size.height) / 1_000_000
        let perMegapixel: Double
        switch format {
        case .jpeg: perMegapixel = quality >= 95 ? 600_000 : quality >= 90 ? 450_000 : quality >= 80 ? 300_000 : 200_000
        case .png: perMegapixel = 1_800_000
        case .tiff: perMegapixel = Double(bitDepth) / 8 * 3_000_000
        case .avif: perMegapixel = quality >= 90 ? 250_000 : 120_000
        case .jpegXl: perMegapixel = Double(bitDepth) / 8 * 1_600_000
        case .dng: perMegapixel = 12_000_000
        }
        var bytes = Int(megapixels * perMegapixel)
        if let limit = maxFileBytes, limit > 0 { bytes = min(bytes, limit) }
        return bytes
    }
}

/// What the Deliver agent field understood from a typed phrase ("export the keeps for
/// Instagram"): a How, a What, or both. Nothing understood is nil, so the caller reports the
/// text instead of guessing.
public struct DeliverAgentPhrase: Equatable, Sendable {
    public var how: DeliverHow?
    public var what: DeliverWhat?

    public static func parse(_ text: String) -> DeliverAgentPhrase? {
        let lower = text.lowercased()
        let words = Set(lower.split { !$0.isLetter && !$0.isNumber }.map(String.init))
        func has(_ candidates: String...) -> Bool { candidates.contains { words.contains($0) } }
        var phrase = DeliverAgentPhrase()
        if lower.contains("insta") { phrase.how = .instagram }
        else if has("tiff", "full", "original") { phrase.how = .fullSize }
        else if has("print", "prints") { phrase.how = .print }
        else if has("web", "jpeg", "jpg", "jpegs", "jpgs") { phrase.how = .web }
        if has("keep", "keeps", "keepers", "picks") { phrase.what = .keeps }
        else if has("selected", "selection", "this", "these") { phrase.what = .selection }
        else if has("all", "whole", "everything", "shoot") { phrase.what = .shoot }
        return phrase.how == nil && phrase.what == nil ? nil : phrase
    }
}
