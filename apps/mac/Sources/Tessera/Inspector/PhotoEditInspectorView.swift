import SwiftUI
import TesseraCore
import TesseraFFI

/// The Edit panel (redesign SPEC §3, WP UX-12; UX-16 removed the Develop / Masks tab switcher).
/// The panel starts with the 40 pt histogram strip, then Light · Color · Detail · Optics ·
/// Effects with the everyday controls visible and the rest of each group behind its More, the
/// agent's change cards once a run from the agent field has finished (`AgentChangesSection`),
/// and the Edits stack. While the Masks tool is active (the strip's button, M) the mask list sits
/// above the histogram as a sheet-like section (`MasksSheet`; `photoInspectorTab == .masks` is
/// that state). Every control of the former panels is still here; the sections regroup them.
struct PhotoEditInspectorView: View {
    @Bindable var model: AppModel
    private var tools: DevelopTools { .shared }
    private var masks: MaskTools { .shared }
    private var dial: PhotoDial { .shared }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if model.photoInspectorTab == .masks {
                    MasksSheet(model: model, masks: masks)
                        .developContext(model, tools)
                        .transition(Theme.Motion.transition(from: .top))
                }
                PhotoDevelopPanels(model: model)
            }
            .animation(Theme.Motion.appear, value: model.photoInspectorTab)
            // The Try cards and the agent row cross the panel's bottom (as in the prototype).
            .padding(.bottom, Theme.Height.bottomRow)
        }
        .scrollIndicators(.never)
        // The prototype's panel: the canvas ground with a hairline on its left, no fill.
        .background(Theme.canvas)
        .overlay(alignment: .leading) { Hairline(vertical: true) }
        .tint(Theme.accent)
        .onChange(of: model.photoInspectorTab) { _, tab in
            if tab == .develop {
                masks.setActive(false)
                if dial.model === model, dial.masksToolShown { dial.toggleMasksTool() }
            }
        }
        .onChange(of: masks.active) { _, active in
            if active { model.photoInspectorTab = .masks }
        }
    }
}

/// The mask list and the selected mask's controls (`MasksPanel`) as a card at the top of the
/// panel while the Masks tool is active: a 13/600 title, Done (Esc does the same), the list.
private struct MasksSheet: View {
    let model: AppModel
    let masks: MaskTools

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(alignment: .center, spacing: Theme.Space.s) {
                Text("Masks")
                    .font(Theme.Fonts.bodySemibold)
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                    .accessibilityIdentifier("develop.panel.Masks")
                Spacer(minLength: 0)
                Button("Done") { model.photoInspectorTab = .develop }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .help("Leave the Masks tool (Esc)")
                    .accessibilityIdentifier("develop.masks.done")
            }
            .frame(height: Theme.Height.large)
            MasksPanel(model: model, masks: masks)
        }
        .padding(.horizontal, Theme.Space.m)
        .padding(.bottom, Theme.Space.m)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.card).fill(Theme.panel))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.card).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
        .padding(.horizontal, Theme.Space.panelInset)
        .padding(.top, Theme.Space.panelInset - Theme.Space.xxs)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Masks")
    }
}

private struct PhotoDevelopPanels: View {
    let model: AppModel
    private var tools: DevelopTools { .shared }
    var body: some View {
        Group {
            HistogramPanel(model: model)
                .frame(height: HistogramView.height)
                .padding(.horizontal, Theme.Space.panelInset)
                .padding(.top, Theme.Space.panelInset - Theme.Space.xxs)
                .padding(.bottom, Theme.Space.s)
                .help("Luminance of the photo as shown; a mark in a corner means that end clips")
            LightSection(model: model, tools: tools)
            ColorSection(model: model, tools: tools)
            DetailSection(model: model, tools: tools)
            OpticsSection(model: model, tools: tools)
            EffectsSection(model: model, tools: tools)
            if model.agentFieldEntry != nil {
                AgentChangesSection(model: model, tools: tools)
            }
            EditsSection(model: model, tools: tools)
        }
        .developContext(model, tools)
    }
}

// MARK: - Section

