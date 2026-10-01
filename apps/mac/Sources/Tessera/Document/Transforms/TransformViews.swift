import AppKit
import SwiftUI
import TesseraCore

/// The options bar while an advanced transform is open (WP B5-12), after the tool icon and name: the
/// operation's controls (compact native fields with 11 pt captions, neutral `SegmentedPicker`s,
/// `MenuPicker` pop-ups, borderless actions), the interpolation pop-up, a limitations help glyph, then
/// borderless Cancel and bordered Apply (DESIGN.md §10). Errors and hints go to the status bar.
struct TransformOptionsBar: View {
    let document: DocumentController
    @Bindable var t: DocumentTransforms

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Height.small) }

    var body: some View {
        if let s = t.session {
            // B5-12b: the uncommitted edit is a transient state here; Layers / Properties keep the original.
            if let p = t.previewLabel {
                Text(p).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
                    .help("Not applied yet: Return or Apply records it, Esc discards it")
                    .accessibilityIdentifier("document.transform.previewState")
            }
            switch s.op {
            case .warp: warp(s)
            case .perspective: perspective
            case .puppet(let p): puppet(p)
            case .contentAwareScale(let c): scale(c)
            }
            if s.op.tag != .contentAwareScale {
                MenuPicker(selection: $t.kernel, options: TransformKernel.allCases.map { ($0, $0.title) })
                    .frame(width: Theme.Width.labelWide + Theme.Space.l)
                    .help("Interpolation")
                    .accessibilityLabel("Interpolation")
                    .accessibilityIdentifier("document.transform.interpolation")
            }
            Image(systemName: "info.circle")
                .font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                .help(s.start.limitations.joined(separator: "\n"))
                .accessibilityLabel("Limitations")
            // B5-12b: why the last edit was refused (also in the status bar).
            if let r = t.refusal {
                HStack(spacing: Theme.Space.xs) {
                    Image(systemName: "exclamationmark.triangle").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.warning)
                    Text(r).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary).lineLimit(1).truncationMode(.tail)
                }
                .frame(maxWidth: Theme.Width.labelWide * 3, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
                .help(r)
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("document.transform.refusal")
            }
            if let r = t.latencyReadout {
                Text(r).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary).fixedSize()
                    .accessibilityIdentifier("document.transform.latency")
            }
            separator
            Button("Reset") { t.resetOperation() }
                .accessibilityIdentifier("document.transform.reset")
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            Button("Cancel") { t.cancel() }
                .accessibilityIdentifier("document.transform.cancel")
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .help("Esc")
            Button(s.start.needsConversion ? "Apply…" : "Apply") { t.apply() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .help(s.start.needsConversion ? "Return · converts the layer to a smart object" : "Return")
                .accessibilityIdentifier("document.transform.apply")
        } else {
            Text("Opening…").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
        }
    }

    private var presets: [(value: String, title: String)] {
        [("Custom", "Custom")] + TransformBridge.presetNames().map { ($0, WarpPresetInfo.title($0)) }
    }

    @ViewBuilder private func warp(_ s: DocumentTransforms.Session) -> some View {
        MenuPicker(selection: Binding(get: { t.warpPreset }, set: { t.applyWarpPreset($0) }), options: presets)
            .frame(width: Theme.Width.labelWide + Theme.Space.l)
            .help("Warp preset")
            .accessibilityIdentifier("document.transform.warpPreset")
            .background(TransformFrameProbe(id: "document.transform.warpPreset"))
        TransformField(title: "Bend", value: t.warpBend, unit: "%", identifier: "document.transform.bend") { t.setWarpBend($0) }
            .disabled(t.warpPreset == "Custom")
        Menu {
            Button("Default (1 × 1)") { t.update { if case .warp = $0 { $0 = .warp(.identity(width: Double(s.start.childWidth), height: Double(s.start.childHeight))) } } }
            ForEach([UInt32(3), 4, 5], id: \.self) { n in Button("\(n) × \(n)") { t.warpGrid(n) } }
        } label: { Label("Grid", systemImage: "square.grid.3x3") }
            .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
            .fixedSize()
            .help("Grid: add splits (the warp does not move)")
            .accessibilityIdentifier("document.transform.grid")
            .accessibilityLabel("Warp grid")
        SegmentedPicker(selection: $t.warpSplit, segments: [
            .init(value: .none, title: "", symbol: "hand.point.up.left", help: "Drag points and handles"),
            .init(value: .vertical, title: "", symbol: "rectangle.split.2x1", help: "Split vertically: click the net"),
            .init(value: .horizontal, title: "", symbol: "rectangle.split.1x2", help: "Split horizontally: click the net"),
            .init(value: .cross, title: "", symbol: "rectangle.split.2x2", help: "Split crosswise: click the net (⇧ keeps splitting)"),
        ], height: Theme.Height.small, fill: false, accessibilityPrefix: "document.transform.warpSplit")
        .fixedSize()
    }

    @ViewBuilder private var perspective: some View {
        SegmentedPicker(selection: $t.perspectiveLayout, segments: [
            .init(value: true, title: "Layout", symbol: nil, help: "Fit the planes to the image"),
            .init(value: false, title: "Warp", symbol: nil, help: "Move the planes"),
        ], height: Theme.Height.small, fill: false, accessibilityPrefix: "document.transform.perspectiveMode")
        .fixedSize()
        .onChange(of: t.perspectiveLayout) { _, _ in t.redraw() }
        Button("Split Vertically") { t.splitPerspective(vertical: true) }
            .accessibilityIdentifier("document.transform.splitVertical")
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            .help("Add a linked plane: the planes share their new edge")
        Button("Split Horizontally") { t.splitPerspective(vertical: false) }
            .accessibilityIdentifier("document.transform.splitHorizontal")
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
    }

    @ViewBuilder private func puppet(_ p: PuppetModel) -> some View {
        MenuPicker(selection: Binding(get: { p.mode == "Rigid" }, set: { t.setPuppetRigid($0) }),
                   options: [(false, "Normal"), (true, "Rigid")])
            .frame(width: Theme.Width.labelWide)
            .help("Mode")
            .accessibilityIdentifier("document.transform.puppetMode")
            .background(TransformFrameProbe(id: "document.transform.puppetMode"))
        SegmentedPicker(selection: Binding(get: { t.puppetDensity }, set: { t.puppetDensity = $0; t.remesh() }),
                        segments: PuppetDensityTag.allCases.map { .init(value: $0, title: $0.title, symbol: nil, help: "Density: \($0.title)") },
                        height: Theme.Height.small, fill: false, accessibilityPrefix: "document.transform.puppetDensity")
            .fixedSize()
        // B5-12b: typed values go to the engine (0…64 px); a refusal keeps the rejected value on show.
        TransformField(title: "Expansion", value: t.puppetExpansionShown, unit: "px", rejected: t.expansionRejected != nil,
                       identifier: "document.transform.expansion") { t.setPuppetExpansion($0) }
        OptionToggle(title: "Show Mesh", on: Binding(get: { t.showMesh }, set: { t.showMesh = $0; t.redraw() }), identifier: "document.transform.showMesh")
        TransformField(title: "Rotate", value: t.selectedPinDegrees ?? 0, unit: "°", fractionDigits: 1,
                       identifier: "document.transform.rotate") { t.setSelectedPinDegrees(min(max($0, -360), 360)) }
            .disabled(t.selectedPin == nil)
        Text("\(p.pins.count) pin\(p.pins.count == 1 ? "" : "s")").font(Theme.Fonts.captionNumeric)
            .foregroundStyle(Theme.textTertiary).fixedSize()
        if let note = t.puppetNote {
            Image(systemName: "exclamationmark.triangle").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.warning)
                .help(note)
        }
    }

    @ViewBuilder private func scale(_ c: ContentAwareScaleModel) -> some View {
        TransformField(title: "W", value: Double(c.width), unit: "px", identifier: "document.transform.width") {
            t.setScale(width: UInt32(min(max($0, 1), Double(c.canvasWidth * 4))))
        }
        TransformField(title: "H", value: Double(c.height), unit: "px", identifier: "document.transform.height") {
            t.setScale(height: UInt32(min(max($0, 1), Double(c.canvasHeight * 4))))
        }
        TransformField(title: "Amount", value: Double(c.amount) * 100, unit: "%", identifier: "document.transform.amount") {
            t.setScale(amount: Float($0 / 100))
        }
        MenuPicker(selection: Binding(get: { c.protectChannel }, set: { t.setScale(protect: .some($0)) }),
                   options: [(UInt64?.none, "Protect: None")] + t.channels.map { (UInt64?.some($0.id), "Protect: \($0.name)") })
            .frame(width: Theme.Width.labelWide + Theme.Space.xxl)
            .help("Protect a saved alpha channel (there is no automatic skin detection)")
            .accessibilityIdentifier("document.transform.protectChannel")
            .accessibilityLabel("Protect channel")
    }
}

