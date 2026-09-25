import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// Masks panel (M2-14): the groups with thumbnails, kind icons, visibility and amount; the selected
/// group's components (add / subtract / intersect, invert, remove) and its local sliders — the same
/// `ValueSlider` NSControl as Basic, so a drag goes straight to the session each display frame.
struct MasksPanel: View {
    let model: AppModel
    @Bindable var masks: MaskTools
    @State private var renaming: UInt32?
    @State private var name = ""

    var body: some View {
        let ready = model.developStatus == .ready
        let _ = masks.revision
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(spacing: Theme.Space.s) {
                Toggle(isOn: Binding(get: { masks.active }, set: { masks.setActive($0) })) {
                    Text("Show mask tools").font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary)
                }
                .toggleStyle(.checkbox)
                .controlSize(.small)
                .help("Show the mask tools on the loupe (M)")
                Spacer()
                createMenu
            }
            if masks.list.groups.isEmpty {
                Hint(ready ? "No masks. Pick a tool on the loupe or from Create; AI masks run on this Mac."
                           : "Open a RAW in the loupe (E)")
            } else {
                VStack(spacing: Theme.Space.xxs) {
                    ForEach(masks.list.groups, id: \.id) { g in row(g) }
                }
            }
            if let g = masks.selected { detail(g) }
        }
        .disabled(!ready)
    }

    // MARK: Create

    private var createMenu: some View {
        Menu {
            Section("AI") {
                Button("Subject") { masks.runAI(.subject, title: "Subject") }
                Button("Sky") { masks.runAI(.sky, title: "Sky") }
                Button("Background") { masks.runAI(.background, title: "Background") }
                Button("People…") { arm(.person) }
                Button("Objects…") { arm(.object) }
            }
            Section("Tools") {
                ForEach([MaskTool.brush, .linear, .radial], id: \.self) { t in Button(t.title) { arm(t) } }
            }
            Section("Range") {
                Button(MaskTool.colorRange.title) { arm(.colorRange) }
                Button(MaskTool.luminanceRange.title) { arm(.luminanceRange) }
            }
        } label: {
            Label("Create", systemImage: "plus")
        }
        .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
        .help("New mask")
    }

    private func arm(_ t: MaskTool) {
        if !masks.active { masks.setActive(true) }
        masks.target = nil
        masks.tool = t
    }

    // MARK: Rows

    private func row(_ g: MaskGroupInfo) -> some View {
        let selected = g.id == masks.list.selectedID
        return HStack(spacing: Theme.Space.s) {
            thumbnail(g)
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                if renaming == g.id {
                    TextField("Name", text: $name)
                        .textFieldStyle(.plain)
                        .font(Theme.Fonts.caption)
                        .onSubmit { masks.rename(g.id, to: name); renaming = nil }
                        .onExitCommand { renaming = nil }
                } else {
                    Text(g.name).font(selected ? Theme.Fonts.captionSemibold : Theme.Fonts.caption)
                        .foregroundStyle(Theme.textPrimary).lineLimit(1)
                }
                HStack(spacing: Theme.Space.xs) {
                    ForEach(Array(g.components.enumerated()), id: \.offset) { i, c in
                        if i > 0 { Text(c.combine.sign).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary) }
                        Image(systemName: c.kind.symbol).font(Theme.Fonts.iconSmall)
                            .foregroundStyle(c.invert ? Theme.accent : Theme.textSecondary)
                    }
                    if g.invert { Text("inverted").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary) }
                    if g.components.contains(where: { if case .failed = $0.ai { true } else { false } }) {
                        Image(systemName: "exclamationmark.triangle.fill").font(Theme.Fonts.iconSmall)
                            .foregroundStyle(Theme.reject)
                    }
                    if g.components.contains(where: { if case .pending = $0.ai { true } else { false } }) {
                        ProgressView().controlSize(.mini).scaleEffect(0.6).frame(width: Theme.Space.m, height: Theme.Space.m)
                    }
                }
            }
            Spacer(minLength: 0)
            IconButton(symbol: g.enabled ? "eye" : "eye.slash",
                       help: g.enabled ? "Hide this mask's adjustments" : "Show this mask's adjustments",
                       size: Theme.Height.small) {
                masks.setEnabled(g.id, !g.enabled)
            }
        }
        .padding(Theme.Space.xs)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(selected ? Theme.accentSubtle : Theme.clear))
        .contentShape(Rectangle())
        .onTapGesture(count: 2) { name = g.name; renaming = g.id }
        .onTapGesture { masks.select(g.id) }
        .contextMenu {
            Button("Rename…") { name = g.name; renaming = g.id }
            Button("Duplicate") { masks.duplicate(g.id) }
            Button(g.invert ? "Uninvert" : "Invert") { masks.select(g.id); masks.invertSelected() }
            Divider()
            Button("Delete") { masks.delete(g.id) }
        }
    }

    private func thumbnail(_ g: MaskGroupInfo) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Theme.well)
            if let image = masks.thumbnails[g.id] {
                Image(decorative: image, scale: 1).resizable().interpolation(.medium).aspectRatio(contentMode: .fit)
            } else {
                Image(systemName: g.components.first?.kind.symbol ?? "circle.dashed")
                    .font(Theme.Fonts.icon).foregroundStyle(Theme.textTertiary)
            }
        }
        .frame(width: 36, height: 28)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
        .opacity(g.enabled ? 1 : Theme.Opacity.hidden)
    }

    // MARK: Selected group

    private func detail(_ g: MaskGroupInfo) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Hairline().padding(.vertical, Theme.Space.xs)
            HStack(spacing: Theme.Space.xs) {
                Text("Components").font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
                Spacer()
                ForEach([MaskCombineMode.add, .subtract, .intersect], id: \.self) { mode in
                    Menu {
                        Section("AI") {
                            Button("Subject") { masks.target = (g.id, mode); masks.runAI(.subject, title: "Subject") }
                            Button("Sky") { masks.target = (g.id, mode); masks.runAI(.sky, title: "Sky") }
                            Button("Background") { masks.target = (g.id, mode); masks.runAI(.background, title: "Background") }
                            Button("People…") { masks.arm(.person, combine: mode) }
                            Button("Objects…") { masks.arm(.object, combine: mode) }
                        }
                        ForEach([MaskTool.brush, .linear, .radial, .colorRange, .luminanceRange], id: \.self) { t in
                            if !(t == .brush && mode != .add) { Button(t.title) { masks.arm(t, combine: mode) } }
                        }
                    } label: {
                        Text(mode.title)
                    }
                    .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
                    .help("\(mode.title) a component \(mode == .add ? "to" : mode == .subtract ? "from" : "with") this mask")
                }
            }
            ForEach(Array(g.components.enumerated()), id: \.offset) { i, c in componentRow(g, i, c) }
            HStack(spacing: Theme.Space.s) {
                Toggle("Invert", isOn: Binding(get: { g.invert }, set: { _ in masks.invertSelected() }))
                    .toggleStyle(.checkbox).font(Theme.Fonts.caption).controlSize(.small)
                    .help("Invert the whole mask (X)")
                Spacer()
                Button("Reset Sliders") { masks.resetParams() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
            }
            VStack(alignment: .leading, spacing: 0) {
                MaskAmountSlider(masks: masks, groupID: g.id).frame(height: Theme.Height.slider)
                ForEach(LocalParam.sections, id: \.0) { section in
                    SubHeader(section.0)
                    ForEach(section.1) { p in
                        MaskParamSlider(masks: masks, param: p, groupID: g.id).frame(height: Theme.Height.slider)
                    }
                }
            }
        }
    }

    private func componentRow(_ g: MaskGroupInfo, _ i: Int, _ c: MaskComponentInfo) -> some View {
        HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
            Image(systemName: c.kind.symbol).font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.Space.l)
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(c.title).font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary).lineLimit(1)
                switch c.ai {
                case .pending(let f, let m):
                    let live = c.aiKey.flatMap { masks.list.progress[$0] }
                    ProgressView(value: Double(live?.fraction ?? f)) {
                        Text(live?.message ?? m).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    }
                    .controlSize(.mini)
                case .failed(let m):
                    HStack(spacing: Theme.Space.xs) {
                        StatusLine(text: m, kind: .error).lineLimit(2)
                        if let key = c.aiKey {
                            Button("Retry") { masks.retry(key) }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        }
                    }
                default:
                    if !c.rendered {
                        Text("Kept, not drawn by this version").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                    }
                }
            }
            Spacer(minLength: 0)
            if i > 0 {
                Menu {
                    ForEach([MaskCombineMode.add, .subtract, .intersect], id: \.self) { m in
                        Button(m.title) { masks.setComponentMode(i, combine: m, invert: c.invert) }
                    }
                } label: {
                    Text(c.combine.sign).font(Theme.Fonts.captionSemibold)
                }
                .menuStyle(IconMenuStyle())
                .help("\(c.combine.title) (change how this component combines)")
            }
            IconButton(symbol: "circle.righthalf.filled", help: "Invert this component", on: c.invert, size: Theme.Height.small) {
                masks.setComponentMode(i, combine: c.combine, invert: !c.invert)
            }
            IconButton(symbol: "xmark", help: g.components.count == 1 ? "Delete the mask" : "Remove this component",
                       size: Theme.Height.small) {
                masks.removeComponent(i)
            }
        }
        .padding(.vertical, Theme.Space.xxs)
    }
}