/// One group of the Edit panel: a 15/600 title with an optional readout at the right (the Light
/// target, the colour temperature, the sharpen amount), the everyday controls, and the rest of
/// the group behind a More disclosure that remembers its state per section. Agent and Edits use
/// the compact 13/600 title. One tooltip per control; no explanatory text (SPEC §7).
struct EditSection<Content: View, More: View>: View {
    let title: String
    var readout: String?
    var identifier: String?
    var compact = false
    var accessory: AnyView?
    @ViewBuilder var content: Content
    let more: More?
    @AppStorage private var showMore: Bool

    init(_ title: String, readout: String? = nil, identifier: String? = nil, compact: Bool = false,
         accessory: AnyView? = nil, @ViewBuilder content: () -> Content, @ViewBuilder more: () -> More) {
        self.title = title
        self.readout = readout
        self.identifier = identifier
        self.compact = compact
        self.accessory = accessory
        self.content = content()
        self.more = more()
        _showMore = AppStorage(wrappedValue: false, "EditPanel." + title + ".more")
    }

    init(_ title: String, readout: String? = nil, identifier: String? = nil, compact: Bool = false,
         accessory: AnyView? = nil, @ViewBuilder content: () -> Content) where More == EmptyView {
        self.title = title
        self.readout = readout
        self.identifier = identifier
        self.compact = compact
        self.accessory = accessory
        self.content = content()
        self.more = nil
        _showMore = AppStorage(wrappedValue: false, "EditPanel." + title + ".more")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .center, spacing: Theme.Space.s) {
                Text(title)
                    .font(compact ? Theme.Fonts.bodySemibold : Theme.Fonts.title)
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                    .accessibilityIdentifier("develop.panel.\(AccessibilityKey.component(title))")
                Spacer(minLength: 0)
                if let readout {
                    Text(readout)
                        .font(Theme.Fonts.labelNumeric)
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                        .accessibilityIdentifierIfPresent(identifier)
                }
                if let accessory { accessory }
            }
            .frame(height: Theme.Height.sectionHeader)
            content
            if let more {
                Button {
                    showMore.toggle()
                } label: {
                    HStack(spacing: Theme.Space.xs) {
                        Image(systemName: "chevron.right")
                            .font(Theme.Fonts.iconSmall)
                            .rotationEffect(.degrees(showMore ? 90 : 0))
                        Text(showMore ? "Less" : "More")
                    }
                    .foregroundStyle(Theme.textSecondary)
                }
                .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                .padding(.top, Theme.Space.xs)
                .help(showMore ? "Hide the rest of \(title)" : "The rest of \(title)")
                .accessibilityLabel("More \(title) controls")
                .accessibilityValue(showMore ? "expanded" : "collapsed")
                .accessibilityIdentifier("develop.panel.\(AccessibilityKey.component(title)).more")
                if showMore {
                    more
                }
            }
        }
        .padding(.horizontal, Theme.Space.panelInset)
        .padding(.bottom, Theme.Space.l)
        Hairline()
    }
}

// MARK: - Light

/// Exposure, Shadows, Highlights, Contrast; Whites, Blacks, the tone curve and HDR behind More.
struct LightSection: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        let id = model.focusedItem?.id
        let ready = model.developStatus == .ready
        let revision = model.developRevision
        EditSection("Light", readout: "Whole photo", identifier: "photo-edit-target") {
            ForEach([BasicKey.exposure, .shadows, .highlights, .contrast]) { key in
                AdjustmentSlider(model: model, key: key, itemID: ready ? id : nil, revision: revision)
                    .frame(height: Theme.Height.slider)
            }
        } more: {
            ForEach([BasicKey.whites, .blacks]) { key in
                AdjustmentSlider(model: model, key: key, itemID: ready ? id : nil, revision: revision)
                    .frame(height: Theme.Height.slider)
            }
            SubHeader("Tone Curve")
            ToneCurvePanel(model: model, tools: tools)
            SubHeader("HDR")
            HDRPanel(model: model, tools: tools)
        }
    }
}

// MARK: - Color

