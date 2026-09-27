import AppKit
import SwiftUI
import TesseraCore

// MARK: - Detail ▸ AI Denoise (M2-48)

/// AI Denoise at the top of Detail ▸ Noise Reduction: the raw-domain neural mode and its amount.
/// While the engine cannot render or export it (`DevelopEngineGaps.aiDenoise`) the toggle is
/// disabled, except to switch off an AI Denoise a recipe already carries.
struct AIDenoiseSection: View {
    let model: AppModel
    let tools: DevelopTools
    @Environment(\.developRevision) private var revision

    var body: some View {
        let settings: [String: Any] = { _ = revision; _ = model.developHistory?.entries; return tools.develop?.settingsObject ?? [:] }()
        let on = AIDenoise.isEnabled(in: settings)
        let gap = DevelopEngineGaps.aiDenoise
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Toggle("AI Denoise", isOn: Binding(get: { on }, set: { v in
                tools.apply(AIDenoise.patch(enabled: v), final: true, label: AIDenoise.historyLabel(v))
                tools.bump()
            }))
            .font(Theme.Fonts.caption)
            .controlSize(.small)
            .disabled(gap != nil && !on)
            .help("Raw-domain neural noise reduction before demosaicing (first use loads the model)")
            .accessibilityIdentifier("detail-ai-denoise")
            ControlSlider(control: AIDenoise.amount)
                .frame(height: Theme.Height.slider)
                .disabled(!on || gap != nil)
                .accessibilityIdentifier("detail-ai-denoise-amount")
            if let gap {
                StatusLine(text: gap, kind: .warning).accessibilityIdentifier("detail-ai-denoise-unavailable")
            } else {
                Hint("First use downloads or loads the denoise model; the loupe refines when it is ready.")
            }
            if on, tools.develop?.ignores("/denoise") == true {
                StatusLine(text: "This photo's AI Denoise is kept in the recipe but not drawn by the loupe.", kind: .warning)
                    .accessibilityIdentifier("detail-ai-denoise-ignored")
            }
        }
        .padding(.bottom, Theme.Space.xs)
    }
}

// MARK: - Lens Blur (M2-48)

/// Lens Blur (docs/01 §2.16): Apply, amount, bokeh, the focal range over the depth strip, Visualize
/// Depth, subject-aware focus and the Refine brushes. The recipe fields are wired, but the engine
/// has no depth map here yet (`DevelopEngineGaps`), so the controls stay disabled with the reason;
/// Apply still switches off a lens blur a recipe already carries.
struct LensBlurPanel: View {
    let model: AppModel
    let tools: DevelopTools
    @Environment(\.developRevision) private var revision

    var body: some View {
        let ready = model.developStatus == .ready
        let settings: [String: Any] = { _ = revision; _ = model.developHistory?.entries; return tools.develop?.settingsObject ?? [:] }()
        let applied = LensBlurControls.isApplied(in: settings)
        let gap = DevelopEngineGaps.lensBlur
        let editable = applied && gap == nil
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if let gap {
                StatusLine(text: gap, kind: .warning).accessibilityIdentifier("lensblur-unavailable")
            }
            Toggle("Apply", isOn: Binding(get: { applied }, set: { v in
                tools.apply(LensBlurControls.applyPatch(v), final: true, label: v ? "Lens Blur On" : "Lens Blur Off")
                tools.bump()
            }))
            .font(Theme.Fonts.caption)
            .controlSize(.small)
            .disabled(gap != nil && !applied)
            .accessibilityIdentifier("lensblur-apply")
            ControlSlider(control: LensBlurControls.amount)
                .frame(height: Theme.Height.slider)
                .disabled(!editable)
                .accessibilityIdentifier("lensblur-amount")
            HStack(spacing: Theme.Space.s) {
                Text("Bokeh").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                Spacer(minLength: 0)
                SegmentedPicker(selection: Binding(get: { LensBlurControls.bokeh(in: settings) }, set: { s in
                    tools.apply(LensBlurControls.bokehPatch(s), final: true, label: "Bokeh: \(s.title)")
                    tools.bump()
                }), segments: BokehShape.allCases.map { .init(value: $0, title: $0.title) }, height: Theme.Height.small, fill: false)
                .fixedSize()
            }
            .frame(height: Theme.Height.regular)
            .disabled(!editable)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("lensblur-bokeh")
            SubHeader("Focal Range")
            FocalRangeStrip(range: FocalRange(settings: settings), histogram: nil) { r, final in
                tools.apply(r.patch, final: final, label: r.historyLabel)
            }
            .frame(height: Theme.Height.large + Theme.Space.m)
            .disabled(!editable)
            .accessibilityIdentifier("lensblur-focal-range")
            Hint(DevelopEngineGaps.lensBlurDepth.map { "\($0): the strip shows near → far without a depth histogram." }
                 ?? "Drag the handles or the band to choose the in-focus depths.")
            Toggle("Visualize Depth", isOn: .constant(false))
                .font(Theme.Fonts.caption).controlSize(.small)
                .disabled(!editable || DevelopEngineGaps.lensBlurDepth != nil)
                .help(DevelopEngineGaps.lensBlurDepth ?? "Show the depth map in the loupe (near is light)")
                .accessibilityIdentifier("lensblur-visualize-depth")
            Toggle("Subject-aware focus", isOn: .constant(false))
                .font(Theme.Fonts.caption).controlSize(.small)
                .disabled(!editable || DevelopEngineGaps.lensBlurSubject != nil)
                .help(DevelopEngineGaps.lensBlurSubject ?? "Keep the detected subject in focus")
                .accessibilityIdentifier("lensblur-subject")
            HStack(spacing: Theme.Space.xs) {
                Text("Refine").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                Spacer(minLength: 0)
                IconButton(symbol: "paintbrush", help: "Focus brush: paint areas to keep sharp", size: Theme.Height.small) {}
                    .accessibilityIdentifier("lensblur-refine-focus")
                IconButton(symbol: "paintbrush.pointed", help: "Blur brush: paint areas to blur", size: Theme.Height.small) {}
                    .accessibilityIdentifier("lensblur-refine-blur")
                Chip(text: "Later")
            }
            .frame(height: Theme.Height.regular)
            .disabled(true)
            .help(DevelopEngineGaps.lensBlurRefine ?? "")
        }
        .disabled(!ready)
    }
}

