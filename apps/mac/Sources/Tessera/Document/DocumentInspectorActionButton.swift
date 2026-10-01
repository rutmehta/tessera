import AppKit
import SwiftUI

/// Narrow native actions for Color Lookup. Actual first-responder ownership uses
/// the existing KeyOwningControl contract, not a SwiftUI proxy class or AX guess.
struct DocumentInspectorActionButton: NSViewRepresentable {
    let title: String
    let identifier: String
    let help: String
    let action: () -> Void
    @Environment(\.isEnabled) private var enabled

    func makeNSView(context: Context) -> DocumentInspectorNativeActionButton {
        let view = DocumentInspectorNativeActionButton(frame: .zero)
        updateNSView(view, context: context)
        return view
    }

    func updateNSView(_ view: DocumentInspectorNativeActionButton, context: Context) {
        view.configure(title: title, identifier: identifier, help: help, enabled: enabled, action: action)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: DocumentInspectorNativeActionButton,
                      context: Context) -> CGSize? {
        let ideal = nsView.intrinsicContentSize
        return CGSize(width: max(0, min(proposal.width ?? ideal.width, ideal.width)), height: Theme.Height.small)
    }

    static func dismantleNSView(_ view: DocumentInspectorNativeActionButton, coordinator: ()) {
        view.onPress = nil
    }
}

@MainActor
final class DocumentInspectorNativeActionButton: NSButton, KeyOwningControl {
    var onPress: (() -> Void)?
    private var tracking: NSTrackingArea?
    private var hovering = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setButtonType(.momentaryPushIn)
        target = self
        action = #selector(invokeAction)
        keyEquivalent = ""
        font = Theme.NSFonts.caption
        focusRingType = .exterior
        setContentHuggingPriority(.defaultHigh, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError() }

    func configure(title: String, identifier: String, help: String, enabled: Bool,
                   action: @escaping () -> Void) {
        self.title = title
        setDocumentAccessibility(identifier: identifier, label: title)
        toolTip = help
        onPress = action
        isEnabled = enabled
        if !enabled { hovering = false }
        alphaValue = enabled ? 1 : CGFloat(Theme.Opacity.disabled)
        invalidateIntrinsicContentSize()
        needsDisplay = true
    }

    // Retain NSButton's native keyboard-navigation policy; never force focus or
    // manually wire nextKeyView. Disabled actions cannot become first responder.
    override var acceptsFirstResponder: Bool { isEnabled && super.acceptsFirstResponder }

    override var intrinsicContentSize: NSSize {
        let width = (title as NSString).size(withAttributes: [.font: Theme.NSFonts.caption]).width
        return NSSize(width: ceil(width) + 2 * Theme.Space.s, height: Theme.Height.small)
    }

    static func isActivationKey(_ code: UInt16, modifiers: NSEvent.ModifierFlags) -> Bool {
        modifiers.intersection([.command, .control, .option]).isEmpty && [UInt16(49), 36, 76].contains(code)
    }

    override func keyDown(with event: NSEvent) {
        if isEnabled, Self.isActivationKey(event.keyCode, modifiers: event.modifierFlags) {
            if !event.isARepeat { performClick(nil) }
            return
        }
        // In particular, Tab and Shift-Tab stay in AppKit's traversal path.
        super.keyDown(with: event)
    }

    @objc private func invokeAction() {
        guard isEnabled else { return }
        onPress?()
    }

    override func updateTrackingAreas() {
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero,
            options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
        super.updateTrackingAreas()
    }

    override func mouseEntered(with event: NSEvent) { hovering = isEnabled; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovering = false; needsDisplay = true }

    /// Same bordered/small tokens as ThemeButtonBody. NSButton retains hit testing,
    /// target/action, AX press, key traversal and native pressed-state tracking.
    override func draw(_ dirtyRect: NSRect) {
        let fill = cell?.isHighlighted == true ? Theme.Palette.pressed
            : hovering ? Theme.Palette.hover : Theme.Palette.raised
        fill.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: Theme.Radius.control, yRadius: Theme.Radius.control).fill()
        let inset = Theme.Space.hairline / 2
        let outline = NSBezierPath(roundedRect: bounds.insetBy(dx: inset, dy: inset),
            xRadius: max(0, Theme.Radius.control - inset), yRadius: max(0, Theme.Radius.control - inset))
        Theme.Palette.hairlineStrong.setStroke()
        outline.lineWidth = Theme.Space.hairline
        outline.stroke()
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = .center
        paragraph.lineBreakMode = .byTruncatingTail
        let attributes: [NSAttributedString.Key: Any] = [
            .font: Theme.NSFonts.caption, .foregroundColor: Theme.Palette.textPrimary, .paragraphStyle: paragraph
        ]
        let text = title as NSString
        let height = text.size(withAttributes: attributes).height
        text.draw(in: NSRect(x: Theme.Space.s, y: (bounds.height - height) / 2,
                            width: max(0, bounds.width - 2 * Theme.Space.s), height: height),
                  withAttributes: attributes)
    }

    override var focusRingMaskBounds: NSRect { bounds }
    override func drawFocusRingMask() {
        Theme.Palette.textPrimary.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: Theme.Radius.control, yRadius: Theme.Radius.control).fill()
    }
}
