import AppKit
import SwiftUI
import TesseraCore

/// Properties for a shape layer (WP B5-11): the live construction parameters (rectangle size and four
/// corner radii, ellipse radii, polygon sides / star inset / rotation, line ends) or the custom path's
/// fill rule; fill (none, solid, gradient; imported patterns kept and shown read-only); stroke
/// (colour, width, alignment, caps, joins, miter limit, dashes and offset); the document-space vector
/// mask (enabled, density, feather, whether Path Selection moves it); and the engine's stated
/// interchange / colour limitations. Sliders preview while dragging and record one node on release.
struct ShapeInspector: View {
    let document: DocumentController
    @Bindable var vector: DocumentVector
    let layer: DocLayerID
    @State private var dashText = ""
    @State private var linkRadii = true

    private var revision: Int { document.revision }

    var body: some View {
        if let info = vector.info(for: document, layer: layer) {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                shape(info)
                fill(info)
                stroke(info)
                mask(info)
                notes(info)
                Button("Convert to Pixels") { vector.convertToPixels() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .padding(.top, Theme.Space.s)
                    .help("Layer ▸ Rasterize ▸ Shape: one history step; undo restores the live shape")
                    .accessibilityIdentifier("document.shape.convert")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("document.shape.inspector")
        } else {
            Hint("This shape could not be read.")
        }
    }

    // MARK: Helpers

    private func slider(_ title: String, _ value: Double, _ range: ClosedRange<Double>, _ def: Double, _ format: String,
                        _ step: Double, _ key: String, _ change: @escaping (Double, Bool) -> Void) -> some View {
        DocSlider(title: title, value: value, range: range, defaultValue: def, format: format, step: step,
                  identifier: "document.shape.\(key)", revision: revision, onChange: change)
            .frame(height: Theme.Height.slider)
    }

    private func label(_ s: String) -> some View {
        Text(s).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary).frame(width: Theme.Width.label, alignment: .leading)
    }

    private func edit(_ info: ShapeLayerInfo, final: Bool, _ f: (inout ShapeSource) -> Void) {
        var s = info.source
        f(&s)
        vector.setSource(document, layer: layer, s, final: final)
    }

    private func colorRow(_ title: String, _ rgba: [Double], _ key: String, _ set: @escaping ([Double]) -> Void) -> some View {
        HStack(spacing: Theme.Space.s) {
            label(title)
            DocColorWell(rgb: rgba, identifier: "document.shape.\(key)") { rgb in set(rgb + [rgba.count > 3 ? rgba[3] : 1]) }
                .frame(width: Theme.Height.large * 2, height: Theme.Height.regular)
            Spacer()
        }
    }

    // MARK: Shape

