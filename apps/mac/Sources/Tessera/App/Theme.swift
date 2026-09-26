import AppKit
import SwiftUI
import TesseraCore

/// Tessera's design system (apps/mac/DESIGN.md). Every colour, spacing, radius, control height,
/// font and animation a view uses comes from here; `Tests/TesseraCoreTests/ThemeLintTests.swift`
/// fails the build on raw literals elsewhere in `Sources/Tessera`.
///
/// Identity: a quiet, precise pro tool. Near-black canvas, warm-neutral graphite panels, 1 px
/// hairlines, one amber accent used only for selection, focus and the primary action, and
/// desaturated semantic colours for cull decisions. Dark first, with a full Light appearance.
enum Theme {

    // MARK: - Colour (AppKit)

    /// Dynamic colours: each resolves per appearance (Dark Aqua first, Aqua second).
    enum Palette {
        // Surfaces, back to front.
        /// Grid, compare and loupe surround.
        static let canvas = dynamic(dark: 0x131312, light: 0xE3E2DF)
        /// Inspector, filter bar, status bar, sheets.
        static let panel = dynamic(dark: 0x1C1B1A, light: 0xF5F4F2)
        /// Fields, bordered buttons, segmented tracks, cards inside panels.
        static let raised = dynamic(dark: 0x272624, light: 0xFFFFFF)
        /// Hover fill for borderless and raised controls.
        static let hover = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.06, lightAlpha: 0.05)
        /// Pressed fill.
        static let pressed = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.11, lightAlpha: 0.09)
        /// 1 px separators and control outlines.
        static let hairline = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.09, lightAlpha: 0.11)
        /// Stronger outline: slider tracks, focus-less field borders.
        static let hairlineStrong = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.16, lightAlpha: 0.18)
        /// Alternate burst groups in the grid get this faint fill so group boundaries read.
        static let groupAlt = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.035, lightAlpha: 0.04)
        /// Plots (histogram, curve, detail preview) sit in a well darker than the panel.
        static let well = dynamic(dark: 0x0E0E0D, light: 0xE9E8E5)

        // Text.
        static let textPrimary = dynamic(dark: 0xECEAE6, light: 0x1D1C1A)
        static let textSecondary = dynamic(dark: 0xA7A39C, light: 0x5E5A54)
        static let textTertiary = dynamic(dark: 0x7C7872, light: 0x858079)
        /// Text and glyphs on an accent or semantic fill.
        static let textOnAccent = dynamic(dark: 0x17130B, light: 0xFFFFFF)

        // Accent: warm amber. Distinct from every decision colour and from the macOS default blue.
        static let accent = dynamic(dark: 0xE2A04A, light: 0xA8620C)
        /// Selection fill (grid cells, list rows, history head).
        static let accentSubtle = dynamic(dark: 0xE2A04A, light: 0xA8620C, darkAlpha: 0.18, lightAlpha: 0.14)

        // Semantic: desaturated, same lightness family, never used for chrome.
        static let keep = dynamic(dark: 0x80B38C, light: 0x2F7A46)
        static let reject = dynamic(dark: 0xD8786C, light: 0xB23B2F)
        static let basket = dynamic(dark: 0x88A8CE, light: 0x3B6699)
        static let warning = dynamic(dark: 0xD8B55C, light: 0x8A690B)
        /// Undecided: a neutral, so an undecided frame carries no colour.
        static let undecided = textSecondary

        // HUD (loupe overlays, toast): a heavy material over any photo.
        static let hud = dynamic(dark: 0x1A1918, light: 0xFBFAF8, darkAlpha: 0.88, lightAlpha: 0.92)

        // Channel colours for plots (histogram, curves): the same in both appearances.
        static let channelRed = NSColor(srgbRed: 0.91, green: 0.36, blue: 0.32, alpha: 1)
        static let channelGreen = NSColor(srgbRed: 0.42, green: 0.80, blue: 0.43, alpha: 1)
        static let channelBlue = NSColor(srgbRed: 0.40, green: 0.56, blue: 0.98, alpha: 1)
        /// Scopes (histogram, tone curve, 1:1 detail) are always dark, in both appearances, so
        /// additive channel colours and the luminance line read the same way (FCP / Resolve scopes).
        static let plotWell = srgb(0x121211)
        /// Slider thumbs and wheel pucks.
        static let thumb = dynamic(dark: 0xE6E4E0, light: 0xFFFFFF)
        /// Luminance line and handles inside a plot well.
        static let plotLine = srgb(0xE6E4E0)
        static let plotGrid = srgb(0xFFFFFF, 0.08)
        static let plotGuide = srgb(0xFFFFFF, 0.16)
        static let plotText = srgb(0xA7A39C)
        /// Transparency checkerboard (document viewport, layer thumbnails): aliases of existing
        /// tokens, light in both appearances like every image editor's grid (DESIGN.md §5).
        static let checkerLight = thumb
        static let checkerDark = plotText

        // On-image chips are drawn over photos, so they use one appearance-independent set.
        enum OnImage {
            static let keep = NSColor(srgbRed: 0.50, green: 0.70, blue: 0.55, alpha: 1)
            static let reject = NSColor(srgbRed: 0.85, green: 0.47, blue: 0.42, alpha: 1)
            static let basket = NSColor(srgbRed: 0.53, green: 0.66, blue: 0.81, alpha: 1)
            static let ink = NSColor(srgbRed: 0.08, green: 0.075, blue: 0.07, alpha: 1)
            static let scrim = NSColor(srgbRed: 0.06, green: 0.055, blue: 0.05, alpha: 0.62)
            static let text = NSColor(srgbRed: 0.93, green: 0.92, blue: 0.90, alpha: 1)
            static let guide = NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 0.9)
            static let guideFaint = NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 0.3)
            static let shadow = NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 0.45)
        }

        /// A colour that resolves per appearance; hex is 0xRRGGBB in sRGB.
        static func dynamic(dark: UInt32, light: UInt32, darkAlpha: CGFloat = 1, lightAlpha: CGFloat = 1) -> NSColor {
            let d = srgb(dark, darkAlpha), l = srgb(light, lightAlpha)
            return NSColor(name: nil) { appearance in
                appearance.isDark ? d : l
            }
        }

        static func srgb(_ hex: UInt32, _ alpha: CGFloat = 1) -> NSColor {
            NSColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                    blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
        }
    }

    // MARK: - Colour (SwiftUI)

    static let canvas = Color(nsColor: Palette.canvas)
    static let panel = Color(nsColor: Palette.panel)
    static let raised = Color(nsColor: Palette.raised)
    static let hover = Color(nsColor: Palette.hover)
    static let pressed = Color(nsColor: Palette.pressed)
    static let hairline = Color(nsColor: Palette.hairline)
    static let hairlineStrong = Color(nsColor: Palette.hairlineStrong)
    static let well = Color(nsColor: Palette.well)
    static let textPrimary = Color(nsColor: Palette.textPrimary)
    static let textSecondary = Color(nsColor: Palette.textSecondary)
    static let textTertiary = Color(nsColor: Palette.textTertiary)
    static let textOnAccent = Color(nsColor: Palette.textOnAccent)
    static let accent = Color(nsColor: Palette.accent)
    static let accentSubtle = Color(nsColor: Palette.accentSubtle)
    static let keep = Color(nsColor: Palette.keep)
    static let reject = Color(nsColor: Palette.reject)
    static let basket = Color(nsColor: Palette.basket)
    static let warning = Color(nsColor: Palette.warning)
    static let hud = Color(nsColor: Palette.hud)
    /// Drop shadow for raised pieces (segmented thumb, HUD bars, the print preview page).
    static let shadow = Color(nsColor: Palette.OnImage.shadow)
    static let clear = Color.clear

    // MARK: - Layout

    /// 8-pt grid with 4-pt micro steps.
    enum Space {
        static let hairline: CGFloat = 1
        static let xxs: CGFloat = 2
        static let xs: CGFloat = 4
        static let s: CGFloat = 8
        static let m: CGFloat = 12
        static let l: CGFloat = 16
        static let xl: CGFloat = 24
        static let xxl: CGFloat = 32
        /// Panel gutter: inspector, filter bar, status bar and sheet content edges.
        static let gutter: CGFloat = 12
    }

    enum Radius {
        /// Chips, badges, inputs, thumbnails in lists.
        static let chip: CGFloat = 4
        /// Buttons, segmented controls, menus, grid selection.
        static let control: CGFloat = 6
        /// Cards, HUD bars, toasts, sheets.
        static let card: CGFloat = 8
    }

    enum Height {
        /// Small controls (inspector toolbars, chips rows).
        static let small: CGFloat = 20
        /// Regular controls (filter bar, panel buttons, segmented controls).
        static let regular: CGFloat = 24
        /// Large controls (sheet footers, HUD tools).
        static let large: CGFloat = 28
        /// Badges drawn on thumbnails.
        static let chip: CGFloat = 16
        /// Inspector section header.
        static let sectionHeader: CGFloat = 32
        /// One slider row: label + readout line, then the track.
        static let slider: CGFloat = 32
        /// Sidebar and list rows.
        static let row: CGFloat = 24
        static let statusBar: CGFloat = 24
        static let filmstrip: CGFloat = 72
        static let progressBar: CGFloat = 32
        /// Slider thumb diameter.
        static let thumb: CGFloat = 12
    }

    enum Width {
        static let sidebarMin: CGFloat = 200, sidebarIdeal: CGFloat = 220, sidebarMax: CGFloat = 300
        static let inspectorMin: CGFloat = 288, inspectorIdeal: CGFloat = 296, inspectorMax: CGFloat = 380
        /// Label column in inspector key/value rows and sheet forms.
        static let label: CGFloat = 72
        static let labelWide: CGFloat = 96
        static let toolbarSegment: CGFloat = 264
        static let thumbnailSlider: CGFloat = 96
    }

    // MARK: - Type

    /// SF Pro at six sizes (11/12/13/15/17/22), three weights (regular/medium/semibold).
    /// Numeric readouts always use monospaced digits. SF's size-specific tracking is left as is;
    /// the only manual tracking is on the 22 pt display size.
    enum Fonts {
        static let caption = Font.system(size: 11)
        static let captionMedium = Font.system(size: 11, weight: .medium)
        static let captionSemibold = Font.system(size: 11, weight: .semibold)
        static let captionNumeric = Font.system(size: 11).monospacedDigit()
        static let label = Font.system(size: 12)
        static let labelMedium = Font.system(size: 12, weight: .medium)
        static let labelSemibold = Font.system(size: 12, weight: .semibold)
        static let labelNumeric = Font.system(size: 12).monospacedDigit()
        static let labelMono = Font.system(size: 12, design: .monospaced)
        static let captionMono = Font.system(size: 11, design: .monospaced)
        static let body = Font.system(size: 13)
        static let bodyMedium = Font.system(size: 13, weight: .medium)
        static let title = Font.system(size: 15, weight: .semibold)
        static let headline = Font.system(size: 17, weight: .semibold)
        static let display = Font.system(size: 22, weight: .semibold)
        static let displayTracking: CGFloat = -0.3
        /// SF Symbol sizes matched to the text they sit beside.
        static let iconSmall = Font.system(size: 11, weight: .medium)
        static let icon = Font.system(size: 13, weight: .regular)
        static let iconLarge = Font.system(size: 22, weight: .regular)
    }

    enum NSFonts {
        static var caption: NSFont { NSFont.systemFont(ofSize: 11) }
        static var captionMedium: NSFont { NSFont.systemFont(ofSize: 11, weight: .medium) }
        static var captionSemibold: NSFont { NSFont.systemFont(ofSize: 11, weight: .semibold) }
        static var captionNumeric: NSFont { NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .regular) }
        static var captionNumericMedium: NSFont { NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium) }
        static var label: NSFont { NSFont.systemFont(ofSize: 12) }
        static var labelMedium: NSFont { NSFont.systemFont(ofSize: 12, weight: .medium) }
        static var labelNumeric: NSFont { NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular) }
        static var labelMono: NSFont { NSFont.monospacedSystemFont(ofSize: 12, weight: .regular) }
        static var body: NSFont { NSFont.systemFont(ofSize: 13) }
    }

    // MARK: - Motion

    /// 150–200 ms ease-out for things that appear; nothing animates on keyboard culling.
    enum Motion {
        static let fast: Double = 0.15
        static let standard: Double = 0.18
        @MainActor static var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }
        @MainActor static var appear: Animation { .easeOut(duration: reduceMotion ? fast : standard) }
        /// Enter / exit along the same edge; opacity only when Reduce Motion is on.
        @MainActor static func transition(from edge: Edge) -> AnyTransition {
            reduceMotion ? .opacity : .opacity.combined(with: .move(edge: edge))
        }
    }

    /// Opacity of a disabled control or dimmed (rejected) image.
    enum Opacity {
        static let disabled: Double = 0.4
        static let rejectedImage: Float = 0.35
        static let hidden: Double = 0.45
    }

    /// Loupe surround in linear light: the canvas colour, per the window's appearance. Read by
    /// the Metal presenter on each draw (a token only; the presentation code is unchanged).
    @MainActor static var loupeBackgroundLinear: Float {
        (NSApp?.effectiveAppearance.isDark ?? true) ? 0.0065 : 0.76
    }
}

