import TesseraCore
import SwiftUI

/// Window layout: sidebar | (grid or loupe) + status bar + filmstrip | inspector.
struct ContentView: View {
    @Bindable var model: AppModel

    var body: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 210, max: 300)
        } detail: {
            VStack(spacing: 0) {
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
                    }
                    if model.library.items.isEmpty {
                        EmptyStateView(model: model)
                    }
                }
                StatusBar(model: model)
                if model.showFilmstrip {
                    Divider()
                    ThumbnailBrowser(model: model, style: .filmstrip)
                        .frame(height: 92)
                }
            }
            .background(Color(nsColor: Theme.gridBackground))
            .navigationTitle(model.library.items.isEmpty ? "Tessera" : model.library.title)
            .navigationSubtitle(subtitle)
        }
        .inspector(isPresented: $model.showInspector) {
            InspectorView(model: model)
                .inspectorColumnWidth(min: 250, ideal: 280, max: 360)
        }
        .toolbar { toolbar }
        .preferredColorScheme(.dark)
    }

    private var subtitle: String {
        let n = model.visibleCount
        guard n > 0 else { return "" }
        return model.source == .all ? "\(n.formatted()) images" : "\(model.source.title) · \(n.formatted()) images"
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            Button("Open Folder…") { model.presentOpenPanel() }
                .help("Open a folder of JPEG / RAW images (⌘O)")
        }
        ToolbarItem(placement: .principal) {
            Picker("View", selection: $model.viewMode) {
                ForEach(ViewMode.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .frame(width: 130)
            .help("Grid (G) / Loupe (E or Return)")
        }
        ToolbarItemGroup(placement: .primaryAction) {
            Slider(value: $model.thumbnailSize, in: 110...360)
                .frame(width: 110)
                .disabled(model.viewMode != .grid)
                .help("Thumbnail size")
            Toggle("Auto-advance", isOn: $model.autoAdvance)
                .toggleStyle(.button)
                .help("Move to the next image after X / U / P / 1–3 (A)")
            Toggle("Inspector", isOn: $model.showInspector)
                .toggleStyle(.button)
        }
    }
}

struct StatusBar: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: 14) {
            if let item = model.focusedItem, let p = model.focusedPosition {
                Text("\((p + 1).formatted()) of \(model.visibleCount.formatted())")
                Text("G\(item.groupID + 1) · \(model.indexInGroup(of: item) + 1)/\(model.groupSize(of: item))")
                Text(stateText).foregroundStyle(Color(nsColor: model.focusedState.decision.color))
                if model.selectionCount > 1 { Text("\(model.selectionCount.formatted()) selected") }
            }
            Spacer(minLength: 8)
            if let msg = model.statusMessage {
                Text(msg).lineLimit(1).truncationMode(.tail).foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            Text("Keep \(model.counts.keep.formatted())  Reject \(model.counts.reject.formatted())  Basket \(model.counts.basket.formatted())")
            Text("Auto-advance \(model.autoAdvance ? "on" : "off")")
                .foregroundStyle(model.autoAdvance ? .primary : .secondary)
        }
        .font(.system(size: 11).monospacedDigit())
        .padding(.horizontal, 12)
        .frame(height: 24)
        .background(Color(nsColor: Theme.windowBackground))
    }

    private var stateText: String {
        let s = model.focusedState
        var parts = [s.decision.label]
        if s.grade > 0 { parts.append("Grade \(s.grade) (\(CullState.gradeNames[Int(s.grade)]))") }
        if s.mark > 0 { parts.append("Mark \(s.mark)") }
        if s.inBasket { parts.append("Basket") }
        return parts.joined(separator: " · ")
    }
}

struct LoupeOverlay: View {
    let model: AppModel
    var body: some View {
        VStack {
            HStack(alignment: .top) {
                if let item = model.focusedItem {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(item.name).font(.system(size: 12, weight: .medium))
                        if let badge = model.focusedState.badgeText {
                            Text(badge).font(.system(size: 10, weight: .bold))
                                .foregroundStyle(Color(nsColor: model.focusedState.decision.color))
                        }
                    }
                }
                Spacer()
                Text(model.loupeInfo).font(.system(size: 10)).foregroundStyle(.secondary)
            }
            Spacer()
            Text("← → group    ↑ ↓ frame in group    X U P decide    1 2 3 grade    Esc grid")
                .font(.system(size: 10)).foregroundStyle(.tertiary)
        }
        .padding(10)
        .allowsHitTesting(false)
    }
}

struct EmptyStateView: View {
    let model: AppModel
    var body: some View {
        VStack(spacing: 14) {
            Text(model.isLoading ? "Reading folder…" : "No images")
                .font(.system(size: 17, weight: .medium))
            Text("Open a folder of JPEG or RAW files to start culling.")
                .font(.system(size: 12)).foregroundStyle(.secondary)
            HStack(spacing: 10) {
                Button("Open Folder…") { model.presentOpenPanel() }
                    .keyboardShortcut(.defaultAction)
                Button("Load 20,000 Stub Items") { model.loadStubItems(count: 20_000) }
            }
            .disabled(model.isLoading)
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: Theme.gridBackground))
    }
}
