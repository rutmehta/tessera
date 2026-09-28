import SwiftUI
import TesseraCore

/// Document mode's content area: the viewport with a tool bar and the zoom HUD, or an empty
/// state with New / Open.
struct DocumentView: View {
    @Bindable var workspace: DocumentWorkspace

    var body: some View {
        ZStack {
            if let doc = workspace.current {
                DocumentViewportRepresentable(workspace: workspace, document: doc)
                    // B5-16 (H10): the zoom chip floats over the viewport; it takes no layout height.
                    .overlay(alignment: .bottom) {
                        ZoomHUD(document: doc)
                            .padding(.bottom, Theme.Space.l)
                    }
                // WP B5-04: tools palette (left) and the options bar (top). B5-16 (H11): the palette
                // scrolls when the canvas is shorter than it (see `ToolsPalette`).
                HStack(alignment: .top, spacing: Theme.Space.s) {
                    ToolsPalette(document: doc, tools: DocumentTools.shared)
                    ToolOptionsBar(document: doc, tools: DocumentTools.shared)
                        .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                }
                .padding(Theme.Space.m)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            } else {
                EmptyStateContent(symbol: "square.3.layers.3d", title: "No document",
                                  message: "Create a layered document, open a .tessera-doc or PSD, or choose a photo and Library ▸ Edit in Layers (⌘E).") {
                    Button("New Document…") { workspace.showNewDocument = true }
                        .buttonStyle(.theme(.primary, height: Theme.Height.large))
                        .accessibilityIdentifier("document.empty.new")
                    Button("Open Document…") { workspace.presentOpen() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.large))
                        .accessibilityIdentifier("document.empty.open")
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Theme.canvas)
            }
        }
        // WP B5-04: the tools serve this workspace; their sheets hang off the document view.
        .onAppear { DocumentTools.shared.attach(workspace) }
        .modifier(ToolSheetsModifier(tools: DocumentTools.shared))
        .modifier(DocumentFilterSheets(filters: workspace.filters))   // WP B5-05
        // B5-08 begin: Channels (sheets, Quick Mask, preview overlay).
        .onAppear { DocumentChannels.shared.attach(workspace) }
        // B5-11 begin: shapes, Pen and vector masks; `--vector-selftest=<dir>` starts here.
        .onAppear {
            DocumentVector.shared.attach(workspace)
            VectorSelfTest.startIfRequested()
        }
        // B5-11 end
        .modifier(ChannelSheetsModifier(channels: DocumentChannels.shared))
        // B5-08 end
        .modifier(RetouchSheets(retouch: DocumentRetouch.shared))   // B5-09
        .onAppear { DocumentText.shared.attach(workspace) }   // B5-10: the Type tool
        // B5-12 begin: Warp / Perspective / Puppet / Content-Aware Scale (Document/Transforms).
        .onAppear { DocumentTransforms.shared.attach(workspace) }
        .onChange(of: workspace.current.map(ObjectIdentifier.init)) { _, _ in DocumentTransforms.shared.documentWillChange() }
        .modifier(TransformSheets(t: DocumentTransforms.shared))
        // B5-12 end
    }
}

/// Move (V) and Rectangular Marquee (M): a HUD bar over the canvas.
struct DocumentToolBar: View {
    @Bindable var document: DocumentController

    var body: some View {
        HStack(spacing: Theme.Space.xxs) {
            ForEach(DocumentTool.allCases, id: \.self) { tool in
                IconButton(symbol: tool.symbol, help: "\(tool.title) (\(tool.key))", on: document.tool == tool,
                           size: Theme.Height.large) { document.tool = tool }
                    .accessibilityIdentifier("document.tool.\(tool.rawValue)")
            }
        }
        .padding(Theme.Space.xxs)
        .background(HUDBackground())
    }
}

/// Zoom percentage, shown for a moment after each zoom change.
struct ZoomHUD: View {
    let document: DocumentController
    @State private var visible = false

    var body: some View {
        Text(DocumentViewportMath.percentText(document.zoom))
            .font(Theme.Fonts.labelNumeric)
            .foregroundStyle(Theme.textPrimary)
            .padding(.horizontal, Theme.Space.m)
            .frame(height: Theme.Height.large)
            .background(HUDBackground())
            .opacity(visible ? 1 : 0)
            .allowsHitTesting(false)
            .accessibilityIdentifier("document.zoomHUD")
            .task(id: document.zoomChangedAt) {
                guard document.zoomChangedAt != nil else { return }
                visible = true
                try? await Task.sleep(for: .milliseconds(1200))
                if !Task.isCancelled { withAnimation(Theme.Motion.appear) { visible = false } }
            }
    }
}

