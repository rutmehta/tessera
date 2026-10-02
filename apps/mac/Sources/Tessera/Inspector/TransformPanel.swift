import AppKit
import Observation
import SwiftUI
import TesseraCore

// MARK: - Guided Upright tool state (M2-48)

/// The Guided Upright tool: while active the loupe overlay draws and edits up to four guides
/// (`LoupeUprightGuides.swift`). Guides are kept here as a draft; each finished gesture writes the
/// recipe as one history step once there are two to four guides (the engine rejects fewer).
/// While armed the session renders the uncorrected image (M2-51: no lens, Upright, transform or
/// crop), so guides are placed on the picture the engine analyses; leaving restores the view.
@MainActor @Observable
final class UprightGuideTool: LibraryObserver {
    static let shared = UprightGuideTool(model: .shared)

    @ObservationIgnored let model: AppModel
    private(set) var active = false
    var guides = UprightGuides()
    /// Bumped after this panel's own commits so SwiftUI rereads the recipe.
    private(set) var revision = 0
    @ObservationIgnored private var session: ObjectIdentifier?
    @ObservationIgnored private var selfTestRan = false
    /// The session showing its uncorrected view while the tool is armed.
    @ObservationIgnored let placement = UncorrectedPlacement()

    /// Guides are drawn over the uncorrected (uncropped) frame while this is on.
    var showsUncorrected: Bool { placement.isActive }

    init(model: AppModel) {
        self.model = model
        model.addObserver(self)
    }

    private var tools: DevelopTools { .shared }
    var develop: DevelopController? { model.develop }

    /// Arms the loupe tool with the recipe's guides (Upright ▸ Guided).
    func begin() {
        guard let d = develop, model.developStatus == .ready else { return }
        guard model.viewMode == .loupe else { model.statusMessage = "Guided Upright works in the loupe (E)"; return }
        guard !MaskTools.shared.active else { model.statusMessage = "Close masking (M) to draw Upright guides"; return }
        if tools.cropActive { tools.cancelCrop() }
        tools.hslPicker = nil
        tools.detailPicking = false
        guides = UprightGuides(settings: d.settingsObject)
        placement.onFailure = { [weak model] in model?.statusMessage = "Guided Upright: \($0)" }
        placement.enter(d.session)
        active = true
        tools.onLoupeToolChange?()
    }

    /// Leaves the tool; an incomplete draft (fewer than two guides) is discarded.
    func end() {
        guard active else { return }
        active = false
        placement.exit()
        guides.selected = nil
        revision += 1
        tools.onLoupeToolChange?()
    }

    /// One finished loupe gesture (draw, endpoint drag, removal): one history step when it changes
    /// the recipe. Incomplete drafts only touch the recipe when they undo a stored Guided mode.
    func commitGuides() {
        guard let d = develop else { return }
        let stored = UprightControls.mode(in: d.settingsObject)
        guard guides.isComplete || stored == .guided else { return }
        tools.apply(guides.patch, final: true, label: guides.historyLabel)
        revision += 1
    }

    /// An Upright button. Guided arms the loupe tool; the other modes are one history step.
    func setMode(_ mode: UprightMode) {
        if mode == .guided { begin(); return }
        end()
        tools.apply(UprightControls.patch(mode: mode), final: true, label: UprightControls.historyLabel(mode))
        revision += 1
    }

    func resetUpright() {
        end()
        guides.removeAll()
        tools.apply(UprightControls.resetPatch, final: true, label: "Reset Upright")
        revision += 1
    }

    func resetTransform() {
        tools.apply(TransformControls.resetPatch, final: true, label: "Reset Transform")
        tools.bump()
        revision += 1
    }

    func setConstrainCrop(_ on: Bool) {
        tools.apply(UprightControls.constrainCropPatch(on), final: true, label: on ? "Constrain Crop On" : "Constrain Crop Off")
        revision += 1
    }

    func clearGuides() {
        guides.removeAll()
        commitGuides()
        tools.onLoupeToolChange?()
    }

