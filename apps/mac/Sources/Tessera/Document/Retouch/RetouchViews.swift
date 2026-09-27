import SwiftUI
import TesseraCore

/// The Remove tool's palette slot, after the Healing Brush (J; ⇧J switches between them).
struct RemoveToolSlot: View {
    @Bindable var retouch: DocumentRetouch

    var body: some View {
        IconButton(symbol: "eraser.line.dashed", help: "Remove (⇧J)", on: retouch.removeActive, size: Theme.Height.large) {
            retouch.removeActive ? retouch.deactivate() : retouch.activate()
        }
        .accessibilityIdentifier("document.tool.remove")
    }
}

/// The options bar while the Remove tool is on: size, backend, dilation, Remove Selection and Remove
/// Distractions; during review, the suggestion count with All / None, Cancel and Remove Selected; while an
/// apply runs, its progress with Cancel. Errors are an inline `StatusLine`.
struct RemoveOptionsBar: View {
    @Bindable var document: DocumentController
    @Bindable var retouch: DocumentRetouch

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Height.small) }

    var body: some View {
        if let busy = retouch.busy {
            ProgressView().controlSize(.small)
            TimelineView(.periodic(from: .now, by: 0.5)) { _ in
                Text(String(format: "%@ %.0f s", busy, retouch.busySeconds))
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary).fixedSize()
            }
            Button("Cancel") { retouch.cancel() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .help("Esc")
                .accessibilityIdentifier("document.remove.cancel")
        } else if let review = retouch.review {
            Text(review.summary).font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary).fixedSize()
                .accessibilityIdentifier("document.remove.review.summary")
            Button("All") { retouch.setAllSuggestions(true) }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            Button("None") { retouch.setAllSuggestions(false) }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
            Text("Click a box to keep it · geometric suggestions, not person segmentation")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
                .help(review.scan.limitation + " Faces: " + review.scan.faces + ".")
            separator
            Button("Cancel") { retouch.endReview() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .help("Esc")
                .accessibilityIdentifier("document.remove.review.cancel")
            Button("Remove Selected") { retouch.applyReview() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(!review.canApply)
                .help("Return")
                .accessibilityIdentifier("document.remove.review.apply")
        } else {
            OptionField(title: "Size", value: Binding(get: { Double(retouch.options.size) },
                                                      set: { retouch.options.setSize(Float($0)) }),
                        range: 1...2000, unit: "px", identifier: "document.remove.size")
            SegmentedPicker(selection: $retouch.options.engine, segments: RemoveEngine.allCases.map {
                .init(value: $0, title: $0.title, help: $0.help)
            }, height: Theme.Height.small, fill: false)
            .fixedSize()
            .accessibilityIdentifier("document.remove.backend")
            if retouch.options.engine != .patchMatch {
                RemoveModelStatus(retouch: retouch)
            }
            OptionField(title: "Expand", value: Binding(get: { Double(retouch.options.dilation) },
                                                        set: { retouch.options.dilation = Int($0) }),
                        range: 0...64, unit: "px", identifier: "document.remove.dilation")
            separator
            Button("Remove Selection") { retouch.removeSelection() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .disabled(document.marquee == nil)
                .accessibilityIdentifier("document.remove.selection")
            Button("Remove Distractions…") { retouch.scanDistractions() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .disabled(document.primary?.kind != .pixel || document.marquee != nil)
                .help("Finds thin wire-like lines and face boxes, shows them for review, then removes only the ones you keep selected")
                .accessibilityIdentifier("document.remove.distractions")
        }
        if retouch.busy == nil, let a = retouch.jobs.abandoned {
            // B5-09b: the bar is idle; the cancelled job is still stopping in the engine.
            separator
            TimelineView(.periodic(from: .now, by: 0.5)) { _ in
                Text(String(format: "Stopping the cancelled %@… %.0f s", a.operation, retouch.busySeconds))
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary).fixedSize()
            }
            .help("New removals wait until the engine has stopped it")
            .accessibilityIdentifier("document.remove.stopping")
        }
        if let n = retouch.notice {
            separator
            StatusLine(text: n, kind: .warning)
                .lineLimit(1).truncationMode(.tail)
                .frame(maxWidth: 420, alignment: .leading)
                .help(n)
                .onTapGesture { retouch.clearNotice() }
                .accessibilityIdentifier("document.remove.notice")
        }
        if let e = retouch.error {
            separator
            StatusLine(text: e.title, kind: e.isMissingWeights ? .warning : .error)
                .fixedSize()
                .help(e.detail)
                .onTapGesture { retouch.clearError() }
                .accessibilityIdentifier("document.remove.error")
        }
    }
}

/// The LaMa model in the Remove options bar (B5-09b): not installed with Download, downloads off with a link
/// to Settings ▸ AI, inline download progress (and what runs when it completes), or the failure with Retry.
struct RemoveModelStatus: View {
    @Bindable var retouch: DocumentRetouch
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        let m = retouch.model("remove/lama")
        let phase = retouch.downloads.phase(m)
        switch phase {
        case .installed:
            EmptyView()
        case .available(let m):
            Text("LaMa not installed").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
                .help(help(m))
            Button("Download LaMa") { retouch.downloadModel(m.modelId) }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .help("Downloads the pinned, checksum-verified \(m.modelId) model into \(m.cachePath). Choosing LaMa and removing also downloads it first.")
                .accessibilityIdentifier("document.remove.download")
        case .downloadsOff(let m):
            StatusLine(text: "LaMa not installed · model downloads are off", kind: .warning).fixedSize()
                .help(help(m) + " Model downloads are off in Settings ▸ AI, so nothing is downloaded.")
                .accessibilityIdentifier("document.remove.downloads-off")
            Button("Settings ▸ AI…") { openSettings() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .accessibilityIdentifier("document.remove.settings")
        case .downloading(let m, _), .failed(let m, _):
            ModelProgressRow(title: "LaMa model", state: phase.progress,
                             id: "document.remove.model") { retouch.downloadModel(m.modelId) }
                .frame(width: 300)
            if let op = retouch.downloads.waiting[m.modelId] {
                Text("\(op) runs when it is ready").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary).fixedSize()
                    .accessibilityIdentifier("document.remove.waiting")
            }
        }
    }

    private func help(_ m: RetouchModelInfo) -> String {
        "Auto uses PatchMatch until the LaMa model (\(m.modelId)) is installed. It comes from \(m.sourceURL) and belongs, "
            + "hash-verified, at \(m.cachePath)."
    }
}