/// The focal range over a near → far depth strip (a scope: `plotWell`, radius 4). The band between
/// the two handles is the in-focus range; drag a handle to move one end or the band to move both.
/// Drags preview through the coalesced path and commit once on release.
struct FocalRangeStrip: View {
    let range: FocalRange
    /// Optional depth histogram (bins near → far), when the engine provides a depth map.
    let histogram: [Double]?
    let onChange: (FocalRange, Bool) -> Void
    @State private var draft: FocalRange?
    @State private var grab: Grab?
    @Environment(\.isEnabled) private var enabled

    private enum Grab { case near, far, band(start: FocalRange) }

    var body: some View {
        let r = draft ?? range
        GeometryReader { geo in
            let w = max(geo.size.width, 1), h = geo.size.height
            ZStack(alignment: .topLeading) {
                RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.plotWell))
                // Near (light) → far (dark) ramp.
                LinearGradient(colors: [Color(nsColor: Theme.Palette.plotGuide), Color(nsColor: Theme.Palette.plotGrid)],
                               startPoint: .leading, endPoint: .trailing)
                    .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                if let bins = histogram, let peak = bins.max(), peak > 0 {
                    Path { p in
                        for (i, v) in bins.enumerated() {
                            let bw = w / Double(bins.count)
                            p.addRect(CGRect(x: Double(i) * bw, y: h * (1 - v / peak), width: bw, height: h * v / peak))
                        }
                    }
                    .fill(Color(nsColor: Theme.Palette.plotLine).opacity(0.35))
                }
                Rectangle()
                    .fill(Theme.accentSubtle)
                    .frame(width: max((r.far - r.near) * w, Theme.Space.hairline), height: h)
                    .offset(x: r.near * w)
                ForEach([r.near, r.far], id: \.self) { x in
                    Rectangle()
                        .fill(Color(nsColor: Theme.Palette.plotLine))
                        .frame(width: Theme.Space.xxs, height: h)
                        .offset(x: min(max(x * w - Theme.Space.xxs / 2, 0), w - Theme.Space.xxs))
                }
                HStack {
                    Text("Near"); Spacer(); Text("Far")
                }
                .font(Theme.Fonts.caption)
                .foregroundStyle(Color(nsColor: Theme.Palette.plotText))
                .padding(.horizontal, Theme.Space.xs)
                .frame(height: h, alignment: .bottom)
                .padding(.bottom, Theme.Space.xxs)
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { g in
                    let x0 = g.startLocation.x / w, x = g.location.x / w
                    if grab == nil {
                        let tolerance = Theme.Space.s / w
                        grab = abs(x0 - range.near) < tolerance ? .near : abs(x0 - range.far) < tolerance ? .far
                            : (range.near...range.far).contains(x0) ? .band(start: range) : (x0 < range.near ? .near : .far)
                    }
                    var next = draft ?? range
                    switch grab {
                    case .near: next.setNear(x)
                    case .far: next.setFar(x)
                    case .band(let start): next = start; next.shift(by: x - x0)
                    case nil: break
                    }
                    draft = next
                    onChange(next, false)
                }
                .onEnded { _ in
                    if let d = draft { onChange(d, true) }
                    draft = nil
                    grab = nil
                })
        }
        .opacity(enabled ? 1 : Theme.Opacity.disabled)
        .accessibilityElement()
        .accessibilityLabel("Focal range")
        .accessibilityValue(String(format: "%.0f to %.0f percent depth", r.near * 100, r.far * 100))
    }
}
