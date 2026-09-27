import Foundation

/// A frozen, partial Copy Settings preview. It does not write recipes or resolve
/// whether different image IDs refer to the same source file.
public struct BatchSettingsDraft: Sendable {
    public static let defaultGroups: Set<PresetGroup> = [
        .whiteBalance, .basicTone, .presence, .toneCurve, .hsl, .colorGrading,
    ]

    public let libraryID: String
    public let sourceImageID: String
    public let targetImageIDs: [String]
    public let groups: Set<PresetGroup>
    public let settingsJSON: String

    private let sourceSettingsJSON: String

    public init(libraryID: String, focusedImageID: String?, selectedImageIDs: [String],
                sourceSettingsJSON: String, groups: Set<PresetGroup> = Self.defaultGroups) throws {
        guard !libraryID.isEmpty, !selectedImageIDs.contains(where: \.isEmpty),
              focusedImageID != "" else { throw BatchSettingsDraftError.invalidIdentity }
        guard let focusedImageID, selectedImageIDs.contains(focusedImageID) else {
            throw BatchSettingsDraftError.referenceNotSelected
        }

        var seen: Set<String> = [focusedImageID]
        let targets = selectedImageIDs.filter { seen.insert($0).inserted }
        guard !targets.isEmpty else { throw BatchSettingsDraftError.noTargets }
        guard !groups.contains(.crop) else { throw BatchSettingsDraftError.unsupportedGroup }
        guard !groups.isEmpty else { throw BatchSettingsDraftError.noSettings }
        guard let source = try? JSONSerialization.jsonObject(with: Data(sourceSettingsJSON.utf8)) as? [String: Any] else {
            throw BatchSettingsDraftError.invalidSourceSettings
        }

        let preset = DevelopPreset(name: "Batch copy", groups: Array(groups), from: source)
        guard Self.hasActionableValue(preset.settings) else { throw BatchSettingsDraftError.noSettings }

        self.libraryID = libraryID
        self.sourceImageID = focusedImageID
        targetImageIDs = targets
        self.groups = groups
        settingsJSON = preset.settingsJSON
        self.sourceSettingsJSON = sourceSettingsJSON
    }

    /// Rebuilds only the selected field paths from the same captured source.
    public func selecting(_ groups: Set<PresetGroup>) throws -> Self {
        try Self(libraryID: libraryID, focusedImageID: sourceImageID,
                 selectedImageIDs: [sourceImageID] + targetImageIDs,
                 sourceSettingsJSON: sourceSettingsJSON, groups: groups)
    }

    private static func hasActionableValue(_ value: Any) -> Bool {
        if let object = value as? [String: Any] {
            return object.values.contains(where: hasActionableValue)
        }
        return !(value is NSNull)
    }
}

public enum BatchSettingsDraftError: Error, Equatable {
    case invalidIdentity
    case referenceNotSelected
    case noTargets
    case unsupportedGroup
    case noSettings
    case invalidSourceSettings
}
