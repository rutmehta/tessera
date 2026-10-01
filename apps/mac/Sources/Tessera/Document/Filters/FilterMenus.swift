import SwiftUI
import TesseraCore

/// Filter menu (document mode, WP B5-05), built from the engine's `list_filters()` catalogue:
/// Last Filter (⌃F), Convert for Smart Filters, then one submenu per group.
struct FilterMenu: View {
    let doc: DocumentController?
    let filters: DocumentFilters

    var body: some View {
        let target = DocumentFilters.target(doc)
        let idle = filters.busy == nil
        let catalogue = filters.catalogue(doc)
        Button(filters.lastFilterTitle) { if let doc { filters.reapplyLast(doc) } }
            .keyboardShortcut("f", modifiers: .control)
            .disabled(target == nil || filters.memory.last == nil || !idle)
        Divider()
        Button("Convert for Smart Filters") { if let doc { filters.convertForSmartFilters(doc) } }
            .disabled(doc?.primary == nil || doc?.primary?.kind == .smartObject || doc?.primary?.kind == .adjustment || !idle)
        Divider()
        // B5-09 begin
        NeuralFiltersMenuItem(doc: doc)
        LiquifyMenuItem(doc: doc)   // B5-13
        Divider()
        // B5-09 end
        CameraRawMenuItem(doc: doc)   // B5-18: Camera Raw Filter… (⇧⌘A)
        AdaptiveWideAngleMenuItem(doc: doc)   // B5-20: Adaptive Wide Angle… (⌥⇧⌘A)
        ForEach(FilterCatalogEntry.grouped(catalogue), id: \.group) { section in
            Menu(section.group) {
                ForEach(section.entries) { e in
                    Button(e.menuTitle) { if let doc { filters.open(e, doc) } }
                }
            }
            .disabled(target == nil || !idle)
        }
    }
}

/// Image menu (document mode): Adjustments ▸ every adjustment in Photoshop's groups, applied to the pixels of
/// the selected pixel layer (the adjustment layers' editors in a sheet; Invert, Desaturate and Equalize at once),
/// then Auto Tone / Auto Contrast / Auto Color (WP B5-06).
struct ImageMenu: View {
    let doc: DocumentController?
    let filters: DocumentFilters

    var body: some View {
        let pixel = doc?.primary?.kind == .pixel && filters.busy == nil
        Menu("Adjustments") {
            ForEach(Array(AdjustmentModel.Kind.imageMenuSections.enumerated()), id: \.offset) { i, section in
                if i > 0 { Divider() }
                ForEach(section) { k in
                    let button = Button(k.appliesDirectly ? k.title : k.title + "…") { if let doc { filters.openAdjustment(k, doc) } }
                    switch k {
                    case .levels: button.keyboardShortcut("l", modifiers: .command)
                    case .hueSaturation: button.keyboardShortcut("u", modifiers: .command)
                    case .colorBalance: button.keyboardShortcut("b", modifiers: .command)
                    case .blackWhite: button.keyboardShortcut("b", modifiers: [.command, .option, .shift])
                    case .invert: button.keyboardShortcut("i", modifiers: .command)
                    case .desaturate: button.keyboardShortcut("u", modifiers: [.command, .shift])
                    default: button
                    }
                }
            }
        }
        .disabled(!pixel)
        Divider()
        ForEach(AutoModeModel.allCases) { mode in
            let button = Button(mode.title) { if let doc { filters.applyAuto(mode, doc) } }.disabled(!pixel)
            switch mode {
            case .tone: button.keyboardShortcut("l", modifiers: [.command, .shift])
            case .contrast: button.keyboardShortcut("l", modifiers: [.command, .option, .shift])
            case .color: button.keyboardShortcut("b", modifiers: [.command, .shift])
            }
        }
    }
}