    @ViewBuilder private func shape(_ info: ShapeLayerInfo) -> some View {
        SubHeader(info.liveKind.map { "Live " + $0.capitalized } ?? "Custom Path")
        let t = info.transform
        InfoRow(label: "Position", value: String(format: "%.1f, %.1f px", t.c, t.f))
        if let b = info.bounds { InfoRow(label: "Size", value: String(format: "%.1f × %.1f px", b.width, b.height)) }
        if !t.isSimilarity || abs(t.rotationDegrees) > 1e-9 {
            InfoRow(label: "Transform", value: String(format: "%.1f° · %.0f × %.0f %%", t.rotationDegrees, t.axisScales.x * 100, t.axisScales.y * 100))
        }
        switch info.source.liveShape {
        case .rectangle(let r, let radii)?:
            rectangle(info, r, radii)
        case .ellipse(let c, let radii)?:
            slider("Width", radii.x * 2, 0...max(4000, radii.x * 4), radii.x * 2, "%.1f px", 0.5, "ellipse.width") { v, f in
                edit(info, final: f) { $0.liveShape = .ellipse(center: c, radii: ShapePoint(x: v / 2, y: radii.y)) }
            }
            slider("Height", radii.y * 2, 0...max(4000, radii.y * 4), radii.y * 2, "%.1f px", 0.5, "ellipse.height") { v, f in
                edit(info, final: f) { $0.liveShape = .ellipse(center: c, radii: ShapePoint(x: radii.x, y: v / 2)) }
            }
        case .polygon(let c, let radius, let sides, let rotation, let inner)?:
            slider("Sides", Double(sides), 3...64, 5, "%.0f", 1, "polygon.sides") { v, f in
                edit(info, final: f) { $0.liveShape = .polygon(center: c, radius: radius, sides: UInt32(v.rounded()), rotation: rotation, innerRadius: inner) }
            }
            slider("Radius", radius, 0...max(4000, radius * 2), radius, "%.1f px", 0.5, "polygon.radius") { v, f in
                edit(info, final: f) {
                    $0.liveShape = .polygon(center: c, radius: v, sides: sides, rotation: rotation, innerRadius: inner.map { $0 / max(radius, 1e-9) * v })
                }
            }
            slider("Rotation", rotation * 180 / .pi, -180...180, 0, "%.1f°", 0.5, "polygon.rotation") { v, f in
                edit(info, final: f) { $0.liveShape = .polygon(center: c, radius: radius, sides: sides, rotation: v * .pi / 180, innerRadius: inner) }
            }
            Toggle("Star", isOn: Binding(get: { inner != nil }, set: { on in
                edit(info, final: true) { $0.liveShape = .polygon(center: c, radius: radius, sides: sides, rotation: rotation, innerRadius: on ? radius * 0.5 : nil) }
            }))
            .toggleStyle(.checkbox).controlSize(.small).font(Theme.Fonts.caption)
            .accessibilityIdentifier("document.shape.polygon.star")
            if let inner {
                slider("Star inset", 100 - inner / max(radius, 1e-9) * 100, 0...99, 50, "%.0f %%", 1, "polygon.inset") { v, f in
                    edit(info, final: f) { $0.liveShape = .polygon(center: c, radius: radius, sides: sides, rotation: rotation, innerRadius: radius * (1 - v / 100)) }
                }
            }
        case .line(let a, let b)?:
            let length = a.distance(to: b), angle = atan2(b.y - a.y, b.x - a.x)
            slider("Length", length, 0...max(8000, length * 2), length, "%.1f px", 0.5, "line.length") { v, f in
                edit(info, final: f) { $0.liveShape = .line(start: a, end: ShapePoint(x: a.x + cos(angle) * v, y: a.y + sin(angle) * v)) }
            }
            slider("Angle", angle * 180 / .pi, -180...180, 0, "%.1f°", 0.5, "line.angle") { v, f in
                let r = v * .pi / 180
                edit(info, final: f) { $0.liveShape = .line(start: a, end: ShapePoint(x: a.x + cos(r) * length, y: a.y + sin(r) * length)) }
            }
        case .custom?, nil:
            Hint("Edit anchors and handles with Direct Selection (A); the Pen (P) adds or deletes anchors. Custom paths no longer regenerate from a primitive.")
            HStack(spacing: Theme.Space.s) {
                label("Fill rule")
                SegmentedPicker(selection: Binding(get: { info.source.path.fillRule }, set: { vector.setFillRule(document, layer: layer, $0) }),
                                segments: ShapeFillRule.allCases.map { .init(value: $0, title: $0.title) }, height: Theme.Height.small)
                    .accessibilityIdentifier("document.shape.fillRule")
            }
        }
    }

