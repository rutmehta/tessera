import AppKit
import SwiftUI
import TesseraCore

/// A filter dialog (WP M5-12): controls generated from the filter's schema, the 1:1 detail pane,
/// Preview (live on the canvas), Reset, Cancel and OK. Identifiers: `document.filter.<id>.<key>`,
/// `.preview`, `.reset`, `.cancel`, `.ok`, `.detail`.
struct FilterSheet: View {
    @Bindable var model: FilterSheetModel

    private var ident: String { "document.filter.\(model.entry.id)" }

    var body: some View {
        SheetScaffold(title: model.title, subtitle: model.subtitle) {
            EmptyView()
        } content: {
            HStack(alignment: .top, spacing: Theme.Space.l) {
                FilterDetailPane(model: model)
                    .frame(width: 180, height: 180)
                    .accessibilityIdentifier("\(ident).detail")
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    ForEach(model.entry.params) { p in
                        FilterControlView(param: p, value: model.value(p), revision: model.revision,
                                          identifier: "\(ident).\(p.key)") { v, final in model.set(p, v, final: final) }
                    }
                    if let e = model.error {
                        StatusLine(text: e, kind: .error).padding(.top, Theme.Space.xs)
                    }
                    Spacer(minLength: 0)
                }
                .frame(maxWidth: .infinity, alignment: .topLeading)
            }
            .padding(Theme.Space.l)
        } leading: {
            Toggle("Preview", isOn: $model.preview)
                .toggleStyle(.checkbox)
                .font(Theme.Fonts.caption)
                .accessibilityIdentifier("\(ident).preview")
        } actions: {
            Button("Reset") { model.reset() }
                .sheetButton()
                .accessibilityIdentifier("\(ident).reset")
            Button("Cancel") { model.cancel() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
                .accessibilityIdentifier("\(ident).cancel")
            Button("OK") { model.ok() }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .accessibilityIdentifier("\(ident).ok")
        }
        .frame(width: 560, height: 300)
        .onAppear { model.start() }
    }
}

/// One generated control.
struct FilterControlView: View {
    let param: FilterParam
    let value: FilterValue
    let revision: Int
    let identifier: String
    let onChange: (FilterValue, Bool) -> Void

    var body: some View {
        switch param.control {
        case .slider(let lo, let hi, let d, let step, _, _, _):
            DocSlider(title: param.label, value: value.number ?? d, range: lo...hi, defaultValue: d, format: param.control.format,
                      step: step, identifier: identifier, revision: revision) { v, final in onChange(.number(v), final) }
                .frame(height: Theme.Height.slider)
        case .angle(let lo, let hi, let d):
            HStack(spacing: Theme.Space.s) {
                AngleDial(degrees: value.number ?? d) { onChange(.number(min(max($0, lo), hi)), $1) }
                    .frame(width: Theme.Height.large, height: Theme.Height.large)
                    .accessibilityIdentifier("\(identifier).dial")
                DocSlider(title: param.label, value: value.number ?? d, range: lo...hi, defaultValue: d, format: param.control.format,
                          step: 1, identifier: identifier, revision: revision) { v, final in onChange(.number(v), final) }
                    .frame(height: Theme.Height.slider)
            }
        case .choice(let options, let d):
            HStack(spacing: Theme.Space.s) {
                Text(param.label).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .frame(width: Theme.Width.labelWide, alignment: .leading)
                if options.count <= 3 {
                    SegmentedPicker(selection: Binding(get: { value.text ?? d }, set: { onChange(.text($0), true) }),
                                    segments: options.map { .init(value: $0.value, title: $0.label) }, height: Theme.Height.small)
                } else {
                    Menu(options.first { $0.value == (value.text ?? d) }?.label ?? d) {
                        ForEach(options, id: \.value) { o in Button(o.label) { onChange(.text(o.value), true) } }
                    }
                    .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
                }
            }
            .frame(height: Theme.Height.slider)
            .accessibilityIdentifier(identifier)
        case .point:
            let (x, y): (Double, Double) = if case .point(let px, let py) = value { (px, py) } else { (0.5, 0.5) }
            HStack(alignment: .top, spacing: Theme.Space.s) {
                Text(param.label).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .frame(width: Theme.Width.labelWide, alignment: .leading)
                PointPad(x: x, y: y) { onChange(.point($0, $1), $2) }
                    .frame(width: 96, height: 64)
                    .accessibilityIdentifier(identifier)
                Text(String(format: "%.0f %%, %.0f %%", x * 100, y * 100))
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
            }
            .padding(.vertical, Theme.Space.xs)
        case .toggle(let d):
            Toggle(param.label, isOn: Binding(get: { value.bool ?? d }, set: { onChange(.bool($0), true) }))
                .toggleStyle(.checkbox)
                .font(Theme.Fonts.caption)
                .frame(height: Theme.Height.slider)
                .accessibilityIdentifier(identifier)
        }
    }
}

