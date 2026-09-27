import AppKit
import SwiftUI
import TesseraCore
import UniformTypeIdentifiers

/// Properties editors for the M5-26 / M5-28 adjustments (WP B5-06), in the layout of the existing ones:
/// a `SubHeader`, `DocSlider` rows (live while dragging, one history row on release), `SegmentedPicker` for up
/// to three choices and a `ThemeMenuStyle` pop-up beyond, checkboxes for toggles. Identifiers are
/// `document.properties.<kind>.<control>`. The same view runs inside the Image ▸ Adjustments sheet
/// (`inSheet`), where image-dependent analysis reads the target pixel layer instead of the composite.
struct ExtendedAdjustmentEditor: View {
    let document: DocumentController
    let id: DocLayerID
    let model: AdjustmentModel
    let revision: Int
    let inSheet: Bool
    let set: (AdjustmentModel, Bool) -> Void

    private var state: AdjustmentEditorState { .shared }

    var body: some View {
        switch model {
        case .brightnessContrast(let b, let c, let legacy): brightnessContrast(b, c, legacy)
        case .vibrance(let v, let s):
            slider("Vibrance", v, -100...100, 0, "%+.0f", 1, "vibrance.vibrance") { x, f in set(.vibrance(vibrance: x, saturation: s), f) }
            slider("Saturation", s, -100...100, 0, "%+.0f", 1, "vibrance.saturation") { x, f in set(.vibrance(vibrance: v, saturation: x), f) }
        case .colorBalance(let m): colorBalance(m)
        case .blackWhite(let sliders, let tint): blackWhite(sliders, tint)
        case .photoFilter(let color, let density, let preserve): photoFilter(color, density, preserve)
        case .gradientMap(let m): gradientMap(m)
        case .selectiveColor(let colors, let absolute): selectiveColor(colors, absolute)
        case .desaturate: Hint("Desaturate has no settings: each pixel becomes the gray of its HSL lightness.")
        case .equalize: equalize()
        case .auto(let m): auto(m)
        case .matchColor(let m): matchColor(m)
        case .replaceColor(let c, let fz, let h, let s, let l): replaceColor(c, fz, h, s, l)
        case .colorLookup(let size, let data, _, _): colorLookup(size, data)
        case .shadowsHighlights(let m): shadowsHighlights(m)
        case .hdrToning(let m): hdrToning(m)
        default: EmptyView()
        }
    }

    // MARK: Controls

    private func slider(_ title: String, _ value: Double, _ range: ClosedRange<Double>, _ def: Double, _ format: String,
                        _ step: Double, _ key: String, enabled: Bool = true,
                        _ change: @escaping (Double, Bool) -> Void) -> some View {
        DocSlider(title: title, value: value, range: range, defaultValue: def, format: format, step: step, enabled: enabled,
                  identifier: "document.properties.\(key)", revision: revision, onChange: change)
            .frame(height: Theme.Height.slider)
    }

    private func checkbox(_ title: String, _ on: Bool, _ key: String, _ change: @escaping (Bool) -> Void) -> some View {
        Toggle(title, isOn: Binding(get: { on }, set: change))
            .toggleStyle(.checkbox)
            .font(Theme.Fonts.caption)
            .accessibilityIdentifier("document.properties.\(key)")
    }

