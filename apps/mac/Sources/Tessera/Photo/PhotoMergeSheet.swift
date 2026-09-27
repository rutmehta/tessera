import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// Photo ▸ Photo Merge ▸ HDR… / Panorama… / HDR Panorama… (WP M2-50, docs/01). The engine's merge
/// preview on the left (≤ 512 px, recomputed as options change), the options for the kind on the
/// right, warnings under the preview. Merge closes the sheet and runs in the background with
/// progress and Cancel above the status bar; the new DNG then appears in the grid, selected.
struct PhotoMergeSheet: View {
    @Bindable var jobs: PhotoJobController
    @Environment(\.dismiss) private var dismiss

    private var kind: PhotoMergeKind { jobs.mergeSettings.kind }
    private var count: Int { jobs.sheetImageIDs.count }

    var body: some View {
        SheetScaffold(title: "\(kind.title) Merge Preview",
                      subtitle: "\(count) photo\(count == 1 ? "" : "s") · \(jobs.sheetTitle)") {
            if jobs.isRenderingPreview {
                ProgressView().controlSize(.small).accessibilityIdentifier("photo-merge-preview-busy")
            }
        } content: {
            HStack(alignment: .top, spacing: Theme.Space.l) {
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    MergePreviewWell(jobs: jobs)
                    warnings
                }
                .frame(width: MergePreviewWell.size.width)
                Hairline(vertical: true)
                ScrollView {
                    options
                        .padding(.trailing, Theme.Space.s)
                }
                .frame(maxWidth: .infinity)
            }
            .padding(Theme.Space.l)
        } leading: {
            if let problem = jobs.mergeProblem {
                StatusLine(text: problem, kind: .warning).accessibilityIdentifier("photo-merge-problem")
            } else if let error = jobs.startError {
                StatusLine(text: error, kind: .error).accessibilityIdentifier("photo-merge-error")
            } else {
                Text("Creates a linear DNG next to the first photo (…\(kind.suffix).dng)")
                    .lineLimit(1)
                    .accessibilityIdentifier("photo-merge-output")
            }
        } actions: {
            Button("Cancel") { close() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
                .accessibilityIdentifier("photo-merge-cancel")
            Button("Merge") {
                if jobs.startMerge() { close() }
            }
            .keyboardShortcut(.defaultAction)
            .sheetButton(primary: true)
            .disabled(jobs.mergeProblem != nil)
            .accessibilityIdentifier("photo-merge-start")
        }
        .frame(width: 820, height: 560)
        .onChange(of: jobs.mergeSettings) { jobs.settingsChanged() }
        .accessibilityIdentifier("photo-merge-sheet")
    }

    private func close() {
        jobs.dismissSheets()
        dismiss()
    }

    // MARK: Warnings