/// The inspector in document mode (WP B5-16, M2-56 handoff H1–H3, H6): a segmented header with the
/// sub-tabs Stack (the Layers panel) · Properties (the selected layer and the tool sections) ·
/// Channels, each scrolling its own content, over a collapsible History pane with a resizable body
/// (states and snapshots in one scroller). ⌃1 / ⌃2 / ⌃3 switch the tabs. The column's minimum
/// (`budget`) fits the 548 pt column of a 960 × 600 window, so nothing is clipped at its bottom.
struct DocumentInspector: View {
    @Bindable var workspace: DocumentWorkspace
    /// The same key the stacked inspector's History section used (self-tests set it).
    @AppStorage("InspectorPanel.History") private var historyExpanded = true
    @AppStorage("DocumentInspector.historyHeight") private var historyRequested: Double = Double(Self.historyDefault)
    @State private var dragBase: CGFloat?
    @State private var headerHover = false

    static let historyDefault: CGFloat = Theme.Height.row * 7
    static let budget = DocumentInspectorBudget(
        tabBar: Theme.Height.sectionHeader,
        separators: Theme.Space.hairline * 2,
        tabMinimum: LayersPanel.minimumHeight,
        historyHeader: Theme.Height.sectionHeader,
        historyMinimum: DocumentHistoryPanel.minimumHeight)

    var body: some View {
        Group {
            if let doc = workspace.current {
                GeometryReader { column in
                    VStack(spacing: 0) {
                        tabBar
                            .inspectorProbe("tabBar")
                        Hairline()
                        tabContent(doc)
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                            .clipped()
                            .inspectorProbe("tabContent")
                        historyPane(doc, column: column.size.height)
                    }
                }
            } else {
                VStack(spacing: 0) {
                    PanelSection("Layers") { Hint("No document is open.") }
                    Spacer()
                }
            }
        }
        .background(Theme.panel)
        .tint(Theme.accent)
    }

    // MARK: Tabs

