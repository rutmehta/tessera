import TesseraCore
import SwiftUI

/// Menu bar. Single-key culling shortcuts are handled by `KeyRouter` (they are listed in the menu
/// titles rather than bound as key equivalents, so they never fire while typing in a field).
struct AppCommands: Commands {
    let model: AppModel

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Open Folder…") { model.presentOpenPanel() }
                .keyboardShortcut("o", modifiers: .command)
            Menu("Open Recent") {
                ForEach(model.recentFolders, id: \.self) { url in
                    Button(url.path) { model.openFolder(url) }
                }
            }
            .disabled(model.recentFolders.isEmpty)
        }
        CommandGroup(replacing: .undoRedo) {
            Button("Undo Cull Change") { model.undo() }
                .keyboardShortcut("z", modifiers: .command)
                .disabled(!model.canUndo)
            Button("Redo Cull Change") { model.redo() }
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .disabled(!model.canRedo)
        }
        CommandGroup(after: .pasteboard) {
            Button("Select All Images") { model.selectAll() }
                .keyboardShortcut("a", modifiers: .command)
        }
        CommandGroup(before: .sidebar) {
            Button("Grid    (G)") { model.viewMode = .grid }
            Button("Loupe    (E / Return)") { model.viewMode = .loupe }
            Divider()
            Button(model.showInspector ? "Hide Inspector" : "Show Inspector") { model.showInspector.toggle() }
                .keyboardShortcut("i", modifiers: [.command, .option])
            Button(model.showFilmstrip ? "Hide Filmstrip" : "Show Filmstrip") { model.showFilmstrip.toggle() }
                .keyboardShortcut("f", modifiers: [.command, .option])
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
            Button("Add to / Remove from Basket    (B)") { model.perform(.toggleBasket) }
            Divider()
            Button("Next Group    (→ in loupe, ⌥→ in grid)") { model.navigate(.right, groupwise: true, extend: false) }
            Button("Previous Group    (← in loupe, ⌥← in grid)") { model.navigate(.left, groupwise: true, extend: false) }
            Button("Next Frame in Group    (↓ in loupe, ⌥↓ in grid)") { model.navigate(.down, groupwise: true, extend: false) }
            Button("Previous Frame in Group    (↑ in loupe, ⌥↑ in grid)") { model.navigate(.up, groupwise: true, extend: false) }
            Divider()
            Toggle("Auto-Advance    (A)", isOn: Binding(get: { model.autoAdvance }, set: { model.autoAdvance = $0 }))
        }
        CommandMenu("Debug") {
            Button("Load 20,000 Stub Items") { model.loadStubItems(count: 20_000) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            Button("Run Grid Scroll Benchmark") { model.requestScrollBenchmark() }
                .keyboardShortcut("b", modifiers: [.command, .shift])
        }
    }
}
