import AppKit
import SwiftUI

/// The toast (UX-15 item 20 put it above the agent row; UX-16 item 28 makes it wrap). A short
/// message hugs its text; a long one takes `Theme.Width.toast` and wraps to at most
/// `ToastLayout.maxLines` lines, never clipped at either end. What does not fit two lines, and
/// the detail lines (export failures), sit behind a Details button.
struct ToastView: View {
    let model: AppModel
    let toast: Toast
    @State private var detailsShown = false

    var body: some View {
        let plan = ToastLayout.plan(for: toast)
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(alignment: plan.wraps ? .top : .center, spacing: Theme.Space.m) {
                Text(toast.message)
                    .font(Theme.Fonts.label)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(ToastLayout.maxLines)
                    .truncationMode(.tail)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: plan.wraps ? .infinity : nil, alignment: .leading)
                    .frame(minHeight: Theme.Height.regular)
                    .shellProbe("toastMessage")
                    .accessibilityIdentifier("toast.message")
                if toast.undoable {
                    Button { model.undo() } label: {
                        HStack(spacing: Theme.Space.xs) {
                            Text(ToastLayout.undoTitle).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.accent)
                            Text(ToastLayout.undoKey).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                        }
                    }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                    .accessibilityIdentifier("toast.undo")
                }
                if plan.hasDetails {
                    Button(detailsShown ? ToastLayout.hideTitle : ToastLayout.detailsTitle) { detailsShown.toggle() }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                        .help(plan.messageOverflows ? "The whole message and its details" : "What went wrong, line by line")
                        .accessibilityIdentifier("toast.details")
                    Button(ToastLayout.dismissTitle) { model.toast = nil }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                        .accessibilityIdentifier("toast.dismiss")
                }
            }
            if detailsShown {
                if plan.messageOverflows {
                    Text(toast.message)
                        .font(Theme.Fonts.label)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("toast.fullMessage")
                }
                ForEach(Array(toast.details.prefix(ToastLayout.maxDetailLines).enumerated()), id: \.offset) { _, line in
                    StatusLine(text: line, kind: .error)
                        .lineLimit(ToastLayout.maxLines)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if toast.details.count > ToastLayout.maxDetailLines {
                    Text("and \(toast.details.count - ToastLayout.maxDetailLines) more")
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                }
            }
        }
        .padding(.leading, Theme.Space.l)
        .padding(.trailing, Theme.Space.xs)
        .padding(.vertical, Theme.Space.xs)
        .frame(width: plan.wraps ? Theme.Width.toast : nil)
        .frame(minHeight: Theme.Height.toast)
        .background(HUDBackground())
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("toast")
    }
}

/// Decides how a toast lays out before SwiftUI measures it, with the same font the view uses, so
/// a short message hugs and a long one takes the capped width (UX-16 item 28).
@MainActor
enum ToastLayout {
    static let maxLines = 2
    static let maxDetailLines = 5
    static let undoTitle = "Undo"
    static let undoKey = "⌘Z"
    static let detailsTitle = "Details"
    static let hideTitle = "Hide"
    static let dismissTitle = "Dismiss"

    struct Plan: Equatable {
        /// The toast takes `Theme.Width.toast` and the message wraps.
        var wraps: Bool
        /// The message needs more than `maxLines` at that width: Details shows it whole.
        var messageOverflows: Bool
        /// Details and Dismiss are offered (detail lines, or an overflowing message).
        var hasDetails: Bool
    }

    static func plan(for toast: Toast) -> Plan {
        plan(message: toast.message, undoable: toast.undoable, detailLines: toast.details.count)
    }

    static func plan(message: String, undoable: Bool, detailLines: Int) -> Plan {
        // First with the buttons the toast is known to need, then once more if the overflow adds
        // the Details pair.
        var hasDetails = detailLines > 0
        var plan = measure(message, chrome: chrome(undo: undoable, details: hasDetails))
        if plan.messageOverflows, !hasDetails {
            hasDetails = true
            plan = measure(message, chrome: chrome(undo: undoable, details: true))
        }
        plan.hasDetails = hasDetails
        return plan
    }

    /// Width beside the message: the paddings and the buttons with their spacing.
    static func chrome(undo: Bool, details: Bool) -> CGFloat {
        var width = Theme.Space.l + Theme.Space.xs
        let pad = 2 * (Theme.Space.m - Theme.Space.xxs)   // the borderless button's horizontal padding
        if undo {
            width += Theme.Space.m + pad + textWidth(undoTitle, Theme.NSFonts.labelMedium) + Theme.Space.xs + textWidth(undoKey, Theme.NSFonts.caption)
        }
        if details {
            width += Theme.Space.m + pad + textWidth(hideTitle, Theme.NSFonts.label)
            width += Theme.Space.m + pad + textWidth(dismissTitle, Theme.NSFonts.label)
        }
        return width
    }

    /// The message's width when it does not wrap.
    static func textWidth(_ text: String, _ font: NSFont = Theme.NSFonts.label) -> CGFloat {
        ceil((text as NSString).size(withAttributes: [.font: font]).width)
    }

    /// Lines the message takes at `width`.
    static func lineCount(_ text: String, width: CGFloat, font: NSFont = Theme.NSFonts.label) -> Int {
        guard width > 0 else { return Int.max }
        let rect = (text as NSString).boundingRect(with: CGSize(width: width, height: .greatestFiniteMagnitude),
                                                   options: [.usesLineFragmentOrigin, .usesFontLeading],
                                                   attributes: [.font: font])
        let line = max(1, font.ascender - font.descender + font.leading)
        return max(1, Int((rect.height / line).rounded()))
    }

    private static func measure(_ message: String, chrome: CGFloat) -> Plan {
        let available = Theme.Width.toast - chrome
        guard textWidth(message) > available else { return Plan(wraps: false, messageOverflows: false, hasDetails: false) }
        return Plan(wraps: true, messageOverflows: lineCount(message, width: available) > maxLines, hasDetails: false)
    }
}
