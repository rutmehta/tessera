import AppKit
import SwiftUI

/// Native buttons expose real AX press actions and participate in the key-view loop.
/// They edit the same requested height as the drag handle; layout alone never writes it.
struct DocumentHistoryHeightControls: NSViewRepresentable {
    @Binding var requested: Double
    let column: CGFloat

    func makeNSView(context: Context) -> DocumentHistoryHeightControl {
        let view = DocumentHistoryHeightControl(frame: .zero)
        updateNSView(view, context: context)
        return view
    }

    func updateNSView(_ view: DocumentHistoryHeightControl, context: Context) {
        view.configure(requested: requested, column: column) { requested = $0 }
    }

    static func dismantleNSView(_ view: DocumentHistoryHeightControl, coordinator: ()) {
        view.onChange = nil
    }
}

@MainActor
final class DocumentHistoryHeightControl: NSStackView {
    let decrease = HistoryHeightButton(title: "−", target: nil, action: nil)
    let increase = HistoryHeightButton(title: "+", target: nil, action: nil)
    let reset = HistoryHeightButton(title: "↺", target: nil, action: nil)
    let readout = NSTextField(labelWithString: "")
    var onChange: ((Double) -> Void)?
    private var requested: Double = 0
    private var column: CGFloat = 0

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        orientation = .horizontal
        alignment = .centerY
        spacing = Theme.Space.xxs
        setAccessibilityElement(false) // Keep the native actionable children exposed.
        for (button, name, action) in [
            (decrease, "Decrease History height", #selector(decreaseHeight)),
            (increase, "Increase History height", #selector(increaseHeight)),
            (reset, "Reset History height", #selector(resetHeight))
        ] {
            button.target = self
            button.action = action
            button.bezelStyle = .inline
            button.controlSize = .small
            button.setAccessibilityLabel(name)
            button.toolTip = name
            button.widthAnchor.constraint(equalToConstant: 22).isActive = true
        }
        decrease.setAccessibilityIdentifier("document.history.height.decrease")
        increase.setAccessibilityIdentifier("document.history.height.increase")
        reset.setAccessibilityIdentifier("document.history.height.reset")
        readout.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        readout.alignment = .center
        readout.setAccessibilityLabel("History height")
        readout.setAccessibilityIdentifier("document.history.height.value")
        readout.widthAnchor.constraint(equalToConstant: 48).isActive = true
        let controls: [NSView] = [decrease, readout, increase, reset]
        controls.forEach { addArrangedSubview($0) }
    }

    required init?(coder: NSCoder) { fatalError() }

    func configure(requested: Double, column: CGFloat, onChange: @escaping (Double) -> Void) {
        self.requested = requested
        self.column = column
        self.onChange = onChange
        refresh()
    }

    private var height: CGFloat {
        DocumentInspector.budget.historyHeight(requested: CGFloat(requested), column: column)
    }

    private func refresh() {
        let value = height
        let spoken = String(format: "%.0f points", Double(value))
        readout.stringValue = String(format: "%.0f pt", Double(value))
        readout.setAccessibilityValue(spoken)
        decrease.isEnabled = DocumentInspector.budget.historyHeight(requested: value - Theme.Height.row, column: column) < value
        increase.isEnabled = DocumentInspector.budget.historyHeight(requested: value + Theme.Height.row, column: column) > value
        reset.isEnabled = requested != Double(DocumentInspector.historyDefault)
        for button in [decrease, increase, reset] { button.setAccessibilityValue(spoken) }
    }

    private func commit(_ next: Double) {
        guard let onChange else { return }
        requested = next
        refresh()
        onChange(next)
        NSAccessibility.post(element: readout, notification: .valueChanged)
    }

    @objc private func decreaseHeight() { commit(Double(DocumentInspector.budget.historyHeight(requested: height - Theme.Height.row, column: column))) }
    @objc private func increaseHeight() { commit(Double(DocumentInspector.budget.historyHeight(requested: height + Theme.Height.row, column: column))) }
    // Like pointer double-click, reset stores the default request even in a short column.
    @objc private func resetHeight() { commit(Double(DocumentInspector.historyDefault)) }
}

@MainActor
final class HistoryHeightButton: NSButton, KeyOwningControl {
    override var acceptsFirstResponder: Bool { isEnabled }

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if isEnabled, modifiers.intersection([.command, .control, .option]).isEmpty,
           [UInt16(49), 36, 76].contains(event.keyCode) {
            if !event.isARepeat { performClick(nil) }
            return
        }
        super.keyDown(with: event)
    }
}