extension NSAppearance {
    var isDark: Bool { bestMatch(from: [.darkAqua, .aqua, .vibrantDark, .vibrantLight]).map { $0 == .darkAqua || $0 == .vibrantDark } ?? true }
}

extension NSColor {
    /// Resolves a dynamic colour for a layer (CGColor has no appearance) using `view`'s appearance.
    @MainActor func cgColor(for view: NSView) -> CGColor {
        var c = cgColor
        view.effectiveAppearance.performAsCurrentDrawingAppearance { c = self.cgColor }
        return c
    }
}

/// App appearance: follows the system unless the user picks one in View ▸ Appearance.
enum AppearancePreference: String, CaseIterable, Identifiable {
    case system, dark, light
    var id: String { rawValue }
    var title: String { rawValue.capitalized }
    static let defaultsKey = "Appearance"

    @MainActor static var current: AppearancePreference {
        get { AppearancePreference(rawValue: UserDefaults.standard.string(forKey: defaultsKey) ?? "") ?? .system }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: defaultsKey); newValue.apply() }
    }

    @MainActor func apply() {
        NSApp.appearance = switch self {
        case .system: nil
        case .dark: NSAppearance(named: .darkAqua)
        case .light: NSAppearance(named: .aqua)
        }
    }
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

    /// Mark colours: four desaturated hues clear of the accent and the decision colours.
    /// Drawn on thumbnails and in the sidebar, so one appearance-independent set.
    static func color(_ m: UInt8) -> NSColor {
        switch m {
        case 6: Theme.Palette.srgb(0xCF7FA8)   // rose
        case 7: Theme.Palette.srgb(0xCCBF62)   // citron
        case 8: Theme.Palette.srgb(0x62B5AF)   // teal
        case 9: Theme.Palette.srgb(0xA394D6)   // lilac
        default: .clear
        }
    }
}

extension Decision {
    /// Text / swatch colour on panels (per appearance).
    var color: NSColor {
        switch self {
        case .undecided: Theme.Palette.undecided
        case .reject: Theme.Palette.reject
        case .keep: Theme.Palette.keep
        }
    }

    /// Fill of the decision chip drawn over a photo.
    var chipColor: NSColor {
        switch self {
        case .undecided: Theme.Palette.OnImage.scrim
        case .reject: Theme.Palette.OnImage.reject
        case .keep: Theme.Palette.OnImage.keep
        }
    }
}

extension CullState {
    /// Short badge text, e.g. "Keep", "Good 2", "Best 3", "Reject" (sentence case; one chip family).
    var badgeText: String? {
        switch decision {
        case .undecided: nil
        case .reject: "Reject"
        case .keep: grade == 0 ? "Keep" : CullState.gradeNames[Int(grade)] + " \(grade)"
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

    /// Engine setting behind the slider.
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
        case .texture: DevelopParameter("tone", "texture")
        case .clarity: DevelopParameter("tone", "clarity")
        case .dehaze: DevelopParameter("tone", "dehaze")
        case .vibrance: DevelopParameter("color", "vibrance")
        case .saturation: DevelopParameter("color", "saturation")
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