/// Edit ▸ Content-Aware Fill (document mode).
struct RetouchEditMenuItems: View {
    let doc: DocumentController?
    private var retouch: DocumentRetouch { DocumentRetouch.shared }

    var body: some View {
        Button("Content-Aware Fill") { retouch.contentAwareFill() }
            .disabled(!RetouchMenuState.contentAwareFillEnabled(layerKind: doc?.primary?.kind, hasSelection: doc?.marquee != nil,
                                                                 jobRunning: retouch.jobs.isBusy))
    }
}

/// Filter ▸ Neural Filters….
struct NeuralFiltersMenuItem: View {
    let doc: DocumentController?

    var body: some View {
        // The menu bar is built at launch: the place `--retouch-selftest` starts without a TesseraApp hook.
        let _ = RetouchSelfTest.startIfRequested()
        let target = doc?.primary.map { $0.kind == .pixel || $0.kind == .smartObject } ?? false
        Button("Neural Filters…") { DocumentRetouch.shared.openNeuralFilters() }
            .disabled(!target)
    }
}

/// The Neural Filters sheet: the filters on the left (a "Model not installed" chip where weights are
/// missing), the chosen filter's controls, output and notes on the right; footer Reset, Cancel, Apply.
/// While applying, Cancel stops the job. Missing weights are a warning `StatusLine` naming the model with
/// its source and cache path; Apply stays disabled (no download, no simulated result).
struct NeuralFiltersSheet: View {
    @Bindable var model: NeuralSheetModel
    private var retouch: DocumentRetouch { DocumentRetouch.shared }

