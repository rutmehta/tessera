import AppKit
import SwiftUI

/// Settings ▸ Appearance (SPEC §6): the accent (six swatches, or any colour from the well) and the
/// ground. Both are `Theme` tokens read from `AppearanceSettings`; nothing here names a colour of
/// its own. Keep and reject are not offered: decisions keep their colours.
struct AppearanceSettingsView: View {
    private var settings: AppearanceSettings { .shared }

    var body: some View {
        Form {
            Section("Accent") {
                HStack(spacing: Theme.Space.m) {
                    ForEach(AppearanceSettings.AccentPreset.allCases) { preset in
                        Swatch(hex: preset.hex, title: preset.title, selected: settings.accentPreset == preset) {
                            settings.setAccent(preset.hex)
                        }
                    }
                    AccentColorWell(hex: settings.accent) { settings.setAccent($0) }
                        .frame(width: Theme.Height.regular, height: Theme.Height.regular)
                        .help("Any colour")
                }
                Hint("Selection, focus, the primary action and the agent mark. Keep and reject keep their own colours.")
            }
            Section("Ground") {
                Picker("Ground", selection: Binding(get: { settings.ground }, set: { settings.setGround($0) })) {
                    ForEach(AppearanceSettings.Ground.allCases) { Text($0.title).tag($0) }
                }
                .accessibilityIdentifier("appearance-ground")
                Hint("The window, panels and controls behind the photo.")
            }
        }
        .formStyle(.grouped)
        .frame(width: 520, height: 280)
        .tint(Theme.accent)
    }
}

/// One accent swatch: a disc, ringed in the primary ink when chosen.
private struct Swatch: View {
    let hex: UInt32
    let title: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Circle()
                .fill(Color(nsColor: Theme.Palette.srgb(hex)))
                .frame(width: Theme.Height.regular, height: Theme.Height.regular)
                .overlay {
                    Circle().strokeBorder(selected ? Theme.textPrimary : Theme.hairlineStrong,
                                          lineWidth: selected ? Theme.Space.xxs : Theme.Space.hairline)
                }
        }
        .buttonStyle(.plain)
        .help(title)
        .accessibilityLabel(title)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("appearance-accent-\(title.lowercased())")
    }
}

/// The system colour well for any accent. Changes apply live while the panel is open.
private struct AccentColorWell: NSViewRepresentable {
    let hex: UInt32
    let onChange: (UInt32) -> Void

    func makeNSView(context: Context) -> NSColorWell {
        let well = NSColorWell()
        well.colorWellStyle = .minimal
        well.color = Theme.Palette.srgb(hex)
        well.target = context.coordinator
        well.action = #selector(Coordinator.changed(_:))
        well.setAccessibilityIdentifier("appearance-accent-well")
        return well
    }

    func updateNSView(_ well: NSColorWell, context: Context) {
        context.coordinator.onChange = onChange
        if Theme.Palette.hex(of: well.color) != hex { well.color = Theme.Palette.srgb(hex) }
    }

    func makeCoordinator() -> Coordinator { Coordinator(onChange: onChange) }

    @MainActor final class Coordinator: NSObject {
        var onChange: (UInt32) -> Void
        init(onChange: @escaping (UInt32) -> Void) { self.onChange = onChange }

        @objc func changed(_ well: NSColorWell) {
            if let hex = Theme.Palette.hex(of: well.color) { onChange(hex) }
        }
    }
}
