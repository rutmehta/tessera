import AppKit
import SwiftUI
import TesseraCore

/// Properties ▸ Character and Paragraph for a text layer (WP B5-10). While the layer is being edited on
/// canvas the controls show and change the selection (mixed values are labelled), or with a caret
/// the style of the next typed text; otherwise they change every run of the selected layer. Each
/// completed edit is one history node; slider drags preview live.
struct TextInspector: View {
    let document: DocumentController
    @Bindable var text: DocumentText
    @State private var sourceText = ""

    var body: some View {
        let _ = document.revision
        if let (model, summary, editing) = text.inspected(document) {
            let src = text.source(document)
            VStack(alignment: .leading, spacing: 0) {
                limitations(src)
                character(model, summary)
                paragraph(model)
                box(model, editing: editing)
                if src?.caretEditable == false { sourceEditor(model) }
                actions(editing: editing)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            Hint("Select a text layer, or click the canvas with the Type tool (T).")
        }
    }

    private var rev: Int { document.revision &+ (text.session?.edit.selection.lowerBound ?? 0) &* 31 }

    private func label(_ title: String, _ v: TextStyleValue<Float>?) -> String {
        v == .mixed ? "\(title) (mixed)" : title
    }

    private func slider(_ title: String, _ value: Double, _ range: ClosedRange<Double>, _ def: Double, _ format: String,
                        _ step: Double, _ key: String, _ change: @escaping (Double, Bool) -> Void) -> some View {
        DocSlider(title: title, value: value, range: range, defaultValue: def, format: format, step: step,
                  identifier: "document.text.\(key)", revision: rev, onChange: change)
            .frame(height: Theme.Height.slider)
    }

    // MARK: Character

    @ViewBuilder private func character(_ model: TextSourceModel, _ s: TextStyleSummary) -> some View {
        let first = model.runs.first ?? TextRunModel()
        SubHeader("Character")
        row("Font") {
            MenuPicker(selection: Binding(get: { s.family?.value ?? "" }, set: { f in
                text.character(document, "Font: \(f)") { $0.family = f }
            }), options: familyOptions(s.family))
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityIdentifier("document.text.family")
        }
        row("Style") {
            MenuPicker(selection: Binding(get: { styleKey(s) }, set: { key in
                let (w, i) = Self.parse(key)
                text.character(document, "Font Style") { $0.weight = w; $0.italic = i }
            }), options: styleOptions(s))
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityIdentifier("document.text.style")
        }
        slider(label("Size", s.size), Double(s.size?.value ?? first.size), 1...1000, 24, "%.1f px", 0.5, "size") { v, f in
            text.character(document, "Font Size", final: f) { $0.size = Float(v) }
        }
        slider(label("Leading", s.leading), Double(s.leading?.value ?? first.leading), 0...1000, 0, "%.1f px", 0.5, "leading") { v, f in
            text.character(document, "Leading", final: f) { $0.leading = Float(v) }
        }
        slider(label("Tracking", s.tracking), Double(s.tracking?.value ?? first.tracking), -100...500, 0, "%+.1f px", 0.1, "tracking") { v, f in
            text.character(document, "Tracking", final: f) { $0.tracking = Float(v) }
        }
        slider(label("Baseline shift", s.baselineShift), Double(s.baselineShift?.value ?? first.baselineShift), -500...500, 0,
               "%+.1f px", 0.5, "baselineShift") { v, f in
            text.character(document, "Baseline Shift", final: f) { $0.baselineShift = Float(v) }
        }
        row("Color") {
            let c = s.color?.value ?? first.color
            DocColorWell(rgb: c.prefix(3).map { Double($0) / 255 }, identifier: "document.text.color") { rgb in
                let tc = ToolColor(r: Float(rgb[0]), g: Float(rgb[1]), b: Float(rgb[2]))
                text.character(document, "Text Color") { $0.setColor(tc) }
            }
            .frame(width: Theme.Height.large * 2, height: Theme.Height.regular)
            if s.color == .mixed { Text("Mixed").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary) }
            Spacer(minLength: 0)
        }
        Toggle("Kerning", isOn: Binding(get: { s.kerning?.value ?? true }, set: { on in
            text.character(document, on ? "Kerning On" : "Kerning Off") { $0.kerning = on }
        }))
        .toggleStyle(.checkbox)
        .font(Theme.Fonts.caption)
        .accessibilityIdentifier("document.text.kerning")
        if text.isEditing(document), text.session?.edit.selection.isEmpty == true {
            Hint("No text selected: Character changes apply to the next text you type.")
        }
    }