    var body: some View {
        SheetScaffold(title: model.title, subtitle: model.subtitle) {
            EmptyView()
        } content: {
            HStack(alignment: .top, spacing: 0) {
                list
                    .frame(width: 220)
                Hairline(vertical: true)
                detail
                    .padding(Theme.Space.l)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
            }
        } leading: {
            if model.busy {
                HStack(spacing: Theme.Space.s) {
                    ProgressView().controlSize(.small)
                    Text("Applying…").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                }
            } else if let a = retouch.jobs.abandoned {
                Text("Stopping the cancelled \(a.operation)…").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                    .accessibilityIdentifier("document.neural.stopping")
            }
        } actions: {
            Button("Reset") { model.reset() }
                .sheetButton()
                .disabled(model.busy)
                .accessibilityIdentifier("document.neural.reset")
            Button("Cancel") { model.cancel() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
                .accessibilityIdentifier("document.neural.cancel")
            let phase = retouch.downloads.phase(model.missingModel)
            Button(applyTitle(phase)) { model.apply() }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .disabled(model.busy || model.waitingForModel || !model.state.allowed(model.state.output)
                          || phase.isDownloading || { if case .downloadsOff = phase { true } else { false } }())
                .accessibilityIdentifier("document.neural.apply")
        }
        .frame(width: 680, height: 460)
    }

    private var list: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            ForEach(model.state.specs) { s in
                let on = s.kind == model.state.kind
                let missing = s.requiresWeights && (retouch.models.first { $0.modelId == modelId(s.kind) }?.installed != true)
                Button { model.choose(s.kind) } label: {
                    HStack(spacing: Theme.Space.s) {
                        Text(s.name).font(Theme.Fonts.label).fontWeight(on ? .medium : .regular)
                            .foregroundStyle(Theme.textPrimary)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        if missing { Chip(text: "No model", color: Theme.warning, style: .outlined).fixedSize()
                                .help("The model this filter needs is not installed") }
                    }
                    .padding(.horizontal, Theme.Space.m)
                    .frame(height: Theme.Height.regular + Theme.Space.xs)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(on ? Theme.accentSubtle : Theme.clear))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(model.state.smartIndex != nil && !on)
                .accessibilityIdentifier("document.neural.filter.\(s.kind.rawValue)")
            }
            Spacer(minLength: 0)
        }
        .padding(Theme.Space.s)
    }

    @ViewBuilder private var detail: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if let s = model.state.spec {
                ForEach(s.controls) { c in
                    DocSlider(title: c.label, value: model.state.value(c), range: c.min...max(c.max, c.min + 1e-6),
                              defaultValue: c.defaultValue, format: c.max - c.min > 2 ? "%.1f" : "%.2f",
                              step: c.max - c.min > 2 ? 0.5 : 0.01, enabled: !c.isFixed,
                              identifier: "document.neural.\(s.kind.rawValue).\(c.key)", revision: model.revision) { v, _ in
                        model.set(c, v)
                    }
                    .frame(height: Theme.Height.slider)
                }
                SubHeader("Output")
                SegmentedPicker(selection: $model.state.output,
                                segments: NeuralOutput.allCases.filter { model.state.allowed($0) }.map {
                                    .init(value: $0, title: $0.title)
                                },
                                height: Theme.Height.small, fill: false)
                    .fixedSize()
                    .accessibilityIdentifier("document.neural.output")
                ForEach(NeuralOutput.allCases.filter { !model.state.allowed($0) }) { o in
                    if let why = model.state.reason(o) {
                        Text("\(o.title): \(why)").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                    }
                }
                if let hint = model.faceHint {
                    Text(hint).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let lim = s.limitation {
                    Text(lim).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let m = model.missingModel {
                    NeuralModelStatus(model: model, info: m, filter: s.name)
                        .padding(.top, Theme.Space.xs)
                        .accessibilityIdentifier("document.neural.missing")
                }
                if let e = model.error {
                    VStack(alignment: .leading, spacing: Theme.Space.xs) {
                        StatusLine(text: e.title, kind: e.isMissingWeights ? .warning : .error)
                        Text(e.detail).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.top, Theme.Space.xs)
                    .accessibilityIdentifier("document.neural.error")
                }
            }
            Spacer(minLength: 0)
        }
    }

    private func applyTitle(_ phase: RetouchDownloadPhase) -> String {
        if model.state.smartIndex != nil { return "OK" }
        switch phase {
        case .available, .failed: return "Download and Apply"
        default: return "Apply"
        }
    }

    private func modelId(_ k: NeuralKind) -> String {
        switch k {
        case .skinSmoothing: ""
        case .colorize: "filters/ddcolor"
        case .jpegArtifactRemoval: "enhance/drunet-color"
        }
    }
}

/// The chosen neural filter's missing model (B5-09b): Apply downloads it first when Settings ▸ AI allows model
/// downloads (inline progress, then the filter applies by itself); with downloads off the sheet says so and
/// links to the setting; a failed download shows its reason with Retry.
struct NeuralModelStatus: View {
    @Bindable var model: NeuralSheetModel
    let info: RetouchModelInfo
    let filter: String
    @Environment(\.openSettings) private var openSettings
    private var retouch: DocumentRetouch { DocumentRetouch.shared }

    var body: some View {
        let phase = retouch.downloads.phase(info)
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            switch phase {
            case .installed:
                EmptyView()
            case .available:
                StatusLine(text: "\(filter) needs the \(info.modelId) model, which is not installed.", kind: .warning)
                note("Download and Apply fetches the pinned, checksum-verified file from \(info.sourceURL) into \(info.cachePath), then applies \(filter).")
            case .downloadsOff:
                StatusLine(text: "\(filter) needs the \(info.modelId) model, which is not installed, and model downloads are off.", kind: .warning)
                HStack(spacing: Theme.Space.s) {
                    note("Nothing is downloaded. Allow model downloads in Settings ▸ AI, or install the file from \(info.sourceURL) at \(info.cachePath).")
                    Button("Settings ▸ AI…") { openSettings() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .accessibilityIdentifier("document.neural.settings")
                }
            case .downloading, .failed:
                ModelProgressRow(title: "\(filter) model", state: phase.progress,
                                 id: "document.neural.model") { model.apply() }
                if model.waitingForModel {
                    note("\(filter) applies when the download completes.")
                }
            }
        }
    }

    private func note(_ t: String) -> some View {
        Text(t).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// The retouch sheets, hung off the document view.
struct RetouchSheets: ViewModifier {
    @Bindable var retouch: DocumentRetouch

    func body(content: Content) -> some View {
        // B5-09b: also the self-test's start in a background launch (`open -g … --new-document`), where the
        // menu bar is never built because the app is never activated.
        let _ = RetouchSelfTest.startIfRequested()
        return content.sheet(item: $retouch.neuralSheet) { NeuralFiltersSheet(model: $0) }
    }
}
