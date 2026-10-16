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
                // UX-16: no automatic sidebar toggle (View ▸ Show Library Sidebar; the folder
                // button is the sidebar's content); document mode keeps its labelled one below.
                .toolbar(removing: .sidebarToggle)
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
                // UX-16: ⌘F drops the search field under the toolbar in Cull.
                if model.showSearch, model.isLibraryWorkspace, !model.isPhotoEditing, !model.deliverMode, model.source != .people {
                    SearchBar(model: model)
                }
                if model.tether.showPanel {
                    TetherPanel(model: model, tether: model.tether)
                        .disabled(model.isCachedPreviewLibrary)
                }
                GeometryReader { area in
                    ZStack {
                    HStack(spacing: 0) {
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
                        // Debug ▸ Show Engine Readouts: the render timing floats top right of the canvas.
                        if model.showRenderReadout, model.viewMode != .document, let readout = model.renderReadout {
                            VStack {
                                HStack {
                                    Spacer()
                                    Text(readout)
                                        .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                                        .padding(.horizontal, Theme.Space.s).frame(height: Theme.Height.regular)
                                        .background(Capsule().fill(Theme.hud))
                                        .overlay(Capsule().strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
                                        .help("Settings change → frame in the loupe surface (Debug ▸ Show Engine Readouts)")
                                        .accessibilityIdentifier("renderReadout")
                                }
                                Spacer()
                            }
                            .padding(Theme.Space.panelInset)
                            .allowsHitTesting(false)
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .clipped()
                    // UX-16: Edit's panel is a plain 320 pt column beside the photo (the prototype's),
                    // not the system inspector, whose macOS 26 column covers the toolbar's right.
                    if model.isPhotoEditing, model.showInspector {
                        PhotoEditInspectorView(model: model)
                            .frame(width: Theme.Width.editPanel)
                            .transition(Theme.Motion.transition(from: .trailing))
                    }
                    }
                    .animation(Theme.Motion.appear, value: model.showInspector)
                    // UX-11: the agent field sits bottom centre of every mode (Review keeps its own
                    // instruction field in the inspector), over the whole width like the prototype's;
                    // the toast moves up above it. In a window too narrow to centre the field beside
                    // Edit's panel it centres over the photo area instead, and the row goes compact.
                    let panelInset = model.isPhotoEditing && model.showInspector
                        && area.size.width < Theme.Width.editPanel * 2 + Theme.Width.agentField ? Theme.Width.editPanel : 0
                    let rowWidth = area.size.width - panelInset - 2 * Theme.Space.l
                    VStack {
                        Spacer()
                        if let toast = model.toast {
                            ToastView(model: model, toast: toast)
                                .padding(.bottom, Theme.Space.l)
                                .transition(Theme.Motion.transition(from: .bottom))
                        }
                        if model.viewMode != .document, !model.isReviewing {
                            AgentRow(model: model, compact: rowWidth < AgentRow.fullWidth)
                                .frame(maxWidth: rowWidth)
                                .padding(.bottom, Theme.Space.l)
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.trailing, panelInset)
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
                // UX-16: no status bar outside document mode. Counts are in the Cull column, the
                // position in the Cull header and the folder button, messages are the toast
                // (`AppModel.statusMessage`), engine readouts a floating overlay behind Debug.
                if model.viewMode == .document {
                    DocumentStatusBar(model: model, workspace: model.documents)
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
            // The title is hidden (the folder button is the title); it still names the window in
            // the Window menu and the Dock.
            .navigationTitle(model.viewMode == .document ? (model.documents.current?.title ?? "Tessera")
                             : model.library.items.isEmpty ? "Tessera" : model.library.title)
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
        // UX-14 / UX-15 / UX-16: in Cull the panel is the triage column on the left, in Deliver the
        // What · How · Where column, in Edit the 320 pt column beside the photo; the inspector
        // stays for the library loupe, Compare, Review and documents.
        .inspector(isPresented: Binding(get: { model.showInspector && !model.isCullWorkspace && !model.deliverMode && !model.isPhotoEditing },
                                        set: { shown in
                                            if !model.isCullWorkspace, !model.deliverMode, !model.isPhotoEditing, shown != model.showInspector { model.toggleInspectorPanel() }
                                        })) {
            Group {
                if model.viewMode == .document {
                    DocumentInspector(workspace: model.documents)
                } else if model.isReviewing {
                    AgentReviewInspector(model: model, busy: model.agent.isRunning || !model.agent.busy.isEmpty,
                                         canEdit: model.canEnterPhotoEdit)
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
        // UX-16: the folder button is the title (none inline either, as happens with the sidebar
        // collapsed), and the bar is the canvas ground with a hairline under it, not the toolbar
        // material.
        .toolbar(removing: .title)
        .toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
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
    /// UX-16: only with View ▸ Filmstrip on, and never under the burst view (its strip is the only
    /// strip) or Deliver.
    private var showsFilmstrip: Bool {
        guard model.showFilmstrip, !model.isReviewing, !model.isReviewEditing, !model.library.items.isEmpty, model.viewMode != .document,
              !model.isBurstViewShown, !model.deliverMode else { return false }
        guard canvasBase > 0 else { return true }
        return ShellBudget.filmstripFits(detailHeight: canvasBase, chrome: 0)
    }

    private static let filmstripSpan = Theme.Height.filmstrip + Theme.Space.hairline

    /// UX-16: the toolbar is the prototype's bar and nothing more: the folder button (the window
    /// title), the mode control, Before (Edit), the info button (loupe), Panel and Export. Open
    /// Folder… lives in the folder popover and the File menu (⌘O); search is ⌘F (a bar under the
    /// toolbar in Cull); thumbnail size is View ▸ Thumbnail Size and a pinch on the grid.
    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(id: "folder", placement: .navigation) {
            // The folder name and count open the source sidebar's content (folders, albums,
            // people, culling lists) as a popover; the sidebar column itself is hidden unless
            // View ▸ Show Library Sidebar brings it back. In Edit a thin "n of 226" line says where
            // ← → are (the filmstrip is off by default).
            if model.viewMode != .document {
                Button { showFolders.toggle() } label: {
                    HStack(spacing: Theme.Space.s) {
                        Image(systemName: "folder").font(Theme.Fonts.icon)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(folderTitle).lineLimit(1).truncationMode(.middle)
                            if let position = folderPosition {
                                Text(position)
                                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                                    .accessibilityIdentifier("\(axMode).toolbar.position")
                            }
                        }
                        Image(systemName: "chevron.down").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                    }
                    .frame(maxWidth: Theme.Width.folderButton)
                }
                .buttonStyle(ToolbarButtonStyle(on: showFolders))
                .help(model.library.items.isEmpty ? "Open a folder; folders, albums and people" : "\(model.library.title): folders, albums, people and culling lists")
                .accessibilityLabel("Folder")
                .accessibilityValue(folderTitle + (folderPosition.map { ", \($0)" } ?? ""))
                .accessibilityIdentifier("\(axMode).toolbar.folder")
                .popover(isPresented: $showFolders, arrowEdge: .bottom) {
                    FolderPopover(model: model, dismiss: { showFolders = false })
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
                // The prototype's modes: titles only, the chosen one ink-filled.
                SegmentedPicker(selection: Binding(get: { model.shellMode }, set: { model.selectShellMode($0) }),
                                segments: ShellMode.allCases.map { mode in
                    .init(value: mode, title: mode.title,
                          help: "\(mode.title) (⌘\(ShellMode.allCases.firstIndex(of: mode)! + 1))")
                }, height: Theme.Height.sectionHeader, fill: false, prominent: true, accessibilityPrefix: "\(axMode).toolbar.workspace")
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
        ToolbarItem(id: "before", placement: .primaryAction) {
            if model.isPhotoEditing { BeforeAfterButton(model: model) }
        }
        .flatToolbarItem()
        ToolbarItem(id: "info", placement: .primaryAction) {
            // UX-16: the display-info popover (colour space, EDR headroom, soft proof) moved off
            // the photo; an `i` right of Before.
            if model.viewMode == .loupe { LoupeInfoButton(model: model) }
        }
        .flatToolbarItem()
        ToolbarItem(id: "inspector", placement: .primaryAction) {
            Toggle(isOn: Binding(get: { model.showInspector }, set: { _ in model.toggleInspectorPanel() })) {
                Label("Panel", systemImage: "sidebar.right")
            }
            .toggleStyle(ToolbarToggleStyle(iconOnly: true))
            .help(model.viewMode == .document ? "Show or hide the inspector (⌥⌘I)" : "Show or hide the panel (Tab)")
            .accessibilityIdentifier(model.viewMode == .document ? "document.toolbar.inspector" : "\(axMode).toolbar.inspector")
        }
        .flatToolbarItem()
        ToolbarItem(id: "export", placement: .primaryAction) {
            // The prototype's one filled button: the accent, 32 pt, radius 8.
            if model.viewMode != .document {
                Button("Export") { model.presentExport() }
                    .buttonStyle(ToolbarPrimaryButtonStyle())
                    .disabled(!model.isEngineBacked || model.exporter.isRunning)
                    .help("Export the selection or the view (⇧⌘E)")
                    .accessibilityIdentifier("\(axMode).toolbar.export")
            }
        }
        .flatToolbarItem()
    }

    /// "World Cup final + NYC · 226": the folder (or album / person) and how many photos are in view.
    private var folderTitle: String {
        // OF-2: an offline folder with no previews yet is still that folder, not "No folder".
        guard !model.library.items.isEmpty else { return model.library.folder == nil ? "No folder" : model.library.title }
        let n = model.visibleCount
        return n > 0 ? "\(model.library.title) · \(n.formatted())" : model.library.title
    }

    /// Edit's thin position line under the folder name ("3 of 226"), where ← → move; with the
    /// original offline it adds "Editing offline preview" (OF-2).
    private var folderPosition: String? {
        guard model.isPhotoEditing, let p = model.focusedPosition, model.visibleCount > 0 else { return nil }
        return "\((p + 1).formatted()) of \(model.visibleCount.formatted())"
            + (model.editingOfflinePreviewLabel.map { " · " + $0 } ?? "")
    }
}

/// The folder button's popover: the source sidebar's content with an Open Folder… row at the
/// bottom (⌘O stays in the File menu).
private struct FolderPopover: View {
    let model: AppModel
    let dismiss: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            SidebarView(model: model)
                .frame(width: Theme.Width.sidebarMax, height: Theme.Height.folderPopover)
            Hairline()
            Button {
                dismiss()
                model.presentOpenPanel()
            } label: {
                HStack(spacing: Theme.Space.s) {
                    Image(systemName: "folder.badge.plus").font(Theme.Fonts.icon)
                    Text("Open Folder…")
                    Spacer()
                    KeyCap("⌘O")
                }
                .font(Theme.Fonts.label)
                .foregroundStyle(Theme.textPrimary)
                .padding(.horizontal, Theme.Space.m)
                .frame(height: Theme.Height.sectionHeader)
                .contentShape(Rectangle())
            }
            .buttonStyle(.theme(.borderless, height: Theme.Height.sectionHeader))
            .help("Open a folder of JPEG / RAW images (⌘O)")
            .accessibilityIdentifier("library.folders.openFolder")
        }
        .background(Theme.panel)
    }
}

/// UX-16: the search field in a bar under the toolbar (⌘F shows it and gives it the keyboard in
/// Cull); the facet menus hang off its filter button as before.
struct SearchBar: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: Theme.Space.m) {
            LibrarySearchField(library: model.collections, model: model, focusRequest: model.searchFieldFocusRequest)
            Spacer(minLength: Theme.Space.m)
            Button { model.toggleSearch() } label: {
                Image(systemName: "xmark").font(Theme.Fonts.iconSmall)
            }
            .buttonStyle(.theme(.borderless, height: Theme.Height.regular, square: true))
            .help("Hide the search bar (⌘F); the filter stays")
            .accessibilityLabel("Hide search")
            .accessibilityIdentifier("library.search.hide")
        }
        .padding(.horizontal, Theme.Space.panelInset)
        .frame(height: Theme.Height.searchBar)
        .background(Theme.canvas)
        Hairline()
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
    var iconOnly = false
    func makeBody(configuration: Configuration) -> some View {
        Button { configuration.isOn.toggle() } label: {
            configuration.label
        }
        .buttonStyle(ToolbarButtonStyle(on: configuration.isOn, iconOnly: iconOnly))
        .accessibilityAddTraits(configuration.isOn ? .isSelected : [])
    }
}

/// Flat toolbar button: 32 pt, radius 6, hover and pressed fills only. `iconOnly` is the
/// prototype's 32 × 32 icon button (Before, the info button, Panel).
struct ToolbarButtonStyle: ButtonStyle {
    var on = false
    var iconOnly = false
    func makeBody(configuration: Configuration) -> some View {
        ToolbarButtonBody(configuration: configuration, on: on, iconOnly: iconOnly)
    }
}

private struct ToolbarButtonBody: View {
    let configuration: ButtonStyle.Configuration
    let on: Bool
    let iconOnly: Bool
    @State private var hovering = false
    /// M2-56: icons only in a narrow window (the title stays the help / accessibility label).
    @Environment(\.toolbarCompact) private var compact
    var body: some View {
        Group {
            if compact || iconOnly {
                configuration.label.labelStyle(.iconOnly).font(Theme.Fonts.icon)
                    .frame(width: iconOnly ? Theme.Height.sectionHeader : nil)
            } else {
                configuration.label.labelStyle(.titleAndIcon).font(Theme.Fonts.bodyMedium)
                    .padding(.horizontal, Theme.Space.s)
            }
        }
            .foregroundStyle(on ? Theme.textPrimary : Theme.textSecondary)
            .frame(height: Theme.Height.sectionHeader)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.control)
                .fill(configuration.isPressed || on ? Theme.pressed : hovering ? Theme.hover : Theme.clear))
            .contentShape(Rectangle())
            .onHover { hovering = $0 }
    }
}

/// The toolbar's one filled button (Export): the accent, 32 pt, radius 8, 13/600 on `textOnAccent`.
struct ToolbarPrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.Fonts.bodySemibold)
            .foregroundStyle(Theme.textOnAccent)
            .padding(.horizontal, Theme.Space.m + Theme.Space.xxs)
            .frame(height: Theme.Height.sectionHeader)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.card)
                .fill(Theme.accent.opacity(configuration.isPressed ? 0.85 : 1)))
            .opacity(enabled ? 1 : Theme.Opacity.disabled)
            .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.card))
    }
}