    func removeSelectedGuide() {
        guard let i = guides.selected else { return }
        guides.remove(i)
        commitGuides()
        tools.onLoupeToolChange?()
    }

    /// Keys while the tool is armed: Return/Esc finish, ⌫ removes the selected guide. Other keys
    /// are swallowed like the crop tool's so a stray culling key cannot move the loupe.
    func handleKey(_ event: NSEvent) -> Bool {
        if event.modifierFlags.contains(.command) { return false }   // ⌘Z and menus still work
        switch event.keyCode {
        case 36, 76, 53: end()
        case 51, 117: removeSelectedGuide()
        default: break
        }
        return true
    }

    // MARK: LibraryObserver

    func workspaceWillLeavePhotoEdit() {
        if active, guides.isComplete { commitGuides() }
        end()
    }

    func libraryDidReload() {}
    func itemsDidChange(_ positions: IndexSet) {}
    func selectionDidChange(scrollToFocus: Bool) {
        if active, model.viewMode != .loupe || develop == nil { end() }
    }

    func developDidChange() {
        let current = develop.map(ObjectIdentifier.init)
        if current != session {
            if placement.session !== develop?.session { placement.abandon() }   // the old session has closed
            end()
            guides = UprightGuides()
        }
        session = current
        revision += 1
        // Screenshot aid: TESSERA_SELFTEST_UPRIGHT=guided arms the tool with two committed guides.
        if let d = develop, !selfTestRan, ProcessInfo.processInfo.environment["TESSERA_SELFTEST_UPRIGHT"] == "guided" {
            selfTestRan = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                MainActor.assumeIsolated {
                    guard self.develop === d else { return }
                    self.model.enterPhotoEdit()
                    self.begin()
                    self.guides.add(UprightGuide(start: (0.22, 0.18), end: (0.25, 0.82)))
                    self.guides.add(UprightGuide(start: (0.80, 0.16), end: (0.76, 0.84)))
                    self.commitGuides()
                    self.tools.onLoupeToolChange?()
                }
            }
        }
    }
}

// MARK: - Transform panel

/// Transform (docs/01 §2.10): Upright modes, Guided guides in the loupe, the manual transform
/// sliders and Constrain Crop, with a reset per group.
struct TransformPanel: View {
    let model: AppModel
    let tools: DevelopTools
    let guideTool: UprightGuideTool
    @Environment(\.developRevision) private var revision