/// A local slider of the selected mask group, bridged like `ControlSlider`: drags bypass SwiftUI.
struct MaskParamSlider: NSViewRepresentable {
    let masks: MaskTools
    let param: LocalParam
    let groupID: UInt32

    @MainActor final class Coordinator { var groupID: UInt32 = 0 }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        s.title = param.title
        s.minValue = param.range.lowerBound
        s.maxValue = param.range.upperBound
        s.defaultValue = 0
        s.valueFormat = param.format
        s.step = param.step
        let (masks, param, coordinator) = (masks, param, context.coordinator)
        s.onChange = { v, final in
            guard masks.list.selectedID == coordinator.groupID else { return }
            masks.setParam(param, v, final: final)
        }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        _ = masks.revision
        context.coordinator.groupID = groupID
        if !s.isDragging { s.doubleValue = masks.list.param(param.name) }
        s.needsDisplay = true
    }
}

struct MaskAmountSlider: NSViewRepresentable {
    let masks: MaskTools
    let groupID: UInt32

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        s.title = "Amount"
        s.minValue = 0
        s.maxValue = 200
        s.defaultValue = 100
        s.valueFormat = "%.0f%%"
        s.step = 1
        let masks = masks
        s.onChange = { v, final in masks.setAmount(v, final: final) }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        _ = masks.revision
        if !s.isDragging { s.doubleValue = Double(masks.selected?.amount ?? 100) }
        s.needsDisplay = true
    }
}