/// Edit ▸ Transform additions (WP B5-12).
struct AdvancedTransformMenuItems: View {
    let doc: DocumentController?
    private var t: DocumentTransforms { DocumentTransforms.shared }

    var body: some View {
        let on = t.canBegin(doc)
        Divider()
        Button("Content-Aware Scale") { t.begin(.contentAwareScale) }
            .keyboardShortcut("c", modifiers: [.command, .option, .shift])
            .disabled(!on)
        Button("Puppet Warp") { t.begin(.puppet) }.disabled(!on)
        Button("Perspective Warp") { t.begin(.perspective) }.disabled(!on)
        Button("Warp") { t.begin(.warp) }.disabled(!on)
    }
}

/// The Apply alert: converting a pixel / text / shape layer to a smart object is explicit.
struct TransformSheets: ViewModifier {
    @Bindable var t: DocumentTransforms

    func body(content: Content) -> some View {
        content.alert("Convert to Smart Object?", isPresented: $t.consentPending) {
            Button("Cancel", role: .cancel) {}
            Button("Convert and Apply") { t.confirmConversion() }
        } message: {
            Text("\(t.session?.op.tag.title ?? "The transform") is kept as an editable stage of a smart object. The layer's pixels, "
                 + "text or shape stay inside it unchanged, with its masks and style on the smart object. Undo restores the layer.")
        }
    }
}

