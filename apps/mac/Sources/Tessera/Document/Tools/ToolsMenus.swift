import SwiftUI
import TesseraCore

/// Select menu in document mode (WP B5-04): All, Deselect, Inverse, Subject, Sky, Color Range…,
/// Select and Mask…, Modify ▸, Save / Load Selection, and the selection tools.
struct SelectMenuItems: View {
    let doc: DocumentController?
    let docMode: Bool
    private var tools: DocumentTools { DocumentTools.shared }

    var body: some View {
        let on = doc != nil
        let hasSelection = doc?.marquee != nil
        Button("All") { tools.selectAll() }
            .disabled(!on)
        Button("Deselect") { tools.deselect() }
            .shortcut(docMode, "d", .command)
            .disabled(!hasSelection)
        Button("Inverse") { tools.inverse() }
            .shortcut(docMode, "i", [.command, .shift])
            .disabled(!hasSelection)
        Divider()
        Button("Subject") { tools.selectSubject() }
            .disabled(!on)
        Button("Sky") { tools.selectSky() }
            .disabled(!on)
        Button("Color Range…") { tools.sheet = .colorRange }
            .disabled(!on)
        Divider()
        Button("Select and Mask…") { tools.sheet = .refineEdge }
            .shortcut(docMode, "r", [.command, .option])
            .disabled(!hasSelection)
        Menu("Modify") {
            ForEach(SelectionModifyKind.allCases) { k in
                Button("\(k.title)…") { tools.sheet = .modify(k) }
            }
        }
        .disabled(!hasSelection)
        Divider()
        Button("Save Selection…") { tools.sheet = .saveSelection }
            .disabled(!hasSelection)
        Menu("Load Selection") {
            ForEach(tools.channels(), id: \.self) { name in
                Button(name) { tools.loadSelection(name) }
            }
        }
        .disabled(!on)
        Divider()
        ForEach([DocumentTool.marquee, .ellipseMarquee, .lasso, .polygonLasso, .magneticLasso, .quickSelect, .wand, .objectSelect],
                id: \.self) { t in
            Button("\(t.title)    (\(t.key))") { tools.select(t) }
                .disabled(!on)
        }
    }
}

/// Edit menu additions in document mode (WP B5-04): Fill…, Clear, Free Transform (⌘T), Transform ▸.
struct EditToolsMenuItems: View {
    let doc: DocumentController?
    let docMode: Bool
    private var tools: DocumentTools { DocumentTools.shared }

    var body: some View {
        let pixel = doc?.primary?.kind == .pixel
        Button("Fill…") { tools.sheet = .fill }
            .disabled(!pixel)
        Button("Clear    (⌫)") { _ = tools.clearSelection() }
            .disabled(!pixel || doc?.marquee == nil)
        Divider()
        Button("Free Transform") { tools.beginFreeTransform() }
            .shortcut(docMode, "t", .command)
            .disabled(!pixel)
        Menu("Transform") {
            Button("Rotate 180°") {
                tools.quickTransform("Rotate 180°") { r in
                    .translation(r.midX, r.midY).concatenating(after: .rotation(degrees: 180))
                        .concatenating(after: .translation(-r.midX, -r.midY))
                }
            }
            Button("Rotate 90° Clockwise") {
                tools.quickTransform("Rotate 90° Clockwise") { r in
                    .translation(r.midX, r.midY).concatenating(after: .rotation(degrees: 90))
                        .concatenating(after: .translation(-r.midX, -r.midY))
                }
            }
            Button("Rotate 90° Counter Clockwise") {
                tools.quickTransform("Rotate 90° Counter Clockwise") { r in
                    .translation(r.midX, r.midY).concatenating(after: .rotation(degrees: -90))
                        .concatenating(after: .translation(-r.midX, -r.midY))
                }
            }
            Divider()
            Button("Flip Horizontal") {
                tools.quickTransform("Flip Horizontal") { r in AffineTransform2D(a: -1, b: 0, c: 2 * r.midX, d: 0, e: 1, f: 0) }
            }
            Button("Flip Vertical") {
                tools.quickTransform("Flip Vertical") { r in AffineTransform2D(a: 1, b: 0, c: 0, d: 0, e: -1, f: 2 * r.midY) }
            }
        }
        .disabled(!pixel)
    }
}