    var body: some View {
        let ready = model.developStatus == .ready
        let settings: [String: Any] = {
            _ = revision; _ = guideTool.revision; _ = model.developHistory?.entries
            return tools.develop?.settingsObject ?? [:]
        }()
        let mode = guideTool.active ? .guided : UprightControls.mode(in: settings)
        let constrain = (DevelopController.value(in: settings, at: UprightControls.constrainCropPath) as? NSNumber)?.boolValue ?? false
        VStack(alignment: .leading, spacing: 0) {
            GroupHeader(title: "Upright", resetHelp: "Upright back to Off (one undo step)", id: "develop.transform.upright-reset") {
                guideTool.resetUpright()
            }
            UprightModeBar(selection: mode) { guideTool.setMode($0) }
                .padding(.bottom, Theme.Space.xs)
            if guideTool.active {
                HStack(spacing: Theme.Space.xs) {
                    Text("\(guideTool.guides.guides.count) of \(UprightGuides.maximum) guides")
                        .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                        .accessibilityIdentifier("develop.transform.guides-count")
                    Spacer()
                    Button("Clear") { guideTool.clearGuides() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .disabled(guideTool.guides.guides.isEmpty)
                        .accessibilityIdentifier("develop.transform.guides-clear")
                    Button("Done") { guideTool.end() }
                        .buttonStyle(.theme(.primary, height: Theme.Height.small))
                        .accessibilityIdentifier("develop.transform.guides-done")
                }
                .frame(height: Theme.Height.regular)
                Hint("Drag in the photo along lines that should be vertical or horizontal (two to four guides). Drag an end to adjust; ⌫ removes the selected guide; Return or Esc when done.")
                if guideTool.showsUncorrected {
                    Hint("The loupe shows the uncorrected photo while you place guides; the correction returns when you are done.")
                        .accessibilityIdentifier("develop.transform.guides-uncorrected")
                }
            } else {
                Hint(mode.help)
            }
            GroupHeader(title: "Transform", resetHelp: "Manual transform back to neutral (one undo step)", id: "develop.transform.reset") {
                guideTool.resetTransform()
            }
            ForEach(TransformControls.all) { c in
                ControlSlider(control: c)
                    .frame(height: Theme.Height.slider)
                    .accessibilityIdentifier("develop.transform." + c.path.last!.replacingOccurrences(of: "_", with: "-"))
            }
            // Constrain Crop: not rendered by the engine (M2-49 handoff), so it stays disabled
            // with the reason; a recipe that already has it on can still switch it off.
            Toggle("Constrain Crop", isOn: Binding(get: { constrain }, set: { guideTool.setConstrainCrop($0) }))
                .font(Theme.Fonts.caption)
                .controlSize(.small)
                .disabled(DevelopEngineGaps.constrainCrop != nil && !constrain)
                .help(DevelopEngineGaps.constrainCrop ?? "Keep the crop inside the transformed image (no blank corners)")
                .accessibilityIdentifier("develop.transform.constrain-crop")
                .padding(.top, Theme.Space.xs)
            if let gap = DevelopEngineGaps.constrainCrop {
                StatusLine(text: gap, kind: .warning).accessibilityIdentifier("develop.transform.constrain-crop-unavailable")
            }
            if let d = tools.develop, d.ignores("/geometry/upright") || d.ignores("/geometry/transform") {
                StatusLine(text: "This photo's Upright or Transform is kept in the recipe but not drawn by the loupe.",
                           kind: .warning)
                    .padding(.top, Theme.Space.s)
                    .accessibilityIdentifier("develop.transform.preview-note")
            }
        }
        .disabled(!ready)
    }
}

/// A `SubHeader` with a borderless Reset on the right (per-group reset).
struct GroupHeader: View {
    let title: String
    let resetHelp: String
    let id: String
    let reset: () -> Void

    var body: some View {
        HStack(alignment: .center) {
            SubHeader(title)
            Spacer()
            Button("Reset", action: reset)
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .help(resetHelp)
                .accessibilityIdentifier(id)
                .padding(.top, Theme.Space.s)
        }
    }
}

/// Upright's six modes as one neutral segmented bar (the `SegmentedPicker` look). Icons keep six
/// segments inside the inspector's minimum width; each segment carries its name as help,
/// accessibility label and identifier (`transform-upright-<mode>`).
struct UprightModeBar: View {
    let selection: UprightMode
    let choose: (UprightMode) -> Void
    @Environment(\.isEnabled) private var enabled

    var body: some View {
        let height = Theme.Height.small
        HStack(spacing: Theme.Space.xxs) {
            ForEach(UprightMode.allCases) { m in
                let on = m == selection
                Button { choose(m) } label: {
                    HStack(spacing: Theme.Space.xs) {
                        Image(systemName: m.symbol).font(Theme.Fonts.iconSmall)
                        if on { Text(m.title).lineLimit(1) }
                    }
                    .font(Theme.Fonts.caption)
                    .fontWeight(on ? .medium : .regular)
                    .foregroundStyle(on ? Theme.textPrimary : Theme.textSecondary)
                    .padding(.horizontal, Theme.Space.xs)
                    .frame(maxWidth: on ? nil : .infinity)
                    .frame(height: height - Theme.Space.xs)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                        .fill(on ? Theme.raised : Theme.clear)
                        .shadow(color: on ? Theme.shadow.opacity(0.4) : Theme.clear, radius: 1, y: 0.5))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .layoutPriority(on ? 1 : 0)
                .help(m.help)
                .accessibilityLabel("Upright \(m.title)")
                .accessibilityIdentifier("develop.transform.upright-\(m.rawValue)")
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(Theme.Space.xxs)
        .frame(height: height)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(Theme.well))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
        .opacity(enabled ? 1 : Theme.Opacity.disabled)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("develop.transform.upright")
    }
}
