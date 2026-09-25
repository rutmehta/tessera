import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// Single-line field for the saved-search grammar. The engine's diagnostic range is underlined
/// in red (and tinted) live while typing, using temporary layout attributes on the field
/// editor so the text itself is never rewritten under the cursor.
struct RuleTextField: NSViewRepresentable {
    @Binding var text: String
    var diagnostic: RuleDiagnostic?
    var placeholder: String
    var onSubmit: () -> Void = {}
    /// Borderless, for embedding in a `FieldContainer` (the filter bar's search field).
    var plain = false
    /// Monospaced grammar text (the smart-album rule).
    var monospaced = false

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: RuleTextField
        init(_ parent: RuleTextField) { self.parent = parent }

        func controlTextDidChange(_ note: Notification) {
            guard let field = note.object as? NSTextField else { return }
            parent.text = field.stringValue
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            if selector == #selector(NSResponder.insertNewline(_:)) {
                parent.onSubmit()
                return true
            }
            if selector == #selector(NSResponder.cancelOperation(_:)) {
                control.window?.makeFirstResponder(nil)
                return true
            }
            return false
        }

        func controlTextDidEndEditing(_ note: Notification) {
            guard let field = note.object as? NSTextField else { return }
            RuleTextField.highlight(field, text: field.stringValue, diagnostic: parent.diagnostic)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField(string: text)
        field.delegate = context.coordinator
        field.placeholderString = placeholder
        field.font = monospaced ? Theme.NSFonts.labelMono : Theme.NSFonts.label
        if plain {
            field.isBordered = false
            field.isBezeled = false
            field.drawsBackground = false
            field.focusRingType = .none
        } else {
            field.bezelStyle = .roundedBezel
            field.isBordered = true
            field.focusRingType = .exterior
        }
        field.cell?.isScrollable = true
        field.cell?.wraps = false
        field.lineBreakMode = .byClipping
        field.allowsEditingTextAttributes = false
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setAccessibilityIdentifier("ruleTextField")
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        let editor = field.currentEditor() as? NSTextView
        if editor == nil, field.stringValue != text { field.stringValue = text }
        else if let editor, editor.string != text {
            // Programmatic change (tree edit, Clear) while the field is focused.
            editor.string = text
        }
        field.placeholderString = placeholder
        Self.highlight(field, text: text, diagnostic: diagnostic)
        field.toolTip = diagnostic?.message
    }

    static func highlight(_ field: NSTextField, text: String, diagnostic: RuleDiagnostic?) {
        let marks: [NSAttributedString.Key: Any] = [
            .underlineStyle: NSUnderlineStyle.thick.rawValue | NSUnderlineStyle.patternDot.rawValue,
            .underlineColor: Theme.Palette.reject,
            .backgroundColor: Theme.Palette.reject.withAlphaComponent(0.22),
        ]
        var range = diagnostic.map { $0.nsRange(in: text) }
        let length = (text as NSString).length
        // A position (e.g. "expected expression" at the end) marks the preceding character.
        if var r = range, r.length == 0, length > 0 {
            r.location = max(0, min(r.location, length) - 1)
            r.length = 1
            range = r
        }
        if let editor = field.currentEditor() as? NSTextView, let lm = editor.layoutManager {
            let whole = NSRange(location: 0, length: (editor.string as NSString).length)
            for key in marks.keys { lm.removeTemporaryAttribute(key, forCharacterRange: whole) }
            if let r = range, NSMaxRange(r) <= whole.length { lm.addTemporaryAttributes(marks, forCharacterRange: r) }
            return
        }
        let attributed = NSMutableAttributedString(string: text, attributes: [
            .font: field.font ?? Theme.NSFonts.label,
            .foregroundColor: Theme.Palette.textPrimary,
        ])
        if let r = range, NSMaxRange(r) <= length { attributed.addAttributes(marks, range: r) }
        if field.attributedStringValue != attributed { field.attributedStringValue = attributed }
    }
}

/// The diagnostic message under a rule field (red), or a hint.
struct RuleMessage: View {
    var diagnostic: RuleDiagnostic?
    var hint: String

    var body: some View {
        if let d = diagnostic {
            StatusLine(text: d.message, kind: .error)
                .lineLimit(2)
                .accessibilityIdentifier("ruleDiagnostic")
        } else {
            Text(hint).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).lineLimit(1)
        }
    }
}
