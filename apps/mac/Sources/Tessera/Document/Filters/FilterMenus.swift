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

/// Image menu (document mode): Adjustments ▸ every adjustment, applied to the pixels of the
/// selected pixel layer (the adjustment layers' editors in a sheet).
struct ImageMenu: View {
    let doc: DocumentController?
    let filters: DocumentFilters

    var body: some View {
        let pixel = doc?.primary?.kind == .pixel && filters.busy == nil
        Menu("Adjustments") {
            ForEach(AdjustmentModel.Kind.allCases) { k in
                let button = Button(k == .invert ? k.title : k.title + "…") { if let doc { filters.openAdjustment(k, doc) } }
                switch k {
                case .levels: button.keyboardShortcut("l", modifiers: .command)
                case .hueSaturation: button.keyboardShortcut("u", modifiers: .command)
                case .invert: button.keyboardShortcut("i", modifiers: .command)
                default: button
                }
            }
        }
        .disabled(!pixel)
    }
}
