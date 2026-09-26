import TesseraCore
import SwiftUI

/// Menu bar. Single-key culling shortcuts are handled by `KeyRouter` (they are listed in the menu
/// titles rather than bound as key equivalents, so they never fire while typing in a field).
struct AppCommands: Commands {
    let model: AppModel
    @AppStorage(AppearancePreference.defaultsKey) private var appearance = AppearancePreference.system.rawValue

    private var docMode: Bool { model.viewMode == .document }
    private var docs: DocumentWorkspace { model.documents }
    private var doc: DocumentController? { docMode ? docs.current : nil }

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            // Layered documents (WP M5-13).
            Group {
                Button("New Document…") { docs.showNewDocument = true }
                    .keyboardShortcut("n", modifiers: .command)
                Button("Open Document…") { docs.presentOpen() }
                    .keyboardShortcut("o", modifiers: [.command, .shift])
            }
            Divider()
            Button("Open Folder…") { model.presentOpenPanel() }
                .keyboardShortcut("o", modifiers: .command)
            Menu("Open Recent") {
                ForEach(model.recentFolders, id: \.self) { url in
                    Button(url.path) { model.openFolder(url) }
                }
            }
            .disabled(model.recentFolders.isEmpty)
            Divider()
            Button("Import Lightroom Catalog…") { model.presentLightroomImport() }
                .keyboardShortcut("i", modifiers: [.command, .shift])
                .disabled(model.lightroomImport.isRunning)
            Divider()
            Button(model.tether.showPanel ? "Hide Tethered Capture" : "Tethered Capture…") { model.tether.togglePanel() }
            Button("Capture") { model.tether.capture() }
                .keyboardShortcut("t", modifiers: [.command, .shift])
            Divider()
            Button("Export…") { model.presentExport() }
                .shortcut(!docMode, "e", [.command, .shift])
                .disabled(model.exporter.isRunning || docMode)
        }
        CommandGroup(replacing: .printItem) {
            Button("Page Setup…") { model.printing.pageSetup() }
                .keyboardShortcut("p", modifiers: [.command, .shift])
            Button("Print…") { model.presentPrint() }
                .keyboardShortcut("p", modifiers: .command)
                .disabled(model.printing.isRunning)
        }
        // Always enabled: SwiftUI can leave a stale disabled state on menu items, which would
        // swallow ⌘Z. The engine's session is the source of truth and reports "Nothing to undo".
        // ⌘Z follows the last kind of change: develop edits go to the develop session's history,
        // culling to the cull session's.
        CommandGroup(replacing: .undoRedo) {
            Button("Undo") { model.undo() }
                .keyboardShortcut("z", modifiers: .command)
            Button("Redo") { model.redo() }
                .keyboardShortcut("z", modifiers: [.command, .shift])
        }
        CommandGroup(after: .pasteboard) {
            Button(docMode ? "Select All" : "Select All Images") { model.selectAll() }
                .keyboardShortcut("a", modifiers: .command)
        }
        CommandGroup(before: .sidebar) {
            Button("Grid    (G)") { model.viewMode = .grid }
            Button("Loupe    (E / Return)") { model.viewMode = .loupe }
            Button("Compare    (C)") { model.enterCompare() }
            Button("Layered Documents") { model.viewMode = .document }
            Divider()
            Group {
            Button("Zoom In") { doc?.viewport?.zoomIn() }
                .shortcut(docMode, "=", .command)
                .disabled(doc == nil)
            Button("Zoom Out") { doc?.viewport?.zoomOut() }
                .shortcut(docMode, "-", .command)
                .disabled(doc == nil)
            Button("Fit on Screen") { doc?.viewport?.zoomToFit() }
                .shortcut(docMode, "0", .command)
                .disabled(doc == nil)
            Button("100 %") { doc?.viewport?.zoomActual() }
                .shortcut(docMode, "1", .command)
                .disabled(doc == nil)
            Button(docs.panelsHidden ? "Show Panels    (Tab)" : "Hide Panels    (Tab)") { docs.togglePanels() }
                .disabled(!docMode)
            Button("Cycle Screen Mode    (F)") { docs.cycleScreenMode() }
                .disabled(!docMode)
            }
            Divider()
            Button(model.showInspector ? "Hide Inspector" : "Show Inspector") { model.showInspector.toggle() }
                .keyboardShortcut("i", modifiers: [.command, .option])
            Button(model.showFilmstrip ? "Hide Filmstrip" : "Show Filmstrip") { model.showFilmstrip.toggle() }
                .keyboardShortcut("f", modifiers: [.command, .option])
            Picker("Appearance", selection: Binding(get: { appearance }, set: { new in
                appearance = new
                AppearancePreference(rawValue: new)?.apply()
            })) {
                ForEach(AppearancePreference.allCases) { Text($0.title).tag($0.rawValue) }
            }
            Divider()
        }
        CommandMenu("Cull") {
            Button("Reject    (X)") { model.perform(.reject) }
            Button("Undecided    (U)") { model.perform(.undecided) }
            Button("Keep    (P)") { model.perform(.keep) }
            Divider()
            Button("Grade 1 · Keep    (1)") { model.perform(.grade(1)) }
            Button("Grade 2 · Good    (2)") { model.perform(.grade(2)) }
            Button("Grade 3 · Best    (3)") { model.perform(.grade(3)) }
            Divider()
            ForEach([UInt8(6), 7, 8, 9], id: \.self) { m in
                Button("Mark \(m): \(MarkStyle.name(m))    (\(m))") { model.perform(.mark(m)) }
            }
            Button("Add to / Remove from \(model.basketTarget)    (B)") { model.perform(.toggleBasket) }
            Menu("Basket Target: \(model.basketTarget)") {
                ForEach(model.albums) { album in
                    Toggle(album.name, isOn: Binding(get: { album.name == model.basketTarget },
                                                     set: { if $0 { model.setBasketTarget(album.name) } }))
                }
                Divider()
                Button("New Album…") { model.promptNewBasketTarget() }
            }
            Divider()
            Button("Keep Best, Reject Rest of Group    (K)") { model.keepBestRejectRest() }
            Button("Defect Sweep…") { model.showDefectSweep = true }
                .keyboardShortcut("d", modifiers: [.command, .shift])
            Divider()
            Toggle("Assist", isOn: Binding(get: { model.assist.enabled }, set: { model.assist.setEnabled($0) }))
                .keyboardShortcut("a", modifiers: [.command, .option])
            Picker("Assist Mode", selection: Binding(get: { model.agent.preferences.assistAutomated },
                                                     set: { model.assist.setAutomated($0) })) {
                Text("Suggest Decisions to Confirm (Automated)").tag(true)
                Text("Predictions and Order Only (Assisted)").tag(false)
            }
            Toggle("Sort by Keep Confidence", isOn: Binding(get: { model.assist.sortByConfidence },
                                                             set: { model.assist.sortByConfidence = $0 }))
            Button("Confirm Suggested Decisions    (Y)") { model.assist.confirmAll() }
                .disabled(!model.assist.enabled)
            Button("Dismiss Suggestion    (N)") { model.assist.dismiss(model.targetIDs) }
                .disabled(!model.assist.enabled)
            Button("Analyze Shoot Again") { model.assist.analyze(faces: false, force: true, title: "Analyzing") }
                .disabled(!model.isEngineBacked || model.assist.isRunning)
            Button("Analyze Faces") { model.assist.analyze(faces: true, force: false, title: "Finding faces") }
                .disabled(!model.isEngineBacked || model.assist.isRunning)
            Button("Clear Person Filter") { model.assist.clearPersonFilter() }
                .disabled(model.assist.personFilter == nil)
            Divider()
            Button("Next Group    (→ in loupe, ⌥→ in grid)") { model.navigate(.right, groupwise: true, extend: false) }
            Button("Previous Group    (← in loupe, ⌥← in grid)") { model.navigate(.left, groupwise: true, extend: false) }
            Button("Next Frame in Group    (↓ in loupe, ⌥↓ in grid)") { model.navigate(.down, groupwise: true, extend: false) }
            Button("Previous Frame in Group    (↑ in loupe, ⌥↑ in grid)") { model.navigate(.up, groupwise: true, extend: false) }
            Divider()
            Toggle("Auto-Advance    (A)", isOn: Binding(get: { model.autoAdvance }, set: { model.autoAdvance = $0 }))
            Divider()
            Button("Remove from Album    (⌫ in an album)") { model.deletePressed() }
            Button("Delete from Disk…") { model.confirmDeleteFromDisk() }
                .keyboardShortcut(.delete, modifiers: .command)
        }
        documentMenus
        CommandMenu("Library") {
            Button("Edit in Layers") { docs.editInLayers(model.focusedItem) }
                .shortcut(!docMode, "e", .command)
                .disabled(docMode || model.focusedItem == nil)
            Divider()
            Button("New Album…") { model.collections.newAlbum() }
                .keyboardShortcut("n", modifiers: [.command, .option])
            Button("New Album Group…") { model.collections.newGroup() }
            Button("New Smart Album…") { model.collections.newSmartAlbum() }
                .keyboardShortcut("n", modifiers: [.command, .option, .shift])
            Divider()
            Button("Save Filter as Smart Album…") { model.collections.saveFilterAsSmartAlbum() }
            Button("Clear Filter") { model.collections.clearFilter() }
                .keyboardShortcut("l", modifiers: [.command, .option])
            Divider()
            Button("Show Photos Not in Any Album") { model.setSource(.notInAlbum) }
            Divider()
            Button("Suggest Keywords for Selection") { model.collections.understanding.suggestForSelection() }
                .keyboardShortcut("k", modifiers: [.command, .option])
                .disabled(!model.collections.understanding.isAvailable)
            Button("Detect Text in Selection") { model.collections.understanding.detectText() }
                .disabled(!model.collections.understanding.isAvailable)
        }
        CommandMenu("Develop") {
            Button("Auto Edit…") { model.agent.present() }
                .keyboardShortcut("a", modifiers: [.command, .shift])
                .disabled(model.agent.isRunning)
            Button("Agent Review…") { model.agent.showReview = true }
                .disabled(model.agent.queue.isEmpty)
            Divider()
            Button("Reset All Settings") { model.resetDevelop() }
                .keyboardShortcut("r", modifiers: [.command, .shift])
            Button("New Snapshot…") { model.promptSnapshot() }
                .shortcut(!docMode, "s", [.command, .shift])
            Menu("Restore Snapshot") {
                ForEach(model.developHistory?.snapshots ?? [], id: \.self) { name in
                    Button(name) { model.restoreSnapshot(name) }
                }
            }
            .disabled((model.developHistory?.snapshots ?? []).isEmpty)
            Divider()
            Toggle("Soft Proofing    (S)", isOn: Binding(get: { SoftProof.shared.enabled },
                                                          set: { _ in SoftProof.shared.toggle() }))
            Toggle("Gamut Warning    (⇧S)", isOn: Binding(get: { SoftProof.shared.gamutWarning },
                                                            set: { SoftProof.shared.gamutWarning = $0 }))
        }
        CommandMenu("Debug") {
            Toggle("Show Render Timing", isOn: Binding(get: { model.showRenderReadout },
                                                       set: { model.showRenderReadout = $0 }))
                .keyboardShortcut("t", modifiers: [.command, .option])
            Divider()
            Button("Load 20,000 Stub Items") { model.loadStubItems(count: 20_000) }
                .shortcut(!docMode, "n", [.command, .shift])
            Button("Run Grid Scroll Benchmark") { model.requestScrollBenchmark() }
                .keyboardShortcut("b", modifiers: [.command, .shift])
        }
    }

    /// Save items, Layer and Select (document mode, WP M5-13).
    @CommandsBuilder private var documentMenus: some Commands {
        // Close / Save / Save As / Export Flat: ⌘W closes the current document in document mode
        // (asking to save), the window otherwise.
        CommandGroup(replacing: .saveItem) {
            Button(docMode && docs.current != nil ? "Close Document" : "Close") { docs.closeCommand() }
                .keyboardShortcut("w", modifiers: .command)
            Button("Save") { docs.save() }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(doc == nil)
            Button("Save As…") { docs.saveAs() }
                .shortcut(docMode, "s", [.command, .shift])
                .disabled(doc == nil)
            Button("Export Flat…") { docs.showExportFlat = true }
                .shortcut(docMode, "e", [.command, .shift])
                .disabled(doc == nil)
        }
        CommandMenu("Layer") { LayerMenu(doc: doc) }
        // Image ▸ Adjustments and Filter (WP M5-12).
        CommandMenu("Image") { ImageMenu(doc: doc, filters: docs.filters) }
        CommandMenu("Filter") { FilterMenu(doc: doc, filters: docs.filters) }
        CommandMenu("Select") {
            Button("All") { doc?.selectAll() }
                .disabled(doc == nil)
            Button("Deselect") { doc?.deselect() }
                .shortcut(docMode, "d", .command)
                .disabled(doc?.marquee == nil)
            Divider()
            Button("Rectangular Marquee    (M)") { doc?.tool = .marquee; doc?.viewport?.cursorDidChange() }
                .disabled(doc == nil)
            Button("Move    (V)") { doc?.tool = .move; doc?.viewport?.cursorDidChange() }
                .disabled(doc == nil)
        }
    }
}

