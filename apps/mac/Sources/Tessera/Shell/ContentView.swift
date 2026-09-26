import TesseraCore
import SwiftUI

/// Window layout: sidebar | (grid or loupe) + status bar + filmstrip | inspector.
struct ContentView: View {
    @Bindable var model: AppModel

    var body: some View {
        NavigationSplitView(columnVisibility: Binding(get: { model.documents.columnVisibility },
                                                      set: { model.documents.columnVisibility = $0 })) {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: Theme.Width.sidebarMin, ideal: Theme.Width.sidebarIdeal,
                                                max: Theme.Width.sidebarMax)
        } detail: {
            VStack(spacing: 0) {
                if model.isEngineBacked, model.viewMode != .document {
                    FilterBar(library: model.collections, model: model)
                }
                if model.tether.showPanel {
                    TetherPanel(model: model, tether: model.tether)
                }
                ZStack {
                    // Both stay alive so grid scroll position and loupe texture survive mode switches.
                    ThumbnailBrowser(model: model, style: .grid)
                        .opacity(model.viewMode == .grid ? 1 : 0)
                        .allowsHitTesting(model.viewMode == .grid)
                    LoupeView(model: model)
                        .opacity(model.viewMode == .loupe ? 1 : 0)
                        .allowsHitTesting(model.viewMode == .loupe)
                    if model.viewMode == .loupe {
                        LoupeOverlay(model: model)
                        MaskToolbar(model: model, masks: .shared)
                    }
                    if model.viewMode == .compare, model.compare != nil {
                        CompareView(model: model)
                    }
                    if model.library.items.isEmpty, model.viewMode != .document {
                        EmptyStateView(model: model)
                    }
                    if model.viewMode == .document {
                        DocumentView(workspace: model.documents)
                    }
                    VStack {
                        Spacer()
                        if let toast = model.toast {
                            ToastView(model: model, toast: toast)
                                .padding(.bottom, Theme.Space.l)
                                .transition(Theme.Motion.transition(from: .bottom))
                        }
                    }
                    .animation(Theme.Motion.appear, value: model.toast)
                }
                if model.viewMode == .loupe, !model.assist.faces.isEmpty {
                    FaceStrip(model: model)
                }
                AssistProgressBar(assist: model.assist)
                UnderstandingProgressBar(understanding: model.collections.understanding)
                AgentProgressBar(agent: model.agent)
                LightroomImportProgressBar(importer: model.lightroomImport)
                ExportProgressBar(exporter: model.exporter)
                PrintProgressBar(printing: model.printing)
                if model.viewMode == .document {
                    DocumentStatusBar(model: model, workspace: model.documents)
                } else {
                    StatusBar(model: model)
                }
                if model.showFilmstrip, !model.library.items.isEmpty, model.viewMode != .document {
                    Hairline()
                    ThumbnailBrowser(model: model, style: .filmstrip)
                        .frame(height: Theme.Height.filmstrip)
                }
            }
            .background(Theme.canvas)
            .navigationTitle(model.viewMode == .document ? (model.documents.current?.title ?? "Tessera")
                             : model.library.items.isEmpty ? "Tessera" : model.library.title)
            .navigationSubtitle(subtitle)
        }
        .sheet(isPresented: Binding(get: { model.documents.showNewDocument }, set: { model.documents.showNewDocument = $0 })) {
            NewDocumentSheet(workspace: model.documents)
        }
        .sheet(isPresented: Binding(get: { model.documents.showExportFlat }, set: { model.documents.showExportFlat = $0 })) {
            ExportFlatSheet(workspace: model.documents)
        }
        .sheet(isPresented: $model.showDefectSweep) {
            DefectSweepSheet(model: model)
        }
        .sheet(isPresented: $model.showAutoEdit) {
            AutoEditSheet(agent: model.agent, model: model)
        }
        .sheet(isPresented: Binding(get: { model.agent.showReview }, set: { model.agent.showReview = $0 })) {
            AgentReviewSheet(agent: model.agent, model: model)
        }
        .sheet(isPresented: $model.showLightroomImport) {
            LightroomImportSheet(importer: model.lightroomImport)
        }
        .sheet(isPresented: $model.showExport) {
            ExportSheet(exporter: model.exporter)
        }
        .sheet(isPresented: $model.showPrint) {
            PrintSheet(printing: model.printing, model: model)
        }
        .sheet(isPresented: Binding(get: { model.collections.editor != nil },
                                    set: { if !$0 { model.collections.editor = nil } })) {
            SmartAlbumSheet(library: model.collections)
        }
        .inspector(isPresented: $model.showInspector) {
            Group {
                if model.viewMode == .document {
                    DocumentInspector(workspace: model.documents)
                } else {
                    InspectorView(model: model)
                }
            }
                .inspectorColumnWidth(min: Theme.Width.inspectorMin, ideal: Theme.Width.inspectorIdeal,
                                      max: Theme.Width.inspectorMax)
        }
        .toolbar { toolbar }
        .tint(Theme.accent)
        .background(WindowToolbarConfigurator())
    }

    private var subtitle: String {
        if model.viewMode == .document {
            guard let doc = model.documents.current else { return "" }
            return (doc.isDirty ? "Edited · " : "") + "\(doc.layers.count) layer\(doc.layers.count == 1 ? "" : "s")"
        }
        let n = model.visibleCount
        guard n > 0 else { return "" }
        return model.source == .all ? "\(n.formatted()) images" : "\(model.source.title) · \(n.formatted()) images"
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(id: "open", placement: .navigation) {
            Button { model.presentOpenPanel() } label: {
                Label("Open Folder…", systemImage: "folder")
            }
            .buttonStyle(ToolbarButtonStyle())
            .help("Open a folder of JPEG / RAW images (⌘O)")
        }
        .flatToolbarItem()
        ToolbarItem(id: "mode", placement: .principal) {
            SegmentedPicker(selection: $model.viewMode, segments: [
                .init(value: ViewMode.grid, title: "Grid", symbol: "square.grid.2x2", help: "Grid (G)"),
                .init(value: ViewMode.loupe, title: "Loupe", symbol: "photo", help: "Loupe (E or Return)"),
                .init(value: ViewMode.compare, title: "Compare", symbol: "rectangle.split.2x1", help: "Compare (C)"),
                .init(value: ViewMode.document, title: "Layers", symbol: "square.3.layers.3d",
                      help: "Layered documents (⌘N new, ⌘E edits the photo in layers)"),
            ], fill: false)
            .fixedSize()
            .accessibilityLabel("View")
        }
        .flatToolbarItem()
        ToolbarItem(id: "documents", placement: .navigation) {
            if model.viewMode == .document, !model.documents.documents.isEmpty {
                DocumentTabs(workspace: model.documents)
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "size", placement: .primaryAction) {
            // Grid only; in the loupe and compare the space stays empty so the toggles do not move.
            HStack(spacing: Theme.Space.xs) {
                Image(systemName: "square.grid.3x3").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                Slider(value: $model.thumbnailSize, in: 110...360)
                    .controlSize(.mini)
                    .frame(width: Theme.Width.thumbnailSlider)
                Image(systemName: "square.grid.2x2").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
            }
            .opacity(model.viewMode == .grid ? 1 : 0)
            .disabled(model.viewMode != .grid)
            .accessibilityHidden(model.viewMode != .grid)
            .help("Thumbnail size")
        }
        .flatToolbarItem()
        ToolbarItem(id: "assist", placement: .primaryAction) {
            HStack(spacing: Theme.Space.xxs) {
                Toggle(isOn: Binding(get: { model.assist.enabled }, set: { model.assist.setEnabled($0) })) {
                    Label("Assist", systemImage: "sparkles")
                }
                .toggleStyle(ToolbarToggleStyle())
                .help("Assisted culling: keep predictions, a confidence order and suggested decisions (Y confirms, N dismisses)")
                .accessibilityIdentifier("toolbar-assist")
                if model.assist.enabled {
                    Menu {
                        Picker("Mode", selection: Binding(get: { model.agent.preferences.assistAutomated },
                                                          set: { model.assist.setAutomated($0) })) {
                            Text("Suggest decisions (automated)").tag(true)
                            Text("Predictions only (assisted)").tag(false)
                        }
                        .pickerStyle(.inline)
                        Divider()
                        Toggle("Sort by Keep Confidence", isOn: Binding(get: { model.assist.sortByConfidence },
                                                                         set: { model.assist.sortByConfidence = $0 }))
                        Button("Confirm \(model.assist.suggestionCount) Suggested    (Y)") { model.assist.confirmAll() }
                            .disabled(model.assist.suggestionCount == 0)
                    } label: {
                        Image(systemName: "chevron.down").font(Theme.Fonts.iconSmall)
                    }
                    .menuStyle(IconMenuStyle())
                    .help("Assist mode and sort")
                    .accessibilityIdentifier("toolbar-assist-menu")
                }
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "autoEdit", placement: .primaryAction) {
            Button { model.agent.present() } label: {
                Label(model.agent.isRunning ? "Editing…" : "Auto Edit", systemImage: "wand.and.stars")
            }
            .buttonStyle(ToolbarButtonStyle())
            .disabled(!model.isEngineBacked || model.agent.isRunning)
            .help("Auto Edit: the agent makes a non-generative base edit (⇧⌘A)")
            .accessibilityIdentifier("toolbar-auto-edit")
        }
        .flatToolbarItem()
        ToolbarItem(id: "review", placement: .primaryAction) {
            if !model.agent.queue.isEmpty {
                Button { model.agent.showReview = true } label: {
                    Label("Review \(model.agent.queue.pendingCount)", systemImage: "checklist")
                }
                .buttonStyle(ToolbarButtonStyle())
                .help("Agent review queue, least confident first")
                .accessibilityIdentifier("toolbar-agent-review")
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "autoAdvance", placement: .primaryAction) {
            Toggle(isOn: $model.autoAdvance) {
                Label("Auto-advance", systemImage: "arrow.right.to.line")
            }
            .toggleStyle(ToolbarToggleStyle())
            .help("Move to the next image after X / U / P / 1–3 (A)")
        }
        .flatToolbarItem()
        ToolbarItem(id: "inspector", placement: .primaryAction) {
            Toggle(isOn: $model.showInspector) {
                Label("Inspector", systemImage: "sidebar.right")
            }
            .toggleStyle(ToolbarToggleStyle())
            .help("Show or hide the inspector (⌥⌘I)")
        }
        .flatToolbarItem()
    }
}

extension ToolbarContent {
    /// macOS 26 wraps each toolbar item in a glass capsule; Tessera's toolbar is one flat bar.
    @ToolbarContentBuilder
    func flatToolbarItem() -> some ToolbarContent {
        if #available(macOS 26.0, *) {
            sharedBackgroundVisibility(.hidden)
        } else {
            self
        }
    }
}

/// Toolbar toggle: icon + label, flat. On = a neutral pressed fill and primary text (the accent
/// stays reserved for content selection, focus and primary actions).
struct ToolbarToggleStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button { configuration.isOn.toggle() } label: {
            configuration.label
                .labelStyle(.titleAndIcon)
        }
        .buttonStyle(ToolbarButtonStyle(on: configuration.isOn))
        .accessibilityAddTraits(configuration.isOn ? .isSelected : [])
    }
}

