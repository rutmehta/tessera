import AppKit
import XCTest
@testable import Tessera

/// Settings ▸ Appearance (SPEC §6): ink on the accent stays legible for any accent, the
/// ground- and accent-dependent tokens follow the choice, and the choice persists.
@MainActor
final class AppearanceSettingsTests: XCTestCase {
    private let suiteName = "dev.tessera.tests.appearance"

    override func tearDown() {
        UserDefaults.standard.removePersistentDomain(forName: suiteName)
        super.tearDown()
    }

    func testInkOnAccentClearsWCAGAAForEverySwatchAndAnyColour() {
        for preset in AppearanceSettings.AccentPreset.allCases {
            let ink = AppearanceSettings.onAccent(for: preset.hex)
            XCTAssertEqual(ink, AppearanceSettings.inkOnAccent, "\(preset.title) takes the warm ink")
            XCTAssertGreaterThanOrEqual(Theme.Palette.contrast(preset.hex, ink), AppearanceSettings.minimumContrast, preset.title)
        }
        // The colour well allows anything: sweep the 6 × 6 × 6 web cube plus the mid-tones that
        // neither warm ink reaches.
        var colours: [UInt32] = [0x777777, 0x808080, 0x8B0000, 0x3366CC]
        for r in stride(from: 0, through: 255, by: 51) {
            for g in stride(from: 0, through: 255, by: 51) {
                for b in stride(from: 0, through: 255, by: 51) { colours.append(UInt32(r) << 16 | UInt32(g) << 8 | UInt32(b)) }
            }
        }
        for hex in colours {
            let ink = AppearanceSettings.onAccent(for: hex)
            XCTAssertGreaterThanOrEqual(Theme.Palette.contrast(hex, ink), AppearanceSettings.minimumContrast,
                                        String(format: "%06X → %06X", hex, ink))
        }
    }

    func testTokensFollowTheChoiceAndPersist() throws {
        let suite = try XCTUnwrap(UserDefaults(suiteName: suiteName))
        suite.removePersistentDomain(forName: suiteName)
        let settings = AppearanceSettings(defaults: suite)
        XCTAssertEqual(settings.accent, AppearanceSettings.AccentPreset.amber.hex)
        XCTAssertEqual(settings.ground, .black)
        XCTAssertEqual(settings.accentPreset, .amber)
        XCTAssertEqual(dark(settings.tokens.canvas), 0x0A0A0A)
        XCTAssertEqual(dark(settings.tokens.textOnAccent), AppearanceSettings.inkOnAccent)

        settings.setAccent(AppearanceSettings.AccentPreset.cobalt.hex)
        settings.setGround(.navy)
        XCTAssertEqual(dark(settings.tokens.accent), 0x4F7BFF)
        XCTAssertEqual(dark(settings.tokens.canvas), AppearanceSettings.Ground.navy.canvas)
        XCTAssertEqual(dark(settings.tokens.panel), AppearanceSettings.Ground.navy.panel)
        XCTAssertEqual(dark(settings.tokens.hud), AppearanceSettings.Ground.navy.canvas)
        // The Light values are untouched by the choice.
        XCTAssertEqual(light(settings.tokens.accent), 0xA8620C)
        XCTAssertEqual(light(settings.tokens.canvas), 0xE3E2DF)

        let again = AppearanceSettings(defaults: suite)
        XCTAssertEqual(again.accent, 0x4F7BFF)
        XCTAssertEqual(again.ground, .navy)
        XCTAssertEqual(again.accentPreset, .cobalt)
    }

    func testThemeTokensReadTheSharedSettings() {
        let shared = AppearanceSettings.shared
        let (accent, ground) = (shared.accent, shared.ground)
        defer { shared.setAccent(accent); shared.setGround(ground) }
        shared.setAccent(AppearanceSettings.AccentPreset.rose.hex)
        shared.setGround(.graphite)
        XCTAssertEqual(dark(Theme.Palette.accent), AppearanceSettings.AccentPreset.rose.hex)
        XCTAssertEqual(dark(Theme.Palette.raised), AppearanceSettings.Ground.graphite.raised)
        if NSApp?.effectiveAppearance.isDark ?? true {   // the loupe surround is the canvas in linear light
            XCTAssertEqual(Theme.loupeBackgroundLinear, Float(Theme.Palette.luminance(AppearanceSettings.Ground.graphite.canvas)), accuracy: 1e-6)
        }
    }

    private func dark(_ color: NSColor) -> UInt32? { resolve(color, .darkAqua) }
    private func light(_ color: NSColor) -> UInt32? { resolve(color, .aqua) }

    private func resolve(_ color: NSColor, _ name: NSAppearance.Name) -> UInt32? {
        var hex: UInt32?
        NSAppearance(named: name)?.performAsCurrentDrawingAppearance { hex = Theme.Palette.hex(of: color) }
        return hex
    }
}
