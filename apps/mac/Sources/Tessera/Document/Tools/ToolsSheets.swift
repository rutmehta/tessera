import SwiftUI
import TesseraCore

/// The tools' sheets (WP M5-11): Select and Mask (Refine Edge, live preview in the chosen view mode),
/// Color Range, Modify ▸ Border / Smooth / Expand / Contract / Feather, Fill, Save Selection.
struct ToolSheetsModifier: ViewModifier {
    @Bindable var tools: DocumentTools

    func body(content: Content) -> some View {
        content.sheet(item: $tools.sheet) { sheet in
            switch sheet {
            case .refineEdge: RefineEdgeSheet(tools: tools)
            case .colorRange: ColorRangeSheet(tools: tools)
            case .modify(let kind): ModifySelectionSheet(tools: tools, kind: kind)
            case .fill: FillSheet(tools: tools)
            case .saveSelection: SaveSelectionSheet(tools: tools)
            }
        }
    }
}

private struct SheetSlider: View {
    let title: String
    let value: Double
    let range: ClosedRange<Double>
    var format = "%.1f"
    let onChange: (Double, Bool) -> Void

    var body: some View {
        DocSlider(title: title, value: value, range: range, defaultValue: range.lowerBound < 0 ? 0 : range.lowerBound,
                  format: format, identifier: "document.sheet.\(title.lowercased())", revision: 0, onChange: onChange)
            .frame(height: Theme.Height.slider)
    }
}

/// Select ▸ Select and Mask… : global refinements, previewed live on the canvas.
struct RefineEdgeSheet: View {
    @Bindable var tools: DocumentTools
    @Environment(\.dismiss) private var dismiss
    @State private var applied = false

    private func set(_ f: (inout RefineEdgeSettings) -> Void, final: Bool) {
        f(&tools.refine)
        tools.refineChanged()
    }

    var body: some View {
        SheetScaffold(title: "Select and Mask", subtitle: "Refine the selection edge against the image") {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                SubHeader("View Mode")
                SegmentedPicker(selection: $tools.refinePreview, segments: DocumentTools.RefinePreview.allCases.map {
                    .init(value: $0, title: $0.title)
                }, height: Theme.Height.regular)
                SubHeader("Edge Detection")
                SheetSlider(title: "Radius", value: Double(tools.refine.radius), range: 0...250, format: "%.0f px") { v, f in
                    set({ $0.radius = Float(v) }, final: f)
                }
                OptionToggle(title: "Smart Radius", on: Binding(get: { tools.refine.smartRadius }, set: { v in
                    set({ $0.smartRadius = v }, final: true)
                }))
                SubHeader("Global Refinements")
                SheetSlider(title: "Smooth", value: Double(tools.refine.smooth), range: 0...100, format: "%.0f px") { v, f in
                    set({ $0.smooth = Float(v) }, final: f)
                }
                SheetSlider(title: "Feather", value: Double(tools.refine.feather), range: 0...250, format: "%.1f px") { v, f in
                    set({ $0.feather = Float(v) }, final: f)
                }
                SheetSlider(title: "Contrast", value: Double(tools.refine.contrast * 100), range: 0...100, format: "%.0f %%") { v, f in
                    set({ $0.contrast = Float(v / 100) }, final: f)
                }
                SheetSlider(title: "Shift Edge", value: Double(tools.refine.shiftEdge), range: -100...100, format: "%.0f px") { v, f in
                    set({ $0.shiftEdge = Float(v) }, final: f)
                }
            }
            .padding(Theme.Space.l)
        } leading: {
            Text("Output: Selection")
        } actions: {
            Button("Cancel") { dismiss() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.large))
                .keyboardShortcut(.cancelAction)
            Button("OK") { applied = true; tools.refineFinish(apply: true); dismiss() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large))
                .keyboardShortcut(.defaultAction)
        }
        .frame(width: Theme.Width.inspectorMax + Theme.Space.xxl, height: Theme.Width.inspectorMax + Theme.Space.xxl * 3)
        .onAppear { tools.refine = RefineEdgeSettings(); tools.refinePreview = .overlay }
        .onDisappear { if !applied { tools.refineFinish(apply: false) } }
    }
}

