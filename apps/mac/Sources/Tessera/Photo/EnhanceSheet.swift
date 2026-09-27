import SwiftUI
import TesseraCore
import TesseraFFI

/// Photo ▸ Enhance… (⌃⌥I, WP M2-50, docs/01): Denoise with an amount, Super Resolution, and consent
/// to download missing model weights. Each photo becomes its own "-Enhanced" DNG, stacked with
/// its source. Runs in the background with progress (including model downloads) and Cancel.
struct EnhanceSheet: View {
    @Bindable var jobs: PhotoJobController
    @Environment(\.dismiss) private var dismiss

    private var count: Int { jobs.sheetImageIDs.count }

    var body: some View {
        SheetScaffold(title: "Enhance",
                      subtitle: "\(count) photo\(count == 1 ? "" : "s") · \(jobs.sheetTitle)") {
            EmptyView()
        } content: {
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    SubHeader("Denoise")
                    Toggle("Denoise", isOn: $jobs.enhanceSettings.denoise)
                        .accessibilityIdentifier("enhance-denoise")
                    PhotoSlider(title: "Amount", value: Double(jobs.enhanceSettings.denoiseAmount), range: 0...100,
                                defaultValue: 50, enabled: jobs.enhanceSettings.denoise,
                                identifier: "enhance-denoise-amount") { v, final in
                        if final { jobs.enhanceSettings.denoiseAmount = Int(v.rounded()) }
                    }
                    .frame(height: Theme.Height.slider)
                    Hint("Learned noise reduction on the whole photo. Amount 0 writes an unchanged copy.")

                    SubHeader("Super Resolution")
                    Toggle("Super Resolution", isOn: $jobs.enhanceSettings.superResolution)
                        .accessibilityIdentifier("enhance-super-resolution")
                    Hint("Doubles the width and height (four times the pixels).")

                    SubHeader("Raw Details")
                    Toggle("Raw Details", isOn: .constant(false))
                        .disabled(true)
                        .accessibilityIdentifier("enhance-raw-details")
                    Hint("Not available: Tessera has no supported learned demosaic model.")

                    SubHeader("Models")
                    Toggle("Download missing models", isOn: $jobs.enhanceSettings.allowModelDownload)
                        .accessibilityIdentifier("enhance-allow-download")
                    Hint(jobs.enhanceSettings.models.isEmpty
                         ? "No model is needed for these options."
                         : "Uses the \(jobs.enhanceSettings.models.joined(separator: " and ")) model\(jobs.enhanceSettings.models.count == 1 ? "" : "s"). "
                            + "When on, a missing model is downloaded once and verified; progress shows in the activity strip. "
                            + "When off, Tessera stays offline and stops with a message if a model is missing.")

                    SubHeader("Preview")
                    Hint("No before / after preview: the engine enhances the full photo in one pass and has no preview call. "
                         + "The result is stacked next to its source, so select both and press C to compare.")
                        .accessibilityIdentifier("enhance-preview-note")

                    if let o = jobs.lastOutcome, o.operation == .enhance, o.state == .failed, let why = o.explanation {
                        StatusLine(text: "Last run: \(why)", kind: .error)
                            .accessibilityIdentifier("enhance-last-error")
                    }
                }
                .toggleStyle(.checkbox)
                .font(Theme.Fonts.label)
                .foregroundStyle(Theme.textPrimary)
                .padding(Theme.Space.l)
            }
        } leading: {
            if let problem = jobs.enhanceProblem {
                StatusLine(text: problem, kind: .warning).accessibilityIdentifier("enhance-problem")
            } else if let error = jobs.startError {
                StatusLine(text: error, kind: .error).accessibilityIdentifier("enhance-error")
            } else {
                Text("Creates …\(jobs.enhanceSettings.suffix).dng next to each photo, stacked with it")
                    .lineLimit(1)
                    .accessibilityIdentifier("enhance-output")
            }
        } actions: {
            Button("Cancel") { close() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
                .accessibilityIdentifier("enhance-cancel")
            Button("Enhance") {
                if jobs.startEnhance() { close() }
            }
            .keyboardShortcut(.defaultAction)
            .sheetButton(primary: true)
            .disabled(jobs.enhanceProblem != nil)
            .accessibilityIdentifier("enhance-start")
        }
        .frame(width: 520, height: 600)
        .accessibilityIdentifier("enhance-sheet")
    }

    private func close() {
        jobs.dismissSheets()
        dismiss()
    }
}
