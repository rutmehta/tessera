import AppKit
import SwiftUI
import TesseraCore
import UniformTypeIdentifiers

/// The inspector's tool sections in document mode (WP M5-11): Color (foreground / background, colour
/// wells, hex, swap, default) and Brushes (presets with tip previews, ABR import, tip settings).
struct ToolInspectorSections: View {
    @Bindable var tools: DocumentTools
    let document: DocumentController

    var body: some View {
        PanelSection("Color") { ColorPanel(tools: tools) }
            .accessibilityIdentifier("document.colorPanel")
        if document.tool.paints {
            PanelSection("Brushes") { BrushesList(tools: tools) }
                .accessibilityIdentifier("document.brushes")
        }
    }
}

/// Foreground and background colour wells.
struct ColorPanel: View {
    @Bindable var tools: DocumentTools

    private func row(_ title: String, _ c: ToolColor, id: String, set: @escaping (ToolColor) -> Void) -> some View {
        HStack(spacing: Theme.Space.s) {
            Text(title).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.Width.label, alignment: .leading)
            DocColorWell(rgb: [Double(c.r), Double(c.g), Double(c.b)], identifier: id) { v in
                set(ToolColor(r: Float(v[0]), g: Float(v[1]), b: Float(v[2])))
            }
            .frame(width: Theme.Height.large * 2, height: Theme.Height.small)
            Text(c.hex).font(Theme.Fonts.captionMono).foregroundStyle(Theme.textSecondary)
            Spacer()
        }
        .frame(height: Theme.Height.regular)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            row("Foreground", tools.colors.foreground, id: "document.color.foreground") { tools.colors.foreground = $0 }
            row("Background", tools.colors.background, id: "document.color.background") { tools.colors.background = $0 }
            HStack(spacing: Theme.Space.s) {
                Button("Swap (X)") { tools.colors.swap() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
                Button("Default (D)") { tools.colors.reset() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
            }
        }
    }
}

/// Brush presets: computed round tips and the tip library (built-in sampled tips and imported ABR),
/// each with its preview; the current tip's settings below.
struct BrushesList: View {
    @Bindable var tools: DocumentTools

    private struct Preset: Identifiable {
        let id: String
        let name: String
        let tipId: String?
        let hardness: Float
        let size: Float?
        let spacing: Float?
    }

    private var presets: [Preset] {
        var p = [
            Preset(id: "round:1", name: "Hard Round", tipId: nil, hardness: 1, size: nil, spacing: nil),
            Preset(id: "round:0.5", name: "Medium Round", tipId: nil, hardness: 0.5, size: nil, spacing: nil),
            Preset(id: "round:0", name: "Soft Round", tipId: nil, hardness: 0, size: nil, spacing: nil),
        ]
        p += tools.tips.map { Preset(id: $0.id, name: $0.name, tipId: $0.id, hardness: 1, size: $0.diameter, spacing: $0.spacing) }
        return p
    }

    private func isCurrent(_ p: Preset) -> Bool {
        let b = tools.currentBrush
        return p.tipId == nil ? (b.tipId == nil && abs(b.hardness - p.hardness) < 0.01) : b.tipId == p.tipId
    }

    private func slider(_ title: String, _ key: WritableKeyPath<BrushOptions, Float>, _ range: ClosedRange<Double>,
                        scale: Double = 1, format: String = "%.0f") -> some View {
        DocSlider(title: title, value: Double(tools.currentBrush[keyPath: key]) * scale, range: range,
                  defaultValue: Double(BrushOptions()[keyPath: key]) * scale, format: format,
                  identifier: "document.brush.\(title.lowercased())", revision: 0) { v, _ in
            var b = tools.currentBrush
            b[keyPath: key] = Float(v / scale)
            tools.currentBrush = b
        }
        .frame(height: Theme.Height.slider)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: Theme.Space.xs), count: 4), spacing: Theme.Space.xs) {
                    ForEach(presets) { p in
                        let on = isCurrent(p)
                        Button {
                            var b = tools.currentBrush
                            b.tipId = p.tipId
                            if p.tipId == nil { b.hardness = p.hardness }
                            if let s = p.spacing { b.spacing = s }
                            if let s = p.size { b.size = s }
                            tools.currentBrush = b
                        } label: {
                            VStack(spacing: Theme.Space.xxs) {
                                Group {
                                    if let img = tools.tipImage(p.id) {
                                        Image(nsImage: img).foregroundStyle(on ? Theme.accent : Theme.textPrimary)
                                    } else {
                                        Image(systemName: "circle.fill").foregroundStyle(Theme.textTertiary)
                                    }
                                }
                                .frame(width: Theme.Height.large, height: Theme.Height.large)
                                Text(p.name).font(Theme.Fonts.caption).foregroundStyle(on ? Theme.textPrimary : Theme.textSecondary)
                                    .lineLimit(1).truncationMode(.tail)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(Theme.Space.xs)
                            .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(on ? Theme.accentSubtle : Theme.clear))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .help(p.name)
                    }
                }
            }
            .frame(maxHeight: Theme.Height.filmstrip * 2)
            HStack(spacing: Theme.Space.s) {
                Button("Import Brushes…") { importAbr() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("document.brushes.import")
                Spacer()
                Text("\(presets.count) presets").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
            }
            slider("Size", \.size, 1...1000, format: "%.0f px")
            slider("Hardness", \.hardness, 0...100, scale: 100, format: "%.0f %%")
            slider("Spacing", \.spacing, 1...200, scale: 100, format: "%.0f %%")
            slider("Angle", \.angle, -180...180, format: "%.0f°")
            slider("Roundness", \.roundness, 1...100, scale: 100, format: "%.0f %%")
        }
        .onAppear { tools.reloadTips() }
    }

    private func importAbr() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "abr") ?? .data]
        panel.allowsMultipleSelection = true
        panel.message = "Photoshop brushes (.abr)"
        guard panel.runModal() == .OK else { return }
        for url in panel.urls { tools.importAbr(url) }
    }
}
