import AppKit
import TesseraCore
import SwiftUI

/// Right inspector: SwiftUI panels. The develop sliders inside are AppKit `ValueSlider`s.
struct InspectorView: View {
    let model: AppModel
    private var tools: DevelopTools { .shared }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                PanelSection("Histogram") { HistogramPanel(model: model).frame(height: HistogramView.height) }
                PanelSection("Image") { ImageInfoPanel(model: model) }
                PanelSection("Selection") { SelectionPanel(model: model) }
                if model.isEngineBacked {
                    PanelSection("Keywords") { KeywordsPanel(model: model, library: model.collections) }
                    PanelSection("Metadata") { MetadataPanel(model: model, library: model.collections) }
                }
                PanelSection("Basic") { BasicPanel(model: model) }
                PanelSection("Masks", expanded: false) { MasksPanel(model: model, masks: .shared) }
                    .developContext(model, tools)
                Group {
                    PanelSection("Tone Curve", expanded: false) { ToneCurvePanel(model: model, tools: tools) }
                    PanelSection("HSL / Color", expanded: false) { HSLPanel(model: model, tools: tools) }
                    PanelSection("Color Grading", expanded: false) { ColorGradingPanel(model: model, tools: tools) }
                    PanelSection("Detail", expanded: false) { DetailPanel(model: model, tools: tools) }
                    PanelSection("Effects", expanded: false) { EffectsPanel(model: model, tools: tools) }
                    PanelSection("Crop & Straighten", expanded: false) { CropPanel(model: model, tools: tools) }
                    PanelSection("Soft Proofing", expanded: false) { SoftProofPanel(proof: .shared) }
                    PanelSection("Presets", expanded: false) { PresetsPanel(model: model, tools: tools) }
                    PanelSection("Snapshots", expanded: false) { SnapshotsPanel(model: model) }
                    PanelSection("History", expanded: false) { HistoryPanel(model: model, tools: tools) }
                }
                .developContext(model, tools)
            }
        }
        .scrollIndicators(.never)
        .background(Theme.panel)
        .tint(Theme.accent)
    }
}

struct ImageInfoPanel: View {
    let model: AppModel
    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if let item = model.focusedItem {
                InfoRow(label: "File", value: item.name)
                InfoRow(label: "Type", value: item.kind.rawValue)
                InfoRow(label: "Size", value: sizeText(item))
                InfoRow(label: "Captured", value: item.captureDate == .distantPast ? "—"
                    : item.captureDate.formatted(.dateTime.year().month().day().hour().minute().second()))
                InfoRow(label: "Group", value: "G\(item.groupID + 1) · frame \(model.indexInGroup(of: item) + 1) of \(model.groupSize(of: item))"
                        + (model.focusedIsBest ? " · suggested best" : ""))
                InfoRow(label: "Status", value: statusText)
            } else {
                Hint("No image selected")
            }
        }
    }
}

extension ImageInfoPanel {
    /// Known from the file, or from the develop session (active area, display orientation).
    private func sizeText(_ item: PhotoItem) -> String {
        if item.pixelWidth > 0 { return "\(item.pixelWidth) × \(item.pixelHeight)" }
        _ = model.developStatus
        guard let d = model.develop, d.itemID == item.id else { return "—" }
        let (w, h) = d.info.orientation >= 5 ? (d.info.height, d.info.width) : (d.info.width, d.info.height)
        return "\(w) × \(h)"
    }

    private var statusText: String {
        let st = model.focusedStatus
        let phase = st.phase.rawValue.capitalized
        return st.albums.isEmpty ? phase : phase + " · in " + st.albums.joined(separator: ", ")
    }
}

struct SelectionPanel: View {
    let model: AppModel
    var body: some View {
        let s = model.focusedState
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(spacing: Theme.Space.xs) {
                DecisionChip(title: "Reject", key: "X", on: s.decision == .reject, color: Theme.Palette.reject) { model.perform(.reject) }
                DecisionChip(title: "Undecided", key: "U", on: s.decision == .undecided, color: Theme.Palette.textSecondary) { model.perform(.undecided) }
                DecisionChip(title: "Keep", key: "P", on: s.decision == .keep, color: Theme.Palette.keep) { model.perform(.keep) }
            }
            HStack(spacing: Theme.Space.xs) {
                ForEach(1...3, id: \.self) { g in
                    DecisionChip(title: CullState.gradeNames[g], key: "\(g)", on: s.grade == g, color: Theme.Palette.keep) {
                        model.perform(.grade(UInt8(g)))
                    }
                }
            }
            HStack(spacing: Theme.Space.xs) {
                ForEach(6...9, id: \.self) { m in
                    DecisionChip(title: "", key: "\(m)", on: s.mark == m, color: MarkStyle.color(UInt8(m)), swatch: true) {
                        model.perform(.mark(UInt8(m)))
                    }
                    .help(MarkStyle.name(UInt8(m)))
                }
                DecisionChip(title: model.basketTarget, key: "B", on: s.inBasket, color: Theme.Palette.basket) { model.perform(.toggleBasket) }
                    .help("Add to / remove from the basket target album")
            }
            if s.mark != 0 {
                Hint("Mark: \(MarkStyle.name(s.mark))")
            }
            if model.selectionCount > 1 {
                Hint("Applies to \(model.selectionCount) selected images")
            }
        }
    }
}