    @ViewBuilder private func rectangle(_ info: ShapeLayerInfo, _ r: ShapeRect, _ radii: [Double]) -> some View {
        slider("Width", r.width, 0...max(8000, r.width * 2), r.width, "%.1f px", 0.5, "rect.width") { v, f in
            edit(info, final: f) { $0.liveShape = .rectangle(rect: ShapeRect(x0: r.x0, y0: r.y0, x1: r.x0 + v, y1: r.y1), radii: radii) }
        }
        slider("Height", r.height, 0...max(8000, r.height * 2), r.height, "%.1f px", 0.5, "rect.height") { v, f in
            edit(info, final: f) { $0.liveShape = .rectangle(rect: ShapeRect(x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y0 + v), radii: radii) }
        }
        Toggle("Same radius for all corners", isOn: $linkRadii)
            .toggleStyle(.checkbox).controlSize(.small).font(Theme.Fonts.caption)
            .accessibilityIdentifier("document.shape.rect.linkRadii")
        let maxR = max(min(r.width, r.height) / 2, 1)
        if linkRadii {
            slider("Corner radius", radii.first ?? 0, 0...maxR, 0, "%.1f px", 0.5, "rect.radius") { v, f in
                edit(info, final: f) { $0.liveShape = .rectangle(rect: r, radii: [v, v, v, v]) }
            }
        } else {
            ForEach(Array(["Top left", "Top right", "Bottom right", "Bottom left"].enumerated()), id: \.offset) { i, title in
                slider(title, i < radii.count ? radii[i] : 0, 0...maxR, 0, "%.1f px", 0.5, "rect.radius\(i)") { v, f in
                    var rr = (0..<4).map { $0 < radii.count ? radii[$0] : 0 }
                    rr[i] = v
                    edit(info, final: f) { $0.liveShape = .rectangle(rect: r, radii: rr) }
                }
            }
        }
    }

    // MARK: Fill

    private enum PaintKind: String, Hashable { case none, solid, gradient, pattern }
    private func kind(_ p: ShapePaint?) -> PaintKind {
        switch p {
        case nil: .none
        case .solid?: .solid
        case .gradient?: .gradient
        case .pattern?: .pattern
        }
    }

    private func paintSegments(allowNone: Bool, pattern: Bool) -> [SegmentedPicker<PaintKind>.Segment] {
        var s: [SegmentedPicker<PaintKind>.Segment] = allowNone ? [.init(value: .none, title: "None")] : []
        s += [.init(value: .solid, title: "Solid"), .init(value: .gradient, title: "Gradient")]
        if pattern { s.append(.init(value: .pattern, title: "Pattern")) }
        return s
    }

    /// A default gradient across the shape's bounds (document pixels: paint does not move with the shape).
    private func defaultGradient(_ info: ShapeLayerInfo, from c: [Double]) -> ShapePaint {
        let b = info.bounds ?? CGRect(x: 0, y: 0, width: 100, height: 100)
        return .gradient(ShapeGradient(kind: .linear, start: ShapePoint(x: Double(b.minX), y: Double(b.midY)),
                                       end: ShapePoint(x: Double(max(b.maxX, b.minX + 1)), y: Double(b.midY)),
                                       stops: [ShapeGradientStop(position: 0, color: c), ShapeGradientStop(position: 1, color: [1, 1, 1, 1])]))
    }