    private func row<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        HStack(spacing: Theme.Space.s) {
            Text(title).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.Width.label, alignment: .leading)
            content()
        }
        .frame(minHeight: Theme.Height.regular)
    }

    private func familyOptions(_ v: TextStyleValue<String>?) -> [(value: String, title: String)] {
        text.loadFonts()
        var opts = text.fonts.map { (value: $0.family, title: $0.family) }
        if let f = v?.value, !text.fonts.contains(where: { $0.family == f }) { opts.insert((f, "\(f) (missing)"), at: 0) }
        if v == .mixed { opts.insert(("", "Mixed"), at: 0) }
        return opts
    }

    private func styleKey(_ s: TextStyleSummary) -> String {
        guard let w = s.weight?.value, let i = s.italic?.value else { return "" }
        return "\(w)/\(i ? 1 : 0)"
    }

    private static func parse(_ key: String) -> (UInt16, Bool) {
        let p = key.split(separator: "/")
        return (UInt16(p.first ?? "400") ?? 400, p.count > 1 && p[1] == "1")
    }

    private func styleOptions(_ s: TextStyleSummary) -> [(value: String, title: String)] {
        let faces = text.fonts.first { $0.family == s.family?.value }?.faces ?? []
        var opts = faces.map { (value: "\($0.weight)/\($0.italic ? 1 : 0)", title: $0.styleName) }
        var seen = Set<String>()
        opts = opts.filter { seen.insert($0.value).inserted }
        if opts.isEmpty {
            opts = [("400/0", "Regular"), ("400/1", "Italic"), ("700/0", "Bold"), ("700/1", "Bold Italic")]
        }
        if styleKey(s).isEmpty { opts.insert(("", "Mixed"), at: 0) }
        else if !opts.contains(where: { $0.value == styleKey(s) }) {
            let (w, i) = Self.parse(styleKey(s))
            opts.insert((styleKey(s), TextFontFaceInfo(postScriptName: "", weight: w, italic: i).styleName + " (no face)"), at: 0)
        }
        return opts
    }

    // MARK: Paragraph

    @ViewBuilder private func paragraph(_ model: TextSourceModel) -> some View {
        let p = model.paragraph
        SubHeader("Paragraph")
        SegmentedPicker(selection: Binding(get: { p.alignment }, set: { a in
            text.paragraph(document, "Align \(a.title)") { $0.paragraph.alignment = a }
        }), segments: TextParagraphAlignment.allCases.map { .init(value: $0, title: "", symbol: $0.symbol, help: $0.title) },
                        height: Theme.Height.small)
        .accessibilityIdentifier("document.text.alignment")
        .padding(.bottom, Theme.Space.xs)
        paraSlider("Left indent", p.leftIndent, "leftIndent") { $0.leftIndent = $1 }
        paraSlider("Right indent", p.rightIndent, "rightIndent") { $0.rightIndent = $1 }
        paraSlider("First line indent", p.firstLineIndent, "firstLineIndent", range: -1000...1000) { $0.firstLineIndent = $1 }
        paraSlider("Space before", p.spaceBefore, "spaceBefore") { $0.spaceBefore = $1 }
        paraSlider("Space after", p.spaceAfter, "spaceAfter") { $0.spaceAfter = $1 }
    }

    private func paraSlider(_ title: String, _ v: Float, _ key: String, range: ClosedRange<Double> = 0...1000,
                            _ set: @escaping @Sendable (inout TextParagraphModel, Float) -> Void) -> some View {
        slider(title, Double(v), range, 0, "%.1f px", 0.5, key) { value, f in
            text.paragraph(document, title, final: f) { m in set(&m.paragraph, Float(value)) }
        }
    }

    @ViewBuilder private func box(_ model: TextSourceModel, editing: Bool) -> some View {
        SubHeader("Text box")
        switch model.textBox {
        case .point:
            InfoRow(label: "Kind", value: "Point text")
        case .paragraph(let w, let h):
            InfoRow(label: "Kind", value: "Paragraph text")
            InfoRow(label: "Box", value: String(format: "%.0f × %.0f px", w, h))
        }
        Button(model.textBox.isParagraph ? "Convert to Point Text" : "Convert to Paragraph Text") { text.toggleBox(document) }
            .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            .accessibilityIdentifier("document.text.toggleBox")
    }

    // MARK: Limitations, source editor, actions

    @ViewBuilder private func limitations(_ src: TextLayerSource?) -> some View {
        let notes = (src?.limitations ?? []) + (text.layoutError.map { ["Layout: \($0)"] } ?? [])
        ForEach(notes, id: \.self) { note in
            HStack(alignment: .firstTextBaseline, spacing: Theme.Space.xs) {
                Image(systemName: "exclamationmark.triangle").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.warning)
                Text(note).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.vertical, Theme.Space.xxs)
            .accessibilityIdentifier("document.text.limitation")
        }
    }

    @ViewBuilder private func sourceEditor(_ model: TextSourceModel) -> some View {
        SubHeader("Source text")
        TextField("Source text", text: $sourceText, axis: .vertical)
            .textFieldStyle(.roundedBorder)
            .font(Theme.Fonts.caption)
            .lineLimit(1...6)
            .onAppear { sourceText = model.text }
            .onChange(of: model.text) { _, t in sourceText = t }
            .onSubmit { text.replaceSourceText(document, with: sourceText) }
            .accessibilityIdentifier("document.text.source")
        Hint("Warped, path and vertical text keep their shape; edit the words here (Return applies).")
    }

    @ViewBuilder private func actions(editing: Bool) -> some View {
        HStack(spacing: Theme.Space.s) {
            if editing {
                Button("Cancel") { text.cancel() }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .help("Esc: discard the typing since the last change")
                Button("Apply") { text.apply() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Enter or ⌘Return")
                    .accessibilityIdentifier("document.text.apply")
            }
            Spacer(minLength: 0)
            Button("Convert to Pixels") { text.convertToPixels(document) }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .accessibilityIdentifier("document.text.convert")
        }
        .padding(.top, Theme.Space.m)
    }
}