    /// Engine warnings (overlap, uncovered borders…) and the exposure-spread advice as warning
    /// lines; the engine's notes about the preview itself and the chosen projection as hints.
    @ViewBuilder private var warnings: some View {
        let engine = jobs.preview?.warnings ?? []
        let notes = engine.filter(Self.isNote)
        let problems = engine.filter { !Self.isNote($0) } + jobs.advice
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            if let error = jobs.previewError {
                StatusLine(text: error, kind: .warning).accessibilityIdentifier("photo-merge-preview-error")
            }
            ForEach(problems, id: \.self) { w in
                StatusLine(text: w, kind: .warning)
            }
            ForEach(notes, id: \.self) { n in
                Hint(Self.noteText(n))
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("photo-merge-warnings")
    }

    static func isNote(_ warning: String) -> Bool {
        warning.hasPrefix("Projection:") || warning.hasPrefix("Preview is a camera-channel") || warning.hasPrefix("Auto FOV estimated")
    }

    static func noteText(_ note: String) -> String {
        if note.hasPrefix("Preview is a camera-channel") { return "The preview is a quick approximation; the merged DNG opens in Develop with full colour." }
        if note.hasPrefix("Auto FOV estimated") { return "Auto projection estimated the lens field of view (no focal length given)." }
        if note.hasPrefix("Projection:") { return "Projection chosen: " + note.dropFirst("Projection:".count).trimmingCharacters(in: .whitespaces) }
        return note
    }

    // MARK: Options

    @ViewBuilder private var options: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if kind == .hdrPanorama { brackets }
            if kind.usesProjection { projection }
            if kind.usesDeghost || kind.usesAutoAlign {
                SubHeader(kind == .hdrPanorama ? "HDR" : "Options")
                if kind.usesAutoAlign {
                    Toggle("Auto Align", isOn: $jobs.mergeSettings.autoAlign)
                        .accessibilityIdentifier("photo-merge-auto-align")
                }
                if kind.usesDeghost {
                    Text("Deghost Amount").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    SegmentedPicker(selection: $jobs.mergeSettings.deghost,
                                    segments: MergeDeghost.allCases.map { .init(value: $0, title: $0.title) },
                                    height: Theme.Height.regular)
                        .accessibilityIdentifier("photo-merge-deghost")
                    Hint("Removes objects that moved between frames; higher values trust one frame more.")
                }
            }
            SubHeader("Result")
            Toggle("Auto Settings", isOn: $jobs.mergeSettings.autoTone)
                .accessibilityIdentifier("photo-merge-auto-tone")
            Hint("Starts the merged photo with an automatic tone edit (editable in Develop).")
            Toggle("Create Stack", isOn: $jobs.mergeSettings.createStack)
                .accessibilityIdentifier("photo-merge-create-stack")
            Hint("Stacks the result with its source photos.")
        }
        .toggleStyle(.checkbox)
        .font(Theme.Fonts.label)
        .foregroundStyle(Theme.textPrimary)
    }

    @ViewBuilder private var brackets: some View {
        SubHeader("Brackets")
        let choices = PhotoMergeSettings.bracketChoices(count: count)
        if choices.isEmpty {
            StatusLine(text: "Select at least 4 photos: two brackets of two or more", kind: .warning)
        } else {
            HStack(spacing: Theme.Space.s) {
                Text("Frames per bracket").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                SegmentedPicker(selection: $jobs.mergeSettings.bracketSize,
                                segments: choices.map { .init(value: $0, title: "\($0)") },
                                height: Theme.Height.regular, fill: false)
                    .accessibilityIdentifier("photo-merge-bracket-size")
            }
            Hint("\(count / max(jobs.mergeSettings.bracketSize, 1)) brackets of \(jobs.mergeSettings.bracketSize), in selection order. Brackets are never guessed from file names.")
        }
    }

    @ViewBuilder private var projection: some View {
        SubHeader("Projection")
        SegmentedPicker(selection: $jobs.mergeSettings.projection,
                        segments: MergeProjection.allCases.map { .init(value: $0, title: $0.title) },
                        height: Theme.Height.regular)
            .accessibilityIdentifier("photo-merge-projection")
        if jobs.mergeSettings.projection.isCurved {
            HStack(spacing: Theme.Space.s) {
                Text("Focal length").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .frame(width: Theme.Width.labelWide, alignment: .leading)
                TextField("pixels", value: $jobs.mergeSettings.focalPixels, format: .number.precision(.fractionLength(0...1)))
                    .textFieldStyle(.roundedBorder)
                    .controlSize(.small)
                    .frame(width: 96)
                    .accessibilityIdentifier("photo-merge-focal")
                Text("px").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
            }
            Hint("\(jobs.mergeSettings.projection.title) needs the lens focal length in source pixels (focal mm × image width ÷ sensor width mm).")
        }
        SubHeader("Edges")
        PhotoSlider(title: "Boundary Warp", value: Double(jobs.mergeSettings.boundaryWarp), range: 0...100,
                    defaultValue: 0, identifier: "photo-merge-boundary-warp") { v, final in
            if final { jobs.mergeSettings.boundaryWarp = Int(v.rounded()) }
        }
        .frame(height: Theme.Height.slider)
        Toggle("Fill Edges", isOn: $jobs.mergeSettings.fillEdges)
            .accessibilityIdentifier("photo-merge-fill-edges")
        Hint("Boundary Warp bends the edges to fill the frame; Fill Edges synthesises what is left (content-aware).")
    }
}

/// The engine's merge preview on a scope well, with an on-image chip naming what it is.
struct MergePreviewWell: View {
    let jobs: PhotoJobController
    static let size = CGSize(width: 400, height: 300)

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.plotWell))
            if let data = jobs.preview?.jpeg, let image = NSImage(data: data) {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: Self.size.width, height: Self.size.height)
            } else {
                RoundedRectangle(cornerRadius: Theme.Radius.chip)
                    .strokeBorder(Color(nsColor: Theme.Palette.plotGuide), lineWidth: Theme.Space.hairline)
                Text(jobs.isRenderingPreview ? "Rendering preview…" : jobs.previewError == nil ? "No preview" : "No preview: see below")
                    .font(Theme.Fonts.caption)
                    .foregroundStyle(Color(nsColor: Theme.Palette.plotText))
            }
            VStack {
                Spacer()
                HStack {
                    Text(chip)
                        .font(Theme.Fonts.caption)
                        .monospacedDigit()
                        .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                        .padding(.horizontal, Theme.Space.xs)
                        .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                        .padding(Theme.Space.xs)
                    Spacer()
                }
            }
        }
        .frame(width: Self.size.width, height: Self.size.height)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
        .opacity(jobs.isRenderingPreview && jobs.preview != nil ? 0.6 : 1)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("photo-merge-preview")
    }

    private var chip: String {
        guard let p = jobs.preview, p.jpeg != nil else { return "Engine preview" }
        return "Engine preview · \(p.width) × \(p.height)"
    }
}

/// A `ValueSlider` for sheets: `onChange(value, final)` is called directly; SwiftUI pushes a new
/// value only when not dragging.
struct PhotoSlider: NSViewRepresentable {
    let title: String
    let value: Double
    let range: ClosedRange<Double>
    var defaultValue: Double
    var format = "%.0f"
    var enabled = true
    var identifier: String
    let onChange: (Double, Bool) -> Void

    final class Coordinator { var onChange: ((Double, Bool) -> Void)? }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ValueSlider {
        let s = ValueSlider(frame: .zero)
        let c = context.coordinator
        s.onChange = { v, final in c.onChange?(v, final) }
        return s
    }

    func updateNSView(_ s: ValueSlider, context: Context) {
        context.coordinator.onChange = onChange
        s.title = title
        s.minValue = range.lowerBound
        s.maxValue = range.upperBound
        s.defaultValue = defaultValue
        s.valueFormat = format
        s.step = 1
        s.isEnabled = enabled
        s.setAccessibilityIdentifier(identifier)
        if !s.isDragging { s.doubleValue = value }
        s.needsDisplay = true
    }
}
