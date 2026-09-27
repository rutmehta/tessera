import Foundation
import TesseraFFI

/// Photo ▸ Photo Merge and Photo ▸ Enhance (WP M2-50, docs/01): the options the sheets edit, how
/// they map onto the engine's `MergeOptions` / `EnhanceOptions` (M2-47), and when each command is
/// available. UI-free so the mapping and the rules are unit tested.

/// Photo ▸ Photo Merge ▸ HDR… / Panorama… / HDR Panorama….
public enum PhotoMergeKind: String, CaseIterable, Identifiable, Sendable {
    case hdr, panorama, hdrPanorama

    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .hdr: "HDR"
        case .panorama: "Panorama"
        case .hdrPanorama: "HDR Panorama"
        }
    }
    public var ffi: MergeKind {
        switch self {
        case .hdr: .hdr
        case .panorama: .panorama
        case .hdrPanorama: .hdrPanorama
        }
    }
    /// HDR Panorama needs at least two brackets of two.
    public var minimumPhotos: Int { self == .hdrPanorama ? 4 : 2 }
    /// Engine limits (merge.rs `MergeOptions::validate`).
    public var maximumPhotos: Int { self == .hdr ? 64 : 128 }
    /// The name the engine gives the result ("IMG_0001-HDR.dng").
    public var suffix: String {
        switch self {
        case .hdr: "-HDR"
        case .panorama: "-Pano"
        case .hdrPanorama: "-HDR-Pano"
        }
    }
    public var usesDeghost: Bool { self != .panorama }
    public var usesProjection: Bool { self != .hdr }
    /// Auto Align is always on for panoramas (registration is the merge).
    public var usesAutoAlign: Bool { self == .hdr }
}

extension MergeDeghost: CaseIterable, Identifiable {
    public static var allCases: [MergeDeghost] { [.none, .low, .medium, .high] }
    public var id: Self { self }
    public var title: String {
        switch self {
        case .none: "None"
        case .low: "Low"
        case .medium: "Medium"
        case .high: "High"
        }
    }
}

extension MergeProjection: CaseIterable, Identifiable {
    public static var allCases: [MergeProjection] { [.auto, .spherical, .cylindrical, .perspective] }
    public var id: Self { self }
    public var title: String {
        switch self {
        case .auto: "Auto"
        case .spherical: "Spherical"
        case .cylindrical: "Cylindrical"
        case .perspective: "Perspective"
        }
    }
    /// Spherical and cylindrical need a calibrated focal length (merge.rs).
    public var isCurved: Bool { self == .spherical || self == .cylindrical }
}

/// What the Photo Merge sheet edits. Defaults follow the engine's (`MergeOptions::default`).
public struct PhotoMergeSettings: Equatable, Sendable {
    public var kind: PhotoMergeKind
    public var autoAlign = true
    public var autoTone = true
    public var deghost: MergeDeghost = .medium
    public var projection: MergeProjection = .auto
    /// 0…100.
    public var boundaryWarp = 0
    public var fillEdges = false
    public var createStack = true
    /// Calibrated focal length in source pixels; required for spherical / cylindrical.
    public var focalPixels: Double?
    /// HDR Panorama: frames per bracket (the selection is split in order, never by name).
    public var bracketSize = 3
    /// Positive sensor exposures in selection order, for LinearRaw DNG brackets without EXIF
    /// exposure tags (not exposed in the sheet; the engine reads EXIF otherwise).
    public var exposureValues: [Double] = []

    public init(kind: PhotoMergeKind = .hdr) { self.kind = kind }

    /// Sequential bracket lengths for HDR Panorama, nil when the selection does not divide.
    public func bracketSizes(count: Int) -> [UInt32]? {
        guard bracketSize >= 2, bracketSize <= 64, count % bracketSize == 0, count / bracketSize >= 2 else { return nil }
        return Array(repeating: UInt32(bracketSize), count: count / bracketSize)
    }

    /// Bracket sizes that divide `count` into at least two brackets (the sheet's choices).
    public static func bracketChoices(count: Int) -> [Int] {
        guard count >= 4 else { return [] }
        return (2...min(9, count / 2)).filter { count % $0 == 0 }
    }