/// Temperature, Tint, Vibrance; Saturation, HSL and colour grading behind More. The readout is
/// the colour temperature.
struct ColorSection: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        let id = model.focusedItem?.id
        let ready = model.developStatus == .ready
        let revision = model.developRevision
        let kelvin: String? = {
            _ = model.developHistory?.entries
            guard ready, let id else { return nil }
            return String(format: BasicKey.temperature.format, model.adjustment(.temperature, for: id))
        }()
        EditSection("Color", readout: kelvin) {
            ForEach([BasicKey.temperature, .tint, .vibrance]) { key in
                AdjustmentSlider(model: model, key: key, itemID: ready ? id : nil, revision: revision)
                    .frame(height: Theme.Height.slider)
            }
        } more: {
            AdjustmentSlider(model: model, key: .saturation, itemID: ready ? id : nil, revision: revision)
                .frame(height: Theme.Height.slider)
            SubHeader("HSL")
            HSLPanel(model: model, tools: tools)
            SubHeader("Grading")
            ColorGradingPanel(model: model, tools: tools)
        }
    }
}

// MARK: - Detail

/// Sharpen; Texture, Clarity, Dehaze and the full sharpening / noise controls (with the 1:1
/// preview and AI Denoise) behind More. The readout is the sharpen amount.
struct DetailSection: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        let id = model.focusedItem?.id
        let ready = model.developStatus == .ready
        let revision = model.developRevision
        let sharpen: String? = {
            _ = model.developHistory?.entries; _ = tools.revision
            guard ready else { return nil }
            return "Sharpen " + String(format: DetailControls.amount.format, tools.value(DetailControls.amount))
        }()
        EditSection("Detail", readout: sharpen) {
            ControlSlider(control: DetailControls.amount, title: "Sharpen")
                .frame(height: Theme.Height.slider)
                .disabled(!ready)
        } more: {
            ForEach([BasicKey.texture, .clarity, .dehaze]) { key in
                AdjustmentSlider(model: model, key: key, itemID: ready ? id : nil, revision: revision)
                    .frame(height: Theme.Height.slider)
            }
            DetailPanel(model: model, tools: tools, amountShown: false)
        }
    }
}

// MARK: - Optics

/// Crop & straighten; Upright / Transform and Lens Blur behind More. Lens corrections from the
/// maker notes are automatic and appear in the Edits stack as an auto step.
struct OpticsSection: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        EditSection("Optics") {
            CropPanel(model: model, tools: tools)
        } more: {
            SubHeader("Transform")
            TransformPanel(model: model, tools: tools, guideTool: .shared)
            SubHeader("Lens Blur")
            LensBlurPanel(model: model, tools: tools)
        }
    }
}

// MARK: - Effects

/// Vignette and grain amounts; the rest of both, presets, soft proofing and snapshots behind More.
struct EffectsSection: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        let ready = model.developStatus == .ready
        EditSection("Effects") {
            ControlSlider(control: EffectsControls.amount, title: "Vignette").frame(height: Theme.Height.slider)
            ControlSlider(control: EffectsControls.grainAmount, title: "Grain").frame(height: Theme.Height.slider)
        } more: {
            EffectsPanel(model: model, tools: tools, amountsShown: false)
            SubHeader("Presets")
            PresetsPanel(model: model, tools: tools)
            SubHeader("Soft Proofing")
            SoftProofPanel(proof: .shared)
            SubHeader("Snapshots")
            SnapshotsPanel(model: model)
        }
        .disabled(!ready)
    }
}

// MARK: - Edits

/// The Edits stack (SPEC §3): the step count, Reset (all develop settings, one undo step) and
/// `HistoryPanel`, which lists every step with its toggle and the agent groups with their fader.
struct EditsSection: View {
    let model: AppModel
    let tools: DevelopTools

