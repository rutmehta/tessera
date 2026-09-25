import AppKit
import SwiftUI
import TesseraCore

/// Defect sweep (docs/06 §3): adjustable thresholds, a reviewable candidate list with
/// checkboxes, and one undoable "Reject" on confirm. Nothing changes until Apply.
struct DefectSweepSheet: View {
    let model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var rules = DefectRule.defaults
    @State private var findings: [DefectFinding] = []
    @State private var excluded: Set<Int> = []

    private var chosen: [Int] { findings.map(\.item).filter { !excluded.contains($0) } }

    var body: some View {
        SheetScaffold(title: "Defect Sweep",
                      subtitle: "Frames whose AI signals cross these thresholds. Nothing is rejected until you apply.") {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: 0) {
                VStack(spacing: Theme.Space.xs) {
                    ForEach($rules) { $rule in
                        ThresholdRow(rule: $rule)
                    }
                }
                .padding(.horizontal, Theme.Space.l)
                .padding(.vertical, Theme.Space.m)
                Hairline()
                HStack(spacing: Theme.Space.xs) {
                    Text("\(findings.count) candidate\(findings.count == 1 ? "" : "s") · \(chosen.count) selected")
                        .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                        .accessibilityIdentifier("defect-count")
                    Spacer()
                    Button("All") { excluded = [] }.buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    Button("None") { excluded = Set(findings.map(\.item)) }.buttonStyle(.theme(.borderless, height: Theme.Height.small))
                }
                .padding(.horizontal, Theme.Space.l)
                .frame(height: Theme.Height.large + Theme.Space.xs)
                if findings.isEmpty {
                    EmptyStateContent(symbol: "checkmark.seal",
                                      title: "Nothing to reject",
                                      message: model.isEngineBacked
                                        ? "No frames cross the enabled thresholds.\nAI scores appear here once they have been computed for this folder."
                                        : "The stub library has no AI scores.") { EmptyView() }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    List(findings) { finding in
                        FindingRow(model: model, finding: finding, included: Binding(
                            get: { !excluded.contains(finding.item) },
                            set: { if $0 { excluded.remove(finding.item) } else { excluded.insert(finding.item) } }))
                    }
                    .listStyle(.plain)
                    .scrollContentBackground(.hidden)
                }
            }
        } leading: {
            Text("Undo with ⌘Z after applying.")
        } actions: {
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
            Button(chosen.isEmpty ? "Reject" : "Reject \(chosen.count) Frame\(chosen.count == 1 ? "" : "s")") {
                let ids = chosen
                dismiss()
                model.applyDefectSweep(ids)
            }
            .keyboardShortcut(.defaultAction)
            .sheetButton(primary: true)
            .disabled(chosen.isEmpty)
        }
        .frame(width: 560, height: 520)
        .onAppear(perform: refresh)
        .onChange(of: rules) { refresh() }
    }

    private func refresh() {
        findings = model.defectFindings(rules)
        excluded.formIntersection(findings.map(\.item))
    }
}

private struct ThresholdRow: View {
    @Binding var rule: DefectRule
    var body: some View {
        HStack(spacing: Theme.Space.m) {
            Toggle(rule.title, isOn: $rule.enabled)
                .toggleStyle(.checkbox)
                .font(Theme.Fonts.label)
                .frame(width: 150, alignment: .leading)
            Text(rule.below ? "below" : "above").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .frame(width: 40, alignment: .leading)
            Slider(value: $rule.threshold, in: 0...1)
                .controlSize(.small)
                .disabled(!rule.enabled)
            Text(String(format: "%.2f", rule.threshold))
                .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textPrimary).frame(width: 34, alignment: .trailing)
        }
        .frame(height: Theme.Height.large)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(rule.title) threshold")
    }
}

private struct FindingRow: View {
    let model: AppModel
    let finding: DefectFinding
    @Binding var included: Bool
    @State private var thumbnail: CGImage?

    var body: some View {
        let item = model.item(id: finding.item)
        let state = model.state(id: finding.item)
        HStack(spacing: Theme.Space.m) {
            Toggle("", isOn: $included).toggleStyle(.checkbox).labelsHidden()
            Group {
                if let thumbnail {
                    Image(decorative: thumbnail, scale: 1).resizable().aspectRatio(contentMode: .fit)
                } else {
                    Theme.well
                }
            }
            .frame(width: 56, height: 40)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(item.name).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary).lineLimit(1)
                Text(finding.reasons.joined(separator: " · ")).font(Theme.Fonts.captionNumeric)
                    .foregroundStyle(Theme.textSecondary).lineLimit(1)
            }
            Spacer()
            if let badge = state.badgeText {
                Chip(text: badge, color: Color(nsColor: state.decision.color), style: .outlined)
            }
        }
        .padding(.vertical, Theme.Space.xxs)
        .contentShape(Rectangle())
        .onTapGesture { included.toggle() }
        .onAppear {
            thumbnail = model.loader.cached(item, tier: .thumbnail)
            if thumbnail == nil {
                _ = model.loader.request(item, tier: .thumbnail, priority: .normal) { image in thumbnail = image }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(item.name), \(finding.reasons.joined(separator: ", ")), \(included ? "selected" : "not selected")")
    }
}
