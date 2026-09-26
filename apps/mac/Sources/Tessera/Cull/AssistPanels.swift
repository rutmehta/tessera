import AppKit
import SwiftUI
import TesseraCore

/// Inspector ▸ Assist: the focused frame's keep prediction and why, its suggestion (confirm /
/// dismiss), and the learner's label count. Explanations are additive log-odds terms of the
/// library's learner, not causal claims.
struct AssistPanel: View {
    let model: AppModel
    private var assist: AssistController { model.assist }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(spacing: Theme.Space.s) {
                Toggle("Assist", isOn: Binding(get: { assist.enabled }, set: { assist.setEnabled($0) }))
                    .toggleStyle(.checkbox)
                    .font(Theme.Fonts.caption)
                    .disabled(!model.isEngineBacked)
                    .accessibilityIdentifier("assist-panel-toggle")
                Spacer(minLength: Theme.Space.xs)
                SegmentedPicker(selection: Binding(get: { model.agent.preferences.assistAutomated },
                                                   set: { assist.setAutomated($0) }), segments: [
                    .init(value: false, title: "Assisted", help: "Predictions and a confidence order; nothing pre-filled"),
                    .init(value: true, title: "Automated", help: "Pre-filled decisions outside the thresholds, to confirm with Y"),
                ], height: Theme.Height.small, fill: false)
                .fixedSize()
            }
            if assist.enabled, let item = model.focusedItem {
                if let p = assist.predictions[item.id] {
                    HStack(spacing: Theme.Space.xs) {
                        Text("Keep likelihood").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        Spacer()
                        Text("\(Int((p.pKeep * 100).rounded())) %").font(Theme.Fonts.captionNumeric)
                            .foregroundStyle(Theme.textPrimary)
                            .accessibilityIdentifier("assist-pkeep")
                    }
                    ProgressView(value: p.pKeep).progressViewStyle(.linear).controlSize(.small).tint(Theme.textSecondary)
                    if !p.explanation.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                            ForEach(Array(p.explanation.prefix(4).enumerated()), id: \.offset) { _, term in
                                HStack(spacing: Theme.Space.xs) {
                                    Image(systemName: term.contribution >= 0 ? "arrow.up.right" : "arrow.down.right")
                                        .font(Theme.Fonts.iconSmall)
                                        .foregroundStyle(term.contribution >= 0 ? Theme.keep : Theme.reject)
                                    Text(term.feature.replacingOccurrences(of: "_", with: " ").capitalizedSentence)
                                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary)
                                    Spacer()
                                    Text(String(format: "%+.2f", term.contribution)).font(Theme.Fonts.captionNumeric)
                                        .foregroundStyle(Theme.textSecondary)
                                }
                            }
                        }
                        .accessibilityIdentifier("assist-explanation")
                    }
                    if let s = assist.suggestion(for: item.id) {
                        HStack(spacing: Theme.Space.xs) {
                            Chip(text: s == .keep ? "Keep?" : "Reject?",
                                 color: s == .keep ? Theme.keep : Theme.reject, style: .outlined)
                            Spacer()
                            Button("Dismiss") { assist.dismiss([item.id]) }
                                .help("Reject this suggestion (N); nothing is decided")
                                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                            Button("Confirm All") { assist.confirmAll() }
                                .help("Confirm every suggested decision in view as one undo step (Y)")
                                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                                .accessibilityIdentifier("assist-confirm-all")
                        }
                    }
                } else {
                    Hint("No prediction for this photo yet.")
                }
                Hint("\(assist.suggestionCount) suggested · learner has \(assist.labels) confirmed label\(assist.labels == 1 ? "" : "s"). Keep and reject decisions teach it.")
                    .accessibilityIdentifier("assist-status")
            } else {
                Hint(model.isEngineBacked
                     ? "Assist predicts keepers from sharpness, blur, exposure, faces and your past decisions, and sorts likely rejects last."
                     : "Assist needs a folder opened on the engine.")
            }
        }
    }
}

/// Inspector ▸ People: identities from face descriptors (this shoot), with per-person filters.
struct PeoplePanel: View {
    let model: AppModel
    private var assist: AssistController { model.assist }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if let title = assist.personFilterTitle {
                HStack(spacing: Theme.Space.xs) {
                    Chip(text: "Showing \(title)", color: Theme.accent, style: .outlined)
                    Spacer()
                    Button("Show All") { assist.clearPersonFilter() }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                        .accessibilityIdentifier("people-clear-filter")
                }
            }
            if assist.people.isEmpty {
                Hint("No faces analysed yet. Cull ▸ Analyze Faces finds faces and groups them into people (the models download once).")
            }
            ForEach(assist.people) { person in
                HStack(spacing: Theme.Space.s) {
                    Text(model.people.person(person.id)?.displayName ?? person.name).font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary)
                    Text("\(person.items.count) frame\(person.items.count == 1 ? "" : "s")")
                        .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                    Spacer()
                    Button("Frames") { assist.filter(person: person, eyesClosed: false) }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                        .help("Show frames with \(person.name)")
                    Button("Eyes closed") { assist.filter(person: person, eyesClosed: true) }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                        .help("Show frames where \(person.name)'s eyes read closed (a geometric proxy)")
                        .accessibilityIdentifier("people-eyes-closed-\(person.id)")
                }
                .frame(height: Theme.Height.regular)
            }
            Button("Analyze Faces") { assist.analyze(faces: true, force: false, title: "Finding faces") }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(!model.isEngineBacked || assist.isRunning)
        }
    }
}

/// Background analysis progress above the status bar.
struct AssistProgressBar: View {
    let assist: AssistController
    var body: some View {
        if let p = assist.progress {
            ProgressStrip(title: p.title, done: p.done, total: p.total, current: p.current) {
                Button("Stop") { assist.cancel() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("analysis-progress")
        }
    }
}

extension String {
    /// "motion blur" → "Motion blur".
    var capitalizedSentence: String { prefix(1).uppercased() + dropFirst() }
}
