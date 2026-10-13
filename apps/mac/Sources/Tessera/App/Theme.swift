import AppKit
import SwiftUI
import TesseraCore

/// Tessera's design system (apps/mac/DESIGN.md). Every colour, spacing, radius, control height,
/// font and animation a view uses comes from here; `Tests/TesseraCoreTests/ThemeLintTests.swift`
/// fails the build on raw literals elsewhere in `Sources/Tessera`.
///
/// Identity: a quiet, precise pro tool. True-black canvas, one warm-white ink family, 1 px
/// hairlines, one accent used only for selection, focus and the primary action, and desaturated
/// semantic colours for cull decisions. Dark is the only appearance during the redesign
/// (docs/design/redesign-2026-10/SPEC.md §2); the Light values stay in place for later.
///
/// The ground (canvas / panel / raised / well / HUD) and the accent are the user's choice in
/// Settings ▸ Appearance (`AppearanceSettings`, SPEC §6). Those tokens are computed properties
/// that read the settings object, so a SwiftUI body that names `Theme.accent` observes the
/// setting and AppKit drawing picks up the new colour on its next draw.
enum Theme {

    // MARK: - Colour (AppKit)

    /// Dynamic colours: each resolves per appearance (Dark Aqua first, Aqua second).
    enum Palette {
        /// The user's ground and accent, resolved once per change (`AppearanceSettings.tokens`).
        private static var chosen: AppearanceSettings.Tokens { AppearanceSettings.shared.tokens }

        // Surfaces, back to front.
        /// Window ground: grid, compare and loupe surround, behind the photo.
        static var canvas: NSColor { chosen.canvas }
        /// Inspector, toolbar, filter bar, status bar, sheets, cards.
        static var panel: NSColor { chosen.panel }
        /// Fields, bordered buttons, segmented tracks, cards inside panels.
        static var raised: NSColor { chosen.raised }
        /// Slider tracks and inactive toggles.
        static let track = dynamic(dark: 0x2A2A2A, light: 0xCFCDC9)
        /// Hover fill for borderless and raised controls.
        static let hover = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.06, lightAlpha: 0.05)
        /// Pressed fill.
        static let pressed = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.11, lightAlpha: 0.09)
        /// 1 px separators and control outlines.
        static let hairline = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.08, lightAlpha: 0.11)
        /// Stronger outline: field and HUD outlines, focus-less field borders.
        static let hairlineStrong = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.14, lightAlpha: 0.18)
        /// Alternate burst groups in the grid get this faint fill so group boundaries read.
        static let groupAlt = dynamic(dark: 0xFFFFFF, light: 0x000000, darkAlpha: 0.035, lightAlpha: 0.04)
        /// Plots (histogram, curve, detail preview) sit in a well darker than the panel.
        static var well: NSColor { chosen.well }

        // Text: one warm-white family.
        static let textPrimary = dynamic(dark: 0xF2EFE9, light: 0x1D1C1A)
        /// Values, secondary labels.
        static let textSecondary = dynamic(dark: 0xB5B1AA, light: 0x5E5A54)
        /// Hints, counts.
        static let textTertiary = dynamic(dark: 0x8D8A84, light: 0x858079)
        /// Disabled text, key caps.
        static let textQuaternary = dynamic(dark: 0x5A5852, light: 0xA19D96)
        /// Text and glyphs on an accent fill: computed from the accent's luminance (≥ 4.5:1).
        static var textOnAccent: NSColor { chosen.textOnAccent }

        // Accent: amber by default, the user's colour otherwise. Distinct from every decision
        // colour and from the macOS default blue.
        static var accent: NSColor { chosen.accent }
        /// Selection fill (grid cells, list rows, history head).
        static var accentSubtle: NSColor { chosen.accentSubtle }

        // Semantic: desaturated, same lightness family, never used for chrome, never user-set.
        static let keep = dynamic(dark: 0x6F9A60, light: 0x2F7A46)
        static let reject = dynamic(dark: 0xC0584A, light: 0xB23B2F)
        static let basket = dynamic(dark: 0x88A8CE, light: 0x3B6699)
        static let warning = dynamic(dark: 0xD8B55C, light: 0x8A690B)
        /// Undecided: a neutral, so an undecided frame carries no colour.
        static let undecided = textSecondary

        // HUD (dial, mask chips, loupe overlays, toast): the ground at 86 % over any photo.
        static var hud: NSColor { chosen.hud }

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

        /// 0xRRGGBB of any colour (converted to sRGB; the colour well hands back other spaces).
        static func hex(of color: NSColor) -> UInt32? {
            guard let c = color.usingColorSpace(.sRGB) else { return nil }
            func byte(_ v: CGFloat) -> UInt32 { UInt32((min(max(v, 0), 1) * 255).rounded()) }
            return byte(c.redComponent) << 16 | byte(c.greenComponent) << 8 | byte(c.blueComponent)
        }

        /// WCAG 2 relative luminance of an sRGB hex (0 black … 1 white).
        static func luminance(_ hex: UInt32) -> Double {
            func channel(_ byte: UInt32) -> Double {
                let v = Double(byte & 0xFF) / 255
                return v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
            }
            return 0.2126 * channel(hex >> 16) + 0.7152 * channel(hex >> 8) + 0.0722 * channel(hex)
        }

        /// WCAG 2 contrast ratio between two sRGB hexes (1 … 21).
        static func contrast(_ a: UInt32, _ b: UInt32) -> Double {
            let (la, lb) = (luminance(a), luminance(b))
            return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
        }
    }

    // MARK: - Colour (SwiftUI)

    // Ground- and accent-dependent tokens are computed so a body that names them observes
    // Settings ▸ Appearance; the rest are constants.
    static var canvas: Color { Color(nsColor: Palette.canvas) }
    static var panel: Color { Color(nsColor: Palette.panel) }
    static var raised: Color { Color(nsColor: Palette.raised) }
    static let track = Color(nsColor: Palette.track)
    static let hover = Color(nsColor: Palette.hover)
    static let pressed = Color(nsColor: Palette.pressed)
    static let hairline = Color(nsColor: Palette.hairline)
    static let hairlineStrong = Color(nsColor: Palette.hairlineStrong)
    static var well: Color { Color(nsColor: Palette.well) }
    static let textPrimary = Color(nsColor: Palette.textPrimary)
    static let textSecondary = Color(nsColor: Palette.textSecondary)
    static let textTertiary = Color(nsColor: Palette.textTertiary)
    static let textQuaternary = Color(nsColor: Palette.textQuaternary)
    static var textOnAccent: Color { Color(nsColor: Palette.textOnAccent) }
    static var accent: Color { Color(nsColor: Palette.accent) }
    static var accentSubtle: Color { Color(nsColor: Palette.accentSubtle) }
    static let keep = Color(nsColor: Palette.keep)
    static let reject = Color(nsColor: Palette.reject)
    static let basket = Color(nsColor: Palette.basket)
    static let warning = Color(nsColor: Palette.warning)
    static var hud: Color { Color(nsColor: Palette.hud) }
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
        /// The agent field.
        static let field: CGFloat = 12
        /// HUD: the dial, mask chips, toasts.
        static let hud: CGFloat = 14
        /// Pill chips.
        static let pill: CGFloat = 999
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
        static var captionMono: NSFont { NSFont.monospacedSystemFont(ofSize: 11, weight: .regular) }
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
        (NSApp?.effectiveAppearance.isDark ?? true) ? Float(Palette.luminance(AppearanceSettings.shared.ground.canvas)) : 0.76
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