/// Flat toolbar button: 28 pt, radius 6, hover and pressed fills only.
struct ToolbarButtonStyle: ButtonStyle {
    var on = false
    func makeBody(configuration: Configuration) -> some View {
        ToolbarButtonBody(configuration: configuration, on: on)
    }
}

private struct ToolbarButtonBody: View {
    let configuration: ButtonStyle.Configuration
    let on: Bool
    @State private var hovering = false
    var body: some View {
        configuration.label
            .labelStyle(.titleAndIcon)
            .font(Theme.Fonts.label)
            .foregroundStyle(on ? Theme.textPrimary : Theme.textSecondary)
            .padding(.horizontal, Theme.Space.s)
            .frame(height: Theme.Height.large)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.control)
                .fill(configuration.isPressed || on ? Theme.pressed : hovering ? Theme.hover : Theme.clear))
            .contentShape(Rectangle())
            .onHover { hovering = $0 }
    }
}

/// Lets the window's toolbar keep the title visible and the items flat (no per-item glass).
struct WindowToolbarConfigurator: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { NSView() }
    func updateNSView(_ view: NSView, context: Context) {
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                guard let window = view.window else { return }
                window.titlebarSeparatorStyle = .line
            }
        }
    }
}

/// One line, three groups: where you are (position, group, decision) · what happened (message) ·
/// the session (counts, basket). Hairline-separated, tabular figures throughout.
struct StatusBar: View {
    let model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            Hairline()
            HStack(spacing: Theme.Space.m) {
                if let item = model.focusedItem, let p = model.focusedPosition {
                    HStack(spacing: Theme.Space.s) {
                        Text("\((p + 1).formatted()) of \(model.visibleCount.formatted())")
                            .foregroundStyle(Theme.textPrimary)
                        Text("G\(item.groupID + 1) · \(model.indexInGroup(of: item) + 1)/\(model.groupSize(of: item))"
                             + (model.focusedIsBest ? " · suggested best" : ""))
                        HStack(spacing: Theme.Space.xs) {
                            Circle().fill(Color(nsColor: model.focusedState.decision.color))
                                .frame(width: Theme.Space.s - Theme.Space.xxs, height: Theme.Space.s - Theme.Space.xxs)
                            Text(stateText)
                        }
                        if model.selectionCount > 1 {
                            Text("\(model.selectionCount.formatted()) selected").foregroundStyle(Theme.accent)
                        }
                    }
                    .fixedSize()
                    separator
                }
                if let person = model.assist.personFilterTitle {
                    Button { model.assist.clearPersonFilter() } label: {
                        HStack(spacing: Theme.Space.xs) {
                            Text(person)
                            Image(systemName: "xmark.circle.fill").font(Theme.Fonts.iconSmall)
                        }
                    }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .foregroundStyle(Theme.accent)
                    .help("Showing frames with this person only. Click to show all.")
                    .accessibilityIdentifier("status-person-filter")
                    separator
                }
                if let msg = model.statusMessage {
                    Text(msg).lineLimit(1).truncationMode(.tail).foregroundStyle(Theme.textTertiary)
                        .help(msg)
                }
                Spacer(minLength: Theme.Space.s)
                if model.showRenderReadout, let readout = model.renderReadout {
                    Text(readout)
                        .foregroundStyle(Theme.textTertiary)
                        .help("Settings change → frame in the loupe surface (Debug ▸ Show Render Timing)")
                        .accessibilityIdentifier("renderReadout")
                    separator
                }
                HStack(spacing: Theme.Space.s) {
                    Text("Keep \(model.counts.keep.formatted())")
                    Text("Reject \(model.counts.reject.formatted())")
                }
                .fixedSize()
                separator
                HStack(spacing: Theme.Space.xs) {
                    RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.basket)
                        .frame(width: Theme.Space.s, height: Theme.Space.s)
                    Text("\(model.basketTarget) \(model.counts.basket.formatted())")
                }
                .fixedSize()
                .help("Basket target: B adds to this album. Change it in Cull ▸ Basket Target or the sidebar.")
            }
            .font(Theme.Fonts.captionNumeric)
            .foregroundStyle(Theme.textSecondary)
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.statusBar)
        }
        .background(Theme.panel)
    }

    private var separator: some View {
        Hairline(vertical: true).frame(height: Theme.Space.m)
    }

    private var stateText: String {
        let s = model.focusedState
        var parts = [s.decision.label]
        if s.grade > 0 { parts.append("Grade \(s.grade) (\(CullState.gradeNames[Int(s.grade)]))") }
        if s.mark > 0 { parts.append("Mark \(s.mark)") }
        if s.inBasket { parts.append("In \(model.basketTarget)") }
        if model.focusedStatus.phase != .unedited { parts.append(model.focusedStatus.phase.rawValue.capitalized) }
        return parts.joined(separator: " · ")
    }
}