    private func labeled<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        HStack(spacing: Theme.Space.s) {
            Text(title).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.Width.label, alignment: .leading)
            content()
            Spacer(minLength: 0)
        }
        .frame(minHeight: Theme.Height.regular)
    }

    private func popUp<T: Hashable>(_ title: String, _ current: String, _ options: [(T, String)], _ key: String,
                                    _ pick: @escaping (T) -> Void) -> some View {
        labeled(title) {
            Menu {
                ForEach(options, id: \.0) { o in Button(o.1) { pick(o.0) } }
            } label: {
                Text(current).frame(minWidth: Theme.Width.labelWide, alignment: .leading)
            }
            .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
            .accessibilityIdentifier("document.properties.\(key)")
        }
    }

    private func button(_ title: String, _ key: String, help: String, _ action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .buttonStyle(.theme(.bordered, height: Theme.Height.small))
            .help(help)
            .accessibilityIdentifier("document.properties.\(key)")
    }

    private func colorWell(_ rgb: [Double], _ key: String, _ change: @escaping ([Double]) -> Void) -> some View {
        DocColorWell(rgb: rgb, identifier: "document.properties.\(key)", onChange: change)
            .frame(width: Theme.Height.large * 2, height: Theme.Height.regular)
    }

    /// Pixels the image-dependent kinds analyse: the composite below this adjustment layer, or in the sheet the
    /// pixel layer being adjusted.
    private func samples() -> [SIMD3<Float>] {
        inSheet ? document.layerSamples(id) : document.compositeSamples(neutralizing: id)
    }

    // MARK: Brightness / Contrast

    @ViewBuilder private func brightnessContrast(_ b: Double, _ c: Double, _ legacy: Bool) -> some View {
        slider("Brightness", b, -150...150, 0, "%+.0f", 1, "brightnessContrast.brightness") { v, f in
            set(.brightnessContrast(brightness: v, contrast: c, legacy: legacy), f)
        }
        slider("Contrast", c, -100...100, 0, "%+.0f", 1, "brightnessContrast.contrast") { v, f in
            set(.brightnessContrast(brightness: b, contrast: v, legacy: legacy), f)
        }
        checkbox("Use Legacy", legacy, "brightnessContrast.legacy") { set(.brightnessContrast(brightness: b, contrast: c, legacy: $0), true) }
            .help("The linear formula of older Photoshop versions; clips highlights and shadows")
    }

    // MARK: Color Balance

    @ViewBuilder private func colorBalance(_ m: ColorBalanceModel) -> some View {
        let range = state.colorBalanceRange[id] ?? 1
        SegmentedPicker(selection: Binding(get: { range }, set: { state.colorBalanceRange[id] = $0 }),
                        segments: [.init(value: 0, title: "Shadows"), .init(value: 1, title: "Midtones"),
                                   .init(value: 2, title: "Highlights")],
                        height: Theme.Height.small)
            .padding(.bottom, Theme.Space.xs)
            .accessibilityIdentifier("document.properties.colorBalance.tone")
        let values = m[range]
        ForEach(0..<3, id: \.self) { i in
            slider(["Cyan – Red", "Magenta – Green", "Yellow – Blue"][i], values[i], -100...100, 0, "%+.0f", 1,
                   "colorBalance.\(["red", "green", "blue"][i])") { v, f in
                var n = m
                n[range][i] = v
                set(.colorBalance(n), f)
            }
        }
        checkbox("Preserve Luminosity", m.preserveLuminosity, "colorBalance.preserveLuminosity") {
            var n = m
            n.preserveLuminosity = $0
            set(.colorBalance(n), true)
        }
    }

    // MARK: Black & White

    @ViewBuilder private func blackWhite(_ sliders: [Double], _ tint: [Double]?) -> some View {
        HStack(spacing: Theme.Space.xs) {
            button("Auto", "blackWhite.auto", help: "Set the mix from the image's colours (keeps its tonal balance)") {
                set(.blackWhite(sliders: AdjustmentAnalysis.blackWhiteAuto(samples()), tint: tint), true)
            }
            button("Default", "blackWhite.default", help: "Photoshop's default mix") {
                set(.blackWhite(sliders: BlackWhitePresets.default, tint: tint), true)
            }
            Spacer()
        }
        .padding(.bottom, Theme.Space.xs)
        let names = ["Reds", "Yellows", "Greens", "Cyans", "Blues", "Magentas"]
        ForEach(0..<6, id: \.self) { i in
            slider(names[i], sliders[i], -200...300, BlackWhitePresets.default[i], "%.0f %%", 1,
                   "blackWhite.\(names[i].lowercased())") { v, f in
                var s = sliders
                s[i] = v
                set(.blackWhite(sliders: s, tint: tint), f)
            }
        }
        HStack(spacing: Theme.Space.s) {
            checkbox("Tint", tint != nil, "blackWhite.tint") { set(.blackWhite(sliders: sliders, tint: $0 ? BlackWhitePresets.tint : nil), true) }
            if let tint {
                colorWell(tint, "blackWhite.tintColor") { set(.blackWhite(sliders: sliders, tint: $0), true) }
            }
            Spacer()
        }
        .padding(.top, Theme.Space.xs)
    }

    // MARK: Photo Filter

    @ViewBuilder private func photoFilter(_ color: [Double], _ density: Double, _ preserve: Bool) -> some View {
        let preset = PhotoFilterPreset.matching(color)
        popUp("Filter", preset?.title ?? "Custom", PhotoFilterPreset.allCases.map { ($0, $0.title) }, "photoFilter.preset") { p in
            set(.photoFilter(color: p.color, density: density, preserveLuminosity: preserve), true)
        }
        labeled("Color") {
            colorWell(color, "photoFilter.color") { set(.photoFilter(color: $0, density: density, preserveLuminosity: preserve), true) }
        }
        slider("Density", density, 0...100, 25, "%.0f %%", 1, "photoFilter.density") { v, f in
            set(.photoFilter(color: color, density: v, preserveLuminosity: preserve), f)
        }
        checkbox("Preserve Luminosity", preserve, "photoFilter.preserveLuminosity") {
            set(.photoFilter(color: color, density: density, preserveLuminosity: $0), true)
        }
    }

    // MARK: Gradient Map

    @ViewBuilder private func gradientMap(_ m: GradientMapModel) -> some View {
        let stops = m.stops.sorted { $0[0] < $1[0] }.map { FillModel.Stop(position: $0[0], color: [$0[1], $0[2], $0[3], 1]) }
        GradientStopsEditor(stops: stops, identifier: "document.properties.gradientMap", revision: revision,
                            showsReverse: false) { s, final in
            var n = m
            n.stops = s.map { [$0.position, $0.color[0], $0.color[1], $0.color[2]] }
            set(.gradientMap(n), final)
        }
        popUp("Method", m.method.title, GradientMethodModel.allCases.map { ($0, $0.title) }, "gradientMap.method") { method in
            var n = m
            n.method = method
            set(.gradientMap(n), true)
        }
        .padding(.top, Theme.Space.xs)
        HStack(spacing: Theme.Space.m) {
            checkbox("Dither", m.dither, "gradientMap.dither") { var n = m; n.dither = $0; set(.gradientMap(n), true) }
            checkbox("Reverse", m.reverse, "gradientMap.reverse") { var n = m; n.reverse = $0; set(.gradientMap(n), true) }
            Spacer()
        }
    }

    // MARK: Selective Color

    @ViewBuilder private func selectiveColor(_ colors: [[Double]], _ absolute: Bool) -> some View {
        let names = ["Reds", "Yellows", "Greens", "Cyans", "Blues", "Magentas", "Whites", "Neutrals", "Blacks"]
        let row = state.selectiveColorRow[id] ?? 0
        popUp("Colors", names[row], Array(names.enumerated()).map { ($0.offset, $0.element) }, "selectiveColor.colors") { r in
            state.selectiveColorRow[id] = r
        }
        ForEach(0..<4, id: \.self) { c in
            slider(["Cyan", "Magenta", "Yellow", "Black"][c], colors[row][c], -100...100, 0, "%+.0f %%", 1,
                   "selectiveColor.\(["cyan", "magenta", "yellow", "black"][c])") { v, f in
                var n = colors
                n[row][c] = v
                set(.selectiveColor(colors: n, absolute: absolute), f)
            }
        }
        labeled("Method") {
            SegmentedPicker(selection: Binding(get: { absolute }, set: { set(.selectiveColor(colors: colors, absolute: $0), true) }),
                            segments: [.init(value: false, title: "Relative"), .init(value: true, title: "Absolute")],
                            height: Theme.Height.small)
                .accessibilityIdentifier("document.properties.selectiveColor.method")
        }
    }

    // MARK: Equalize and Auto

    @ViewBuilder private func equalize() -> some View {
        Hint("Spreads each channel's levels evenly. The mapping is measured once, from the image below the layer.")
        HStack {
            button("Analyze Again", "equalize.analyze", help: "Measure the image as it is now") {
                set(document.analyzed(model, samples: samples()), true)
            }
            Spacer()
        }
    }

    @ViewBuilder private func auto(_ m: AutoAdjustmentModel) -> some View {
        let clip = state.autoClip[id] ?? AdjustmentEditorState.defaultAutoClip
        SegmentedPicker(selection: Binding(get: { m.mode }, set: { mode in
            var n = m
            n.mode = mode
            set(document.analyzed(.auto(n), samples: samples(), clip: clip), true)
        }), segments: AutoModeModel.allCases.map { .init(value: $0, title: $0.title.replacingOccurrences(of: "Auto ", with: "")) },
                        height: Theme.Height.small)
            .padding(.bottom, Theme.Space.xs)
            .accessibilityIdentifier("document.properties.auto.mode")
        slider("Clip", clip * 100, 0...9.99, 0.1, "%.2f %%", 0.01, "auto.clip") { v, f in
            state.autoClip[id] = v / 100
            if f { set(document.analyzed(.auto(m), samples: samples(), clip: v / 100), true) }
        }
        InfoRow(label: "Black", value: m.black.map { String(format: "%.0f", $0 * 255) }.joined(separator: " · "))
            .accessibilityIdentifier("document.properties.auto.black")
        InfoRow(label: "White", value: m.white.map { String(format: "%.0f", $0 * 255) }.joined(separator: " · "))
            .accessibilityIdentifier("document.properties.auto.white")
        InfoRow(label: "Gamma", value: m.gamma.map { String(format: "%.2f", $0) }.joined(separator: " · "))
            .accessibilityIdentifier("document.properties.auto.gamma")
        HStack {
            button("Analyze Again", "auto.analyze", help: "Measure the image as it is now") {
                set(document.analyzed(.auto(m), samples: samples(), clip: clip), true)
            }
            Spacer()
        }
        .padding(.top, Theme.Space.xs)
    }

    // MARK: Match Color

    @ViewBuilder private func matchColor(_ m: MatchColorModel) -> some View {
        let sources = document.matchColorSources.filter { $0.id != id || !inSheet }
        let current = sources.first { $0.id == m.sourceLayer }?.name ?? "None"
        let rematch = { (source: DocLayerID, neutralize: Bool) in
            let target = samples()
            if let n = AdjustmentAnalysis.matchColor(sourceLayer: source, source: document.layerSamples(source), target: target,
                                                      neutralize: neutralize, keeping: m) {
                set(.matchColor(n), true)
            } else {
                document.report?("Match Color: “\(document.node(source)?.name ?? "the source")” has no visible pixels")
            }
        }
        popUp("Source", current, sources.map { ($0.id, $0.name) }, "matchColor.source") { rematch($0, m.neutralized) }
        if sources.isEmpty { Hint("Match Color takes its colours from a pixel layer; this document has none.") }
        slider("Luminance", m.luminance, 1...200, 100, "%.0f", 1, "matchColor.luminance") { v, f in
            var n = m
            n.luminance = v
            set(.matchColor(n), f)
        }
        slider("Color Intensity", m.colorIntensity, 1...200, 100, "%.0f", 1, "matchColor.colorIntensity") { v, f in
            var n = m
            n.colorIntensity = v
            set(.matchColor(n), f)
        }
        slider("Fade", m.fade, 0...100, 0, "%.0f", 1, "matchColor.fade") { v, f in
            var n = m
            n.fade = v
            set(.matchColor(n), f)
        }
        checkbox("Neutralize", m.neutralized, "matchColor.neutralize") { on in
            if sources.contains(where: { $0.id == m.sourceLayer }) { rematch(m.sourceLayer, on) }
        }
        .disabled(!sources.contains { $0.id == m.sourceLayer })
        .help("Remove the source's colour cast from the match")
    }

    // MARK: Replace Color

    @ViewBuilder private func replaceColor(_ c: [Double], _ fz: Double, _ h: Double, _ s: Double, _ l: Double) -> some View {
        labeled("Color") {
            colorWell(c, "replaceColor.color") { set(.replaceColor(color: $0, fuzziness: fz, hue: h, saturation: s, lightness: l), true) }
            button("Use Foreground", "replaceColor.useForeground",
                   help: "Take the foreground colour (pick it from the canvas with the Eyedropper, I)") {
                let f = DocumentTools.shared.colors.foreground
                set(.replaceColor(color: [Double(f.r), Double(f.g), Double(f.b)], fuzziness: fz, hue: h, saturation: s, lightness: l), true)
            }
        }
        slider("Fuzziness", fz, 0...200, 40, "%.0f", 1, "replaceColor.fuzziness") { v, f in
            set(.replaceColor(color: c, fuzziness: v, hue: h, saturation: s, lightness: l), f)
        }
        SubHeader("Replacement")
        slider("Hue", h, -180...180, 0, "%+.0f°", 1, "replaceColor.hue") { v, f in
            set(.replaceColor(color: c, fuzziness: fz, hue: v, saturation: s, lightness: l), f)
        }
        slider("Saturation", s, -100...100, 0, "%+.0f", 1, "replaceColor.saturation") { v, f in
            set(.replaceColor(color: c, fuzziness: fz, hue: h, saturation: v, lightness: l), f)
        }
        slider("Lightness", l, -100...100, 0, "%+.0f", 1, "replaceColor.lightness") { v, f in
            set(.replaceColor(color: c, fuzziness: fz, hue: h, saturation: s, lightness: v), f)
        }
    }

    // MARK: Color Lookup

    @ViewBuilder private func colorLookup(_ size: Int, _ data: [Double]) -> some View {
        let key = "\(document.id):\(id)"
        let identity = size == 2 && data == ColorLookupFile.identity(size: 2)
        InfoRow(label: "3D LUT", value: identity ? "None (identity)" : (state.lookupFile[key].map { ($0 as NSString).lastPathComponent }
            ?? "Embedded \(size)³ table"))
            .help(state.lookupFile[key] ?? "The table's samples are stored in the document")
            .accessibilityIdentifier("document.properties.colorLookup.file")
        HStack(spacing: Theme.Space.xs) {
            button("Load 3D LUT…", "colorLookup.load", help: "Load a .cube or .3dl file; its samples are stored in the document") {
                loadLookup(key)
            }
            button("Reset", "colorLookup.reset", help: "Back to the identity") {
                state.lookupFile[key] = nil
                set(.colorLookup(size: 2, data: ColorLookupFile.identity(size: 2)), true)
            }
            .disabled(identity)
            Spacer()
        }
        Hint("Unit-domain 3D .cube and uniform-grid .3dl files, up to 256 knots per axis.")
    }

    private func loadLookup(_ key: String) {
        let panel = NSOpenPanel()
        panel.title = "Load 3D LUT"
        panel.allowedContentTypes = ["cube", "3dl"].compactMap { UTType(filenameExtension: $0) }
        panel.allowsMultipleSelection = false
        if let path = state.lookupFile[key] { panel.directoryURL = URL(fileURLWithPath: path).deletingLastPathComponent() }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let lut = try ColorLookupFile.load(url)
            state.lookupFile[key] = url.path
            set(.colorLookup(size: lut.size, data: lut.data), true)
            document.report?("Color Lookup: loaded \(url.lastPathComponent) (\(lut.size)³)")
        } catch {
            document.report?("Color Lookup: \(url.lastPathComponent): \(error.localizedDescription)")
        }
    }

    // MARK: Shadows / Highlights

    @ViewBuilder private func shadowsHighlights(_ m: ShadowsHighlightsModel) -> some View {
        let edit = { (change: (inout ShadowsHighlightsModel) -> Void, final: Bool) in
            var n = m
            change(&n)
            set(.shadowsHighlights(n), final)
        }
        SubHeader("Shadows")
        slider("Amount", m.shadowsAmount * 100, 0...100, 0, "%.0f %%", 1, "shadowsHighlights.shadowsAmount") { v, f in edit({ $0.shadowsAmount = v / 100 }, f) }
        slider("Tone", m.shadowsTone * 100, 0...100, 50, "%.0f %%", 1, "shadowsHighlights.shadowsTone") { v, f in edit({ $0.shadowsTone = v / 100 }, f) }
        slider("Radius", m.shadowsRadius, 0...250, 30, "%.0f px", 1, "shadowsHighlights.shadowsRadius") { v, f in edit({ $0.shadowsRadius = v }, f) }
        SubHeader("Highlights")
        slider("Amount", m.highlightsAmount * 100, 0...100, 0, "%.0f %%", 1, "shadowsHighlights.highlightsAmount") { v, f in edit({ $0.highlightsAmount = v / 100 }, f) }
        slider("Tone", m.highlightsTone * 100, 0...100, 50, "%.0f %%", 1, "shadowsHighlights.highlightsTone") { v, f in edit({ $0.highlightsTone = v / 100 }, f) }
        slider("Radius", m.highlightsRadius, 0...250, 30, "%.0f px", 1, "shadowsHighlights.highlightsRadius") { v, f in edit({ $0.highlightsRadius = v }, f) }
        SubHeader("Adjustments")
        slider("Color", m.color * 100, -100...100, 0, "%+.0f", 1, "shadowsHighlights.color") { v, f in edit({ $0.color = v / 100 }, f) }
        slider("Midtone", m.midtone * 100, -100...100, 0, "%+.0f", 1, "shadowsHighlights.midtone") { v, f in edit({ $0.midtone = v / 100 }, f) }
        // The engine needs black + white clip below 1; each slider stops short of the other's value.
        slider("Black Clip", m.blackClip * 100, 0...49.99, 0, "%.2f %%", 0.01, "shadowsHighlights.blackClip") { v, f in
            edit({ $0.blackClip = min(v / 100, 0.9999 - $0.whiteClip) }, f)
        }
        slider("White Clip", m.whiteClip * 100, 0...49.99, 0, "%.2f %%", 0.01, "shadowsHighlights.whiteClip") { v, f in
            edit({ $0.whiteClip = min(v / 100, 0.9999 - $0.blackClip) }, f)
        }
    }

    // MARK: HDR Toning

    @ViewBuilder private func hdrToning(_ m: HDRToningModel) -> some View {
        let edit = { (change: (inout HDRToningModel) -> Void, final: Bool) in
            var n = m
            change(&n)
            set(.hdrToning(n), final)
        }
        popUp("Method", m.method.title, HDRMethodModel.allCases.map { ($0, $0.title) }, "hdrToning.method") { method in
            var n = m
            n.method = method
            if method == .equalizeHistogram, n.equalizeMap.isEmpty {
                set(document.analyzed(.hdrToning(n), samples: samples()), true)
            } else {
                set(.hdrToning(n), true)
            }
        }
        switch m.method {
        case .localAdaptation:
            SubHeader("Edge Glow")
            slider("Radius", m.radius, 0...250, 30, "%.0f px", 1, "hdrToning.radius") { v, f in edit({ $0.radius = v }, f) }
            slider("Strength", m.strength * 100, 0...100, 0, "%.0f %%", 1, "hdrToning.strength") { v, f in edit({ $0.strength = v / 100 }, f) }
            SubHeader("Tone and Detail")
            slider("Gamma", m.gamma, 0.1...10, 1, "%.2f", 0.01, "hdrToning.gamma") { v, f in edit({ $0.gamma = v }, f) }
            slider("Exposure", m.exposure, -5...5, 0, "%+.2f", 0.01, "hdrToning.exposure") { v, f in edit({ $0.exposure = v }, f) }
            slider("Detail", m.detail * 100, -100...100, 0, "%+.0f %%", 1, "hdrToning.detail") { v, f in edit({ $0.detail = v / 100 }, f) }
            slider("Shadow", m.shadows * 100, -100...100, 0, "%+.0f %%", 1, "hdrToning.shadows") { v, f in edit({ $0.shadows = v / 100 }, f) }
            slider("Highlight", m.highlights * 100, -100...100, 0, "%+.0f %%", 1, "hdrToning.highlights") { v, f in edit({ $0.highlights = v / 100 }, f) }
            SubHeader("Color")
            slider("Vibrance", m.vibrance * 100, -100...100, 0, "%+.0f %%", 1, "hdrToning.vibrance") { v, f in edit({ $0.vibrance = v / 100 }, f) }
            slider("Saturation", m.saturation * 100, -100...100, 0, "%+.0f %%", 1, "hdrToning.saturation") { v, f in edit({ $0.saturation = v / 100 }, f) }
            SubHeader("Toning Curve")
            DocCurveEditor(points: m.curve, channel: .rgb, revision: revision, identifier: "document.properties.hdrToning.curve") { pts, f in
                edit({ $0.curve = Self.validCurve(pts) }, f)
            }
            .aspectRatio(1, contentMode: .fit)
        case .exposureGamma:
            slider("Exposure", m.exposure, -5...5, 0, "%+.2f", 0.01, "hdrToning.exposure") { v, f in edit({ $0.exposure = v }, f) }
            slider("Gamma", m.gamma, 0.1...10, 1, "%.2f", 0.01, "hdrToning.gamma") { v, f in edit({ $0.gamma = v }, f) }
        case .equalizeHistogram:
            Hint("Maps luminance through the image's measured histogram (measured once, stored in the layer).")
            HStack {
                button("Analyze Again", "hdrToning.analyze", help: "Measure the image as it is now") {
                    set(document.analyzed(.hdrToning(m), samples: samples()), true)
                }
                Spacer()
            }
        case .highlightCompression:
            Hint("Highlight Compression has no settings: it compresses highlights so nothing clips.")
        }
    }

    /// The engine requires strictly increasing inputs and values in [0, 1].
    static func validCurve(_ pts: [[Double]]) -> [[Double]] {
        var out: [[Double]] = []
        for p in pts.sorted(by: { $0[0] < $1[0] }) where p.count == 2 {
            let q = [min(max(p[0], 0), 1), min(max(p[1], 0), 1)]
            if let last = out.last, q[0] <= last[0] { continue }
            out.append(q)
        }
        return out
    }
}