/// App appearance: Dark unless the user picks another in View ▸ Appearance. Dark is the only
/// appearance during the redesign (SPEC §2); the Light values are kept for the parity pass.
enum AppearancePreference: String, CaseIterable, Identifiable {
    case system, dark, light
    var id: String { rawValue }
    var title: String { rawValue.capitalized }
    static let defaultsKey = "Appearance"
    static let `default` = AppearancePreference.dark

    @MainActor static var current: AppearancePreference {
        get { AppearancePreference(rawValue: UserDefaults.standard.string(forKey: defaultsKey) ?? "") ?? .default }
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

/// Settings ▸ Appearance (SPEC §6): the accent and the ground. Persisted in user defaults and
/// applied live. Views never name a colour: `Theme.Palette` reads `tokens`, which is rebuilt on
/// every change, so a SwiftUI body that uses one of those tokens observes the choice; AppKit
/// views resolve the colour when they draw and are redrawn by the setters.
@Observable
final class AppearanceSettings {
    /// One instance for the process. Written on the main actor (the Settings window); read
    /// wherever a view draws.
    nonisolated(unsafe) static let shared = AppearanceSettings()
    static let accentKey = "AccentColor"
    static let groundKey = "Ground"

    /// Accent swatches; any other colour comes from the colour well.
    enum AccentPreset: String, CaseIterable, Identifiable {
        case amber, coral, cobalt, teal, rose, lime
        var id: String { rawValue }
        var title: String { rawValue.capitalized }
        var hex: UInt32 {
            switch self {
            case .amber: 0xF0A52A
            case .coral: 0xFF7A66
            case .cobalt: 0x4F7BFF
            case .teal: 0x2FB3A6
            case .rose: 0xE8709A
            case .lime: 0xA6D24A
            }
        }
    }

    /// The surfaces behind everything, back to front. Graphite is the pre-redesign set.
    enum Ground: String, CaseIterable, Identifiable {
        case black, graphite, navy
        var id: String { rawValue }
        var title: String { rawValue.capitalized }
        /// Window ground, behind the photo.
        var canvas: UInt32 {
            switch self {
            case .black: 0x0A0A0A
            case .graphite: 0x131312
            case .navy: 0x12151C
            }
        }
        /// Panel, toolbar fills, cards.
        var panel: UInt32 {
            switch self {
            case .black: 0x161616
            case .graphite: 0x1C1B1A
            case .navy: 0x1A1E27
            }
        }
        /// Buttons, fields, hover.
        var raised: UInt32 {
            switch self {
            case .black: 0x1C1C1C
            case .graphite: 0x272624
            case .navy: 0x232833
            }
        }
        /// Plot wells, darker than the panel.
        var well: UInt32 {
            switch self {
            case .black: 0x0F0F0F
            case .graphite: 0x0E0E0D
            case .navy: 0x0D1016
            }
        }
    }

    /// The dynamic colours behind the ground- and accent-dependent `Theme.Palette` tokens. The
    /// Light values are the pre-redesign ones, untouched until light parity is restored.
    struct Tokens {
        let canvas: NSColor, panel: NSColor, raised: NSColor, well: NSColor, hud: NSColor
        let accent: NSColor, accentSubtle: NSColor, textOnAccent: NSColor

        init(accent: UInt32, ground: Ground) {
            canvas = Theme.Palette.dynamic(dark: ground.canvas, light: 0xE3E2DF)
            panel = Theme.Palette.dynamic(dark: ground.panel, light: 0xF5F4F2)
            raised = Theme.Palette.dynamic(dark: ground.raised, light: 0xFFFFFF)
            well = Theme.Palette.dynamic(dark: ground.well, light: 0xE9E8E5)
            hud = Theme.Palette.dynamic(dark: ground.canvas, light: 0xFBFAF8, darkAlpha: 0.86, lightAlpha: 0.92)
            self.accent = Theme.Palette.dynamic(dark: accent, light: 0xA8620C)
            accentSubtle = Theme.Palette.dynamic(dark: accent, light: 0xA8620C, darkAlpha: 0.18, lightAlpha: 0.14)
            textOnAccent = Theme.Palette.dynamic(dark: AppearanceSettings.onAccent(for: accent), light: 0xFFFFFF)
        }
    }

    /// 0xRRGGBB. Amber by default.
    private(set) var accent: UInt32
    private(set) var ground: Ground
    private(set) var tokens: Tokens
    @ObservationIgnored private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        let accent = defaults.string(forKey: Self.accentKey).flatMap { UInt32($0, radix: 16) } ?? AccentPreset.amber.hex
        let ground = defaults.string(forKey: Self.groundKey).flatMap(Ground.init(rawValue:)) ?? .black
        self.defaults = defaults
        self.accent = accent
        self.ground = ground
        tokens = Tokens(accent: accent, ground: ground)
    }

    /// The swatch matching the current accent, if any.
    var accentPreset: AccentPreset? { AccentPreset.allCases.first { $0.hex == accent } }

    @MainActor func setAccent(_ hex: UInt32) {
        accent = hex & 0xFFFFFF
        defaults.set(String(format: "%06X", accent), forKey: Self.accentKey)
        apply()
    }

    @MainActor func setGround(_ ground: Ground) {
        self.ground = ground
        defaults.set(ground.rawValue, forKey: Self.groundKey)
        apply()
    }

    /// Ink on the accent: the warm ink or the warm white, whichever contrasts more. Every swatch
    /// clears 4.5:1 with the warm ink; a dark colour from the well gets the white, and a mid-tone
    /// that neither warm ink reaches falls back to pure black or white, which always does.
    static func onAccent(for accent: UInt32) -> UInt32 {
        let warm = [inkOnAccent, whiteOnAccent].max { Theme.Palette.contrast(accent, $0) < Theme.Palette.contrast(accent, $1) }!
        if Theme.Palette.contrast(accent, warm) >= minimumContrast { return warm }
        return Theme.Palette.contrast(accent, 0x000000) >= Theme.Palette.contrast(accent, 0xFFFFFF) ? 0x000000 : 0xFFFFFF
    }
    static let inkOnAccent: UInt32 = 0x1A1200
    static let whiteOnAccent: UInt32 = 0xF2EFE9
    /// WCAG AA for text.
    static let minimumContrast = 4.5

    /// Rebuilds the tokens (SwiftUI bodies that read them re-render) and gives every AppKit view
    /// the appearance-change pass it already implements, so layer colours re-resolve.
    @MainActor private func apply() {
        tokens = Tokens(accent: accent, ground: ground)
        for window in NSApp?.windows ?? [] {
            if let content = window.contentView { Self.redraw(content) }
        }
    }

    private static func redraw(_ view: NSView) {
        view.viewDidChangeEffectiveAppearance()
        view.needsDisplay = true
        view.subviews.forEach(redraw)
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
