import AppKit
import SwiftUI
import TesseraCore

// MARK: - Detail ▸ AI Denoise (M2-48, M2-51)

/// AI Denoise at the top of Detail ▸ Noise Reduction: the raw-domain neural mode and its amount.
/// The first time it is switched on the pinned CFA model is acquired through `ModelAcquisition`
/// (inline progress: queued, bytes, ready or the failure's reason); the recipe changes only once
/// the model is ready, and the loupe refines then. Off always works.
struct AIDenoiseSection: View {
    let model: AppModel
    let tools: DevelopTools
    var models: ModelAcquisition = .shared
    @Environment(\.developRevision) private var revision

    var body: some View {
        let settings: [String: Any] = { _ = revision; _ = model.developHistory?.entries; return tools.develop?.settingsObject ?? [:] }()
        let on = AIDenoise.isEnabled(in: settings)
        let state = models.state(.cfaDenoise)
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Toggle("AI Denoise", isOn: Binding(get: { on || state.isBusy }, set: { v in
                if v { enable() } else { setEnabled(false) }
            }))
            .font(Theme.Fonts.caption)
            .controlSize(.small)
            .disabled(state.isBusy && !on)
            .help("Raw-domain neural noise reduction before demosaicing (first use downloads the model)")
            .accessibilityIdentifier("detail-ai-denoise")
            ControlSlider(control: AIDenoise.amount)
                .frame(height: Theme.Height.slider)
                .disabled(!on)
                .accessibilityIdentifier("detail-ai-denoise-amount")
            if state == .idle || (state.isReady && !on) {
                Hint(models.allowDownloads
                     ? "First use downloads the denoise model; the loupe refines when it is ready."
                     : "Model downloads are off (Settings ▸ AI): only a model already on this Mac is used.")
            } else {
                ModelProgressRow(title: ModelRequirement.cfaDenoise.title, state: state, id: "detail-ai-denoise-model") { enable() }
            }
            if on, tools.develop?.ignores("/denoise") == true {
                StatusLine(text: "This photo's AI Denoise is kept in the recipe but not drawn by the loupe.", kind: .warning)
                    .accessibilityIdentifier("detail-ai-denoise-ignored")
            }
        }
        .padding(.bottom, Theme.Space.xs)
        // A recipe that already carries AI Denoise: fetch the model so the loupe can draw it.
        .task(id: on) {
            guard on, models.state(.cfaDenoise) == .idle else { return }
            if await models.ensure([.cfaDenoise]).isReady { try? tools.develop?.session.refresh() }
        }
    }

    private func setEnabled(_ v: Bool) {
        tools.apply(AIDenoise.patch(enabled: v), final: true, label: AIDenoise.historyLabel(v))
        tools.bump()
    }

    /// Acquire first; switch the recipe on only once the weights are cached.
    private func enable() {
        let develop = tools.develop
        models.reset([.cfaDenoise])
        Task { @MainActor in
            guard await models.ensure([.cfaDenoise]).isReady, tools.develop === develop else { return }
            setEnabled(true)
        }
    }
}

// MARK: - Lens Blur (M2-48, M2-51)

extension LensBlurDepthModel {
    /// The inspector's instance; it follows the open develop session.
    @MainActor static let shared = LensBlurDepthModel(models: .shared)
}

/// Lens Blur (docs/01 §2.16): Apply, amount, the aperture, the focal range over the depth
/// histogram, Visualize Depth, Subject and the Refine brushes (still disabled: no engine brush).
/// Apply, Visualize Depth and Subject acquire the depth (and segmentation) weights first, with
/// inline progress; a failure is an inline error and leaves the recipe unchanged.
struct LensBlurPanel: View {
    let model: AppModel
    let tools: DevelopTools
    var depth: LensBlurDepthModel = .shared
    @Environment(\.developRevision) private var revision