/// A cull action with its key. On: the semantic colour as a 1 px outline, a subtle tint and
/// the title in that colour; off: a neutral bordered control. One height (24), radius 6.
struct DecisionChip: View {
    let title: String
    let key: String
    let on: Bool
    let color: NSColor
    /// Marks show their colour as a swatch even when off.
    var swatch = false
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        let c = Color(nsColor: color)
        Button(action: action) {
            HStack(spacing: Theme.Space.xs + Theme.Space.xxs) {
                if swatch {
                    RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(c).frame(width: Theme.Space.s, height: Theme.Space.s)
                }
                Text(key).font(Theme.Fonts.caption).monospacedDigit()
                    .foregroundStyle(on ? c : Theme.textTertiary)
                if !title.isEmpty {
                    Text(title).font(Theme.Fonts.caption).lineLimit(1)
                        .foregroundStyle(on ? c : Theme.textPrimary)
                }
            }
            .padding(.horizontal, Theme.Space.s)
            .frame(minWidth: Theme.Height.regular, minHeight: Theme.Height.regular, maxHeight: Theme.Height.regular)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.control)
                .fill(on ? c.opacity(0.16) : hovering ? Theme.hover : Theme.raised))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control)
                .strokeBorder(on ? c.opacity(0.7) : Theme.hairline, lineWidth: Theme.Space.hairline))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .accessibilityLabel(title.isEmpty ? "Key \(key)" : "\(title), key \(key)")
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

struct BasicPanel: View {
    let model: AppModel
    var body: some View {
        let id = model.focusedItem?.id
        let ready = model.developStatus == .ready
        let revision = model.developRevision
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: Theme.Space.xs) {
                Button("Reset") { model.resetDevelop() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Reset all develop settings (one undo step)")
                Menu("Snapshots") {
                    Button("New Snapshot…") { model.promptSnapshot() }
                    let names = model.developHistory?.snapshots ?? []
                    if !names.isEmpty { Divider() }
                    ForEach(names, id: \.self) { name in
                        Button(name) { model.restoreSnapshot(name) }
                    }
                }
                .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
                Spacer(minLength: Theme.Space.xs)
                Text(statusText).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).lineLimit(1)
            }
            .disabled(!ready)
            .padding(.bottom, Theme.Space.xs)
            ForEach(BasicKey.sections, id: \.0) { section in
                SubHeader(section.0)
                ForEach(section.1) { key in
                    AdjustmentSlider(model: model, key: key, itemID: ready ? id : nil, revision: revision)
                        .frame(height: Theme.Height.slider)
                }
            }
        }
    }

    private var statusText: String {
        switch model.developStatus {
        case .none: "Open a RAW in the loupe (E)"
        case .loading: "Opening…"
        case .ready: model.developHistory.map { h in
            h.headLabel.map { "History: \($0)" } ?? "Unedited"
        } ?? ""
        case .unavailable(let why): why
        }
    }
}

/// Bridges one `ValueSlider` into SwiftUI. SwiftUI only re-runs `updateNSView` when the focused
/// image or the develop revision (open, undo, reset) changes; drag updates bypass SwiftUI entirely.
struct AdjustmentSlider: NSViewRepresentable {
    let model: AppModel
    let key: BasicKey
    let itemID: Int?
    let revision: Int

    final class Coordinator {
        var itemID: Int?
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        s.title = key.title
        s.minValue = key.range.lowerBound
        s.maxValue = key.range.upperBound
        s.defaultValue = key.defaultValue
        s.valueFormat = key.format
        s.step = key.step
        let coordinator = context.coordinator
        s.onChange = { [weak model] value, isFinal in
            guard let model, let id = coordinator.itemID else { return }
            model.setAdjustment(key, value, final: isFinal, for: id)
        }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        let enabled = itemID != nil && key.parameter != nil
        context.coordinator.itemID = enabled ? itemID : nil
        s.isEnabled = enabled
        s.defaultValue = model.defaultValue(key)
        s.doubleValue = itemID.map { model.adjustment(key, for: $0) } ?? model.defaultValue(key)
        s.needsDisplay = true
    }
}