    /// Why Merge cannot run with these options, or nil.
    public func problem(count: Int) -> String? {
        if count < kind.minimumPhotos {
            return kind == .hdrPanorama ? "HDR Panorama needs at least two brackets of two photos"
                : "Select at least 2 photos to merge"
        }
        if count > kind.maximumPhotos { return "\(kind.title) merges at most \(kind.maximumPhotos) photos" }
        if kind == .hdrPanorama, bracketSizes(count: count) == nil {
            return "\(count) photos do not split into brackets of \(bracketSize)"
        }
        if kind.usesProjection, projection.isCurved, (focalPixels ?? 0) <= 0 {
            return "\(projection.title) needs the focal length in pixels"
        }
        if !exposureValues.isEmpty, exposureValues.count != count || exposureValues.contains(where: { !($0 > 0) || !$0.isFinite }) {
            return "Exposure values must give one positive value per photo"
        }
        return nil
    }

    /// The engine options. Controls that do not apply to `kind` go at the engine's defaults.
    public func options(count: Int) -> MergeOptions {
        MergeOptions(kind: kind.ffi,
                     autoAlign: kind.usesAutoAlign ? autoAlign : true,
                     autoTone: autoTone,
                     deghost: kind.usesDeghost ? deghost : .medium,
                     projection: kind.usesProjection ? projection : .auto,
                     boundaryWarp: kind.usesProjection ? UInt8(min(max(boundaryWarp, 0), 100)) : 0,
                     fillEdges: kind.usesProjection ? fillEdges : false,
                     createStack: createStack,
                     focalPixels: kind.usesProjection ? focalPixels.flatMap { $0 > 0 ? $0 : nil } : nil,
                     bracketSizes: kind == .hdrPanorama ? (bracketSizes(count: count) ?? []) : [],
                     exposureValues: exposureValues.count == count ? exposureValues : [])
    }

    /// The preview is recomputed only when something the engine reads changes (not Create Stack).
    public func previewKey(count: Int) -> MergeOptions {
        var o = options(count: count)
        o.createStack = true
        return o
    }
}

/// What the Enhance sheet edits.
public struct PhotoEnhanceSettings: Equatable, Sendable {
    public var denoise = true
    /// 0…100 (0 is an exact copy: the engine bypasses the model).
    public var denoiseAmount = 50
    public var superResolution = false
    /// Explicit consent to fetch missing weights; the engine is cache-only otherwise.
    public var allowModelDownload = false

    public init() {}

    public var problem: String? {
        (!denoise && !superResolution) ? "Choose Denoise, Super Resolution or both" : nil
    }

    public var options: EnhanceOptions {
        EnhanceOptions(denoiseAmount: denoise ? UInt8(min(max(denoiseAmount, 0), 100)) : nil,
                       superResolution: superResolution,
                       rawDetails: false,   // unsupported by the engine (M2-47); never requested
                       allowModelDownload: allowModelDownload)
    }

    /// The name the engine gives each result ("IMG_0001-Enhanced-NR.dng").
    public var suffix: String {
        switch (denoise, superResolution) {
        case (true, true): "-Enhanced-NR-SR"
        case (true, false): "-Enhanced-NR"
        default: "-Enhanced-SR"
        }
    }

    /// The models a run would load (for the sheet's download note).
    public var models: [String] {
        (denoise && denoiseAmount > 0 ? ["DRUNet denoiser"] : []) + (superResolution ? ["Real-ESRGAN ×2"] : [])
    }
}

/// When the Photo menu's commands are available (docs/01: merge for 2+, enhance for 1+).
public enum PhotoCommandRules {
    public static func mergeProblem(_ kind: PhotoMergeKind, selected: Int, engineBacked: Bool, running: Bool) -> String? {
        if !engineBacked { return "Photo Merge needs a folder opened on the engine" }
        if running { return "A photo merge or enhance is already running" }
        if selected < kind.minimumPhotos {
            return kind == .hdrPanorama ? "Select at least 4 photos (two brackets of two or more)" : "Select 2 or more photos to merge"
        }
        if selected > kind.maximumPhotos { return "\(kind.title) merges at most \(kind.maximumPhotos) photos" }
        return nil
    }

