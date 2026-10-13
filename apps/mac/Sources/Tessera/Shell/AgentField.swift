import SwiftUI
import TesseraCore

/// Bottom centre of every mode (SPEC §1, WP UX-11): the tools strip (Edit only), the agent field
/// and, so the field stays centred, an invisible copy of the strip on the other side. In Edit the
/// Try cards sit above the field (SPEC §3, UX-13).
struct AgentRow: View {
    let model: AppModel

    var body: some View {
        let mode = model.shellMode
        VStack(spacing: Theme.Space.m) {
            if mode == .edit, model.viewMode == .loupe { TryCardsRow(model: model) }
            HStack(alignment: .center, spacing: Theme.Space.m) {
                if mode == .edit {
                    ToolsStrip(model: model)
                    AgentField(model: model, mode: mode)
                    ToolsStrip(model: model).hidden().accessibilityHidden(true)
                } else {
                    AgentField(model: model, mode: mode)
                }
            }
        }
    }
}

/// A 560 × 48 field with the sparkle mark, the mode's placeholder, a mic button and the ⌘K key cap.
/// ⌘K (`AppModel.focusAgentField`) gives it the keyboard; Return submits.
struct AgentField: View {
    let model: AppModel
    let mode: ShellMode
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: Theme.Space.m) {
            Image(systemName: "sparkles")
                .font(Theme.Fonts.icon)
                .foregroundStyle(Theme.accent)
                .accessibilityHidden(true)
            TextField(mode.agentPlaceholder, text: $text)
                .textFieldStyle(.plain)
                .font(Theme.Fonts.body)
                .foregroundStyle(Theme.textPrimary)
                .focused($focused)
                .onSubmit {
                    model.submitAgentInstruction(text)
                    text = ""
                }
                .accessibilityLabel("Ask the agent")
                .accessibilityIdentifier("\(mode.axNamespace).agent.field")
            Button {} label: {
                Image(systemName: "mic").font(Theme.Fonts.icon)
            }
            .buttonStyle(.theme(.borderless, height: Theme.Height.large, square: true))
            .help("Voice input arrives later; type for now")
            .accessibilityLabel("Speak")
            .accessibilityIdentifier("\(mode.axNamespace).agent.voice")
            KeyCap("⌘K")
        }
        .padding(.leading, Theme.Space.l)
        .padding(.trailing, Theme.Space.m)
        .frame(width: Theme.Width.agentField, height: Theme.Height.agentField)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.agentField).fill(Theme.panel))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.agentField)
            .strokeBorder(focused ? Theme.accent.opacity(0.6) : Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
        .shadow(color: Theme.shadow.opacity(0.45), radius: Theme.Space.s, y: Theme.Space.xxs)
        .onChange(of: model.agentFieldFocusRequest) { _, _ in focused = true }
        .accessibilityElement(children: .contain)
    }
}

/// A key cap, e.g. ⌘K, beside the control it drives.
struct KeyCap: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text)
            .font(Theme.Fonts.caption)
            .foregroundStyle(Theme.textTertiary)
            .padding(.horizontal, Theme.Space.xs)
            .frame(height: Theme.Height.small)
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                .strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
            .accessibilityHidden(true)
    }
}

/// Masks · Crop · Heal, left of the agent field in Edit (SPEC §3, UX-13). Masks shows the Subject /
/// Sky / Background / Whole photo chips over the photo (the brush and gradient toolbar stays on M);
/// Crop is the existing crop tool with the aspect chips; Heal is click-to-heal.
struct ToolsStrip: View {
    let model: AppModel
    private var dial: PhotoDial { .shared }
    private var tools: DevelopTools { .shared }
    private var owned: Bool { dial.model === model && tools.model === model }

    var body: some View {
        HStack(spacing: Theme.Space.xs) {
            IconButton(symbol: "circle.lefthalf.filled", help: "Masks: target the subject, sky or background (M for the brush and gradients)",
                       on: owned && dial.masksToolShown, size: Theme.Height.toolStrip) {
                if owned { dial.toggleMasksTool() }
            }
            .accessibilityIdentifier("develop.tools.masks")
            IconButton(symbol: "crop", help: "Crop and straighten (R)", on: owned && tools.cropActive, size: Theme.Height.toolStrip) {
                if owned { tools.toggleCrop() }
            }
            .accessibilityIdentifier("develop.tools.crop")
            IconButton(symbol: "bandage", help: "Heal: click a spot on the photo", on: owned && dial.healActive, size: Theme.Height.toolStrip) {
                if owned { dial.toggleHeal() }
            }
            .accessibilityIdentifier("develop.tools.heal")
        }
        .padding(Theme.Space.xs)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.card).fill(Theme.panel))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.card).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Tools")
    }
}

/// Deliver until UX-15: one sentence, one button (SPEC §7) over the Library canvas.
struct DeliverView: View {
    let model: AppModel
    var body: some View {
        EmptyStateContent(symbol: "square.and.arrow.up", title: "Deliver",
                          message: "Export the keeps, a selection or the whole shoot; the Deliver workspace arrives with UX-15.") {
            Button("Export…") { model.presentExport() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large))
                .disabled(!model.isEngineBacked || model.exporter.isRunning)
                .accessibilityIdentifier("library.deliver.export")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.canvas)
    }
}