/// A 28 pt angle dial: drag around it to set degrees (counter-clockwise from the right, as in
/// Photoshop's Motion Blur).
struct AngleDial: View {
    let degrees: Double
    let onChange: (Double, Bool) -> Void

    var body: some View {
        GeometryReader { g in
            let r = min(g.size.width, g.size.height) / 2
            let c = CGPoint(x: g.size.width / 2, y: g.size.height / 2)
            let a = degrees * .pi / 180
            ZStack {
                Circle().fill(Theme.well)
                Circle().strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline)
                Path { p in
                    p.move(to: c)
                    p.addLine(to: CGPoint(x: c.x + cos(a) * (r - Theme.Space.xs), y: c.y - sin(a) * (r - Theme.Space.xs)))
                }
                .stroke(Theme.textPrimary, lineWidth: Theme.Space.xxs)
            }
            .contentShape(Circle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { v in onChange(Self.angle(v.location, c), false) }
                .onEnded { v in onChange(Self.angle(v.location, c), true) })
        }
        .help("Drag to set the angle")
        .accessibilityElement()
        .accessibilityLabel("Angle")
        .accessibilityValue("\(Int(degrees.rounded())) degrees")
    }

    static func angle(_ p: CGPoint, _ c: CGPoint) -> Double {
        var d = atan2(-(p.y - c.y), p.x - c.x) * 180 / .pi
        if d > 180 { d -= 360 }
        return d.rounded()
    }
}

/// A normalized point: click or drag in the pad.
struct PointPad: View {
    let x: Double
    let y: Double
    let onChange: (Double, Double, Bool) -> Void

    var body: some View {
        GeometryReader { g in
            let p = CGPoint(x: x * g.size.width, y: y * g.size.height)
            ZStack(alignment: .topLeading) {
                RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Theme.well)
                RoundedRectangle(cornerRadius: Theme.Radius.chip).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline)
                Path { path in
                    path.move(to: CGPoint(x: p.x, y: 0)); path.addLine(to: CGPoint(x: p.x, y: g.size.height))
                    path.move(to: CGPoint(x: 0, y: p.y)); path.addLine(to: CGPoint(x: g.size.width, y: p.y))
                }
                .stroke(Theme.hairlineStrong, lineWidth: Theme.Space.hairline)
                Circle().strokeBorder(Theme.textPrimary, lineWidth: Theme.Space.xxs)
                    .frame(width: Theme.Space.m, height: Theme.Space.m)
                    .position(p)
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { v in onChange(Self.unit(v.location.x, g.size.width), Self.unit(v.location.y, g.size.height), false) }
                .onEnded { v in onChange(Self.unit(v.location.x, g.size.width), Self.unit(v.location.y, g.size.height), true) })
        }
        .help("Click or drag to place the centre")
    }

    static func unit(_ v: CGFloat, _ size: CGFloat) -> Double { min(max(Double(v / max(size, 1)), 0), 1) }
}

