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
                VStack(spacing: 0) {
                    HStack(alignment: .top, spacing: Theme.Space.s) {
                        DocumentToolBar(document: doc)
                        Spacer()
                    }
                    .padding(Theme.Space.m)
                    Spacer()
                    ZoomHUD(document: doc)
                        .padding(.bottom, Theme.Space.l)
                }
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

/// The inspector in document mode: Properties, Layers, History.
struct DocumentInspector: View {
    let workspace: DocumentWorkspace

    var body: some View {
        VStack(spacing: 0) {
            if let doc = workspace.current {
                ScrollView {
                    PanelSection("Properties") { PropertiesPanel(document: doc) }
                }
                .scrollIndicators(.never)
                .frame(minHeight: Theme.Height.sectionHeader * 6, maxHeight: .infinity)
                .layoutPriority(0)
                .accessibilityIdentifier("document.properties")
                VStack(alignment: .leading, spacing: 0) {
                    panelHeader("Layers")
                    LayersPanel(document: doc)
                }
                .frame(minHeight: Theme.Height.sectionHeader * 8, maxHeight: .infinity)
                .layoutPriority(1)
                Hairline()
                PanelSection("History") { DocumentHistoryPanel(document: doc, workspace: workspace) }
                    .accessibilityIdentifier("document.history")
            } else {
                PanelSection("Layers") { Hint("No document is open.") }
                Spacer()
            }
        }
        .background(Theme.panel)
        .tint(Theme.accent)
    }

    private func panelHeader(_ title: String) -> some View {
        HStack {
            Text(title).font(Theme.Fonts.labelSemibold).foregroundStyle(Theme.textPrimary)
            Spacer()
        }
        .padding(.horizontal, Theme.Space.gutter)
        .frame(height: Theme.Height.sectionHeader)
        .accessibilityAddTraits(.isHeader)
    }
}

/// Toolbar document switcher: one tab per open document (the segmented style of DESIGN.md §5,
/// neutral: the chosen tab is raised), a dirty dot, and a close button on the chosen tab.
struct DocumentTabs: View {
    let workspace: DocumentWorkspace

    var body: some View {
        HStack(spacing: Theme.Space.xxs) {
            ForEach(Array(workspace.documents.enumerated()), id: \.element.id) { i, doc in
                DocumentTab(doc: doc, selected: workspace.current === doc, index: i,
                            select: { workspace.select(doc) }, close: { workspace.close(doc) })
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
        .fixedSize()
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
struct DocumentStatusBar: View {
    let model: AppModel
    let workspace: DocumentWorkspace

    var body: some View {
        VStack(spacing: 0) {
            Hairline()
            HStack(spacing: Theme.Space.m) {
                if let doc = workspace.current {
                    let i = doc.info
                    Text("\(i.width) × \(i.height) px · \(i.depth.title) · \(i.profileName ?? "Untagged (sRGB)")")
                        .foregroundStyle(Theme.textPrimary).fixedSize()
                        .accessibilityIdentifier("document.status.canvas")
                    separator
                    Text(DocumentViewportMath.percentText(doc.zoom)).fixedSize()
                        .accessibilityIdentifier("document.status.zoom")
                    separator
                    Text("\(doc.tool.title) (\(doc.tool.key))").fixedSize()
                    if let m = doc.marquee {
                        separator
                        Text("Selection \(m.width) × \(m.height)").fixedSize()
                            .accessibilityIdentifier("document.status.selection")
                    }
                    separator
                    if model.showRenderReadout, let readout = doc.renderReadout {
                        Text(readout).fixedSize().accessibilityIdentifier("document.status.render")
                        separator
                    }
                }
                if let msg = model.statusMessage {
                    Text(msg).lineLimit(1).truncationMode(.tail).foregroundStyle(Theme.textTertiary).help(msg)
                        .accessibilityIdentifier("document.status.message")
                }
                Spacer(minLength: Theme.Space.s)
                Text("\(workspace.documents.count) open").fixedSize()
            }
            .font(Theme.Fonts.captionNumeric)
            .foregroundStyle(Theme.textSecondary)
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.statusBar)
        }
        .background(Theme.panel)
    }

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Space.m) }
}
