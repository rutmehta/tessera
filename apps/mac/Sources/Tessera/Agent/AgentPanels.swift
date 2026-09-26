import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

// MARK: - History ▸ Agent base edit

/// The History panel's agent groups (docs/10 §2 "fine-tune surface"): one Amount slider that
/// fades the whole group (0–100 %), per-step toggles with each step's rationale, and a
/// "Redo with instruction…" field. Manual edits made after the group are kept.
struct AgentGroupSection: View {
    let model: AppModel
    let tools: DevelopTools
    let group: HistoryGroupState
    @State private var instruction = ""
    @State private var redoOpen = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            HStack(spacing: Theme.Space.xs) {
                Chip(text: "AI", color: Theme.accent, style: .outlined, height: Theme.Height.chip)
                Text(group.name).font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textPrimary).lineLimit(1)
                Spacer()
                Text(AgentFade.percent(group.amount)).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                    .accessibilityIdentifier("agent-group-amount-readout")
            }
            .frame(height: Theme.Height.regular)
            AmountSlider(group: group, tools: tools)
                .frame(height: Theme.Height.slider)
                .accessibilityIdentifier("agent-group-amount")
            ForEach(group.steps, id: \.self) { id in
                if let item = tools.historyItems.first(where: { $0.id == id }) {
                    AgentStepRow(item: item, tools: tools)
                }
            }
            if redoOpen {
                HStack(spacing: Theme.Space.xs) {
                    TextField("e.g. “warmer, keep the sky”", text: $instruction)
                        .textFieldStyle(.roundedBorder)
                        .controlSize(.small)
                        .onSubmit(redo)
                        .accessibilityIdentifier("agent-group-instruction")
                    Button("Redo", action: redo)
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .disabled(instruction.trimmingCharacters(in: .whitespaces).isEmpty || model.agent.isRunning)
                }
                Hint("Redo changes only the controls the instruction names (warmth, tint, exposure, contrast) as a new group.")
            } else {
                Button("Redo with Instruction…") { redoOpen = true }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .disabled(model.agent.isRunning)
                    .accessibilityIdentifier("agent-group-redo")
            }
        }
        .padding(.bottom, Theme.Space.s)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("agent-group")
    }

    private func redo() {
        guard let item = model.focusedItem else { return }
        model.agent.redo([item.id], instruction: instruction)
        instruction = ""
        redoOpen = false
    }
}

private struct AgentStepRow: View {
    let item: HistoryItem
    let tools: DevelopTools

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.s - Theme.Space.xxs) {
            Toggle("", isOn: Binding(get: { item.enabled }, set: { tools.setStep(item, enabled: $0) }))
                .toggleStyle(.checkbox)
                .labelsHidden()
                .controlSize(.mini)
                .frame(width: Theme.Space.l)
                .disabled(!item.applied)
                .help(item.enabled ? "Turn this step off (recorded as a new step)" : "Turn this step back on")
                .accessibilityIdentifier("agent-step-toggle-\(item.id)")
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(item.label)
                    .font(Theme.Fonts.caption)
                    .strikethrough(!item.enabled)
                    .foregroundStyle(item.applied ? Theme.textPrimary : Theme.textTertiary)
                if let why = item.rationale, !why.isEmpty {
                    Text(why).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("agent-step-rationale")
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, Theme.Space.xxs)
    }
}

/// The group's Amount slider (a `ValueSlider`, like every develop control): previews while
/// dragging, records one step on release.
private struct AmountSlider: NSViewRepresentable {
    let group: HistoryGroupState
    let tools: DevelopTools

    final class Coordinator { var group: HistoryGroupState?; weak var tools: DevelopTools? }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        s.title = "Amount"
        s.minValue = 0
        s.maxValue = 100
        s.defaultValue = 100
        s.valueFormat = "%.0f %%"
        s.step = 1
        let c = context.coordinator
        s.onChange = { value, isFinal in
            MainActor.assumeIsolated {
                guard let g = c.group, let tools = c.tools else { return }
                tools.setGroupAmount(g, value / 100, final: isFinal)
            }
        }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        context.coordinator.group = group
        context.coordinator.tools = tools
        if !s.isDragging { s.doubleValue = group.amount * 100 }
        s.needsDisplay = true
    }
}

// MARK: - Inspector ▸ Agent Edit

/// The focused photo's agent edit: provenance, confidence, steps with rationales, and the review
/// queue's accept / revert / redo (JPEGs included; Develop is not required).
struct AgentEditPanel: View {
    let model: AppModel
    @State private var provenance: AgentProvenance?
    @State private var instruction = ""

    var body: some View {
        let key = "\(model.focusedItem?.id ?? -1)|\(model.agentRevision)|\(model.developHistory?.entries ?? 0)|\(model.agent.queue.summary)"
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if let p = provenance, let item = model.focusedItem {
                let entry = AgentReviewEntry(p.item, itemID: item.id)
                HStack(spacing: Theme.Space.xs) {
                    Chip(text: p.provenance, color: Theme.accent, style: .outlined)
                        .accessibilityIdentifier("agent-provenance")
                    Spacer()
                }
                InfoRow(label: "Confidence", value: entry.confidenceText)
                InfoRow(label: "Review", value: entry.status.title)
                InfoRow(label: "Stopped", value: p.item.stopReason.isEmpty ? "—" : p.item.stopReason)
                if p.runs > 1 { InfoRow(label: "Runs", value: "\(p.runs) (base edit and redos)") }
                ForEach(entry.steps) { step in
                    VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                        Text(step.title).font(Theme.Fonts.caption).foregroundStyle(step.enabled ? Theme.textPrimary : Theme.textTertiary)
                            .strikethrough(!step.enabled)
                        Text(step.rationale).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                HStack(spacing: Theme.Space.xs) {
                    Button("Accept") { model.agent.accept(entry) }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .disabled(entry.status == .accepted)
                    Button("Revert") { model.agent.revert(entry) }
                        .buttonStyle(.theme(.destructive, height: Theme.Height.small))
                        .disabled(entry.status == .reverted || entry.groupID == nil)
                }
                HStack(spacing: Theme.Space.xs) {
                    TextField("Redo with instruction…", text: $instruction)
                        .textFieldStyle(.roundedBorder)
                        .controlSize(.small)
                        .onSubmit { redo(item.id) }
                    Button("Redo") { redo(item.id) }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .disabled(instruction.trimmingCharacters(in: .whitespaces).isEmpty || model.agent.isRunning)
                }
            } else {
                Hint(model.isEngineBacked ? "No agent edit on this photo. Develop ▸ Auto Edit… (⇧⌘A) makes one."
                     : "Auto Edit needs a folder opened on the engine.")
                Button("Auto Edit…") { model.agent.present() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .disabled(!model.isEngineBacked || model.agent.isRunning)
            }
        }
        .task(id: key) { load() }
    }

    private func load() {
        guard let item = model.focusedItem, let ref = item.engineImage else { provenance = nil; return }
        provenance = (try? ref.engine.agentProvenance(imageId: ref.imageID)) ?? nil
    }

    private func redo(_ id: Int) {
        model.agent.redo([id], instruction: instruction)
        instruction = ""
    }
}