    @ViewBuilder private func paintEditor(_ info: ShapeLayerInfo, _ paint: ShapePaint?, key: String, allowNone: Bool,
                                          set: @escaping (ShapePaint?, Bool) -> Void) -> some View {
        let k = kind(paint)
        SegmentedPicker(selection: Binding(get: { k }, set: { new in
            let base = paint?.representativeColor ?? vector.options.fillColor
            switch new {
            case .none: set(nil, true)
            case .solid: set(.solid(base), true)
            case .gradient: set(defaultGradient(info, from: base), true)
            case .pattern: break
            }
        }), segments: paintSegments(allowNone: allowNone, pattern: k == .pattern), height: Theme.Height.small)
        .accessibilityIdentifier("document.shape.\(key).kind")
        switch paint {
        case .solid(let c)?:
            colorRow("Color", c, "\(key).color") { set(.solid($0), true) }
        case .gradient(let g)?:
            HStack(spacing: Theme.Space.s) {
                label("Style")
                MenuPicker(selection: Binding(get: { g.kind }, set: { var n = g; n.kind = $0; set(.gradient(n), true) }),
                           options: ShapeGradientKind.allCases.map { ($0, $0.title) })
                    .accessibilityIdentifier("document.shape.\(key).gradientKind")
            }
            colorRow("Start", g.stops.first?.color ?? [0, 0, 0, 1], "\(key).start") { c in
                var n = g; n.stops[0].color = c; set(.gradient(n), true)
            }
            colorRow("End", g.stops.last?.color ?? [1, 1, 1, 1], "\(key).end") { c in
                var n = g; n.stops[n.stops.count - 1].color = c; set(.gradient(n), true)
            }
            let angle = atan2(g.end.y - g.start.y, g.end.x - g.start.x) * 180 / .pi
            slider("Angle", angle, -180...180, 0, "%.0f°", 1, "\(key).angle") { v, f in
                let mid = g.start.lerp(g.end, 0.5), half = g.start.distance(to: g.end) / 2, r = v * .pi / 180
                var n = g
                n.start = ShapePoint(x: mid.x - cos(r) * half, y: mid.y - sin(r) * half)
                n.end = ShapePoint(x: mid.x + cos(r) * half, y: mid.y + sin(r) * half)
                set(.gradient(n), f)
            }
            Hint("Gradients are anchored to the document: moving the shape does not move its paint.")
        case .pattern?:
            StatusLine(text: "Imported pattern: kept as-is in Tessera documents; PSD export does not write pattern shape fills yet.", kind: .warning)
        case nil:
            EmptyView()
        }
    }

    @ViewBuilder private func fill(_ info: ShapeLayerInfo) -> some View {
        SubHeader("Fill")
        paintEditor(info, info.source.fill, key: "fill", allowNone: true) { p, final in
            edit(info, final: final) { $0.fill = p }
        }
    }

    // MARK: Stroke

    @ViewBuilder private func stroke(_ info: ShapeLayerInfo) -> some View {
        SubHeader("Stroke")
        let pair = info.source.stroke
        paintEditor(info, pair?.1, key: "stroke", allowNone: true) { p, final in
            edit(info, final: final) { s in
                if let p { s.stroke = (s.stroke?.0 ?? ShapeStroke(width: vector.options.strokeWidth, alignment: info.hasOpenSubpaths ? .center : .center), p) }
                else { s.stroke = nil }
            }
        }
        if let (st, paint) = pair {
            let set = { (final: Bool, f: (inout ShapeStroke) -> Void) in
                var n = st
                f(&n)
                edit(info, final: final) { $0.stroke = (n, paint) }
            }
            slider("Width", st.width, 0...max(200, st.width * 2), 1, "%.1f px", 0.5, "stroke.width") { v, f in set(f) { $0.width = v } }
            HStack(spacing: Theme.Space.s) {
                label("Align")
                SegmentedPicker(selection: Binding(get: { st.alignment }, set: { a in set(true) { $0.alignment = a } }),
                                segments: ShapeStrokeAlignment.allCases.map { .init(value: $0, title: $0.title) }, height: Theme.Height.small)
                    .accessibilityIdentifier("document.shape.stroke.alignment")
            }
            if info.hasOpenSubpaths && info.source.liveShape.map({ ShapePrimitives.path($0).hasOpenSubpaths }) ?? true {
                Hint("Open paths and lines take Center alignment only.")
            }
            HStack(spacing: Theme.Space.s) {
                label("Caps")
                SegmentedPicker(selection: Binding(get: { st.cap }, set: { c in set(true) { $0.cap = c } }),
                                segments: ShapeLineCap.allCases.map { .init(value: $0, title: $0.title) }, height: Theme.Height.small)
                    .accessibilityIdentifier("document.shape.stroke.cap")
            }
            HStack(spacing: Theme.Space.s) {
                label("Corners")
                MenuPicker(selection: Binding(get: { st.join }, set: { j in set(true) { $0.join = j } }),
                           options: ShapeLineJoin.allCases.map { ($0, $0.title) })
                    .accessibilityIdentifier("document.shape.stroke.join")
                Spacer()
            }
            if st.join == .miter || st.join == .miterClip {
                slider("Miter limit", st.miterLimit, 1...20, 4, "%.1f", 0.1, "stroke.miter") { v, f in set(f) { $0.miterLimit = v } }
            }
            HStack(spacing: Theme.Space.s) {
                label("Dashes")
                TextField("Solid", text: $dashText)
                    .textFieldStyle(.roundedBorder).controlSize(.small).font(Theme.Fonts.captionNumeric)
                    .onAppear { dashText = Self.dashString(st.dashes) }
                    .onChange(of: st.dashes) { _, d in dashText = Self.dashString(d) }
                    .onSubmit {
                        guard let d = Self.parseDashes(dashText) else {
                            document.report?("Dashes: positive lengths separated by commas, e.g. 12, 6 (empty for a solid stroke)")
                            dashText = Self.dashString(st.dashes)
                            return
                        }
                        set(true) { $0.dashes = d }
                    }
                    .help("Dash and gap lengths in pixels, e.g. 12, 6; empty for a solid stroke")
                    .accessibilityIdentifier("document.shape.stroke.dashes")
            }
            if !st.dashes.isEmpty {
                slider("Dash offset", st.dashOffset, -200...200, 0, "%.1f px", 0.5, "stroke.dashOffset") { v, f in set(f) { $0.dashOffset = v } }
            }
        }
    }

