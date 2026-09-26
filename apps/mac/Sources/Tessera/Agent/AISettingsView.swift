import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// Settings ▸ AI (⌘,): planner providers with API keys stored in the login Keychain (never in
/// preferences, never logged), default models, Ollama, auto-edit guardrails, assisted-culling
/// thresholds and the style profile (questionnaire, learning from your edits).
struct AISettingsView: View {
    @Bindable var agent: AgentController
    let model: AppModel

    var body: some View {
        Form {
            Section("Planner") {
                Picker("Default provider", selection: $agent.preferences.provider) {
                    ForEach(AIProviderKind.visible) { Text($0.title).tag($0) }
                }
                .accessibilityIdentifier("ai-default-provider")
                Hint("Style profile only runs on this Mac. Anthropic and OpenAI receive a low-resolution preview and measurements; Ollama runs locally.")
            }
            KeySection(agent: agent, kind: .anthropic, model: $agent.preferences.anthropicModel)
            KeySection(agent: agent, kind: .openAI, model: $agent.preferences.openAIModel)
            Section("Ollama") {
                TextField("Host", text: $agent.preferences.ollamaHost)
                TextField("Model", text: $agent.preferences.ollamaModel)
                Toggle("Model accepts images", isOn: $agent.preferences.ollamaVision)
            }
            Section("Auto edit guardrails") {
                Toggle("Allow masks", isOn: $agent.preferences.allowMasks)
                Toggle("Allow crop and straighten", isOn: $agent.preferences.allowCrop)
                Toggle("Same scene or burst: one tone and white balance", isOn: $agent.preferences.sceneConsistency)
                Toggle("Same person: one exposure and skin treatment", isOn: $agent.preferences.personConsistency)
                Stepper("Up to \(agent.preferences.maxIterations) plan–critique rounds", value: $agent.preferences.maxIterations, in: 1...10)
            }
            Section("Assisted culling") {
                Toggle("Suggest decisions to confirm (automated)", isOn: $agent.preferences.assistAutomated)
                LabeledContent("Suggest reject below") {
                    HStack {
                        Slider(value: $agent.preferences.rejectBelow, in: 0...0.49)
                        Text("\(Int((agent.preferences.rejectBelow * 100).rounded())) %").font(Theme.Fonts.captionNumeric).frame(width: 40)
                    }
                }
                LabeledContent("Suggest keep above") {
                    HStack {
                        Slider(value: $agent.preferences.keepAbove, in: 0.51...1)
                        Text("\(Int((agent.preferences.keepAbove * 100).rounded())) %").font(Theme.Fonts.captionNumeric).frame(width: 40)
                    }
                }
            }
            KeywordsCaptionsSettingsSection(model: model)
            StyleProfileSection(agent: agent, model: model)
        }
        .formStyle(.grouped)
        .frame(width: 520, height: 640)
        .tint(Theme.accent)
        .onChange(of: agent.preferences) { agent.savePreferences() }
        .onChange(of: agent.preferences.assistMode) { if model.assist.enabled { model.assist.setEnabled(true) } }
        .onAppear { agent.refreshProfile() }
    }
}

private struct KeySection: View {
    let agent: AgentController
    let kind: AIProviderKind
    @Binding var model: String
    @State private var entry = ""
    @State private var message: String?
    @State private var revision = 0

    var body: some View {
        Section(kind.title) {
            TextField("Model", text: $model)
            let status = revision >= 0 ? agent.keyStatus(kind) : nil
            HStack(spacing: Theme.Space.s) {
                SecureField(status == nil ? "API key" : "Replace key", text: $entry)
                    .accessibilityIdentifier("ai-key-\(kind.rawValue)")
                Button("Save") {
                    message = agent.setKey(entry, for: kind)
                    entry = ""
                    revision += 1
                }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(entry.trimmingCharacters(in: .whitespaces).isEmpty)
                Button("Remove") {
                    message = agent.setKey(nil, for: kind)
                    revision += 1
                }
                .buttonStyle(.theme(.destructive, height: Theme.Height.small))
                .disabled(status == nil)
            }
            if let message {
                StatusLine(text: message, kind: .error)
            } else if let status {
                StatusLine(text: status, kind: .success).accessibilityIdentifier("ai-key-status-\(kind.rawValue)")
            } else {
                Hint("Stored in your login Keychain, never in Tessera's files or logs.")
            }
        }
    }
}

private struct StyleProfileSection: View {
    let agent: AgentController
    let model: AppModel
    @State private var answers = StyleQuestionnaire(brightness: 0, contrast: 0, warmth: 0, saturation: 0, skinTonePriority: 0)

    var body: some View {
        Section("Style profile") {
            if let p = agent.profile {
                Text(p.samples == 0 ? "Not trained yet: the questionnaire sets the starting look."
                     : "Learned from \(p.samples) edited photo\(p.samples == 1 ? "" : "s").")
                    .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .accessibilityIdentifier("ai-profile-status")
                answer("Brighter", \.brightness, -1...1)
                answer("More contrast", \.contrast, -1...1)
                answer("Warmer", \.warmth, -1...1)
                answer("More saturated", \.saturation, -1...1)
                answer("Protect skin tones", \.skinTonePriority, 0...1)
                HStack(spacing: Theme.Space.s) {
                    Button("Save Answers") { agent.saveQuestionnaire(answers) }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    Button("Learn from My Edits") { agent.trainProfile() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .disabled(agent.training != nil)
                    if let t = agent.training {
                        ProgressView().controlSize(.small)
                        Text("\(t.done) / \(t.total) \(t.current)").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                    }
                }
            } else {
                Hint("Open a folder to see its style profile.")
            }
        }
        .onAppear { if let q = agent.profile?.questionnaire { answers = q } }
        .onChange(of: agent.profile) { if let q = agent.profile?.questionnaire { answers = q } }
    }

    private func answer(_ title: String, _ path: WritableKeyPath<StyleQuestionnaire, Double>, _ range: ClosedRange<Double>) -> some View {
        LabeledContent(title) {
            HStack {
                Slider(value: Binding(get: { answers[keyPath: path] }, set: { answers[keyPath: path] = $0 }), in: range)
                Text(String(format: "%+.2f", answers[keyPath: path])).font(Theme.Fonts.captionNumeric).frame(width: 44)
            }
        }
    }
}