/// Loupe chrome: file and decision (top left), display / proof state (top right), shortcuts
/// (bottom). A reserved 32 pt top strip keeps the mask toolbar clear of this text.
struct LoupeOverlay: View {
    let model: AppModel
    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: Theme.Space.s) {
                if let item = model.focusedItem {
                    Text(item.name).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary)
                        .allowsHitTesting(false)
                    if let badge = model.focusedState.badgeText {
                        Chip(text: badge, color: Color(nsColor: model.focusedState.decision.color), style: .outlined,
                             height: Theme.Height.chip)
                    }
                    if model.focusedIsBest {
                        Chip(text: "Suggested best · K keeps it, rejects the rest", color: Theme.keep, style: .outlined,
                             height: Theme.Height.chip)
                    }
                }
                Spacer(minLength: Theme.Space.s)
                if SoftProof.shared.enabled {
                    Chip(text: SoftProof.shared.lut.map { "Soft proof · \($0.profileName)" + (SoftProof.shared.gamutWarning ? " · gamut warning" : "") }
                         ?? SoftProof.shared.status, color: Theme.accent, style: .outlined, height: Theme.Height.chip)
                        .accessibilityIdentifier("softproof-badge")
                }
                Text(model.loupeInfo).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).lineLimit(1)
                    .allowsHitTesting(false)
                if model.developStatus == .ready, !MaskTools.shared.active {
                    Button { MaskTools.shared.setActive(true) } label: {
                        Label("Masks", systemImage: "circle.lefthalf.striped.horizontal")
                    }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Local adjustments with masks (M)")
                }
            }
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.sectionHeader)
            Spacer()
                .allowsHitTesting(false)
            Text("← → group  ·  ↑ ↓ frame in group  ·  X U P decide  ·  1 2 3 grade  ·  K keep best  ·  C compare  ·  Y N suggestions  ·  ⌘Z undo  ·  Esc grid")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                .lineLimit(1)
                .padding(.bottom, Theme.Space.s)
                .allowsHitTesting(false)
        }
    }
}

