import CoreGraphics
import Foundation

/// Everything the Print sheet chooses besides the paper (which lives in `NSPrintInfo`).
public struct PrintSettings: Codable, Equatable, Sendable {
    public enum Sharpening: String, Codable, CaseIterable, Sendable, Identifiable {
        case none, matte, glossy
        public var id: String { rawValue }
        public var title: String { switch self { case .none: "None"; case .matte: "Matte"; case .glossy: "Glossy" } }
    }
    public enum ColorHandling: String, Codable, CaseIterable, Sendable, Identifiable {
        /// Tessera converts to the printer profile; turn colour management off in the driver.
        case application
        /// Display P3 pixels; the printer driver converts.
        case printer
        public var id: String { rawValue }
        public var title: String { switch self { case .application: "Tessera manages colour"; case .printer: "Printer manages colour" } }
    }
    public enum Intent: String, Codable, CaseIterable, Sendable, Identifiable {
        case perceptual, relative
        public var id: String { rawValue }
        public var title: String { switch self { case .perceptual: "Perceptual"; case .relative: "Relative colorimetric" } }
    }

    public var layout = PrintLayout()
    /// Print resolution: engine renders are made at this many pixels per inch.
    public var dpi: Double = 300
    public var sharpening: Sharpening = .glossy
    public var colorHandling: ColorHandling = .printer
    /// ICC output profile for application-managed colour (see `printer_profiles`).
    public var profilePath: String?
    public var intent: Intent = .perceptual
    public var blackPointCompensation = true
    /// "Print to file": JPEG pages at this resolution.
    public var fileDPI: Double = 300

    public init() {}

    public static let resolutions: [Double] = [150, 180, 240, 300, 360, 600, 720]

    /// Pixel box to request from the engine for a picture area: the area's long side at `dpi` in
    /// both directions, so the render's long edge covers the cell however the picture is turned.
    public func renderBox(for area: CGRect) -> (width: UInt32, height: UInt32) {
        let long = UInt32(max((max(area.width, area.height) * dpi / 72).rounded(), 1))
        return (long, long)
    }

    // MARK: Persistence (UserDefaults JSON)

    public static let defaultsKey = "PrintSettings"

    public static func load(_ defaults: UserDefaults = .standard) -> PrintSettings {
        guard let data = defaults.data(forKey: defaultsKey),
              let settings = try? JSONDecoder().decode(PrintSettings.self, from: data) else { return PrintSettings() }
        return settings
    }

    public func save(_ defaults: UserDefaults = .standard) {
        if let data = try? JSONEncoder().encode(self) { defaults.set(data, forKey: Self.defaultsKey) }
    }
}
