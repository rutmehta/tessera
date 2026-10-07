import SwiftUI

/// Loupe badge for a Lightroom Smart Preview whose original is offline. Lists every
/// setting the proxy render omits; the list persists while the photo is open and is
/// independent of the shared one-line status message.
struct SmartPreviewLoupeBadge: View {
    let notices: [String]

    static let badgeLabel = "Smart Preview, original offline"

    static func noticeText(_ notices: [String]) -> String { notices.joined(separator: "\n") }

    static func accessibilityLabel(_ notices: [String]) -> String {
        notices.isEmpty ? badgeLabel : ([badgeLabel] + notices).joined(separator: ". ")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            // Not a static-text element: SwiftUI reports a static text's label as
            // its AXValue, and the badge's accessible name must carry the notices.
            Text("Smart Preview").font(Theme.Fonts.caption)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Self.accessibilityLabel(notices))
                .accessibilityIdentifier("loupe.smart-preview-badge")
            if !notices.isEmpty {
                Text(Self.noticeText(notices)).font(Theme.Fonts.caption)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityElement(children: .ignore)
                    .accessibilityAddTraits(.isStaticText)
                    .accessibilityLabel("Settings not rendered")
                    .accessibilityValue(Self.noticeText(notices))
                    .accessibilityIdentifier("loupe.smart-preview-notice")
            }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: 360, alignment: .leading)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: Theme.Radius.chip))
    }
}