    var body: some View {
        let ready = model.developStatus == .ready
        let steps = tools.historyItems.filter(\.applied).count
        let reset = AnyView(
            Button("Reset") { model.resetDevelop() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .disabled(!ready || steps == 0)
                .help("Back to the unedited photo (one undo step)")
                .accessibilityIdentifier("develop.basic.reset"))
        EditSection("Edits", readout: ready ? "\(steps) step\(steps == 1 ? "" : "s")" : nil, compact: true, accessory: reset) {
            HistoryPanel(model: model, tools: tools)
        }
    }
}

// MARK: - Agent change cards

/// The agent field's answer (SPEC §1, §3): one card per change in the finished run's plan, with
/// the control and its value, where it applies, its own switch, and the photo without that change
/// while the pointer is over the card; then Keep / Undo all / Apply to burst. The run is already
/// one named group in the photo's history (`AgentGroupSection` in the Edits stack carries its
/// fader); the cards are its steps, so a switch is the same step toggle as in the stack.
struct AgentChangesSection: View {
    let model: AppModel
    let tools: DevelopTools
    /// Values put back when the pointer leaves a card (the live values before the hover preview).
    @State private var restore: [BasicKey: Double] = [:]

    var body: some View {
        if let entry = model.agentFieldEntry {
            let group = tools.historyGroups.first { $0.groupId == entry.groupID }
            let items = tools.historyItems
            let changes = entry.steps.map { step in
                AgentChange(step: step, item: items.first { $0.id == step.entryID }, model: model)
            }
            let key = "\(model.agentRevision)|\(model.developRevision)|\(model.agent.queue.summary)|\(model.agent.reviewGeneration)"
            let busy = model.agent.isRunning || !model.agent.busy.isEmpty
            let burst = model.agentFieldBurstFrames.count
            EditSection("Agent", readout: "\(changes.count) change\(changes.count == 1 ? "" : "s") · preview", compact: true) {
                VStack(spacing: Theme.Space.xs + Theme.Space.xxs) {
                    ForEach(changes) { change in
                        ChangeCard(change: change, enabled: change.enabled) { on in
                            preview(change, group: group, on: false)
                            guard let item = change.item else { return }
                            tools.setStep(item, enabled: on)
                        } hover: { on in
                            preview(change, group: group, on: on)
                        }
                    }
                }
                HStack(spacing: Theme.Space.xs + Theme.Space.xxs) {
                    Button { model.keepAgentChanges() } label: { Text("Keep").frame(maxWidth: .infinity) }
                        .buttonStyle(.theme(.primary, height: Theme.Height.large))
                        .help("Keep these changes: one grouped step in Edits with an amount fader")
                        .accessibilityIdentifier("develop.agent.keep")
                    Button("Undo all") { model.undoAgentChanges() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.large))
                        .help("Take every change back (one step in Edits)")
                        .accessibilityIdentifier("develop.agent.undoAll")
                    if burst > 0 {
                        Button("Apply to burst (\(burst))") { model.applyAgentChangesToBurst() }
                            .buttonStyle(.theme(.bordered, height: Theme.Height.large))
                            .help("Run “\(model.agentFieldRun?.instruction ?? "")” on the other \(burst) frame\(burst == 1 ? "" : "s") of this burst; they land in Agent Review")
                            .accessibilityIdentifier("develop.agent.burst")
                    }
                }
                .padding(.top, Theme.Space.xs)
                .disabled(busy)
            }
            .task(id: key) { tools.refreshHistory() }
            .onDisappear { preview(nil, group: nil, on: false) }
        }
    }

    /// Hover: the photo without this change, through the interactive (uncommitted) patch path,
    /// then the live values again. "Without" is the group's settings at 0 % for the controls the
    /// step names (the agent's redo path only moves Basic controls); other steps have no preview.
    private func preview(_ change: AgentChange?, group: HistoryGroupState?, on: Bool) {
        guard let d = tools.develop else { return }
        if on {
            guard restore.isEmpty, let change, let group,
                  let without = (try? JSONSerialization.jsonObject(with: Data(group.withoutJson.utf8))) as? [String: Any]
            else { return }
            var saved: [BasicKey: Double] = [:]
            for key in change.keys {
                guard let p = key.parameter else { continue }
                var node: Any? = without
                for part in p.path { node = (node as? [String: Any])?[part] }
                guard let before = (node as? NSNumber)?.doubleValue else { continue }
                saved[key] = d.number(at: p.path) ?? key.defaultValue
                d.set(p, before, interactive: true)
            }
            restore = saved
        } else {
            for (key, value) in restore {
                if let p = key.parameter { d.set(p, value, interactive: true) }
            }
            restore = [:]
        }
    }
}