/// B5-12b: the options bar's numeric field while a transform session is open (the shared `OptionField`
/// swallowed Esc and Return, and clamped through its formatter). Caption, value, unit like `OptionField`.
/// Keys while editing: Return commits the value, ends editing and applies the session (unless the value
/// was refused); Esc with an edited value reverts it and ends editing (a second Esc, now on the canvas,
/// cancels the session); Esc with the value unchanged cancels the session at once.
struct TransformField: View {
    let title: String
    let value: Double
    var unit = ""
    var fractionDigits = 0
    /// The value on show was refused (drawn in the reject colour until corrected).
    var rejected = false
    let identifier: String
    let commit: (Double) -> Void

    var body: some View {
        HStack(spacing: Theme.Space.xs) {
            Text(title).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            TransformFieldCell(value: value, fractionDigits: fractionDigits, rejected: rejected, identifier: identifier,
                               title: title, commit: commit)
                .frame(width: Theme.Width.label - Theme.Space.l)
            if !unit.isEmpty { Text(unit).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary) }
        }
        .fixedSize()
    }
}

private struct TransformFieldCell: NSViewRepresentable {
    let value: Double
    let fractionDigits: Int
    let rejected: Bool
    let identifier: String
    let title: String
    let commit: (Double) -> Void
    @Environment(\.isEnabled) private var isEnabled

    func makeNSView(context: Context) -> TransformNumberField {
        let f = TransformNumberField()
        f.setDocumentAccessibility(identifier: identifier, label: title)
        f.onReturn = { DocumentTransforms.shared.applyAfterFieldCommit() }
        f.onEscape = { DocumentTransforms.shared.cancel() }
        f.focusAfterEditing = { DocumentTransforms.shared.document?.viewport }
        return f
    }