    public static func canMerge(_ kind: PhotoMergeKind, selected: Int, engineBacked: Bool, running: Bool) -> Bool {
        mergeProblem(kind, selected: selected, engineBacked: engineBacked, running: running) == nil
    }

    public static func enhanceProblem(selected: Int, engineBacked: Bool, running: Bool) -> String? {
        if !engineBacked { return "Enhance needs a folder opened on the engine" }
        if running { return "A photo merge or enhance is already running" }
        if selected < 1 { return "Select a photo to enhance" }
        if selected > 128 { return "Enhance at most 128 photos at a time" }
        return nil
    }

    public static func canEnhance(selected: Int, engineBacked: Bool, running: Bool) -> Bool {
        enhanceProblem(selected: selected, engineBacked: engineBacked, running: running) == nil
    }
}

/// Shutter, aperture and ISO of one photo, for the sheet's exposure-spread warning.
public struct ExposureFacts: Equatable, Sendable {
    public var shutter: Double
    public var aperture: Double
    public var iso: Double

    public init(shutter: Double, aperture: Double = 1, iso: Double = 100) {
        self.shutter = shutter
        self.aperture = aperture
        self.iso = iso
    }

    /// Light gathered, in stops (higher is brighter): log2(t · ISO / N²).
    public var stops: Double { log2(shutter * iso / 100 / (aperture * aperture)) }

    /// From the engine's metadata fields (`ExposureTime`, `FNumber`, `PhotographicSensitivity`).
    /// Tolerant of "1/250", "1/250 s", "0.5 s", "f/2.8", "F2.8", "ISO 100". Nil without a shutter.
    public init?(fields: [(name: String, value: String)]) {
        func find(_ names: [String]) -> String? { fields.first { names.contains($0.name) }?.value }
        guard let t = find(["ExposureTime", "Exposure Time", "Shutter"]).flatMap(Self.number), t > 0 else { return nil }
        let n = find(["FNumber", "F Number", "Aperture"]).flatMap(Self.number) ?? 1
        let iso = find(["PhotographicSensitivity", "ISOSpeedRatings", "ISO"]).flatMap(Self.number) ?? 100
        self.init(shutter: t, aperture: n > 0 ? n : 1, iso: iso > 0 ? iso : 100)
    }

    /// First number in `text`, with "a/b" read as a fraction.
    static func number(_ text: String) -> Double? {
        let scanner = text.lowercased().replacingOccurrences(of: "f/", with: "")
        var digits = ""
        var started = false
        for ch in scanner {
            if ch.isNumber || ch == "." || ch == "/" { digits.append(ch); started = true } else if started { break }
        }
        let parts = digits.split(separator: "/").compactMap { Double($0) }
        switch parts.count {
        case 1: return parts[0]
        case 2 where parts[1] != 0: return parts[0] / parts[1]
        default: return nil
        }
    }
}

/// Warnings the sheet shows before merging (docs/01: exposure spread, missing metadata).
public enum PhotoMergeAdvice {
    /// `facts` in selection order (nil where the photo has no exposure metadata).
    public static func warnings(kind: PhotoMergeKind, facts: [ExposureFacts?], exposureValuesGiven: Bool = false) -> [String] {
        guard !facts.isEmpty else { return [] }
        var out: [String] = []
        let known = facts.compactMap { $0?.stops }
        let missing = facts.count - known.count
        if kind != .panorama, missing > 0, !exposureValuesGiven {
            out.append("\(missing) of \(facts.count) photos have no exposure metadata: HDR cannot weight them")
        }
        guard known.count >= 2, let lo = known.min(), let hi = known.max() else { return out }
        let spread = hi - lo
        switch kind {
        case .hdr:
            if spread < 0.66 {
                out.append(String(format: "Exposures differ by %.1f EV: HDR adds little range. Bracket at least 1 EV apart", spread))
            }
        case .panorama:
            if spread > 1 {
                out.append(String(format: "Exposures differ by %.1f EV: for bracketed frames use HDR Panorama", spread))
            }
        case .hdrPanorama:
            if spread < 0.66 {
                out.append(String(format: "Exposures differ by %.1f EV: these may not be brackets", spread))
            }
        }
        return out
    }
}