    private var tabBar: some View {
        SegmentedPicker(selection: $workspace.inspectorTab,
                        segments: DocumentInspectorTab.allCases.map { .init(value: $0, title: $0.title, help: $0.help) },
                        height: Theme.Height.small)
            .accessibilityLabel("Inspector")
            .accessibilityIdentifier("document.inspector.tabs")
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.sectionHeader)
            .background(shortcuts)
    }

    /// ⌃1 / ⌃2 / ⌃3: invisible buttons carrying the key equivalents (the segments are plain buttons of
    /// the shared control, which takes no shortcuts).
    private var shortcuts: some View {
        ZStack {
            ForEach(DocumentInspectorTab.allCases) { tab in
                Button(tab.title) { workspace.inspectorTab = tab }
                    .keyboardShortcut(KeyEquivalent(tab.shortcutDigit), modifiers: .control)
                    .accessibilityIdentifier("document.inspector.shortcut.\(tab.rawValue)")
            }
        }
        .frame(width: 0, height: 0)
        .opacity(0)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    @ViewBuilder private func tabContent(_ doc: DocumentController) -> some View {
        switch workspace.inspectorTab {
        case .stack:
            // The outline scrolls itself; the controls above it and the footer stay put.
            LayersPanel(document: doc)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("document.inspector.stack")
        case .properties:
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    PropertiesPanel(document: doc)
                        .padding(.horizontal, Theme.Space.gutter)
                        .padding(.top, Theme.Space.m)
                        .padding(.bottom, Theme.Space.l)
                    Hairline()
                    // WP B5-04: Color and Brushes.
                    ToolInspectorSections(tools: DocumentTools.shared, document: doc)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.automatic)
            .accessibilityIdentifier("document.properties")
        case .channels:
            // B5-08: saved alpha and spot channels (the list scrolls, the footer stays).
            ChannelsPanel(document: doc, channels: DocumentChannels.shared)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("document.channels")
        }
    }

    // MARK: History

    private func historyPane(_ doc: DocumentController, column: CGFloat) -> some View {
        let height = Self.budget.historyHeight(requested: CGFloat(historyRequested), column: column)
        return VStack(spacing: 0) {
            Hairline()
                .overlay {
                    if historyExpanded { resizeHandle(current: height, column: column) }
                }
                .zIndex(1)
            historyHeader(column: column)
                .inspectorProbe("historyHeader")
            if historyExpanded {
                DocumentHistoryPanel(document: doc, workspace: workspace)
                    .padding(.horizontal, Theme.Space.gutter)
                    .frame(height: height)
                    .inspectorProbe("historyBody")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("document.history")
    }

    private func historyHeader(column: CGFloat) -> some View {
        HStack(spacing: Theme.Space.xs) {
            historyToggle
            if historyExpanded {
                DocumentHistoryHeightControls(requested: $historyRequested, column: column)
                    .frame(width: 132, height: Theme.Height.small)
            }
        }
        .padding(.horizontal, Theme.Space.gutter)
        .frame(height: Theme.Height.sectionHeader)
    }

    private var historyToggle: some View {
        Button {
            historyExpanded.toggle()
        } label: {
            HStack(spacing: Theme.Space.s) {
                Text("History")
                    .font(Theme.Fonts.labelSemibold)
                    .foregroundStyle(historyExpanded ? Theme.textPrimary : Theme.textSecondary)
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(Theme.Fonts.iconSmall)
                    .foregroundStyle(headerHover ? Theme.textSecondary : Theme.textTertiary)
                    .rotationEffect(.degrees(historyExpanded ? 90 : 0))
            }
            .frame(height: Theme.Height.sectionHeader)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { headerHover = $0 }
        .help(historyExpanded ? "Collapse History" : "Expand History")
        .accessibilityLabel("History")
        .accessibilityValue(historyExpanded ? "expanded" : "collapsed")
        .accessibilityIdentifier("document.history.toggle")
    }

    /// Drag the hairline above History to give it more or less of the column (the tab content keeps
    /// its minimum); double-click restores the default height.
    private func resizeHandle(current: CGFloat, column: CGFloat) -> some View {
        Color.clear
            .frame(height: Theme.Space.s)
            .contentShape(Rectangle())
            .pointerStyle(.rowResize)
            .gesture(DragGesture(minimumDistance: 1)
                .onChanged { g in
                    let base = dragBase ?? current
                    dragBase = base
                    historyRequested = Double(Self.budget.historyHeight(requested: base - g.translation.height, column: column))
                }
                .onEnded { _ in dragBase = nil })
            .onTapGesture(count: 2) { historyRequested = Double(Self.historyDefault) }
            .help("Drag to resize History")
            .accessibilityIdentifier("document.history.resize")
    }
}

/// B5-16: the inspector regions' frames (root coordinates, y down) for the layout tests. Off unless
/// a test turns it on; `inspectorProbe(_:)` records a region.
@MainActor
enum DocumentInspectorProbe {
    static var isEnabled = false
    static var frames: [String: CGRect] = [:]
}

extension View {
    func inspectorProbe(_ name: String) -> some View {
        onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { rect in
            if DocumentInspectorProbe.isEnabled { DocumentInspectorProbe.frames[name] = rect }
        }
    }
}

/// Toolbar document switcher: one tab per open document (the segmented style of DESIGN.md §5,
/// neutral: the chosen tab is raised), a dirty dot, and a close button on the chosen tab.
/// B5-16 (H8): at most `DocumentTabStrip.cap` tabs (one in a compact toolbar), always including the
/// current document; the others are in a "+n" overflow menu, so many documents never widen the toolbar item without limit.
struct DocumentTabs: View {
    let workspace: DocumentWorkspace
    @Environment(\.toolbarCompact) private var compact

    var body: some View {
        let docs = workspace.documents
        let current = docs.firstIndex { $0 === workspace.current }
        let cap = compact ? DocumentTabStrip.compactCap : DocumentTabStrip.cap
        let shown = DocumentTabStrip.visible(count: docs.count, current: current, cap: cap)
        let hidden = DocumentTabStrip.overflow(count: docs.count, current: current, cap: cap)
        HStack(spacing: Theme.Space.xxs) {
            ForEach(Array(docs.enumerated()).filter { shown.contains($0.offset) }, id: \.element.id) { i, doc in
                DocumentTab(doc: doc, selected: workspace.current === doc, index: i,
                            select: { workspace.select(doc) }, close: { workspace.close(doc) })
            }
            if !hidden.isEmpty {
                Menu {
                    ForEach(hidden, id: \.self) { i in
                        Button(docs[i].title + (docs[i].isDirty ? " (edited)" : "")) { workspace.select(docs[i]) }
                    }
                } label: {
                    HStack(spacing: Theme.Space.xxs) {
                        Text("+\(hidden.count)").font(Theme.Fonts.captionNumeric)
                        Image(systemName: "chevron.down").font(Theme.Fonts.iconSmall).imageScale(.small)
                    }
                    .foregroundStyle(Theme.textSecondary)
                    .padding(.horizontal, Theme.Space.xs)
                    .frame(height: Theme.Height.regular - Theme.Space.xs)
                    .contentShape(Rectangle())
                }
                .menuStyle(.button)
                .buttonStyle(.plain)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("\(hidden.count) more open document\(hidden.count == 1 ? "" : "s")")
                .accessibilityLabel("\(hidden.count) more documents")
                .accessibilityIdentifier("document.tabs.overflow")
            }
            Button { workspace.showNewDocument = true } label: {
                Image(systemName: "plus").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textSecondary)
                    .frame(width: Theme.Height.regular - Theme.Space.xs, height: Theme.Height.regular - Theme.Space.xs)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("New Document… (⌘N)")
            .accessibilityIdentifier("document.tabs.new")
        }
        .padding(Theme.Space.xxs)
        .frame(height: Theme.Height.regular)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(Theme.well))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
        // B5-16: no `fixedSize()`: the strip is capped by its tab count, and titles truncate when the
        // toolbar gives it less than its ideal width.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("document.tabs")
    }
}

private struct DocumentTab: View {
    let doc: DocumentController
    let selected: Bool
    let index: Int
    let select: () -> Void
    let close: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(spacing: Theme.Space.xs) {
            Circle().fill(doc.isDirty ? Theme.textSecondary : Theme.clear)
                .frame(width: Theme.Space.xs + Theme.Space.xxs, height: Theme.Space.xs + Theme.Space.xxs)
                .help(doc.isDirty ? "Unsaved changes" : "")
                .accessibilityIdentifier("document.tabs.\(index).dirty")
            Text(doc.title)
                .font(Theme.Fonts.caption)
                .fontWeight(selected ? .medium : .regular)
                .foregroundStyle(selected ? Theme.textPrimary : Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: Theme.Width.sidebarMin - Theme.Space.xxl)
            Button(action: close) {
                Image(systemName: "xmark").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
            }
            .buttonStyle(.plain)
            // B5-16 (H9): the one kept `opacity(0)` slot (DESIGN.md §10): the close glyph keeps its
            // place on unselected tabs so the title does not shift when the pointer enters the tab.
            .opacity(selected || hovering ? 1 : 0)
            .help("Close (⌘W)")
            .accessibilityIdentifier("document.tabs.\(index).close")
        }
        .padding(.horizontal, Theme.Space.s)
        .frame(height: Theme.Height.regular - Theme.Space.xs)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.chip)
            .fill(selected ? Theme.raised : hovering ? Theme.hover : Theme.clear)
            .shadow(color: selected ? Theme.shadow.opacity(0.4) : Theme.clear, radius: 1, y: 0.5))
        .contentShape(Rectangle())
        .onTapGesture(perform: select)
        .onHover { hovering = $0 }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(doc.title + (doc.isDirty ? ", edited" : ""))
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("document.tabs.\(index)")
    }
}

