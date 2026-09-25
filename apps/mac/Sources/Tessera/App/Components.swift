import AppKit
import SwiftUI
import TesseraCore

// Shared SwiftUI components of the design system (apps/mac/DESIGN.md §4). Views compose these
// rather than styling controls ad hoc; every value comes from `Theme`.

// MARK: - Buttons

/// Rectangular buttons with one radius (6) and three heights (20 / 24 / 28): never capsules.
struct ThemeButtonStyle: ButtonStyle {
    enum Kind { case bordered, borderless, primary, destructive }
    var kind: Kind = .bordered
    var height: CGFloat = Theme.Height.regular
    /// Icon-only buttons are square.
    var square = false

    func makeBody(configuration: Configuration) -> some View {
        ThemeButtonBody(configuration: configuration, kind: kind, height: height, square: square)
    }
}

private struct ThemeButtonBody: View {
    let configuration: ButtonStyle.Configuration
    let kind: ThemeButtonStyle.Kind
    let height: CGFloat
    let square: Bool
    @Environment(\.isEnabled) private var enabled
    @State private var hovering = false

    var body: some View {
        let pressed = configuration.isPressed
        configuration.label
            .font(height <= Theme.Height.small ? Theme.Fonts.caption : Theme.Fonts.label)
            .lineLimit(1)
            .foregroundStyle(foreground)
            .padding(.horizontal, square ? 0 : (height <= Theme.Height.small ? Theme.Space.s : Theme.Space.m - Theme.Space.xxs))
            .frame(minWidth: square ? height : nil, minHeight: height, maxHeight: height)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(fill(pressed: pressed)))
            .overlay {
                if kind == .bordered {
                    RoundedRectangle(cornerRadius: Theme.Radius.control)
                        .strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline)
                }
            }
            .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.control))
            .opacity(enabled ? 1 : Theme.Opacity.disabled)
            .onHover { hovering = $0 && enabled }
    }

    private var foreground: Color {
        switch kind {
        case .primary: Theme.textOnAccent
        case .destructive: Theme.reject
        case .bordered, .borderless: Theme.textPrimary
        }
    }

    private func fill(pressed: Bool) -> Color {
        switch kind {
        case .primary: return pressed ? Theme.accent.opacity(0.8) : hovering ? Theme.accent.opacity(0.9) : Theme.accent
        case .bordered: return pressed ? Theme.pressed : hovering ? Theme.hover : Theme.raised
        case .borderless, .destructive: return pressed ? Theme.pressed : hovering ? Theme.hover : Theme.clear
        }
    }
}

extension ButtonStyle where Self == ThemeButtonStyle {
    static var themeBordered: ThemeButtonStyle { ThemeButtonStyle(kind: .bordered) }
    static var themeBorderless: ThemeButtonStyle { ThemeButtonStyle(kind: .borderless) }
    static var themePrimary: ThemeButtonStyle { ThemeButtonStyle(kind: .primary) }
    static func theme(_ kind: ThemeButtonStyle.Kind, height: CGFloat = Theme.Height.regular, square: Bool = false) -> ThemeButtonStyle {
        ThemeButtonStyle(kind: kind, height: height, square: square)
    }
}

/// Sheet footer buttons: 28 pt, the default action filled with the accent (primary action right).
extension View {
    func sheetButton(primary: Bool = false) -> some View {
        buttonStyle(ThemeButtonStyle(kind: primary ? .primary : .bordered, height: Theme.Height.large))
    }
}

// MARK: - Menus

/// A pull-down that looks like a bordered button with a chevron.
struct ThemeMenuStyle: MenuStyle {
    var height: CGFloat = Theme.Height.regular
    var active = false

    func makeBody(configuration: Configuration) -> some View {
        // `.button` + `.plain` keeps the label in the text colour (a borderless menu takes the tint).
        HStack(spacing: Theme.Space.xs) {
            Menu(configuration)
                .menuStyle(.button)
                .buttonStyle(.plain)
                .menuIndicator(.hidden)
            Image(systemName: "chevron.down")
                .font(Theme.Fonts.iconSmall)
                .imageScale(.small)
                .foregroundStyle(Theme.textTertiary)
                .allowsHitTesting(false)
        }
        .font(height <= Theme.Height.small ? Theme.Fonts.caption : Theme.Fonts.label)
        .foregroundStyle(Theme.textPrimary)
        .fixedSize()
        .padding(.horizontal, Theme.Space.s)
        .frame(height: height)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(active ? Theme.accentSubtle : Theme.raised))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control)
            .strokeBorder(active ? Theme.accent.opacity(0.5) : Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
    }
}