/// The Type tool's options bar: defaults for new text, then Apply / Cancel while editing.
struct TextOptionsBar: View {
    let document: DocumentController
    @Bindable var text: DocumentText

    var body: some View {
        MenuPicker(selection: $text.family, options: families)
            .frame(width: Theme.Width.labelWide + Theme.Space.xxl)
            .help("Font for new text")
            .onAppear { text.loadFonts() }
        OptionField(title: "Size", value: Binding(get: { Double(text.size) }, set: { text.size = Float($0) }),
                    range: 1...1000, unit: "px", identifier: "document.option.textSize")
        SegmentedPicker(selection: $text.alignment, segments: TextParagraphAlignment.allCases.map {
            .init(value: $0, title: "", symbol: $0.symbol, help: $0.title)
        }, height: Theme.Height.small, fill: false)
        .fixedSize()
        if text.isEditing(document) {
            Hairline(vertical: true).frame(height: Theme.Height.small)
            Button("Cancel") { text.cancel() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .help("Esc")
            Button("Apply") { text.apply() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .help("Enter or ⌘Return")
                .accessibilityIdentifier("document.text.optionsApply")
        } else {
            Text("Click for point text · drag for area text · click text to edit").font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textTertiary).fixedSize()
        }
        if let r = text.latencyReadout {
            Text(r).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary).fixedSize()
                .accessibilityIdentifier("document.text.latency")
        }
    }

    private var families: [(value: String, title: String)] {
        let f = text.fonts.map { (value: $0.family, title: $0.family) }
        return f.isEmpty ? [(text.family, text.family)] : f
    }
}
