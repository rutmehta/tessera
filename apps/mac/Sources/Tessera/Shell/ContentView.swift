import TesseraCore
import SwiftUI

/// Window layout: sidebar | (grid or loupe) + status bar + filmstrip | inspector.
struct ContentView: View {
    @Bindable var model: AppModel
    /// B5-10: document mode's detail minimum (see the frame below and DocumentInspectorLayoutTests).
    static let documentDetailMinWidth = ShellBudget.detailMinWidth
    /// M2-56: the window's content size (from `root`; zero when hosted directly), for the yield
    /// order (ShellBudget).
    var windowSize: CGSize = .zero
    /// The canvas height without the filmstrip (0 before the first layout).
    @State private var canvasBase: CGFloat = 0
    /// The sidebar visibility the person had when the window became too narrow for the sidebar
    /// (nil when the sidebar was not collapsed by the budget).
    @State private var sidebarAutoCollapsed: NavigationSplitViewVisibility?

    /// The window's root view: the shell at the declared minimum window size (WP M2-56).
    /// The GeometryReader reports the window's size (not the split view's), and places an
    /// oversized shell at the top-leading corner instead of centring it at negative origins.
    static func root(model: AppModel) -> some View {
        GeometryReader { window in
            ContentView(model: model, windowSize: window.size)
        }
        .frame(minWidth: ShellBudget.minWindow.width, minHeight: ShellBudget.minWindow.height)
    }