/// A pop-up choice in the `ThemeMenuStyle` family (inspector rows); the current value is the
/// label and the menu shows a checkmark on it.
struct MenuPicker<Value: Hashable>: View {
    @Binding var selection: Value
    let options: [(value: Value, title: String)]
    var height: CGFloat = Theme.Height.small

    var body: some View {
        Menu {
            Picker("", selection: $selection) {
                ForEach(options.indices, id: \.self) { i in Text(options[i].title).tag(options[i].value) }
            }
            .pickerStyle(.inline)
            .labelsHidden()
        } label: {
            Text(Self.clipped(options.first { $0.value == selection }?.title ?? ""))
        }
        .menuStyle(ThemeMenuStyle(height: height))
    }

    /// Long names (ICC profiles) are shortened in the middle so the control keeps panel width.
    private static func clipped(_ s: String, max: Int = 26) -> String {
        guard s.count > max else { return s }
        return String(s.prefix(max / 2 - 1)) + "…" + String(s.suffix(max / 2 - 1))
    }
}

/// An icon-only pull-down (overflow menus, add menus).
struct IconMenuStyle: MenuStyle {
    func makeBody(configuration: Configuration) -> some View {
        Menu(configuration)
            .menuStyle(.button)
            .buttonStyle(.plain)
            .menuIndicator(.hidden)
            .foregroundStyle(Theme.textSecondary)
            .fixedSize()
            .frame(width: Theme.Height.regular, height: Theme.Height.regular)
    }
}

// MARK: - Segmented control

/// Segmented control: a well-coloured track with the chosen segment raised. Neutral, not accent:
/// the accent marks content selection and focus, not a control's mode.
struct SegmentedPicker<Value: Hashable>: View {
    struct Segment {
        let value: Value
        let title: String
        var symbol: String? = nil
        var help: String? = nil
    }

    @Binding var selection: Value
    let segments: [Segment]
    var height: CGFloat = Theme.Height.regular
    /// Stretch segments to fill the width (inspector) or hug the titles (toolbars, sheets).
    var fill = true
    @Environment(\.isEnabled) private var enabled

    var body: some View {
        HStack(spacing: Theme.Space.xxs) {
            ForEach(segments.indices, id: \.self) { i in
                let s = segments[i]
                let on = s.value == selection
                Button { selection = s.value } label: {
                    HStack(spacing: Theme.Space.xs) {
                        if let symbol = s.symbol { Image(systemName: symbol).font(Theme.Fonts.iconSmall) }
                        if !s.title.isEmpty { Text(s.title) }
                    }
                    .font(height <= Theme.Height.small ? Theme.Fonts.caption : Theme.Fonts.label)
                    .fontWeight(on ? .medium : .regular)
                    .lineLimit(1)
                    .foregroundStyle(on ? Theme.textPrimary : Theme.textSecondary)
                    .padding(.horizontal, Theme.Space.s)
                    .frame(maxWidth: fill ? .infinity : nil)
                    .frame(height: height - Theme.Space.xs)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                        .fill(on ? Theme.raised : Theme.clear)
                        .shadow(color: on ? Theme.shadow.opacity(0.4) : Theme.clear, radius: 1, y: 0.5))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(s.help ?? s.title)
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(Theme.Space.xxs)
        .frame(height: height)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(Theme.well))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
        .opacity(enabled ? 1 : Theme.Opacity.disabled)
        .accessibilityElement(children: .contain)
    }
}

// MARK: - Icon button

/// Square icon button for tool toggles (targeted adjustment, straighten, mask tools).
struct IconButton: View {
    let symbol: String
    let help: String
    var on = false
    var size: CGFloat = Theme.Height.regular
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.isEnabled) private var enabled

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(size <= Theme.Height.small ? Theme.Fonts.iconSmall : Theme.Fonts.icon)
                .foregroundStyle(on ? Theme.accent : Theme.textSecondary)
                .frame(width: size, height: size)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.control)
                    .fill(on ? Theme.accentSubtle : hovering ? Theme.hover : Theme.clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .opacity(enabled ? 1 : Theme.Opacity.disabled)
        .onHover { hovering = $0 && enabled }
        .help(help)
        .accessibilityLabel(help)
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

// MARK: - Chips

/// One chip family: 16 pt (on images) or 20 pt (panels) tall, radius 4, 11 pt medium, sentence case.
struct Chip: View {
    enum Style { case filled, outlined, neutral }
    let text: String
    var color: Color = Theme.textSecondary
    var style: Style = .neutral
    var height: CGFloat = Theme.Height.small

    var body: some View {
        Text(text)
            .font(Theme.Fonts.captionMedium)
            .monospacedDigit()
            .lineLimit(1)
            .foregroundStyle(style == .filled ? Theme.textOnAccent : style == .outlined ? color : Theme.textSecondary)
            .padding(.horizontal, Theme.Space.s - Theme.Space.xxs)
            .frame(height: height)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                .fill(style == .filled ? color : style == .neutral ? Theme.raised : Theme.clear))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                .strokeBorder(style == .outlined ? color.opacity(0.6) : style == .neutral ? Theme.hairline : Theme.clear,
                              lineWidth: Theme.Space.hairline))
    }
}

