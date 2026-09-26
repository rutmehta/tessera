import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// Develop ▸ Agent Review… (docs/10 §2 "Batch mode"): the agent's base edits, least confident
/// first, each with its rationale and Accept (teaches the style profile), Redo with an
/// instruction, or Revert (the agent group at 0 %, one undoable step). "Show" closes the queue
/// on that photo in the loupe.
struct AgentReviewSheet: View {
    let agent: AgentController
    let model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var redoing: String?
    @State private var instruction = ""

    var body: some View {
        SheetScaffold(title: "Agent Review",
                      subtitle: "\(agent.queue.summary) · planned by \(agent.queue.provider.isEmpty ? "the agent" : agent.queue.provider)") {
            EmptyView()
        } content: {
            if agent.queue.isEmpty {
                EmptyStateContent(symbol: "wand.and.stars", title: "Nothing to review",
                                  message: "Run Develop ▸ Auto Edit… to have the agent make base edits.") { EmptyView() }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(agent.queue.entries) { entry in
                    ReviewRow(agent: agent, model: model, entry: entry, redoing: $redoing, instruction: $instruction) {
                        if let item = entry.itemID {
                            dismiss()
                            model.showInLoupe(item)
                        }
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .accessibilityIdentifier("agent-review-list")
            }
        } leading: {
            Text("Accept teaches your style profile. Revert and redo are ordinary history steps.")
        } actions: {
            Button("Done") { dismiss() }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
        }
        .frame(width: 720, height: 560)
    }
}

private struct ReviewRow: View {
    let agent: AgentController
    let model: AppModel
    let entry: AgentReviewEntry
    @Binding var redoing: String?
    @Binding var instruction: String
    let show: () -> Void
    @State private var thumbnail: CGImage?
    private let thumbWidth: CGFloat = 72

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(alignment: .top, spacing: Theme.Space.m) {
                Group {
                    if let thumbnail { Image(decorative: thumbnail, scale: 1).resizable().aspectRatio(contentMode: .fit) }
                    else { Theme.well }
                }
                .frame(width: thumbWidth, height: 48)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                    HStack(spacing: Theme.Space.xs) {
                        Text(entry.name).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary).lineLimit(1)
                        Chip(text: entry.confidenceText, color: bandColor, style: .outlined, height: Theme.Height.chip)
                            .accessibilityIdentifier("agent-review-confidence")
                        if entry.status != .needsReview {
                            Chip(text: entry.status.title, color: entry.status == .accepted ? Theme.keep : Theme.textSecondary,
                                 style: .outlined, height: Theme.Height.chip)
                        }
                    }
                    Text(entry.summary).font(Theme.Fonts.caption).foregroundStyle(entry.error == nil ? Theme.textSecondary : Theme.reject)
                        .lineLimit(2)
                    if entry.error == nil {
                        Text(stepsLine).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).lineLimit(1)
                    }
                }
                Spacer(minLength: Theme.Space.s)
                actions
            }
            if redoing == entry.imageID {
                HStack(spacing: Theme.Space.xs) {
                    TextField("Redo with instruction, e.g. “warmer, keep the sky”", text: $instruction)
                        .textFieldStyle(.roundedBorder)
                        .controlSize(.small)
                        .onSubmit(redo)
                        .accessibilityIdentifier("agent-review-instruction")
                    Button("Redo", action: redo)
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .disabled(instruction.trimmingCharacters(in: .whitespaces).isEmpty || agent.isRunning)
                    Button("Cancel") { redoing = nil }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                }
                .padding(.leading, thumbWidth + Theme.Space.m)
            }
        }
        .padding(.vertical, Theme.Space.xs)
        .onAppear(perform: loadThumbnail)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("agent-review-row")
    }

    private var bandColor: Color {
        if entry.error != nil { return Theme.reject }
        return entry.confidence < 0.4 ? Theme.warning : entry.confidence < 0.7 ? Theme.textSecondary : Theme.keep
    }

    private var stepsLine: String {
        let n = entry.steps.count
        var seen: [String] = []
        for t in entry.steps.map(\.title) where t != "No change" && !seen.contains(t) { seen.append(t) }
        let titles = seen.joined(separator: " · ")
        return "\(n) step\(n == 1 ? "" : "s")" + (titles.isEmpty ? "" : ": \(titles)")
            + (entry.criticReasons.isEmpty ? "" : " · critic: \(entry.criticReasons.joined(separator: ", "))")
    }

    @ViewBuilder private var actions: some View {
        let busy = agent.busy.contains(entry.imageID)
        HStack(spacing: Theme.Space.xs) {
            if busy { ProgressView().controlSize(.small) }
            Button("Show") { show() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .disabled(entry.itemID == nil)
            Button("Accept") { agent.accept(entry) }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(busy || entry.error != nil || entry.groupID == nil || entry.status == .accepted)
                .accessibilityIdentifier("agent-review-accept")
            Button("Redo…") { instruction = ""; redoing = entry.imageID }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(busy || entry.itemID == nil || agent.isRunning)
                .accessibilityIdentifier("agent-review-redo")
            Button("Revert") { agent.revert(entry) }
                .buttonStyle(.theme(.destructive, height: Theme.Height.small))
                .disabled(busy || entry.groupID == nil || entry.status == .reverted)
                .accessibilityIdentifier("agent-review-revert")
        }
    }

    private func redo() {
        guard let item = entry.itemID else { return }
        agent.redo([item], instruction: instruction)
        redoing = nil
    }

    private func loadThumbnail() {
        guard let id = entry.itemID else { return }
        let item = model.item(id: id)
        thumbnail = model.loader.cached(item, tier: .thumbnail)
        if thumbnail == nil {
            _ = model.loader.request(item, tier: .thumbnail, priority: .normal) { image in thumbnail = image }
        }
    }
}