    var body: some View {
        let ready = model.developStatus == .ready
        let settings: [String: Any] = { _ = revision; _ = model.developHistory?.entries; return tools.develop?.settingsObject ?? [:] }()
        let applied = LensBlurControls.isApplied(in: settings)
        let weights = depth.busy == .subject ? depth.subjectWeights : depth.weights
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Toggle("Apply", isOn: Binding(get: { applied }, set: { v in v ? apply() : setApplied(false) }))
                .font(Theme.Fonts.caption)
                .controlSize(.small)
                .disabled(depth.busy != nil && !applied)
                .accessibilityIdentifier("lensblur-apply")
            if weights.isBusy {
                ModelProgressRow(title: depth.busy == .subject ? "Subject models" : ModelRequirement.depth.title,
                                 state: weights, id: "lensblur-model") {}
            } else if depth.busy != nil {
                HStack(spacing: Theme.Space.xs) {
                    ProgressView().controlSize(.mini)
                    Text(depth.busy == .subject ? "Finding the subject…" : "Estimating depth…")
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                }
                .accessibilityIdentifier("lensblur-busy")
            }
            if let error = depth.error {
                // A missing model is a warning (DESIGN.md), anything else an error.
                StatusLine(text: error, kind: weights.failure != nil || depth.subjectWeights.failure != nil ? .warning : .error)
                    .accessibilityIdentifier("lensblur-error")
            }
            ControlSlider(control: LensBlurControls.amount)
                .frame(height: Theme.Height.slider)
                .disabled(!applied)
                .accessibilityIdentifier("lensblur-amount")
            HStack(spacing: Theme.Space.s) {
                Text("Bokeh").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                Spacer(minLength: 0)
                MenuPicker(selection: Binding(get: { LensBlurControls.bokeh(in: settings) }, set: { s in
                    tools.apply(LensBlurControls.bokehPatch(s), final: true, label: "Bokeh: \(s.title)")
                    tools.bump()
                }), options: BokehShape.allCases.map { (value: $0, title: $0.title) })
            }
            .frame(height: Theme.Height.regular)
            .disabled(!applied)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("lensblur-bokeh")
            SubHeader("Focal Range")
            FocalRangeStrip(range: FocalRange(settings: settings), histogram: depth.histogram) { r, final in
                tools.apply(r.patch, final: final, label: r.historyLabel)
            }
            .frame(height: Theme.Height.large + Theme.Space.m)
            .disabled(!applied)
            .accessibilityIdentifier("lensblur-focal-range")
            Hint(depth.histogram == nil && applied
                 ? "The depth histogram appears under the range once depth is estimated."
                 : "Drag the handles or the band to choose the in-focus depths.")
            HStack(spacing: Theme.Space.s) {
                Toggle("Visualize Depth", isOn: Binding(get: { depth.visualize }, set: { v in Task { await depth.setVisualize(v) } }))
                    .font(Theme.Fonts.caption).controlSize(.small)
                    .disabled(!applied || (depth.busy != nil && depth.busy != .visualize))
                    .help("Show the depth map in the loupe instead of the photo (near is light)")
                    .accessibilityIdentifier("lensblur-visualize-depth")
                Spacer(minLength: 0)
                Button("Subject") { focusOnSubject() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .disabled(!applied || depth.busy != nil)
                    .help("Set the focal range around the main subject's depth (one undo step)")
                    .accessibilityIdentifier("lensblur-subject")
            }
            .frame(height: Theme.Height.regular)
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
            if let gap = DevelopEngineGaps.lensBlurRefine {
                StatusLine(text: gap, kind: .warning).accessibilityIdentifier("lensblur-refine-unavailable")
            }
        }
        .disabled(!ready)
        // Follow the open session; a photo that already has Lens Blur gets its histogram.
        .task(id: tools.develop.map(ObjectIdentifier.init)) {
            depth.bind(tools.develop?.session)
            if applied { await depth.refreshHistogram() }
        }
    }

    private func setApplied(_ on: Bool) {
        tools.apply(LensBlurControls.applyPatch(on), final: true, label: on ? "Lens Blur On" : "Lens Blur Off")
        tools.bump()
        if !on, depth.visualize { Task { await depth.setVisualize(false) } }
    }

    /// Acquire the depth weights first; the recipe changes only once they are cached.
    private func apply() {
        let develop = tools.develop
        Task { @MainActor in
            guard await depth.ensureWeights(), tools.develop === develop else { return }
            setApplied(true)
            await depth.refreshHistogram()
        }
    }

    private func focusOnSubject() {
        Task { @MainActor in
            await depth.focusOnSubject { r in
                tools.apply(r.patch, final: true, label: LensBlurControls.subjectHistoryLabel(r))
                tools.bump()
            }
        }
    }
}

/// Inline model acquisition progress (DESIGN.md: progress sits with the control it gates):
/// queued, a determinate bar with bytes while downloading, Ready, or the failure with Retry.
struct ModelProgressRow: View {
    let title: String
    let state: ModelAcquisitionState
    let id: String
    let retry: () -> Void

    var body: some View {
        Group {
            switch state {
            case .failed(let reason):
                HStack(alignment: .firstTextBaseline, spacing: Theme.Space.xs) {
                    StatusLine(text: "\(title): \(reason)", kind: .warning)
                    Spacer(minLength: 0)
                    Button("Retry", action: retry)
                        .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                        .accessibilityIdentifier(id + "-retry")
                }
            case .ready:
                StatusLine(text: "\(title) ready", kind: .success)
            case .idle:
                Hint("\(title): \(state.label)")
            case .queued, .downloading:
                VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                    if let f = state.fraction {
                        ProgressView(value: f).progressViewStyle(.linear).controlSize(.small)
                    } else {
                        ProgressView().progressViewStyle(.linear).controlSize(.small)
                    }
                    Text("\(title): \(state.label)")
                        .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                        .lineLimit(1).truncationMode(.middle)
                }
                .tint(Theme.accent)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(id)
        .accessibilityValue(state.label)
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