/// Gradient stops: a preview, one row per stop (colour well, position, remove), Add Stop and optionally
/// Reverse. Shared by gradient fill layers and Gradient Map (`identifier` is the prefix: `…fill`, `…gradientMap`).
struct GradientStopsEditor: View {
    let stops: [FillModel.Stop]
    let identifier: String
    let revision: Int
    var showsReverse = true
    let onChange: ([FillModel.Stop], Bool) -> Void

    var body: some View {
        let sorted = stops.sorted { $0.position < $1.position }
        VStack(alignment: .leading, spacing: 0) {
            LinearGradient(stops: sorted.map { .init(color: documentColor($0.color), location: $0.position) },
                           startPoint: .leading, endPoint: .trailing)
                .frame(height: Theme.Height.small)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
                .padding(.vertical, Theme.Space.xs)
                .accessibilityIdentifier("\(identifier).gradientPreview")
            ForEach(Array(sorted.enumerated()), id: \.offset) { i, stop in
                HStack(spacing: Theme.Space.s) {
                    DocColorWell(rgb: stop.color, identifier: "\(identifier).stop.\(i).color") { rgb in
                        var s = sorted
                        s[i].color = rgb + [s[i].color.count > 3 ? s[i].color[3] : 1]
                        onChange(s, true)
                    }
                    .frame(width: Theme.Height.large * 2, height: Theme.Height.small)
                    DocSlider(title: "Stop \(i + 1)", value: stop.position * 100, range: 0...100, defaultValue: i == 0 ? 0 : 100,
                              format: "%.0f %%", identifier: "\(identifier).stop.\(i).position", revision: revision) { v, final in
                        var s = sorted
                        s[i].position = v / 100
                        onChange(s, final)
                    }
                    .frame(minWidth: 0, maxWidth: .infinity)
                    .frame(height: Theme.Height.slider)
                    IconButton(symbol: "minus", help: "Remove this stop", size: Theme.Height.small) {
                        var s = sorted
                        s.remove(at: i)
                        onChange(s, true)
                    }
                    .disabled(sorted.count <= 2)
                    .accessibilityIdentifier("\(identifier).stop.\(i).remove")
                }
            }
            HStack(spacing: Theme.Space.xs) {
                Button("Add Stop") {
                    var s = sorted
                    let mid = s.count >= 2 ? (s[0].position + s[1].position) / 2 : 0.5
                    s.append(.init(position: mid, color: s.first?.color ?? [0.5, 0.5, 0.5, 1]))
                    onChange(s, true)
                }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .accessibilityIdentifier("\(identifier).addStop")
                if showsReverse {
                    Button("Reverse") {
                        onChange(sorted.map { .init(position: 1 - $0.position, color: $0.color) }, true)
                    }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("\(identifier).reverse")
                }
                Spacer()
            }
        }
    }
}
