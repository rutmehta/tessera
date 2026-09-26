import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// Develop ▸ Auto Edit… (⇧⌘A, docs/10): who plans, which photos, batch consistency and
/// guardrails. The run is non-modal (progress strip with Cancel) and ends in the review queue,
/// least confident first. Every agent step is an ordinary recipe step; nothing is generated.
struct AutoEditSheet: View {
    @Bindable var agent: AgentController
    let model: AppModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        SheetScaffold(title: "Auto Edit",
                      subtitle: "A base edit with the engine's own controls. Every step is undoable and editable by hand.") {
            EmptyView()
        } content: {
            Form {
                Section("Planner") {
                    Picker("Provider", selection: $agent.provider) {
                        ForEach(agent.providers) { Text($0.title).tag($0) }
                    }
                    .accessibilityIdentifier("autoedit-provider")
                    providerDetail
                }
                Section("Photos") {
                    SegmentedPicker(selection: $agent.scope, segments: AgentController.Scope.allCases.map { s in
                        .init(value: s, title: "\(agent.title(s)) (\(agent.count(s)))")
                    }, fill: false)
                    .accessibilityIdentifier("autoedit-scope")
                    Toggle("Same scene or burst: one tone and white balance", isOn: $agent.preferences.sceneConsistency)
                    Toggle("Same person: one exposure and skin treatment", isOn: $agent.preferences.personConsistency)
                }
                Section("Guardrails") {
                    Toggle("Allow masks (subject, sky, people)", isOn: $agent.preferences.allowMasks)
                    Toggle("Allow crop and straighten", isOn: $agent.preferences.allowCrop)
                    Toggle("Allow skin retouch", isOn: $agent.preferences.allowSkinRetouch)
                        .disabled(true)
                        .help("Retouch stays off until the engine has a reversible retouch operator")
                    Toggle("Visual critic (the model also judges the rendered preview)", isOn: $agent.preferences.visualCritic)
                        .disabled(agent.provider == .styleProfile)
                    Stepper("Plan and critique up to \(agent.preferences.maxIterations) time\(agent.preferences.maxIterations == 1 ? "" : "s") per photo",
                            value: $agent.preferences.maxIterations, in: 1...10)
                    Stepper("Time budget \(agent.preferences.timeBudgetSeconds) s per photo",
                            value: $agent.preferences.timeBudgetSeconds, in: 30...600, step: 30)
                    Hint("Never: generated pixels, body reshaping, or edits outside these photos.")
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            .onChange(of: agent.preferences) { agent.savePreferences() }
        } leading: {
            if let blocker = agent.blocker {
                StatusLine(text: blocker, kind: .warning).accessibilityIdentifier("autoedit-blocker")
            } else if let error = agent.error {
                StatusLine(text: error, kind: .error)
            } else {
                Text(profileLine)
            }
        } actions: {
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
            Button("Edit \(agent.count(agent.scope)) Photo\(agent.count(agent.scope) == 1 ? "" : "s")") {
                let ids = agent.itemIDs(for: agent.scope)
                dismiss()
                agent.start(itemIDs: ids)
            }
            .keyboardShortcut(.defaultAction)
            .sheetButton(primary: true)
            .disabled(agent.blocker != nil)
            .accessibilityIdentifier("autoedit-start")
        }
        .frame(width: 560, height: 600)
    }

    @ViewBuilder private var providerDetail: some View {
        switch agent.provider {
        case .styleProfile:
            Hint("Predicts a base recipe from your style profile. No network, no key.")
        case .anthropic, .openAI:
            TextField("Model", text: agent.provider == .anthropic ? $agent.preferences.anthropicModel : $agent.preferences.openAIModel)
            HStack(spacing: Theme.Space.s) {
                if let status = agent.keyStatus(agent.provider) {
                    StatusLine(text: status, kind: .success)
                } else {
                    StatusLine(text: "No API key", kind: .warning)
                }
                Spacer()
                Button("Settings ▸ AI…") { openSettings() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            }
            Hint("A low-resolution preview and measurements are sent to \(agent.provider.title); the photo stays on this Mac.")
        case .ollama:
            TextField("Host", text: $agent.preferences.ollamaHost)
            TextField("Model", text: $agent.preferences.ollamaModel)
            Toggle("Model accepts images", isOn: $agent.preferences.ollamaVision)
        case .scripted:
            Hint("Test planner: a fixed three-step script through the engine's FakePlanner (no network, no key).")
        }
    }

    private var profileLine: String {
        guard let p = agent.profile else { return "Style profile: not loaded" }
        return p.samples == 0 ? "Style profile: questionnaire only (Settings ▸ AI)"
            : "Style profile: learned from \(p.samples) edit\(p.samples == 1 ? "" : "s")"
    }
}

/// Agent run progress above the status bar.
struct AgentProgressBar: View {
    let agent: AgentController
    var body: some View {
        if let p = agent.progress {
            ProgressStrip(title: agent.runningTitle, done: Int(p.done), total: Int(p.total),
                          detail: p.phase.isEmpty ? "" : " · \(p.phase)", current: p.current) {
                Button("Cancel") { agent.cancel() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("autoedit-cancel")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("autoedit-progress")
        }
    }
}