/// Select ▸ Color Range… : the foreground colour (or a colour picked with the eyedropper) ± fuzziness.
struct ColorRangeSheet: View {
    @Bindable var tools: DocumentTools
    @Environment(\.dismiss) private var dismiss
    @State private var fuzziness = 40.0

    var body: some View {
        SheetScaffold(title: "Color Range", subtitle: "Sampled colour: foreground \(tools.colors.foreground.hex)") {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                SheetSlider(title: "Fuzziness", value: fuzziness, range: 0...200, format: "%.0f") { v, _ in fuzziness = v }
                Hint("Pick the colour with the Eyedropper (I) first; it becomes the foreground colour.")
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { dismiss() }.buttonStyle(.theme(.bordered, height: Theme.Height.large)).keyboardShortcut(.cancelAction)
            Button("OK") { tools.colorRange(tools.colors.foreground, fuzziness: Float(fuzziness)); dismiss() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large)).keyboardShortcut(.defaultAction)
        }
        .frame(width: Theme.Width.inspectorMax, height: Theme.Width.sidebarIdeal)
    }
}

struct ModifySelectionSheet: View {
    @Bindable var tools: DocumentTools
    let kind: SelectionModifyKind
    @Environment(\.dismiss) private var dismiss
    @State private var amount = 5.0

    var body: some View {
        SheetScaffold(title: "\(kind.title) Selection", subtitle: nil) {
            EmptyView()
        } content: {
            OptionField(title: kind == .feather ? "Feather Radius" : kind == .border ? "Width" : kind == .smooth ? "Sample Radius"
                            : kind == .expand ? "Expand By" : "Contract By",
                        value: $amount, range: 0...500, unit: "pixels", fractionDigits: 1)
                .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { dismiss() }.buttonStyle(.theme(.bordered, height: Theme.Height.large)).keyboardShortcut(.cancelAction)
            Button("OK") { tools.modify(kind, px: Float(amount)); dismiss() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large)).keyboardShortcut(.defaultAction)
        }
        .frame(width: Theme.Width.inspectorMax, height: Theme.Height.filmstrip * 2 + Theme.Space.xl)
    }
}

struct FillSheet: View {
    @Bindable var tools: DocumentTools
    @Environment(\.dismiss) private var dismiss
    @State private var contents = 0
    @State private var opacity = 100.0

    var body: some View {
        SheetScaffold(title: "Fill", subtitle: "The selection, or the whole layer without one") {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                SegmentedPicker(selection: $contents, segments: [
                    .init(value: 0, title: "Foreground"), .init(value: 1, title: "Background"),
                    .init(value: 2, title: "Content-Aware", help: "Placeholder: a smooth fill from the surroundings"),
                ])
                OptionField(title: "Opacity", value: $opacity, range: 0...100, unit: "%")
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { dismiss() }.buttonStyle(.theme(.bordered, height: Theme.Height.large)).keyboardShortcut(.cancelAction)
            Button("OK") {
                let fill: SelectionFillKind = contents == 0 ? .color(tools.colors.foreground)
                    : contents == 1 ? .color(tools.colors.background) : .contentAware
                tools.fillSelection(fill, opacity: Float(opacity / 100))
                dismiss()
            }
            .buttonStyle(.theme(.primary, height: Theme.Height.large)).keyboardShortcut(.defaultAction)
        }
        .frame(width: Theme.Width.inspectorMax, height: Theme.Width.sidebarIdeal)
    }
}

struct SaveSelectionSheet: View {
    @Bindable var tools: DocumentTools
    @Environment(\.dismiss) private var dismiss
    @State private var name = "Alpha 1"

    var body: some View {
        SheetScaffold(title: "Save Selection", subtitle: "Saved as a channel for this session") {
            EmptyView()
        } content: {
            HStack(spacing: Theme.Space.s) {
                Text("Name").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                TextField("Name", text: $name).textFieldStyle(.roundedBorder).controlSize(.small)
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { dismiss() }.buttonStyle(.theme(.bordered, height: Theme.Height.large)).keyboardShortcut(.cancelAction)
            Button("OK") { tools.saveSelection(name); dismiss() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large)).keyboardShortcut(.defaultAction)
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .frame(width: Theme.Width.inspectorMax, height: Theme.Height.filmstrip * 2)
    }
}