    func updateNSView(_ f: TransformNumberField, context: Context) {
        f.onCommit = commit
        f.fractionDigits = fractionDigits
        f.isEnabled = isEnabled
        f.rejected = rejected
        f.value = value
    }
}

/// The AppKit field behind `TransformField` (its own delegate, so Esc / Return are handled while the field
/// editor has the keyboard — the key router leaves text fields alone).
@MainActor
final class TransformNumberField: NSTextField, NSTextFieldDelegate {
    /// The accepted value (shown whenever the field is not being edited).
    var value: Double = 0 { didSet { if !isEditingText { show() } } }
    var fractionDigits = 0 { didSet { if oldValue != fractionDigits, !isEditingText { show() } } }
    /// The value on show was refused.
    var rejected = false {
        didSet { textColor = rejected ? Theme.Palette.reject : Theme.Palette.textPrimary }
    }
    var onCommit: ((Double) -> Void)?
    var onReturn: (() -> Void)?
    var onEscape: (() -> Void)?
    /// Where the keyboard goes when editing ends (the canvas, so the next Esc / Return reach the session).
    var focusAfterEditing: (() -> NSResponder?)?
    private var ending = false

    override init(frame: NSRect) {
        super.init(frame: frame)
        bezelStyle = .roundedBezel
        isBezeled = true
        controlSize = .small
        font = Theme.NSFonts.captionNumeric
        alignment = .right
        textColor = Theme.Palette.textPrimary
        usesSingleLineMode = true
        cell?.isScrollable = true
        delegate = self
        show()
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    var isEditingText: Bool { currentEditor() != nil }
    /// The text as typed (the field editor's while editing).
    var text: String { currentEditor()?.string ?? stringValue }

    private var numberFormat: NumberFormatter {
        let f = NumberFormatter()
        f.numberStyle = .decimal
        f.minimumFractionDigits = 0
        f.maximumFractionDigits = fractionDigits
        f.usesGroupingSeparator = false
        return f
    }

    func formatted(_ v: Double) -> String { numberFormat.string(from: NSNumber(value: v)) ?? "\(v)" }

    /// The typed number (no range: the session or the engine decides and says why it refuses). Never NaN
    /// or infinite ("nan", "inf", "1e999"): those are not values, and would trap in `UInt32(_:)` (W / H)
    /// or poison Rotate / Bend.
    func parsed() -> Double? {
        let s = text.trimmingCharacters(in: .whitespaces)
        let v = numberFormat.number(from: s)?.doubleValue ?? Double(s.replacingOccurrences(of: ",", with: "."))
        guard let v, v.isFinite else { return nil }
        return v
    }

    private func show() { stringValue = formatted(value) }

    /// Commits the typed value when it differs from what is on show.
    private func commitText() {
        guard let v = parsed() else { show(); return }
        if text != formatted(value) || v != value { onCommit?(v) }
    }

    private func endEditing() {
        ending = true
        if let w = window { w.makeFirstResponder(focusAfterEditing?() ?? nil) }
        ending = false
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)):
            commitText()
            endEditing()
            onReturn?()
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            let edited = text != formatted(value)
            textView.string = formatted(value)
            endEditing()
            show()
            if !edited { onEscape?() }
            return true
        default:
            return false
        }
    }

    func controlTextDidEndEditing(_ obj: Notification) {
        guard !ending else { return }
        commitText()
    }
}

/// B5-12b: an inert AppKit view behind a SwiftUI control, so the self-test can read where the control
/// sits in the window (is it inside the visible options bar, not under the toolbar or scrolled off).
struct TransformFrameProbe: NSViewRepresentable {
    let id: String
    func makeNSView(context: Context) -> TransformProbeView {
        let v = TransformProbeView()
        v.identifier = NSUserInterfaceItemIdentifier(id)
        return v
    }
    func updateNSView(_ v: TransformProbeView, context: Context) {}
}

final class TransformProbeView: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func isAccessibilityElement() -> Bool { false }
}
