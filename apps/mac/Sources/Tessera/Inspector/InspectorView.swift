import AppKit
import TesseraCore
import SwiftUI

/// Right inspector: SwiftUI panels. The develop sliders inside are AppKit `ValueSlider`s.
struct InspectorView: View {
    let model: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                PanelSection("Histogram") { HistogramPanel(model: model).frame(height: 86) }
                PanelSection("Image") { ImageInfoPanel(model: model) }
                PanelSection("Selection") { SelectionPanel(model: model) }
                PanelSection("Basic") { BasicPanel(model: model) }
            }
        }
        .scrollIndicators(.never)
        .background(Color(nsColor: Theme.windowBackground))
    }
}

struct PanelSection<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content
    @State private var expanded = true

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                expanded.toggle()
            } label: {
                HStack {
                    Text(title.uppercased())
                        .font(.system(size: 10, weight: .semibold))
                        .tracking(0.8)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text(expanded ? "–" : "+")
                        .font(.system(size: 12, weight: .regular, design: .monospaced))
                        .foregroundStyle(.tertiary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            if expanded {
                content
                    .padding(.horizontal, 14)
                    .padding(.bottom, 12)
            }
            Divider().opacity(0.5)
        }
    }
}

private struct InfoRow: View {
    let label: String
    let value: String
    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label).foregroundStyle(.secondary).frame(width: 74, alignment: .leading)
            Text(value).foregroundStyle(.primary).lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .font(.system(size: 11))
    }
}

struct ImageInfoPanel: View {
    let model: AppModel
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
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
                Text("No image selected").font(.system(size: 11)).foregroundStyle(.secondary)
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
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 4) {
                DecisionChip(title: "Reject", key: "X", on: s.decision == .reject, color: Theme.reject) { model.perform(.reject) }
                DecisionChip(title: "Undecided", key: "U", on: s.decision == .undecided, color: Theme.textSecondary) { model.perform(.undecided) }
                DecisionChip(title: "Keep", key: "P", on: s.decision == .keep, color: Theme.keep) { model.perform(.keep) }
            }
            HStack(spacing: 4) {
                ForEach(1...3, id: \.self) { g in
                    DecisionChip(title: CullState.gradeNames[g], key: "\(g)", on: s.grade == g, color: Theme.keep) {
                        model.perform(.grade(UInt8(g)))
                    }
                }
            }
            HStack(spacing: 4) {
                ForEach(6...9, id: \.self) { m in
                    DecisionChip(title: "", key: "\(m)", on: s.mark == m, color: MarkStyle.color(UInt8(m))) {
                        model.perform(.mark(UInt8(m)))
                    }
                    .help(MarkStyle.name(UInt8(m)))
                }
                DecisionChip(title: model.basketTarget, key: "B", on: s.inBasket, color: Theme.basket) { model.perform(.toggleBasket) }
                    .help("Add to / remove from the basket target album")
            }
            if s.mark != 0 {
                Text("Mark: \(MarkStyle.name(s.mark))").font(.system(size: 11)).foregroundStyle(.secondary)
            }
            if model.selectionCount > 1 {
                Text("Applies to \(model.selectionCount) selected images").font(.system(size: 11)).foregroundStyle(.secondary)
            }
        }
    }
}

struct DecisionChip: View {
    let title: String
    let key: String
    let on: Bool
    let color: NSColor
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 4) {
                Text(key).font(.system(size: 10, weight: .semibold, design: .monospaced))
                    .foregroundStyle(on ? Color.black.opacity(0.75) : .secondary)
                if !title.isEmpty {
                    Text(title).font(.system(size: 11))
                        .foregroundStyle(on ? Color.black.opacity(0.85) : .primary)
                }
            }
            .padding(.horizontal, 7)
            .frame(minWidth: 24, minHeight: 22)
            .background(RoundedRectangle(cornerRadius: 4).fill(on ? Color(nsColor: color) : Color.white.opacity(0.06)))
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Color(nsColor: color).opacity(on ? 0 : 0.35), lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

struct BasicPanel: View {
    let model: AppModel
    var body: some View {
        let id = model.focusedItem?.id
        let ready = model.developStatus == .ready
        let revision = model.developRevision
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Button("Reset") { model.resetDevelop() }
                    .help("Reset all develop settings (one undo step)")
                Menu("Snapshots") {
                    Button("New Snapshot…") { model.promptSnapshot() }
                    let names = model.developHistory?.snapshots ?? []
                    if !names.isEmpty { Divider() }
                    ForEach(names, id: \.self) { name in
                        Button(name) { model.restoreSnapshot(name) }
                    }
                }
                .fixedSize()
                Spacer()
                Text(statusText).font(.system(size: 10)).foregroundStyle(.tertiary).lineLimit(1)
            }
            .controlSize(.small)
            .disabled(!ready)
            ForEach(BasicKey.sections, id: \.0) { section in
                Text(section.0).font(.system(size: 10, weight: .medium)).foregroundStyle(.tertiary).padding(.top, 4)
                ForEach(section.1) { key in
                    AdjustmentSlider(model: model, key: key, itemID: ready ? id : nil, revision: revision)
                        .frame(height: 30)
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