/// One change of the plan: the step, its history item (enabled state) and the Basic controls the
/// step's title names, with their live values for the card.
// Built on the main actor (it reads the model); plain data afterwards.
struct AgentChange: Identifiable {
    let step: AgentReviewEntry.Step
    let item: HistoryItem?
    let keys: [BasicKey]
    let value: String
    let target: String
    var id: UInt64 { step.entryID }
    var title: String { step.title }
    var enabled: Bool { item?.enabled ?? step.enabled }

    @MainActor init(step: AgentReviewEntry.Step, item: HistoryItem?, model: AppModel) {
        self.step = step
        self.item = item
        // Agent steps are titled after their controls ("Exposure", "Exposure, Contrast").
        let names = step.title.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
        keys = names.compactMap { name in BasicKey.allCases.first { $0.title.lowercased() == name } }
        if let id = model.editTarget?.id, !keys.isEmpty {
            value = keys.map { String(format: $0.format, model.adjustment($0, for: id)) }.joined(separator: " · ")
        } else {
            value = "on"
        }
        let text = (step.title + " " + step.rationale).lowercased()
        target = ["Subject", "Sky", "Background", "Face", "Person"].first { text.contains($0.lowercased()) }.map { "On \($0)" }
            ?? "Whole photo"
    }
}

/// A change card: the control and its value (accent while the change is on), where it applies,
/// and its switch. Dimmed while off.
private struct ChangeCard: View {
    let change: AgentChange
    let enabled: Bool
    let toggle: (Bool) -> Void
    let hover: (Bool) -> Void
    @State private var hovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.s) {
                Text(change.title).font(Theme.Fonts.bodyMedium).foregroundStyle(Theme.textPrimary).lineLimit(1)
                Text(change.value).font(Theme.Fonts.bodyNumeric).foregroundStyle(enabled ? Theme.accent : Theme.textSecondary)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Toggle("", isOn: Binding(get: { enabled }, set: toggle))
                    .toggleStyle(CardSwitchStyle())
                    .labelsHidden()
                    .accessibilityLabel(change.title)
                    .accessibilityIdentifier("develop.agent.toggle.\(change.id)")
            }
            Text(enabled ? change.target : "\(change.target) · off")
                .font(Theme.Fonts.label).foregroundStyle(Theme.textTertiary)
        }
        .padding(.horizontal, Theme.Space.m - Theme.Space.xxs)
        .padding(.vertical, Theme.Space.s + Theme.Space.xxs)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.card).fill(hovering ? Theme.raised : Theme.clear))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.card)
            .strokeBorder(hovering ? Theme.hairlineStrong : Theme.hairline, lineWidth: Theme.Space.hairline))
        .opacity(enabled ? 1 : Theme.Opacity.hidden)
        .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.card))
        .onHover { on in
            hovering = on
            hover(on)
        }
        .help(change.keys.isEmpty ? "The switch turns this change off (a step in Edits)"
              : "Hover: the photo without this change. The switch turns it off (a step in Edits)")
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("develop.agent.card.\(change.id)")
    }
}

/// A change card's switch: a 30 × 18 pill, accent with the knob in `textOnAccent` when on,
/// `track` with the knob in the canvas colour when off (the system switch ignores the tint).
private struct CardSwitchStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        let on = configuration.isOn
        Button { configuration.isOn.toggle() } label: {
            RoundedRectangle(cornerRadius: Theme.Radius.pill)
                .fill(on ? Theme.accent : Theme.track)
                .frame(width: Theme.Width.cardSwitch, height: Theme.Height.cardSwitch)
                .overlay(alignment: on ? .trailing : .leading) {
                    Circle()
                        .fill(on ? Theme.textOnAccent : Theme.canvas)
                        .padding(Theme.Space.xxs)
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(.isToggle)
        .accessibilityValue(on ? "on" : "off")
    }
}
