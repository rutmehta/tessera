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
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Defect Sweep").font(.system(size: 15, weight: .semibold))
                Text("Frames whose AI signals cross these thresholds. Review the list; nothing is rejected until you apply.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
            .padding(16)
            Divider()
            VStack(spacing: 6) {
                ForEach($rules) { $rule in
                    ThresholdRow(rule: $rule)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            Divider()
            HStack {
                Text("\(findings.count) candidate\(findings.count == 1 ? "" : "s") · \(chosen.count) selected")
                    .font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
                    .accessibilityIdentifier("defect-count")
                Spacer()
                Button("All") { excluded = [] }.buttonStyle(.link).font(.system(size: 11))
                Button("None") { excluded = Set(findings.map(\.item)) }.buttonStyle(.link).font(.system(size: 11))
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            if findings.isEmpty {
                Text(model.isEngineBacked
                     ? "No frames cross the enabled thresholds.\nAI scores appear here once they have been computed for this folder."
                     : "The stub library has no AI scores.")
                    .font(.system(size: 12)).foregroundStyle(.secondary).multilineTextAlignment(.center)
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
            Divider()
            HStack {
                Text("Undo with ⌘Z after applying.").font(.system(size: 11)).foregroundStyle(.tertiary)
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(chosen.isEmpty ? "Reject" : "Reject \(chosen.count) Frame\(chosen.count == 1 ? "" : "s")") {
                    let ids = chosen
                    dismiss()
                    model.applyDefectSweep(ids)
                }
                .keyboardShortcut(.defaultAction)
                .disabled(chosen.isEmpty)
            }
            .padding(12)
        }
        .frame(width: 560, height: 520)
        .background(Color(nsColor: Theme.windowBackground))
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
        HStack(spacing: 10) {
            Toggle(rule.title, isOn: $rule.enabled)
                .toggleStyle(.checkbox)
                .font(.system(size: 12))
                .frame(width: 150, alignment: .leading)
            Text(rule.below ? "below" : "above").font(.system(size: 11)).foregroundStyle(.secondary)
                .frame(width: 40, alignment: .leading)
            Slider(value: $rule.threshold, in: 0...1)
                .controlSize(.small)
                .disabled(!rule.enabled)
            Text(String(format: "%.2f", rule.threshold))
                .font(.system(size: 11).monospacedDigit()).frame(width: 34, alignment: .trailing)
        }
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
        HStack(spacing: 10) {
            Toggle("", isOn: $included).toggleStyle(.checkbox).labelsHidden()
            Group {
                if let thumbnail {
                    Image(decorative: thumbnail, scale: 1).resizable().aspectRatio(contentMode: .fit)
                } else {
                    Color(nsColor: Theme.cellBackground)
                }
            }
            .frame(width: 54, height: 38)
            .clipShape(RoundedRectangle(cornerRadius: 2))
            VStack(alignment: .leading, spacing: 2) {
                Text(item.name).font(.system(size: 12, weight: .medium)).lineLimit(1)
                Text(finding.reasons.joined(separator: " · ")).font(.system(size: 11).monospacedDigit())
                    .foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            if let badge = state.badgeText {
                Text(badge).font(.system(size: 9.5, weight: .bold))
                    .foregroundStyle(Color(nsColor: state.decision.color))
            }
        }
        .padding(.vertical, 2)
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
