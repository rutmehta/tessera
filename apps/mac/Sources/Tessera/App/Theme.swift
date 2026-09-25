import AppKit
import SwiftUI
import TesseraCore

/// Restrained, dark-first palette (Capture One / Pixelmator Pro direction): neutral greys,
/// one accent, semantic colours only for decisions and marks.
enum Theme {
    static let windowBackground = NSColor(calibratedWhite: 0.105, alpha: 1)
    static let gridBackground = NSColor(calibratedWhite: 0.085, alpha: 1)
    static let cellBackground = NSColor(calibratedWhite: 0.14, alpha: 1)
    static let cellBackgroundAlt = NSColor(calibratedWhite: 0.165, alpha: 1)   // alternate groups
    static let cellSelected = NSColor(calibratedWhite: 0.30, alpha: 1)
    static let accent = NSColor(srgbRed: 0.93, green: 0.62, blue: 0.24, alpha: 1)   // warm amber focus ring
    static let textPrimary = NSColor(calibratedWhite: 0.88, alpha: 1)
    static let textSecondary = NSColor(calibratedWhite: 0.55, alpha: 1)

    static let reject = NSColor(srgbRed: 0.90, green: 0.30, blue: 0.27, alpha: 1)
    static let keep = NSColor(srgbRed: 0.36, green: 0.78, blue: 0.45, alpha: 1)
    static let basket = NSColor(srgbRed: 0.40, green: 0.62, blue: 0.95, alpha: 1)
    static let toastBackground = NSColor(calibratedWhite: 0.19, alpha: 0.96)
    static let statusText = NSColor(calibratedWhite: 0.78, alpha: 1)

    /// Linear-light background of the Metal loupe (≈ 0.10 sRGB grey).
    static let loupeBackgroundLinear: Float = 0.010
}

enum MarkStyle {
    /// Default mark set; user-nameable later (docs/06 §2 "Mark").
    static func name(_ m: UInt8) -> String {
        switch m {
        case 6: "Needs Retouch"
        case 7: "Client Favourite"
        case 8: "Print"
        case 9: "Review"
        default: "No Mark"
        }
    }

    static func color(_ m: UInt8) -> NSColor {
        switch m {
        case 6: NSColor(srgbRed: 0.93, green: 0.35, blue: 0.62, alpha: 1)   // magenta
        case 7: NSColor(srgbRed: 0.98, green: 0.78, blue: 0.25, alpha: 1)   // yellow
        case 8: NSColor(srgbRed: 0.30, green: 0.80, blue: 0.85, alpha: 1)   // teal
        case 9: NSColor(srgbRed: 0.65, green: 0.50, blue: 0.95, alpha: 1)   // violet
        default: .clear
        }
    }
}

extension Decision {
    var color: NSColor {
        switch self {
        case .undecided: Theme.textSecondary
        case .reject: Theme.reject
        case .keep: Theme.keep
        }
    }
}

extension CullState {
    /// Short badge text, e.g. "KEEP", "GOOD", "BEST", "REJECT".
    var badgeText: String? {
        switch decision {
        case .undecided: nil
        case .reject: "REJECT"
        case .keep: grade == 0 ? "KEEP" : CullState.gradeNames[Int(grade)].uppercased() + " \(grade)"
        }
    }
}

/// Stub Basic panel keys. Only Exposure is wired to the loupe shader for now; the rest update
/// state through the same path so the < 16 ms plumbing is exercised.
enum BasicKey: String, CaseIterable, Identifiable {
    case temperature, tint, exposure, contrast, highlights, shadows, whites, blacks
    case texture, clarity, dehaze, vibrance, saturation

    var id: String { rawValue }
    var title: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }
    var range: ClosedRange<Double> {
        switch self {
        case .temperature: 2000...25000
        case .tint: -150...150
        case .exposure: -5...5
        default: -100...100
        }
    }
    /// Without a session (Temperature shows a neutral daylight value, not the slider minimum).
    var defaultValue: Double { self == .temperature ? 5500 : 0 }
    var step: Double {
        switch self {
        case .temperature: 50
        case .exposure: 0.01
        default: 1
        }
    }
    var format: String {
        switch self {
        case .temperature: "%.0f K"
        case .exposure: "%+.2f"
        default: "%+.0f"
        }
    }

    /// Engine setting behind the slider; nil where the M1 pipeline has no operator yet.
    var parameter: DevelopParameter? {
        switch self {
        case .temperature: .temperature
        case .tint: .tint
        case .exposure: .exposure
        case .contrast: .contrast
        case .highlights: .highlights
        case .shadows: .shadows
        case .whites: .whites
        case .blacks: .blacks
        case .texture, .clarity, .dehaze, .vibrance, .saturation: nil
        }
    }

    /// History entry label, e.g. "Exposure +0.50".
    func historyLabel(_ value: Double) -> String {
        "\(title) " + String(format: format, value)
    }

    static let sections: [(String, [BasicKey])] = [
        ("White Balance", [.temperature, .tint]),
        ("Tone", [.exposure, .contrast, .highlights, .shadows, .whites, .blacks]),
        ("Presence", [.texture, .clarity, .dehaze, .vibrance, .saturation]),
    ]
}