struct EmptyStateView: View {
    let model: AppModel
    var body: some View {
        EmptyStateContent(symbol: "photo.on.rectangle.angled",
                          title: model.isLoading ? "Reading folder…" : "No images",
                          message: "Open a folder of JPEG or RAW files to start culling.") {
            Button("Open Folder…") { model.presentOpenPanel() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large))
                .keyboardShortcut(.defaultAction)
            Button("Load 20,000 Stub Items") { model.loadStubItems(count: 20_000) }
                .buttonStyle(.theme(.bordered, height: Theme.Height.large))
        }
        .disabled(model.isLoading)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.canvas)
    }
}

struct ToastView: View {
    let model: AppModel
    let toast: Toast
    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.m) {
                Text(toast.message).font(Theme.Fonts.label).foregroundStyle(Theme.textPrimary).lineLimit(1)
                if toast.undoable {
                    Button { model.undo() } label: {
                        HStack(spacing: Theme.Space.xs) {
                            Text("Undo").font(Theme.Fonts.labelMedium).foregroundStyle(Theme.accent)
                            Text("⌘Z").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                        }
                    }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                }
                if !toast.details.isEmpty {
                    Button("Dismiss") { model.toast = nil }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                }
            }
            ForEach(Array(toast.details.prefix(5).enumerated()), id: \.offset) { _, line in
                StatusLine(text: line, kind: .error).lineLimit(1).truncationMode(.middle)
            }
            if toast.details.count > 5 {
                Text("and \(toast.details.count - 5) more").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            }
        }
        .padding(.leading, Theme.Space.l)
        .padding(.trailing, Theme.Space.xs)
        .padding(.vertical, Theme.Space.xs)
        .frame(minHeight: Theme.Height.large + Theme.Space.s)
        .background(HUDBackground())
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("toast")
    }
}