    var body: some View {
        NavigationSplitView(columnVisibility: columnVisibility) {
            SidebarView(model: model)
                .containedColumn()
                .navigationSplitViewColumnWidth(min: Theme.Width.sidebarMin, ideal: Theme.Width.sidebarIdeal,
                                                max: Theme.Width.sidebarMax)
        } detail: {
            VStack(spacing: 0) {
                if model.viewMode != .document { WorkspaceHeader(model: model) }
                if model.isEngineBacked, model.source != .people, model.isLibraryWorkspace {
                    FilterBar(library: model.collections, model: model)
                }
                if model.tether.showPanel {
                    TetherPanel(model: model, tether: model.tether)
                }
                GeometryReader { area in
                    ZStack {
                        // Both stay alive so grid scroll position and loupe texture survive mode switches.
                        ThumbnailBrowser(model: model, style: .grid)
                            .onAppear { PerformanceTrace.shared.record("grid_appeared") }
                            .opacity(model.viewMode == .grid ? 1 : 0)
                            .allowsHitTesting(model.viewMode == .grid)
                        LoupeView(model: model)
                            .opacity(model.viewMode == .loupe ? 1 : 0)
                            .allowsHitTesting(model.viewMode == .loupe)
                        if model.viewMode == .loupe {
                            LoupeOverlay(model: model)
                            if model.isPhotoEditing { MaskToolbar(model: model, masks: .shared) }
                        }
                        if model.viewMode == .compare, model.compare != nil {
                            CompareView(model: model)
                        }
                        if model.viewMode == .document {
                            DocumentView(workspace: model.documents)
                        } else if model.isReviewing {
                            AgentReviewWorkspace(model: model)
                        } else if model.source == .people && !model.isPhotoEditing {
                            PeopleView(model: model)
                        } else if model.library.items.isEmpty {
                            EmptyStateView(model: model)
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
                    .frame(width: area.size.width, height: area.size.height)
                    .clipped()
                }
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height in
                    canvasBase = height + (showsFilmstrip ? Self.filmstripSpan : 0)
                }
                if model.viewMode == .loupe, model.source != .people, !model.assist.faces.isEmpty {
                    FaceStrip(model: model)
                }
                AssistProgressBar(assist: model.assist)
                UnderstandingProgressBar(understanding: model.collections.understanding)
                AgentProgressBar(agent: model.agent)
                LightroomImportProgressBar(importer: model.lightroomImport)
                ExportProgressBar(exporter: model.exporter)
                PrintProgressBar(printing: model.printing)
                PhotoJobProgressBar(jobs: model.photoJobs)   // M2-50
                if model.viewMode == .document {
                    DocumentStatusBar(model: model, workspace: model.documents)
                } else if model.isReviewing {
                    HStack {
                        Text(model.statusMessage ?? model.agent.queue.summary)
                            .lineLimit(1).truncationMode(.tail)
                            .help(model.statusMessage ?? model.agent.queue.summary)
                        Spacer(minLength: Theme.Space.m)
                        Text("↑ ↓ Browse · D Edit · Esc Back").fixedSize()
                    }
                    .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .padding(.horizontal, Theme.Space.gutter).frame(height: Theme.Height.statusBar)
                    .background(Theme.panel)
                } else {
                    StatusBar(model: model)
                }
                if showsFilmstrip {
                    Hairline()
                    ThumbnailBrowser(model: model, style: .filmstrip)
                        .frame(height: Theme.Height.filmstrip)
                }
            }
            .background(Theme.canvas)
            .containedColumn()
            // B5-10 begin: document mode's detail column has a small, explicit minimum and ideal width.
            // On macOS 26 the floating sidebar and the inspector overlay the detail, and the split
            // view counts the detail's minimum PLUS both overlays, then adds the inspector column
            // again: with the detail's own minimum taken from its ideal width (the status bar's full
            // message and fixed labels, ~800 pt) the content needed ~1450 pt, so at 1440 pt and below
            // the sidebar and the document inspector were pushed past the window edges (clipped
            // right edges of Properties and Layers). The canvas and its bars shrink instead (the
            // options bar scrolls, the status bar truncates). Library modes are unchanged.
            // M2-56: every mode now has this minimum; the contained column no longer takes its minimum
            // from its content, so library modes need the explicit one too.
            .frame(minWidth: Self.documentDetailMinWidth,
                   idealWidth: model.viewMode == .document ? Self.documentDetailMinWidth : nil, maxWidth: .infinity)
            // B5-10 end
            .navigationTitle(model.viewMode == .document ? (model.documents.current?.title ?? "Tessera")
                             : model.library.items.isEmpty ? "Tessera" : model.library.title)
            .navigationSubtitle(subtitle)
        }
        .sheet(item: $model.layeredCopyRequest) { request in
            LayeredCopySheet(model: model, request: request)
        }
        .sheet(isPresented: Binding(get: { model.documents.showNewDocument }, set: { model.documents.showNewDocument = $0 })) {
            NewDocumentSheet(workspace: model.documents)
        }
        .sheet(isPresented: Binding(get: { model.documents.showExportFlat }, set: { model.documents.showExportFlat = $0 })) {
            ExportFlatSheet(workspace: model.documents)
        }
        .sheet(item: Binding(get: { model.documents.saveAsRequest }, set: { model.documents.saveAsRequest = $0 })) { r in
            SaveAsSheet(workspace: model.documents, request: r)
        }
        .sheet(isPresented: $model.showDefectSweep) {
            DefectSweepSheet(model: model)
        }
        .sheet(isPresented: $model.showAutoEdit) {
            AutoEditSheet(agent: model.agent, model: model)
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
        .photoJobSheets(model)   // M2-50: Photo Merge / Enhance
        .sheet(isPresented: Binding(get: { model.collections.editor != nil },
                                    set: { if !$0 { model.collections.editor = nil } })) {
            SmartAlbumSheet(library: model.collections)
        }
        .inspector(isPresented: $model.showInspector) {
            Group {
                if model.viewMode == .document {
                    DocumentInspector(workspace: model.documents)
                } else if model.isReviewing {
                    AgentReviewInspector(model: model, busy: model.agent.isRunning || !model.agent.busy.isEmpty,
                                         canEdit: model.canEnterPhotoEdit)
                } else if model.isPhotoEditing {
                    PhotoEditInspectorView(model: model)
                } else {
                    InspectorView(model: model)
                }
            }
                .containedColumn()
                // M2-56 yield order, step 2: in a narrow window the inspector's ideal and maximum
                // drop to what fits beside the detail minimum (down to its minimum width).
                .inspectorColumnWidth(min: Theme.Width.inspectorMin, ideal: min(Theme.Width.inspectorIdeal, inspectorFit),
                                      max: inspectorFit)
        }
        .toolbar { toolbar }
        .environment(\.toolbarCompact, windowSize.width > 0 && windowSize.width < ShellBudget.compactToolbarWidth)
        .tint(Theme.accent)
        .background(WindowToolbarConfigurator())
        .onChange(of: sidebarFits, initial: true) { _, fits in applySidebarBudget(fits: fits) }
        .onChange(of: model.agent.queue) { _, _ in model.reconcileReviewNavigation() }
        .onChange(of: model.agent.reviewGeneration) { _, _ in model.reconcileReviewNavigation() }
    }

    /// M2-56 yield order, step 1: the sidebar collapses while the window is too narrow for it beside
    /// the detail minimum and the inspector; the person's own choice returns when it fits again
    /// (they can still show it themselves while narrow).
    private var sidebarFits: Bool {
        windowSize.width <= 0 || ShellBudget.sidebarFits(windowWidth: windowSize.width, inspector: model.showInspector)
    }

    private var inspectorFit: CGFloat {
        guard windowSize.width > 0 else { return Theme.Width.inspectorMax }
        let sidebarShown = columnVisibility.wrappedValue != .detailOnly
        return ShellBudget.inspectorFit(windowWidth: windowSize.width, sidebar: sidebarShown)
    }

    private var columnVisibility: Binding<NavigationSplitViewVisibility> {
        Binding(get: { (model.isPhotoEditing || model.isReviewing) ? .detailOnly : model.documents.columnVisibility }, set: { value in
            guard !model.isPhotoEditing, !model.isReviewing else { return }
            // The person's own choice; showing the sidebar in a narrow window keeps it shown.
            if value != .detailOnly { sidebarAutoCollapsed = nil }
            model.documents.columnVisibility = value
        })
    }

    /// Applies step 1 when the window crosses the threshold: collapse (remembering the choice),
    /// and restore that choice once the sidebar fits again.
    private func applySidebarBudget(fits: Bool) {
        guard !model.isPhotoEditing, !model.isReviewing else { return }
        var t = Transaction(); t.disablesAnimations = true
        withTransaction(t) {
            if !fits, sidebarAutoCollapsed == nil, model.documents.columnVisibility != .detailOnly {
                sidebarAutoCollapsed = model.documents.columnVisibility
                model.documents.columnVisibility = .detailOnly
            } else if fits, let restore = sidebarAutoCollapsed {
                sidebarAutoCollapsed = nil
                // Not while the person has hidden all panels (Tab / full screen without panels).
                if model.documents.columnVisibility == .detailOnly, !model.documents.panelsHidden {
                    model.documents.columnVisibility = restore
                }
            }
        }
    }

    /// M2-56 yield order, step 3: the filmstrip hides when the canvas above it would be shorter than
    /// `ShellBudget.canvasMinHeight`. Decided from the height without the strip, so it cannot flicker.
    private var showsFilmstrip: Bool {
        guard model.showFilmstrip, !model.isReviewing, !model.isReviewEditing, !model.library.items.isEmpty, model.viewMode != .document else { return false }
        guard canvasBase > 0 else { return true }
        return ShellBudget.filmstripFits(detailHeight: canvasBase, chrome: 0)
    }

    private static let filmstripSpan = Theme.Height.filmstrip + Theme.Space.hairline

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
            if model.viewMode == .document {
                Button("Library") { model.viewMode = .grid }
                    .buttonStyle(ToolbarButtonStyle())
                    .help("Return to Library; open documents stay available")
            } else {
                SegmentedPicker(selection: Binding(get: {
                    model.isReviewing ? "review" : model.isPhotoEditing ? "edit" : "library"
                }, set: { destination in
                    switch destination {
                    case "review": model.enterReview()
                    case "edit": model.enterPhotoEdit()
                    default: model.returnToLibrary()
                    }
                }), segments: [
                    .init(value: "library", title: "Library", symbol: "square.grid.2x2"),
                    .init(value: "edit", title: "Edit photo", symbol: "slider.horizontal.3"),
                    .init(value: "review", title: "Review", symbol: "checklist"),
                ], fill: false)
                .accessibilityLabel("Workspace")
                .help("Library, one-photo editing, or the agent review queue")
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "library-view", placement: .primaryAction) {
            if model.isLibraryWorkspace {
                SegmentedPicker(selection: $model.viewMode, segments: [
                    .init(value: ViewMode.grid, title: "Grid", symbol: "square.grid.2x2", help: "Grid (G)"),
                    .init(value: ViewMode.loupe, title: "Loupe", symbol: "photo", help: "Loupe (E or Return)"),
                    .init(value: ViewMode.compare, title: "Compare", symbol: "rectangle.split.2x1", help: "Compare (C)"),
                ], fill: false)
                .accessibilityLabel("Library view")
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "documents", placement: .navigation) {
            if model.viewMode == .document, !model.documents.documents.isEmpty {
                DocumentTabs(workspace: model.documents)
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "size", placement: .primaryAction) {
            // Grid only. M2-56: removed (not transparent) in the other modes, so its 130 pt slot
            // goes back to the toolbar instead of pushing items into the overflow menu.
            if model.viewMode == .grid {
                HStack(spacing: Theme.Space.xs) {
                    Image(systemName: "square.grid.3x3").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                    Slider(value: $model.thumbnailSize, in: 110...360)
                        .controlSize(.mini)
                        .frame(width: Theme.Width.thumbnailSlider)
                    Image(systemName: "square.grid.2x2").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                }
                .help("Thumbnail size")
            }
        }
        .flatToolbarItem()
        libraryActionToolbar
        ToolbarItem(id: "inspector", placement: .primaryAction) {
            Toggle(isOn: $model.showInspector) {
                Label("Inspector", systemImage: "sidebar.right")
            }
            .toggleStyle(ToolbarToggleStyle())
            .help("Show or hide the inspector (⌥⌘I)")
        }
        .flatToolbarItem()
    }

    @ToolbarContentBuilder
    private var libraryActionToolbar: some ToolbarContent {
        ToolbarItem(id: "assist", placement: .primaryAction) {
            if model.isLibraryWorkspace {
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
        }
        .flatToolbarItem()
        ToolbarItem(id: "peopleMerge", placement: .primaryAction) {
            if model.source == .people, model.isLibraryWorkspace {
                Button { model.people.mergeSelection(); model.peopleDidChange() } label: {
                    Label("Merge", systemImage: "person.2.badge.plus")
                }
                .buttonStyle(ToolbarButtonStyle())
                .disabled(!model.people.canMerge || model.people.detailID != nil)
                .help("Merge the selected people into one (a named person's name wins). Select tiles with ⌘-click.")
                .accessibilityIdentifier("toolbar-people-merge")
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "autoEdit", placement: .primaryAction) {
            if model.isLibraryWorkspace {
            Button { model.agent.present() } label: {
                Label(model.agent.isRunning ? "Editing…" : "Auto Edit", systemImage: "wand.and.stars")
            }
            .buttonStyle(ToolbarButtonStyle())
            .disabled(!model.isEngineBacked || model.agent.isRunning)
            .help("Auto Edit: the agent makes a non-generative base edit (⇧⌘A)")
            .accessibilityIdentifier("toolbar-auto-edit")
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "review", placement: .primaryAction) {
            if model.viewMode != .document {
                Button { model.enterReview() } label: {
                    Label("Review \(model.agent.queue.pendingCount)", systemImage: "checklist")
                }
                .buttonStyle(ToolbarButtonStyle())
                .help("Agent review queue, least confident first")
                .accessibilityIdentifier("toolbar-agent-review")
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "autoAdvance", placement: .primaryAction) {
            if model.isLibraryWorkspace {
            Toggle(isOn: $model.autoAdvance) {
                Label("Auto-advance", systemImage: "arrow.right.to.line")
            }
            .toggleStyle(ToolbarToggleStyle())
            .help("Move to the next image after X / U / P / 1–3 (A)")
            }
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
    /// M2-56: icons only in a narrow window (the title stays the help / accessibility label).
    @Environment(\.toolbarCompact) private var compact
    var body: some View {
        Group {
            if compact {
                configuration.label.labelStyle(.iconOnly)
            } else {
                configuration.label.labelStyle(.titleAndIcon)
            }
        }
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
            // M2-56 (L3): the full row when it fits, else a compact one (position, decision, counts
            // as K / R, the basket's count without its name; every full string stays in help).
            // The message never decides which fits: it takes whatever width is left and truncates.
            ViewThatFits(in: .horizontal) {
                row(compact: false)
                row(compact: true)
            }
            .font(Theme.Fonts.captionNumeric)
            .foregroundStyle(Theme.textSecondary)
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.statusBar)
        }
        .background(Theme.panel)
    }

    private func row(compact: Bool) -> some View {
        HStack(spacing: Theme.Space.m) {
            if let item = model.focusedItem, let p = model.focusedPosition {
                HStack(spacing: Theme.Space.s) {
                    Text("\((p + 1).formatted()) of \(model.visibleCount.formatted())")
                        .foregroundStyle(Theme.textPrimary)
                    if !compact {
                        Text("G\(item.groupID + 1) · \(model.indexInGroup(of: item) + 1)/\(model.groupSize(of: item))"
                             + (model.focusedIsBest ? " · suggested best" : ""))
                    }
                    HStack(spacing: Theme.Space.xs) {
                        Circle().fill(Color(nsColor: model.focusedState.decision.color))
                            .frame(width: Theme.Space.s - Theme.Space.xxs, height: Theme.Space.s - Theme.Space.xxs)
                        Text(compact ? model.focusedState.decision.label : stateText)
                    }
                    .help(stateText)
                    if model.selectionCount > 1 {
                        Text(compact ? "+\((model.selectionCount - 1).formatted())" : "\(model.selectionCount.formatted()) selected")
                            .foregroundStyle(Theme.accent)
                            .help("\(model.selectionCount.formatted()) selected")
                    }
                }
                .fixedSize()
                separator
            }
            if let person = model.assist.personFilterTitle {
                Button { model.assist.clearPersonFilter() } label: {
                    HStack(spacing: Theme.Space.xs) {
                        Text(person).lineLimit(1).truncationMode(.middle)
                        Image(systemName: "xmark.circle.fill").font(Theme.Fonts.iconSmall)
                    }
                }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .foregroundStyle(Theme.accent)
                .help("Showing frames with \(person) only. Click to show all.")
                .accessibilityIdentifier("status-person-filter")
                .frame(maxWidth: compact ? Theme.Width.labelWide : nil)
                separator
            }
            // Ideal width 0: the message fills what is left and truncates; it never makes a row "not fit".
            Group {
                if let msg = model.statusMessage {
                    Text(msg).lineLimit(1).truncationMode(.tail).foregroundStyle(Theme.textTertiary)
                        .help(msg)
                } else {
                    Color.clear
                }
            }
            .frame(minWidth: 0, idealWidth: 0, maxWidth: .infinity, alignment: .leading)
            if !compact, model.showRenderReadout, let readout = model.renderReadout {
                Text(readout)
                    .foregroundStyle(Theme.textTertiary)
                    .help("Settings change → frame in the loupe surface (Debug ▸ Show Render Timing)")
                    .accessibilityIdentifier("renderReadout")
                    .fixedSize()
                separator
            }
            HStack(spacing: Theme.Space.s) {
                Text(compact ? "K \(model.counts.keep.formatted())" : "Keep \(model.counts.keep.formatted())")
                Text(compact ? "R \(model.counts.reject.formatted())" : "Reject \(model.counts.reject.formatted())")
            }
            .fixedSize()
            .help("Keep \(model.counts.keep.formatted()) · Reject \(model.counts.reject.formatted())")
            separator
            HStack(spacing: Theme.Space.xs) {
                RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.basket)
                    .frame(width: Theme.Space.s, height: Theme.Space.s)
                Text(compact ? model.counts.basket.formatted() : "\(model.basketTarget) \(model.counts.basket.formatted())")
            }
            .fixedSize()
            .help("Basket target \(model.basketTarget): B adds to this album. Change it in Cull ▸ Basket Target or the sidebar.")
        }
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

struct EmptyStateView: View {
    let model: AppModel
    var body: some View {
        EmptyStateContent(symbol: "photo.on.rectangle.angled",
                          title: model.isLoading ? "Reading folder…" : "No images",
                          message: "Open a folder of JPEG or RAW files to start culling.") {
            Button("Open Folder…") { model.presentOpenPanel() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large))
                .keyboardShortcut(.defaultAction)
            if StubLibraryDiagnostics.isEnabled {
                Button("Load 20,000 Stub Items") { model.loadStubItems(count: 20_000) }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.large))
            }
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
