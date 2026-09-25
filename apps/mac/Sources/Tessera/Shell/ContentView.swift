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
                if model.isEngineBacked {
                    FilterBar(library: model.collections, model: model)
                    Divider()
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
                    if model.library.items.isEmpty {
                        EmptyStateView(model: model)
                    }
                    VStack {
                        Spacer()
                        if let toast = model.toast {
                            ToastView(model: model, toast: toast)
                                .padding(.bottom, 14)
                                .transition(.opacity.combined(with: .move(edge: .bottom)))
                        }
                    }
                    .animation(.easeOut(duration: 0.18), value: model.toast)
                }
                LightroomImportProgressBar(importer: model.lightroomImport)
                ExportProgressBar(exporter: model.exporter)
                PrintProgressBar(printing: model.printing)
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
        .sheet(isPresented: $model.showDefectSweep) {
            DefectSweepSheet(model: model)
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
            .frame(width: 210)
            .help("Grid (G) / Loupe (E or Return) / Compare (C)")
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
                Text("G\(item.groupID + 1) · \(model.indexInGroup(of: item) + 1)/\(model.groupSize(of: item))"
                     + (model.focusedIsBest ? " · suggested best" : ""))
                Text(stateText).foregroundStyle(Color(nsColor: model.focusedState.decision.color))
                if model.selectionCount > 1 { Text("\(model.selectionCount.formatted()) selected") }
            }
            Spacer(minLength: 8)
            if let msg = model.statusMessage {
                Text(msg).lineLimit(1).truncationMode(.tail).foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            if model.showRenderReadout, let readout = model.renderReadout {
                Text(readout)
                    .foregroundStyle(.secondary)
                    .help("Settings change → frame in the loupe surface (Debug ▸ Show Render Timing)")
                    .accessibilityIdentifier("renderReadout")
            }
            Text("Keep \(model.counts.keep.formatted())  Reject \(model.counts.reject.formatted())")
            Text("Basket → \(model.basketTarget) \(model.counts.basket.formatted())")
                .foregroundStyle(Color(nsColor: Theme.basket))
                .help("B adds to this album. Change it in Cull ▸ Basket Target or the sidebar.")
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
        if s.inBasket { parts.append("In \(model.basketTarget)") }
        if model.focusedStatus.phase != .unedited { parts.append(model.focusedStatus.phase.rawValue.capitalized) }
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
                        if model.focusedIsBest {
                            Text("SUGGESTED BEST · K keeps it and rejects the rest")
                                .font(.system(size: 10, weight: .medium))
                                .foregroundStyle(Color(nsColor: Theme.keep).opacity(0.85))
                        }
                    }
                }
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text(model.loupeInfo).font(.system(size: 10)).foregroundStyle(.secondary)
                    if SoftProof.shared.enabled {
                        Text(SoftProof.shared.lut.map { "SOFT PROOF · \($0.profileName)" + (SoftProof.shared.gamutWarning ? " · GAMUT WARNING" : "") }
                             ?? SoftProof.shared.status)
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(Color(nsColor: Theme.accent))
                            .accessibilityIdentifier("softproof-badge")
                    }
                }
            }
            Spacer()
            Text("← → group    ↑ ↓ frame in group    X U P decide    1 2 3 grade    K keep best    C compare    ⌘Z undo    Esc grid")
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

struct ToastView: View {
    let model: AppModel
    let toast: Toast
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 12) {
                Text(toast.message).font(.system(size: 12)).lineLimit(1)
                if toast.undoable {
                    Button("Undo  ⌘Z") { model.undo() }
                        .buttonStyle(.plain)
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(Color(nsColor: Theme.accent))
                }
                if !toast.details.isEmpty {
                    Button("Dismiss") { model.toast = nil }
                        .buttonStyle(.plain)
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                }
            }
            ForEach(Array(toast.details.prefix(5).enumerated()), id: \.offset) { _, line in
                Text(line).font(.system(size: 11)).foregroundStyle(Color(nsColor: Theme.reject))
                    .lineLimit(1).truncationMode(.middle)
            }
            if toast.details.count > 5 {
                Text("and \(toast.details.count - 5) more").font(.system(size: 11)).foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color(nsColor: Theme.toastBackground)))
        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Color.white.opacity(0.08)))
        .shadow(color: .black.opacity(0.35), radius: 10, y: 3)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("toast")
    }
}
