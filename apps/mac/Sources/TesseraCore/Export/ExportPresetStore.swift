import Foundation
import TesseraFFI

/// A named export configuration (a JSON file under `<app-dir>/ExportPresets/`).
public struct ExportPresetEntry: Identifiable, Equatable, Sendable {
    public let name: String
    public let settings: ExportSettings
    public var id: String { name }
}

/// Export preset CRUD through the engine. The engine installs the shipped presets (Web 2048 sRGB,
/// Full-size JPEG, 16-bit TIFF ProPhoto, Print 300 dpi) on first use; after that every preset is
/// an ordinary, editable file. Blocking file I/O, small: fine on the main actor.
public struct ExportPresetStore: Sendable {
    public let engine: Engine
    public init(engine: Engine) { self.engine = engine }

    public func list() throws -> [ExportPresetEntry] {
        try engine.exportPresets().compactMap { p in
            (try? ExportSettings(json: p.settingsJson)).map { ExportPresetEntry(name: p.name, settings: $0) }
        }
    }

    /// Creates or replaces. The destination is kept only when `keepDestination` (a preset usually
    /// describes the file, not where it goes).
    public func save(_ name: String, _ settings: ExportSettings, keepDestination: Bool = false) throws {
        var s = settings
        if !keepDestination { s.destination = "" }
        try engine.saveExportPreset(name: name, settingsJson: s.json)
    }

    public func delete(_ name: String) throws { try engine.deleteExportPreset(name: name) }
    public func rename(_ name: String, to newName: String) throws { try engine.renameExportPreset(name: name, newName: newName) }
    public func restoreDefaults() throws { try engine.restoreDefaultExportPresets() }
}