/// The 1:1 detail pane: the filter on the layer's own pixels, one image pixel per device pixel.
/// Drag to move.
struct FilterDetailPane: View {
    let model: FilterSheetModel
    @Environment(\.displayScale) private var scale
    @State private var last: CGPoint?

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .bottomLeading) {
                RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.plotWell))
                if let image = model.detail {
                    Image(decorative: image, scale: scale)
                        .interpolation(.none)
                        .frame(width: g.size.width, height: g.size.height)
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                }
                Text(model.detailLevel == 0 ? "1:1" : "1:\(1 << Int(model.detailLevel))")
                    .font(Theme.Fonts.captionNumeric)
                    .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                    .padding(.horizontal, Theme.Space.xs)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                    .padding(Theme.Space.xs)
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 1)
                .onChanged { v in
                    let prev = last ?? v.startLocation
                    model.panDetail(dx: (v.location.x - prev.x) * scale, dy: (v.location.y - prev.y) * scale)
                    last = v.location
                }
                .onEnded { _ in last = nil })
            .onAppear { model.setDetailPixels(CGSize(width: g.size.width * scale, height: g.size.height * scale)) }
        }
        .help("The filter at 100 %. Drag to look at another part of the layer.")
        .accessibilityElement()
        .accessibilityLabel("Filter detail preview")
    }
}

/// Image ▸ Adjustments ▸ <adjustment>…: the Properties editor of that adjustment in a sheet,
/// previewed live on the layer, applied to its pixels on OK.
struct AdjustmentSheet: View {
    @Bindable var model: AdjustmentSheetModel
    let owner: DocumentFilters

    private var ident: String { "document.adjustment.\(model.model.kind.rawValue)" }

    var body: some View {
        SheetScaffold(title: model.model.kind.title, subtitle: "Image ▸ Adjustments on “\(model.layer.name)”") {
            EmptyView()
        } content: {
            ScrollView {
                AdjustmentEditor(document: model.doc, id: model.layer.id, model: model.model,
                                 onEdit: { m, final in model.edit(m, final: final) })
                    .id(model.revision)
                    .padding(Theme.Space.l)
                if let e = model.error { StatusLine(text: e, kind: .error).padding(.horizontal, Theme.Space.l) }
            }
            .scrollIndicators(.never)
            .accessibilityIdentifier(ident)
        } leading: {
            Toggle("Preview", isOn: $model.preview)
                .toggleStyle(.checkbox)
                .font(Theme.Fonts.caption)
                .accessibilityIdentifier("\(ident).preview")
        } actions: {
            Button("Reset") { model.reset() }
                .sheetButton()
                .accessibilityIdentifier("\(ident).reset")
            Button("Cancel") { model.cancel(owner) }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
                .accessibilityIdentifier("\(ident).cancel")
            Button("OK") { model.ok(owner) }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .accessibilityIdentifier("\(ident).ok")
        }
        .frame(width: 380, height: model.model.kind == .curves ? 560 : 420)
        .onAppear { model.start() }
    }
}

/// Blending Options of a smart filter: mode and opacity of the filter over its input.
struct SmartFilterBlendingSheet: View {
    @Bindable var model: SmartFilterBlendingModel
    let owner: DocumentFilters

    var body: some View {
        SheetScaffold(title: "Blending Options", subtitle: model.row.name) {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                HStack(spacing: Theme.Space.s) {
                    Text("Mode").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        .frame(width: Theme.Width.label, alignment: .leading)
                    Menu(model.mode.title) {
                        ForEach(Array(DocBlendMode.grouped.enumerated()), id: \.offset) { i, section in
                            if i > 0 { Divider() }
                            ForEach(section.1) { mode in Button(mode.title) { model.mode = mode } }
                        }
                    }
                    .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
                    .accessibilityIdentifier("document.smartFilter.blending.mode")
                }
                DocSlider(title: "Opacity", value: model.opacity, range: 0...100, defaultValue: 100, format: "%.0f %%",
                          identifier: "document.smartFilter.blending.opacity", revision: 0) { v, _ in model.opacity = v }
                    .frame(height: Theme.Height.slider)
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { owner.blendingSheet = nil }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
            Button("OK") { model.ok(owner) }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .accessibilityIdentifier("document.smartFilter.blending.ok")
        }
        .frame(width: 360, height: 200)
    }
}

/// The three dialogs, attached to the document view.
struct DocumentFilterSheets: ViewModifier {
    @Bindable var filters: DocumentFilters

    func body(content: Content) -> some View {
        content
            .sheet(item: $filters.filterSheet) { FilterSheet(model: $0) }
            .sheet(item: $filters.adjustmentSheet) { AdjustmentSheet(model: $0, owner: filters) }
            .sheet(item: $filters.blendingSheet) { SmartFilterBlendingSheet(model: $0, owner: filters) }
    }
}