/// Before / after (SPEC §1, UX-13): hold the button (or `\`) to see the original; the loupe shows
/// the edit again on release. Drawn like a toolbar button, pressed while held.
struct BeforeAfterButton: View {
    let model: AppModel
    @State private var hovering = false

    var body: some View {
        let on = model.showingBefore
        // The prototype's 32 × 32 icon button; the title is the help and accessibility label.
        Label("Before", systemImage: "rectangle.split.2x1").labelStyle(.iconOnly)
        .font(Theme.Fonts.icon)
        .foregroundStyle(on ? Theme.textPrimary : Theme.textSecondary)
        .frame(width: Theme.Height.sectionHeader, height: Theme.Height.sectionHeader)
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
            // UX-16: the folder button is the title; the bar is the canvas ground with a hairline
            // under it (the prototype's 52 pt bar), not the system's titlebar material.
            window.titleVisibility = .hidden
            window.titlebarAppearsTransparent = true
            window.titlebarSeparatorStyle = .line
            window.backgroundColor = Theme.Palette.canvas
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

struct EmptyStateView: View {
    let model: AppModel
    var body: some View {
        EmptyStateContent(symbol: "photo.on.rectangle.angled",
                          // OF-2 copy for an offline folder without previews.
                          title: model.isLoading ? "Reading folder…" : (model.isCachedPreviewLibrary ? OfflineCopy.emptyTitle : "No images"),
                          message: model.isCachedPreviewLibrary ? model.offlineEmptyMessage
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
