import TesseraCore
import SwiftUI

/// Window layout: one toolbar (folder popover · Cull / Edit / Deliver · search · panel · Export) over
/// (grid or loupe, with the agent field at the bottom) + status bar + filmstrip | inspector. The
/// source sidebar is hidden by default (View ▸ Show Library Sidebar); Tab hides the inspector per
/// mode (WP UX-11, redesign SPEC §1).
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
    /// The folder button's popover: the source sidebar's content (folders, albums, people, lists).
    @State private var showFolders = false

    /// The window's root view: the shell at the declared minimum window size (WP M2-56).
    /// The GeometryReader reports the window's size (not the split view's), and places an
    /// oversized shell at the top-leading corner instead of centring it at negative origins.
    static func root(model: AppModel) -> some View {
        GeometryReader { window in
            ContentView(model: model, windowSize: window.size)
        }
        .frame(minWidth: ShellBudget.minWindow.width, minHeight: ShellBudget.minWindow.height)
    }

    private var axMode: String { model.viewMode == .document ? "document" : model.isPhotoEditing ? "develop" : "library" }

    var body: some View {
        NavigationSplitView(columnVisibility: columnVisibility) {
            SidebarView(model: model)
                .toolbar(removing: model.viewMode == .document ? .sidebarToggle : nil)
                .toolbar {
                    if model.viewMode == .document {
                        // SwiftUI's automatic item has no hook for AX metadata. Keep its native
                        // responder-chain action and symbol in an explicitly labelled item.
                        ToolbarItem(id: "document-sidebar", placement: .navigation) {
                            Button {
                                NSApp.sendAction(#selector(NSSplitViewController.toggleSidebar(_:)), to: nil, from: nil)
                            } label: {
                                Label(columnVisibility.wrappedValue == .detailOnly ? "Show Sidebar" : "Hide Sidebar",
                                      systemImage: "sidebar.left")
                            }
                            .help("Toggle sidebar")
                            .accessibilityIdentifier("document.toolbar.sidebar")
                        }
                    }
                }
                .containedColumn()
                .navigationSplitViewColumnWidth(min: Theme.Width.sidebarMin, ideal: Theme.Width.sidebarIdeal,
                                                max: Theme.Width.sidebarMax)
        } detail: {
            VStack(spacing: 0) {
                ForEach(model.developRecoveries) { recovery in
                    if case .failed(let message) = recovery.phase {
                        HStack(spacing: Theme.Space.m) {
                            Text("Could not finish saving \(recovery.displayName): \(message)")
                                .lineLimit(2)
                            Spacer(minLength: Theme.Space.m)
                            Button("Retry Save") { model.retryDevelopRecovery(recovery.id) }.accessibilityIdentifier("develop.recovery.retry.\(recovery.id)")
                            if model.canKeepEditingDevelopRecovery(recovery.id) {
                                Button("Keep Editing") { model.keepEditingDevelopRecovery() }.accessibilityIdentifier("develop.recovery.keepEditing")
                            }
                        }
                        .font(Theme.Fonts.caption)
                        .padding(.horizontal, Theme.Space.gutter)
                        .padding(.vertical, Theme.Space.s)
                        .background(Theme.panel)
                        .accessibilityIdentifier("develop-save-recovery")
                    }
                }
                if model.isCachedPreviewLibrary, model.viewMode != .document { CachedPreviewNotice(model: model) }
                if model.tether.showPanel {
                    TetherPanel(model: model, tether: model.tether)
                        .disabled(model.isCachedPreviewLibrary)
                }
                GeometryReader { area in
                    ZStack {
                        // Both stay alive so grid scroll position and loupe texture survive mode switches.
                        // UX-14: in Cull the grid shows through the burst workspace's centre (grouped by
                        // burst) and hides under the burst view.
                        ThumbnailBrowser(model: model, style: .grid)
                            .onAppear { PerformanceTrace.shared.record("grid_appeared") }
                            .padding(.leading, model.isCullWorkspace && model.showInspector ? Theme.Width.triageColumn : 0)
                            .padding(.top, model.isCullWorkspace ? CullWorkspace.gridTopInset : 0)
                            .padding(.bottom, model.isCullWorkspace ? CullWorkspace.gridBottomInset : 0)
                            .opacity(model.viewMode == .grid && !model.isBurstViewShown ? 1 : 0)
                            .allowsHitTesting(model.viewMode == .grid && !model.isBurstViewShown)
                        LoupeView(model: model)
                            .opacity(model.viewMode == .loupe ? 1 : 0)
                            .allowsHitTesting(model.viewMode == .loupe)
                        if model.viewMode == .loupe {
                            LoupeOverlay(model: model)
                            if model.focusedItem?.engineImage?.lightroomSmartPreview == true {
                                VStack {
                                    HStack {
                                        SmartPreviewLoupeBadge(
                                            notices: model.developRenderNotices(for: model.focusedItem))
                                        Spacer()
                                    }
                                    Spacer()
                                }
                                .padding(Theme.Space.m)
                                .allowsHitTesting(false)
                            }
                            if model.isPhotoEditing {
                                MaskToolbar(model: model, masks: .shared)
                                // UX-13: the mask chips, the crop aspect chips and the dial.
                                EditPhotoHUDs(model: model)
                            }
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
                        } else if model.deliverMode {
                            DeliverView(model: model)
                        } else if model.isCullWorkspace {
                            CullWorkspace(model: model)
                        }
                        // UX-11: the agent field sits bottom centre of every mode (Review keeps its own
                        // instruction field in the inspector); the toast moves up above it.
                        VStack {
                            Spacer()
                            if let toast = model.toast {
                                ToastView(model: model, toast: toast)
                                    .padding(.bottom, Theme.Space.l)
                                    .transition(Theme.Motion.transition(from: .bottom))
                            }
                            if model.viewMode != .document, !model.isReviewing {
                                AgentRow(model: model)
                                    .padding(.bottom, Theme.Space.l)
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
        .background(DocumentSaveParentBridge(presenter: model.documents.savePresenter).frame(width: 0, height: 0))
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
        .stackSheets(model)   // B5-19: Auto-Align / Auto-Blend / Photomerge
        .sheet(isPresented: Binding(get: { model.collections.editor != nil },
                                    set: { if !$0 { model.collections.editor = nil } })) {
            SmartAlbumSheet(library: model.collections)
        }
        // UX-14 / UX-15: in Cull the panel is the triage column on the left, in Deliver the
        // What · How · Where column; the inspector stays for the loupe, Compare, Edit, Review and
        // documents.
        .inspector(isPresented: Binding(get: { model.showInspector && !model.isCullWorkspace && !model.deliverMode },
                                        set: { shown in
                                            if !model.isCullWorkspace, !model.deliverMode, shown != model.showInspector { model.toggleInspectorPanel() }
                                        })) {
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
        // UX-11: the panel state is remembered per mode.
        .onChange(of: model.shellMode) { _, mode in model.restorePanelState(for: mode) }
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
                Label("Open Folder…", systemImage: "folder.badge.plus")
            }
            .buttonStyle(ToolbarButtonStyle())
            .help("Open a folder of JPEG / RAW images (⌘O)")
            .accessibilityIdentifier(model.viewMode == .document ? "document.toolbar.open" : "\(axMode).toolbar.open")
        }
        .flatToolbarItem()
        ToolbarItem(id: "folder", placement: .navigation) {
            // The folder name opens the source sidebar's content (folders, albums, people, culling
            // lists) as a popover; the sidebar column itself is hidden unless View ▸ Show Library
            // Sidebar brings it back.
            if model.viewMode != .document {
                Button { showFolders.toggle() } label: {
                    HStack(spacing: Theme.Space.xs) {
                        Image(systemName: "folder").font(Theme.Fonts.iconSmall)
                        Text(model.library.items.isEmpty ? "No folder" : model.library.title)
                            .lineLimit(1).truncationMode(.middle)
                        Image(systemName: "chevron.down").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                    }
                    .frame(maxWidth: Theme.Width.folderButton)
                }
                .buttonStyle(ToolbarButtonStyle(on: showFolders))
                .help(model.library.items.isEmpty ? "Folders, albums and people" : "\(model.library.title): folders, albums, people and culling lists")
                .accessibilityLabel("Folder")
                .accessibilityIdentifier("\(axMode).toolbar.folder")
                .popover(isPresented: $showFolders, arrowEdge: .bottom) {
                    SidebarView(model: model)
                        .frame(width: Theme.Width.sidebarMax, height: Theme.Height.folderPopover)
                }
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "mode", placement: .principal) {
            if model.viewMode == .document {
                Button("Library") { model.requestLibraryViewMode(.grid) }
                    .buttonStyle(ToolbarButtonStyle())
                    .help("Return to Library; open documents stay available")
                    .accessibilityIdentifier("document.toolbar.library")
            } else {
                SegmentedPicker(selection: Binding(get: { model.shellMode }, set: { model.selectShellMode($0) }),
                                segments: ShellMode.allCases.map { mode in
                    .init(value: mode, title: mode.title, symbol: mode.symbol,
                          help: "\(mode.title) (⌘\(ShellMode.allCases.firstIndex(of: mode)! + 1))")
                }, height: Theme.Height.large, fill: false, accessibilityPrefix: "\(axMode).toolbar.workspace")
                .accessibilityLabel("Mode")
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "documents", placement: .navigation) {
            if model.viewMode == .document, !model.documents.documents.isEmpty {
                DocumentTabs(workspace: model.documents)
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "search", placement: .primaryAction) {
            if model.isEngineBacked, !model.isCachedPreviewLibrary, model.source != .people, model.isLibraryWorkspace, !model.deliverMode {
                LibrarySearchField(library: model.collections, model: model)
            }
        }
        .flatToolbarItem()
        ToolbarItem(id: "size", placement: .primaryAction) {
            // Grid only. M2-56: removed (not transparent) in the other modes, so its 130 pt slot
            // goes back to the toolbar instead of pushing items into the overflow menu.
            if model.viewMode == .grid, !model.deliverMode {
                HStack(spacing: Theme.Space.xs) {
                    Image(systemName: "square.grid.3x3").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                    Slider(value: $model.thumbnailSize, in: 110...360)
                        .accessibilityIdentifier("library.toolbar.thumbnailSize").accessibilityLabel("Thumbnail size")
                        .controlSize(.mini)
                        .frame(width: Theme.Width.thumbnailSlider)
                    Image(systemName: "square.grid.2x2").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                }
                .help("Thumbnail size")
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
        ToolbarItem(id: "before", placement: .primaryAction) {
            if model.isPhotoEditing { BeforeAfterButton(model: model) }
        }
        .flatToolbarItem()
        ToolbarItem(id: "inspector", placement: .primaryAction) {
            Toggle(isOn: Binding(get: { model.showInspector }, set: { _ in model.toggleInspectorPanel() })) {
                Label("Panel", systemImage: "sidebar.right")
            }
            .toggleStyle(ToolbarToggleStyle())
            .help(model.viewMode == .document ? "Show or hide the inspector (⌥⌘I)" : "Show or hide the panel (Tab)")
            .accessibilityIdentifier(model.viewMode == .document ? "document.toolbar.inspector" : "\(axMode).toolbar.inspector")
        }
        .flatToolbarItem()
        ToolbarItem(id: "export", placement: .primaryAction) {
            if model.viewMode != .document {
                Button { model.presentExport() } label: {
                    Label("Export", systemImage: "square.and.arrow.up")
                }
                .buttonStyle(ToolbarButtonStyle())
                .disabled(!model.isEngineBacked || model.exporter.isRunning)
                .help("Export the selection or the view (⇧⌘E)")
                .accessibilityIdentifier("\(axMode).toolbar.export")
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

/// Before / after (SPEC §1, UX-13): hold the button (or `\`) to see the original; the loupe shows
/// the edit again on release. Drawn like a toolbar button, pressed while held.
struct BeforeAfterButton: View {
    let model: AppModel
    @State private var hovering = false
    @Environment(\.toolbarCompact) private var compact

    var body: some View {
        let on = model.showingBefore
        Group {
            if compact {
                Label("Before", systemImage: "rectangle.split.2x1").labelStyle(.iconOnly)
            } else {
                Label("Before", systemImage: "rectangle.split.2x1").labelStyle(.titleAndIcon)
            }
        }
        .font(Theme.Fonts.label)
        .foregroundStyle(on ? Theme.textPrimary : Theme.textSecondary)
        .padding(.horizontal, Theme.Space.s)
        .frame(height: Theme.Height.large)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control)
            .fill(on ? Theme.pressed : hovering ? Theme.hover : Theme.clear))
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .onLongPressGesture(minimumDuration: .infinity, maximumDistance: Theme.Space.xxl, perform: {},
                            onPressingChanged: { model.setShowingBefore($0) })
        .disabled(model.developStatus != .ready)
        .help("Hold to see the original (\\)")
        .accessibilityLabel("Before")
        .accessibilityValue(on ? "showing the original" : "showing the edit")
        .accessibilityIdentifier("develop.toolbar.before")
    }
}

/// Lets the window's toolbar keep the title visible and the items flat (no per-item glass).
struct WindowToolbarConfigurator: NSViewRepresentable {
    private final class Anchor: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { configure(window) }
        }

        func configure(_ window: NSWindow) {
            window.titlebarSeparatorStyle = .line
            RecoveryWindowCloseGuard.install(on: window)
        }
    }

    func makeNSView(context: Context) -> NSView { Anchor() }
    func updateNSView(_ view: NSView, context: Context) {
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                guard let anchor = view as? Anchor, let window = anchor.window else { return }
                anchor.configure(window)
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
                    .help("Settings change → frame in the loupe surface (Debug ▸ Show Engine Readouts)")
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
                          title: model.isLoading ? "Reading folder…" : (model.isCachedPreviewLibrary ? "No cached Smart Previews" : "No images"),
                          message: model.isCachedPreviewLibrary ? LibraryAccessMode.cachedSmartPreviews.emptyMessage
                            : "Open a folder of JPEG or RAW files to start culling.") {
            Button("Open Folder…") { model.presentOpenPanel() }.accessibilityIdentifier("library.empty.openFolder")
                .buttonStyle(.theme(.primary, height: Theme.Height.large))
                .keyboardShortcut(.defaultAction)
            if StubLibraryDiagnostics.isEnabled {
                Button("Load 20,000 Stub Items") { model.loadStubItems(count: 20_000) }.accessibilityIdentifier("library.empty.loadStubs")
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
