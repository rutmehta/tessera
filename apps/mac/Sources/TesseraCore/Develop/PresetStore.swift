import Foundation

/// Which parts of the settings a preset carries (docs/01 §2.17: presets are partial recipes).
public enum PresetGroup: String, CaseIterable, Codable, Sendable, Identifiable {
    case whiteBalance, basicTone, presence, toneCurve, hsl, colorGrading, detail, effects, crop

    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .whiteBalance: "White Balance"
        case .basicTone: "Tone"
        case .presence: "Presence"
        case .toneCurve: "Tone Curve"
        case .hsl: "HSL"
        case .colorGrading: "Color Grading"
        case .detail: "Detail"
        case .effects: "Effects"
        case .crop: "Crop"
        }
    }

    /// Member paths the group copies. Whole objects are copied, so applying a preset sets every
    /// field of the group (not only the ones that differ from the defaults).
    public var paths: [[String]] {
        switch self {
        case .whiteBalance: [["white_balance"]]
        case .basicTone: ["exposure", "contrast", "highlights", "shadows", "whites", "blacks"].map { ["tone", $0] }
        case .presence: [["tone", "texture"], ["tone", "clarity"], ["tone", "dehaze"],
                         ["color", "vibrance"], ["color", "saturation"]]
        case .toneCurve: [["tone", "curves"]]
        case .hsl: [["color", "hsl"]]
        case .colorGrading: [["color", "grading"]]
        case .detail: [["detail"]]
        case .effects: [["effects", "vignette"], ["effects", "grain"]]
        case .crop: [["geometry", "crop"]]
        }
    }

    /// Crop is image-specific: off by default when saving.
    public static let defaultSelection: Set<PresetGroup> = Set(allCases).subtracting([.crop])
}

/// A named partial recipe stored as JSON: `{"name":…, "groups":[…], "settings":{…}}`.
public struct DevelopPreset: Identifiable, Sendable, Equatable {
    public let name: String
    public let groups: [PresetGroup]
    /// Partial `DevelopSettings` JSON (a merge patch).
    public let settingsJSON: String
    public var id: String { name }

    public var settings: [String: Any] {
        (try? JSONSerialization.jsonObject(with: Data(settingsJSON.utf8)) as? [String: Any]) ?? [:]
    }

    /// Copies `groups` out of a full settings document.
    public init(name: String, groups: [PresetGroup], from full: [String: Any]) {
        self.name = name
        self.groups = PresetGroup.allCases.filter(groups.contains)
        var partial: [String: Any] = [:]
        for g in self.groups {
            for path in g.paths {
                var node: Any? = full
                for key in path { node = (node as? [String: Any])?[key] }
                if let node { partial = DevelopController.merge(partial, DevelopController.patch(path, node), keepNulls: false) }
            }
        }
        settingsJSON = DevelopController.encode(partial) ?? "{}"
    }

    init(name: String, groups: [PresetGroup], settingsJSON: String) {
        self.name = name
        self.groups = groups
        self.settingsJSON = settingsJSON
    }
}

/// Presets saved as one JSON file each under `<app support>/Presets`.
public final class PresetStore: @unchecked Sendable {
    public let folder: URL

    public init(folder: URL) { self.folder = folder }

    /// `~/Library/Application Support/Tessera/Presets`.
    public static var standard: PresetStore {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return PresetStore(folder: base.appendingPathComponent("Tessera/Presets", isDirectory: true))
    }

    public func list() -> [DevelopPreset] {
        let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
        return files.filter { $0.pathExtension == "json" }.compactMap(load)
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    public func load(_ url: URL) -> DevelopPreset? {
        guard let data = try? Data(contentsOf: url),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let name = obj["name"] as? String, let settings = obj["settings"] as? [String: Any],
              let json = DevelopController.encode(settings) else { return nil }
        let groups = (obj["groups"] as? [String] ?? []).compactMap(PresetGroup.init(rawValue:))
        return DevelopPreset(name: name, groups: groups, settingsJSON: json)
    }

    @discardableResult
    public func save(_ preset: DevelopPreset) throws -> URL {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let obj: [String: Any] = ["name": preset.name, "groups": preset.groups.map(\.rawValue),
                                  "settings": preset.settings, "format": "tessera-preset/1"]
        let url = fileURL(preset.name)
        try Data((DevelopController.encode(obj, pretty: true) ?? "{}").utf8).write(to: url, options: .atomic)
        return url
    }

    public func delete(_ name: String) throws {
        try FileManager.default.removeItem(at: fileURL(name))
    }

    func fileURL(_ name: String) -> URL {
        let safe = name.map { "/:\\".contains($0) ? "-" : $0 }
        return folder.appendingPathComponent(String(safe) + ".json")
    }
}
