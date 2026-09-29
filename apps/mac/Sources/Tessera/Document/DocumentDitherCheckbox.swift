import AppKit
import SwiftUI

/// Dither only: native responder ownership without changing the other adjustment toggles.
struct DocumentDitherCheckbox: NSViewRepresentable {
    @Binding var isOn: Bool
    @Environment(\.isEnabled) private var enabled

    func makeNSView(context: Context) -> DocumentDitherNativeCheckbox {
        let view = DocumentDitherNativeCheckbox(frame: .zero)
        updateNSView(view, context: context)
        return view
    }

    func updateNSView(_ view: DocumentDitherNativeCheckbox, context: Context) {
        view.configure(isOn: isOn, enabled: enabled) { value in isOn = value }
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: DocumentDitherNativeCheckbox,
                      context: Context) -> CGSize? {
        let ideal = nsView.intrinsicContentSize
        return CGSize(width: max(0, min(proposal.width ?? ideal.width, ideal.width)), height: ideal.height)
    }

    static func dismantleNSView(_ view: DocumentDitherNativeCheckbox, coordinator: ()) {
        view.onChange = nil
    }
}

@MainActor
final class DocumentDitherNativeCheckbox: NSButton, KeyOwningControl {
    var onChange: ((Bool) -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setButtonType(.switch)
        allowsMixedState = false
        title = "Dither"
        font = Theme.NSFonts.caption
        controlSize = .small
        keyEquivalent = ""
        target = self
        action = #selector(valueChanged)
        // The view is ignored by AX (single-cell control); the cell is the exposed checkbox.
        // Its label comes from the title: an explicit view label would blank the cell's.
        setAccessibilityIdentifier("document.properties.colorLookup.dither")
        cell?.setAccessibilityIdentifier("document.properties.colorLookup.dither")
        toolTip = "Add fine noise so smooth gradients do not band"
        setContentHuggingPriority(.defaultHigh, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError() }

    func configure(isOn: Bool, enabled: Bool, onChange: @escaping (Bool) -> Void) {
        self.onChange = onChange
        state = isOn ? .on : .off
        isEnabled = enabled
    }

    override var acceptsFirstResponder: Bool { isEnabled && super.acceptsFirstResponder }

    static func isToggleKey(_ code: UInt16, modifiers: NSEvent.ModifierFlags) -> Bool {
        code == 49 && modifiers.intersection([.command, .control, .option]).isEmpty
    }

    override func keyDown(with event: NSEvent) {
        if isEnabled, Self.isToggleKey(event.keyCode, modifiers: event.modifierFlags) {
            if !event.isARepeat { performClick(nil) }
            return
        }
        // Tab / Shift-Tab and Return retain native checkbox behavior, not action-button semantics.
        super.keyDown(with: event)
    }

    @objc private func valueChanged() {
        guard isEnabled else { return }
        onChange?(state == .on)
    }
}