// MARK: - Panels

/// Collapsible inspector section: 32 pt header, title in 12 pt semibold, a chevron that turns.
/// Open/closed state persists per panel.
struct PanelSection<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content
    @AppStorage private var expanded: Bool
    @State private var hovering = false

    init(_ title: String, expanded: Bool = true, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
        _expanded = AppStorage(wrappedValue: expanded, "InspectorPanel." + title)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                expanded.toggle()
            } label: {
                HStack(spacing: Theme.Space.s) {
                    Text(title)
                        .font(Theme.Fonts.labelSemibold)
                        .foregroundStyle(expanded ? Theme.textPrimary : Theme.textSecondary)
                    Spacer(minLength: 0)
                    Image(systemName: "chevron.right")
                        .font(Theme.Fonts.iconSmall)
                        .foregroundStyle(hovering ? Theme.textSecondary : Theme.textTertiary)
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                }
                .padding(.horizontal, Theme.Space.gutter)
                .frame(height: Theme.Height.sectionHeader)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hovering = $0 }
            .accessibilityLabel(title)
            .accessibilityValue(expanded ? "expanded" : "collapsed")
            if expanded {
                content
                    .padding(.horizontal, Theme.Space.gutter)
                    .padding(.bottom, Theme.Space.l)
            }
            Hairline()
        }
    }
}

/// Group label inside a panel ("Tone", "Sharpening").
struct SubHeader: View {
    let title: String
    init(_ title: String) { self.title = title }
    var body: some View {
        Text(title)
            .font(Theme.Fonts.captionMedium)
            .foregroundStyle(Theme.textSecondary)
            .padding(.top, Theme.Space.m)
            .padding(.bottom, Theme.Space.xs)
            .accessibilityAddTraits(.isHeader)
    }
}

/// Key / value row: a fixed label column so values align across a panel.
struct InfoRow: View {
    let label: String
    let value: String
    var labelWidth: CGFloat = Theme.Width.label
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s) {
            Text(label).foregroundStyle(Theme.textSecondary).frame(width: labelWidth, alignment: .leading)
            Text(value).foregroundStyle(Theme.textPrimary).lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .font(Theme.Fonts.caption)
        .monospacedDigit()
    }
}

/// Text-field container matching the control family (radius 6, hairline, 24 pt): wraps the
/// borderless rule field in the filter bar and the smart-album sheet.
struct FieldContainer<Content: View>: View {
    var symbol: String?
    var invalid = false
    @ViewBuilder var content: Content
    var body: some View {
        HStack(spacing: Theme.Space.xs) {
            if let symbol { Image(systemName: symbol).font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary) }
            content
        }
        .padding(.horizontal, Theme.Space.s)
        .frame(height: Theme.Height.regular)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(Theme.raised))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control)
            .strokeBorder(invalid ? Theme.reject : Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
    }
}