/// The status bar in document mode: document facts, zoom, tool and marquee, then the message.
/// B5-16 (H7, M2-56 L3): the full row when it fits, else a compact one (canvas size without depth and
/// profile, the tool without its key, the selection size, no stroke or render readouts; the full
/// strings stay in help). The message never decides which fits: its ideal width is 0.
struct DocumentStatusBar: View {
    let model: AppModel
    let workspace: DocumentWorkspace

    var body: some View {
        VStack(spacing: 0) {
            Hairline()
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
            if let doc = workspace.current {
                let i = doc.info
                let canvas = "\(i.width) × \(i.height) px · \(i.depth.title) · \(i.profileName ?? "Untagged (sRGB)")"
                Text(compact ? "\(i.width) × \(i.height) px" : canvas)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1).truncationMode(.middle).layoutPriority(1)   // B5-10: truncates in narrow windows
                    .help(canvas)
                    .accessibilityIdentifier("document.status.canvas")
                separator
                Text(DocumentViewportMath.percentText(doc.zoom)).fixedSize()
                    .accessibilityIdentifier("document.status.zoom")
                separator
                let tool = DocumentRetouch.shared.removeActive ? "Remove (⇧J)" : "\(doc.tool.title) (\(doc.tool.key))"   // B5-09
                Text(compact ? (DocumentRetouch.shared.removeActive ? "Remove" : doc.tool.title) : tool).fixedSize()
                    .help(tool)
                if !compact, let r = DocumentTools.shared.strokeReadout, model.showRenderReadout {   // WP B5-04
                    separator
                    Text(r).lineLimit(1).truncationMode(.tail).accessibilityIdentifier("document.status.stroke")
                }
                if let m = doc.marquee {
                    separator
                    Text(compact ? "\(m.width) × \(m.height)" : "Selection \(m.width) × \(m.height)").fixedSize()
                        .help("Selection \(m.width) × \(m.height)")
                        .accessibilityIdentifier("document.status.selection")
                }
                if !compact, model.showRenderReadout, let readout = doc.renderReadout {
                    separator
                    Text(readout).fixedSize().accessibilityIdentifier("document.status.render")
                }
                separator
            }
            // Ideal width 0: the message fills what is left and truncates; it never makes a row "not fit".
            Group {
                if let msg = model.statusMessage {
                    Text(msg).lineLimit(1).truncationMode(.tail).foregroundStyle(Theme.textTertiary).help(msg)
                        .accessibilityIdentifier("document.status.message")
                } else {
                    Color.clear
                }
            }
            .frame(minWidth: 0, idealWidth: 0, maxWidth: .infinity, alignment: .leading)
            Text("\(workspace.documents.count) open").fixedSize()
        }
    }

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Space.m) }
}