/// Layer menu (document mode, WP M5-13). The Layers panel's context menu mirrors it.
struct LayerMenu: View {
    let doc: DocumentController?

    var body: some View {
        let primary = doc?.primary
        let on = doc != nil
        Menu("New") {
            Button("Layer") { doc?.addLayer(.pixel) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            Button("Group") { doc?.addLayer(.group(mode: .passThrough)) }
            Menu("Adjustment Layer") {
                ForEach(AdjustmentModel.Kind.allCases) { k in Button(k.title) { doc?.addAdjustment(k) } }
            }
            Menu("Fill Layer") {
                ForEach(FillModel.Kind.allCases) { k in Button(k.title) { doc?.addFill(k) } }
            }
        }
        .disabled(!on)
        Button("Duplicate Layer") { doc?.duplicateSelection() }
            .keyboardShortcut("j", modifiers: .command)
            .disabled(primary == nil)
        Button("Delete Layer") { doc?.deleteSelection() }
            .disabled(primary == nil)
        Button("Rename Layer…") { doc?.report?("Double-click the layer name in the Layers panel to rename it") }
            .disabled(primary == nil)
        Divider()
        Button("Group Layers") { doc?.groupSelection() }
            .keyboardShortcut("g", modifiers: .command)
            .disabled(!on)
        Button("Ungroup Layers") { doc?.ungroupSelection() }
            .keyboardShortcut("g", modifiers: [.command, .shift])
            .disabled(primary?.kind != .group)
        Button(primary?.clipped == true ? "Release Clipping Mask" : "Create Clipping Mask") { doc?.toggleClipping() }
            .keyboardShortcut("g", modifiers: [.command, .option])
            .disabled(primary == nil)
        Divider()
        Menu("Layer Mask") {
            Button("Reveal All") { doc?.addMask(.revealAll) }
                .disabled(primary == nil || primary?.hasMask == true)
            Button("Hide All") { doc?.addMask(.hideAll) }
                .disabled(primary == nil || primary?.hasMask == true)
            Button("From Selection") { doc?.addMask(.fromSelection) }
                .disabled(primary == nil || primary?.hasMask == true || doc?.marquee == nil)
            Divider()
            Button("Delete") { doc?.deleteMask() }
                .disabled(primary?.hasMask != true)
            Button(primary?.hasMask == true && primary?.maskEnabled == false ? "Enable" : "Disable") { doc?.toggleMaskEnabled() }
                .disabled(primary?.hasMask != true)
        }
        .disabled(!on)
        Divider()
        Button("Merge Down") { doc?.mergeDown() }
            .keyboardShortcut("e", modifiers: .command)
            .disabled(primary == nil)
        Button("Flatten Image") { doc?.flatten() }
            .disabled(!on)
    }
}

extension View {
    /// A keyboard shortcut only while `enabled` (⌘E is Edit in Layers in the library and Merge
    /// Down in document mode; ⇧⌘S is New Snapshot in develop and Save As in document mode).
    func shortcut(_ enabled: Bool, _ key: KeyEquivalent, _ modifiers: EventModifiers) -> some View {
        keyboardShortcut(enabled ? KeyboardShortcut(key, modifiers: modifiers) : nil)
    }
}