/// Explanatory text inside panels and sheets.
struct Hint: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text)
            .font(Theme.Fonts.caption)
            .foregroundStyle(Theme.textTertiary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// 1 px separator.
struct Hairline: View {
    var vertical = false
    var body: some View {
        Rectangle()
            .fill(Theme.hairline)
            .frame(width: vertical ? Theme.Space.hairline : nil, height: vertical ? nil : Theme.Space.hairline)
    }
}

/// Inline warning / error line with an icon (colour is never the only signal).
struct StatusLine: View {
    enum Kind { case warning, error, success }
    let text: String
    var kind: Kind = .error
    var body: some View {
        Label {
            Text(text).foregroundStyle(Theme.textPrimary)
        } icon: {
            Image(systemName: kind == .success ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                .foregroundStyle(kind == .error ? Theme.reject : kind == .warning ? Theme.warning : Theme.keep)
        }
        .font(Theme.Fonts.caption)
    }
}

// MARK: - Sheets

/// Sheet chrome: header (title, subtitle, trailing accessory), content, footer with the primary
/// action on the right. 16 pt edges, 1 px hairlines between the three parts.
struct SheetScaffold<Accessory: View, Content: View, Leading: View, Actions: View>: View {
    let title: String
    var subtitle: String?
    @ViewBuilder var accessory: Accessory
    @ViewBuilder var content: Content
    @ViewBuilder var leading: Leading
    @ViewBuilder var actions: Actions

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: Theme.Space.m) {
                VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                    Text(title).font(Theme.Fonts.title).foregroundStyle(Theme.textPrimary)
                    if let subtitle {
                        Text(subtitle).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            .lineLimit(1).truncationMode(.middle)
                    }
                }
                Spacer(minLength: Theme.Space.s)
                accessory
            }
            .padding(.horizontal, Theme.Space.l)
            .padding(.vertical, Theme.Space.m)
            Hairline()
            content.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            Hairline()
            HStack(spacing: Theme.Space.s) {
                leading
                Spacer(minLength: Theme.Space.s)
                actions
            }
            .font(Theme.Fonts.caption)
            .foregroundStyle(Theme.textSecondary)
            .padding(.horizontal, Theme.Space.l)
            .padding(.vertical, Theme.Space.m)
        }
        .background(Theme.panel)
        .tint(Theme.accent)
    }
}

// MARK: - HUD, progress, empty state

/// Floating bar over the photo (mask tools, brush settings): heavy material, 8 pt radius.
struct HUDBackground: View {
    var body: some View {
        RoundedRectangle(cornerRadius: Theme.Radius.card)
            .fill(Theme.hud)
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.card).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
            .shadow(color: Theme.shadow.opacity(0.55), radius: Theme.Space.s, y: Theme.Space.xxs)
    }
}

/// Non-modal job progress above the status bar (import, export, print).
struct ProgressStrip<Trailing: View>: View {
    let title: String
    let done: Int
    let total: Int
    var detail: String = ""
    var current: String = ""
    @ViewBuilder var trailing: Trailing

    var body: some View {
        VStack(spacing: 0) {
            Hairline()
            HStack(spacing: Theme.Space.m) {
                Text(title).font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textPrimary)
                if total > 0 {
                    ProgressView(value: Double(min(done, total)), total: Double(max(total, 1)))
                        .progressViewStyle(.linear)
                        .controlSize(.small)
                        .frame(width: 160)
                    Text("\(done) / \(total)" + detail).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                } else {
                    ProgressView().controlSize(.small)
                }
                Text(current).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                    .lineLimit(1).truncationMode(.middle)
                Spacer(minLength: Theme.Space.s)
                trailing
            }
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.progressBar)
        }
        .background(Theme.panel)
        .tint(Theme.accent)
    }
}

struct EmptyStateContent<Actions: View>: View {
    let symbol: String
    let title: String
    let message: String
    @ViewBuilder var actions: Actions

    var body: some View {
        VStack(spacing: Theme.Space.m) {
            Image(systemName: symbol).font(Theme.Fonts.iconLarge).foregroundStyle(Theme.textTertiary)
            Text(title).font(Theme.Fonts.headline).foregroundStyle(Theme.textPrimary)
            Text(message).font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary).multilineTextAlignment(.center)
            HStack(spacing: Theme.Space.s) { actions }.padding(.top, Theme.Space.s)
        }
        .padding(Theme.Space.xxl)
    }
}