    static func dashString(_ d: [Double]) -> String { d.map { String(format: "%g", $0) }.joined(separator: ", ") }
    static func parseDashes(_ s: String) -> [Double]? {
        let parts = s.split(whereSeparator: { $0 == "," || $0 == " " }).map(String.init).filter { !$0.isEmpty }
        let values = parts.compactMap(Double.init)
        guard values.count == parts.count, values.allSatisfy({ $0.isFinite && $0 > 0 }) else { return nil }
        return values
    }

    // MARK: Vector mask

    @ViewBuilder private func mask(_ info: ShapeLayerInfo) -> some View {
        SubHeader("Vector Mask")
        if let m = info.vectorMask {
            let set = { (final: Bool, f: (inout VectorMaskInfo) -> Void) in
                var n = m
                f(&n)
                vector.setMask(document, layer: layer, n, final: final)
            }
            Toggle("Enabled", isOn: Binding(get: { m.enabled }, set: { on in set(true) { $0.enabled = on } }))
                .toggleStyle(.checkbox).controlSize(.small).font(Theme.Fonts.caption)
                .accessibilityIdentifier("document.shape.mask.enabled")
            slider("Density", Double(m.density) * 100, 0...100, 100, "%.0f %%", 1, "mask.density") { v, f in set(f) { $0.density = Float(v / 100) } }
            slider("Feather", Double(m.feather), 0...Double(VectorMaskInfo.featherRange.upperBound), 0, "%.1f px", 0.5, "mask.feather") { v, f in
                set(f) { $0.feather = Float(v) }
            }
            Toggle("Move with the shape (Path Selection)", isOn: $vector.moveMaskWithShape)
                .toggleStyle(.checkbox).controlSize(.small).font(Theme.Fonts.caption)
                .help("Off: the mask stays fixed in the document when the shape moves. On: a Path Selection drag moves both in one step.")
                .accessibilityIdentifier("document.shape.mask.linked")
            Button("Delete Vector Mask") { vector.deleteVectorMask() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .accessibilityIdentifier("document.shape.mask.delete")
        } else {
            HStack(spacing: Theme.Space.s) {
                Button("Add Vector Mask") { vector.addVectorMask(fromSelection: false) }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("document.shape.mask.add")
                Button("From Selection") { vector.addVectorMask(fromSelection: true) }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .disabled(document.marquee == nil)
            }
            Hint("A vector mask sits next to any layer mask; both apply.")
        }
    }

    @ViewBuilder private func notes(_ info: ShapeLayerInfo) -> some View {
        if !info.notes.isEmpty {
            SubHeader("Interchange")
            ForEach(info.notes, id: \.self) { n in
                StatusLine(text: n, kind: .warning)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// Options bar of the shape tools, the Pen and the path selection tools.
struct ShapeOptionsBar: View {
    @Bindable var document: DocumentController
    @Bindable var vector: DocumentVector

    private func colorWell(_ rgba: [Double], _ key: String, _ set: @escaping ([Double]) -> Void) -> some View {
        DocColorWell(rgb: rgba, identifier: "document.option.\(key)") { set($0 + [1]) }
            .frame(width: Theme.Height.large, height: Theme.Height.small)
    }

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Height.small) }

    var body: some View {
        switch document.tool {
        case .rectangleShape, .ellipseShape, .polygonShape, .lineShape, .pen:
            if document.tool != .lineShape {
                OptionToggle(title: "Fill", on: $vector.options.fillEnabled)
                colorWell(vector.options.fillColor, "fillColor") { vector.options.fillColor = $0 }
                separator
            }
            OptionToggle(title: document.tool == .lineShape ? "Stroke colour" : "Stroke", on: $vector.options.strokeEnabled)
            colorWell(vector.options.strokeColor, "strokeColor") { vector.options.strokeColor = $0 }
            if document.tool == .lineShape {
                OptionField(title: "Weight", value: $vector.options.lineWeight, range: 0.5...500, unit: "px", fractionDigits: 1)
            } else {
                OptionField(title: "Width", value: $vector.options.strokeWidth, range: ShapeToolOptions.widthRange, unit: "px", fractionDigits: 1)
            }
            separator
            switch document.tool {
            case .rectangleShape:
                OptionField(title: "Radius", value: $vector.options.cornerRadius, range: 0...5000, unit: "px", fractionDigits: 1)
                hint("Drag · ⇧ square · ⌥ from the centre")
            case .ellipseShape:
                hint("Drag · ⇧ circle · ⌥ from the centre")
            case .polygonShape:
                OptionField(title: "Sides", value: Binding(get: { Double(vector.options.sides) }, set: { vector.options.setSides(Int($0)) }),
                            range: 3...100)
                OptionToggle(title: "Star", on: $vector.options.star)
                if vector.options.star {
                    OptionField(title: "Inset", value: Binding(get: { (1 - vector.options.starInset) * 100 },
                                                               set: { vector.options.starInset = min(max(1 - $0 / 100, 0.01), 1) }),
                                range: 0...99, unit: "%")
                }
                hint("Drag from the centre · ⇧ snaps the angle")
            case .lineShape:
                hint("Drag · ⇧ snaps to 45°")
            default:
                hint("Click corners · drag for curves (⌥ breaks handles) · click the first point to close · Return finishes · Esc cancels")
            }
        case .pathSelect:
            OptionToggle(title: "Move vector mask with shape", on: $vector.moveMaskWithShape)
            separator
            Menu("Combine") {
                ForEach(ShapeOperation.allCases) { op in Button(op.title) { vector.combineSelected(op) } }
            }
            .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
            .fixedSize()
            .disabled(!vector.canCombine)
            hint("Click a shape · drag handles to scale, outside to rotate · each drag is one undo step, Esc cancels")
        default:
            hint("Click an anchor to select it (⇧ adds) · drag anchors or handles (⌥ breaks a handle) · ⌥-click a segment adds a point · ⌫ deletes")
        }
    }

    private func hint(_ s: String) -> some View {
        Text(s).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
    }
}
