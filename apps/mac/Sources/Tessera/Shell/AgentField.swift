import SwiftUI
import TesseraCore

/// Bottom centre of every mode (SPEC §1, WP UX-11): in Edit the tools strip, the agent field and
/// the agent status row (UX-12) in two equal side slots, so the field stays centred, with the Try
/// cards above the field (SPEC §3, UX-13); in Cull the quick actions sit above the field (UX-14).
struct AgentRow: View {
    let model: AppModel

    var body: some View {
        let mode = model.shellMode
        VStack(spacing: Theme.Space.s + Theme.Space.xxs) {
            // UX-14: the quick actions of Cull (SPEC §4), each one undoable step with a toast.
            if mode == .cull, model.isCullWorkspace { CullQuickActions(model: model) }
            // UX-13: the Try cards, real renders of this photo, above the field in Edit.
            if mode == .edit, model.viewMode == .loupe { TryCardsRow(model: model) }
            // UX-12: in Edit the tools strip and the agent status sit in two equal side slots, so
            // the field stays centred.
            HStack(alignment: .center, spacing: Theme.Space.m) {
                if mode == .edit {
                    ToolsStrip(model: model).frame(width: Theme.Width.agentSide, alignment: .trailing)
                    AgentField(model: model, mode: mode)
                    AgentStatus(model: model).frame(width: Theme.Width.agentSide, alignment: .leading)
                } else {
                    AgentField(model: model, mode: mode)
                }
            }
        }
    }
}

/// Keep the pick in every burst · Reject the blurry ones · Find closed eyes · Show me <person>.
struct CullQuickActions: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            chip("Keep the pick in every burst", help: "Keep the agent's pick in every burst and reject the other undecided frames: one undo step") {
                model.keepPickInEveryBurst()
            }
            .accessibilityIdentifier("library.cull.quick.keepPicks")
            chip("Reject the blurry ones", help: "Reject every undecided frame with missed focus or motion blur: one undo step") {
                model.rejectBlurry()
            }
            .accessibilityIdentifier("library.cull.quick.rejectBlurry")
            chip("Find closed eyes", help: "Show the frames where the eyes proxy reads closed") {
                model.findClosedEyes()
            }
            .accessibilityIdentifier("library.cull.quick.closedEyes")
            if let person = model.firstNamedPerson {
                chip("Show me \(person.name)", help: "Show the frames with \(person.name)") {
                    model.showPerson(named: person.name)
                }
                .accessibilityIdentifier("library.cull.quick.person")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Quick actions")
        .accessibilityIdentifier("library.cull.quick")
    }

    private func chip(_ title: String, help: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
                .padding(.horizontal, Theme.Space.m)
                .frame(height: Theme.Height.large)
                .background(Capsule().fill(Theme.panel))
                .overlay(Capsule().strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .help(help)
    }
}

/// Right of the agent field in Edit (SPEC §1): a mark and "n edits by agent · review" once a
/// field run has finished on the photo (review shows the cards in the panel), what the agent is
/// doing meanwhile, "No agent edits yet" otherwise.
struct AgentStatus: View {
    let model: AppModel

    var body: some View {
        let entry = model.agentFieldEntry
        let working = model.agent.isRunning && model.agentFieldRun != nil
        HStack(spacing: Theme.Space.s) {
            Circle()
                .fill(entry != nil ? Theme.accent : Theme.track)
                .frame(width: Theme.Height.large, height: Theme.Height.large)
                .accessibilityHidden(true)
            if let entry {
                let n = entry.steps.count
                HStack(spacing: Theme.Space.xs) {
                    Text("\(n) edit\(n == 1 ? "" : "s") by agent ·").lineLimit(1)
                    Button("review") { model.revealAgentChanges() }
                        .buttonStyle(.plain)
                        .foregroundStyle(Theme.accent)
                        .help("Show the agent's changes in the panel")
                        .accessibilityIdentifier("develop.agent.review")
                }
            } else {
                Text(working ? "Agent is working…" : "No agent edits yet").lineLimit(1)
            }
        }
        .font(Theme.Fonts.label)
        .foregroundStyle(Theme.textTertiary)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Agent status")
        .accessibilityIdentifier("develop.agent.status")
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
