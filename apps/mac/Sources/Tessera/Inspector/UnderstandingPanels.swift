import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

// Keyword suggestions, caption / alt text generation, text in images and their background jobs
// (WP M3-15). Composed into the Keywords and Metadata panels, the status strip and Settings ▸ AI.

// MARK: - Keywords ▸ Suggested

/// Suggested keywords for the selection: click accepts one, ⇧-click accepts every chip at or
/// above the threshold, ✕ rejects. Chips show confidence as a bar and whether they map onto an
/// existing keyword in the tree (↳) or will be created under "Suggested" (+).
struct SuggestedKeywordsSection: View {
    let model: AppModel
    @Bindable var understanding: UnderstandingController

    var body: some View {
        let chips = understanding.chips
        let n = max(model.selectionCount, 1)
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.s) {
                SubHeader("Suggested")
                Spacer(minLength: Theme.Space.xs)
                Button(n > 1 ? "Suggest for \(n) Photos" : "Suggest") { understanding.suggestForSelection() }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .padding(.top, Theme.Space.s)
                    .disabled(model.focusedItem == nil || !understanding.isAvailable)
                    .help("Suggest keywords for the selected photos on this Mac (Settings ▸ AI)")
                    .accessibilityIdentifier("keyword-suggest-selection")
            }
            if chips.isEmpty {
                Hint(understanding.info?.hasKeywords == true
                     ? "No more suggestions for this photo."
                     : "Suggestions appear here with their confidence. Nothing is applied until you accept it.")
            } else {
                FlowRow(spacing: Theme.Space.xs) {
                    ForEach(chips.items) { s in
                        SuggestionChipView(suggestion: s, strong: chips.isAboveThreshold(s), selectionCount: n,
                                           accept: { all in understanding.accept(s.keyword, all: all) },
                                           reject: { understanding.reject(s.keyword) })
                    }
                }
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("keyword-suggestions")
                HStack(spacing: Theme.Space.s) {
                    Text("Accept all ≥ \(Int((chips.threshold * 100).rounded())) %")
                        .font(Theme.Fonts.captionNumeric)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize()
                    Slider(value: $understanding.chips.threshold, in: 0.05...0.95, step: 0.05)
                        .controlSize(.mini)
                        .tint(Theme.textSecondary)
                        .accessibilityLabel("Accept-all threshold")
                        .accessibilityIdentifier("keyword-suggestion-threshold")
                    Button("Accept \(chips.aboveThreshold.count)") { understanding.accept("", all: true) }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .disabled(chips.aboveThreshold.isEmpty)
                        .help("Accept every suggestion at or above the threshold (⇧-click a chip)")
                        .accessibilityIdentifier("keyword-suggestions-accept-all")
                }
            }
        }
    }
}

private struct SuggestionChipView: View {
    let suggestion: SuggestedKeyword
    /// At or above the accept-all threshold.
    let strong: Bool
    let selectionCount: Int
    let accept: (_ all: Bool) -> Void
    let reject: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(spacing: Theme.Space.xs) {
            Button {
                accept(NSEvent.modifierFlags.contains(.shift))
            } label: {
                HStack(spacing: Theme.Space.xs) {
                    if suggestion.ambiguous {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Theme.warning)
                    } else if suggestion.mappedPath != nil {
                        Image(systemName: "arrow.turn.down.right").foregroundStyle(Theme.textTertiary)
                    } else if !suggestion.existing {
                        Image(systemName: "plus").foregroundStyle(Theme.textTertiary)
                    }
                    Text(suggestion.keyword)
                        .foregroundStyle(strong ? Theme.textPrimary : Theme.textSecondary)
                    if selectionCount > 1, suggestion.images < selectionCount {
                        Text("\(suggestion.images)").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                    }
                }
                .font(Theme.Fonts.caption)
                .imageScale(.small)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(help)
            .accessibilityLabel("Accept \(suggestion.keyword)")
            .accessibilityValue(suggestion.percent)
            .accessibilityIdentifier("keyword-suggestion-\(suggestion.keyword)")
            Button(action: reject) {
                Image(systemName: "xmark").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
            }
            .buttonStyle(.plain)
            .help("Reject: not suggested again for these photos")
            .accessibilityLabel("Reject \(suggestion.keyword)")
            .accessibilityIdentifier("keyword-suggestion-reject-\(suggestion.keyword)")
        }
        .padding(.horizontal, Theme.Space.s - Theme.Space.xxs)
        .frame(height: Theme.Height.small)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(hovering ? Theme.hover : Theme.clear))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
            .strokeBorder(Theme.hairlineStrong, style: StrokeStyle(lineWidth: Theme.Space.hairline, dash: [2, 2])))
        // Confidence bar along the bottom edge (independent scores, not shares of 100 %).
        .overlay(alignment: .bottomLeading) {
            GeometryReader { g in
                Rectangle()
                    .fill(strong ? Theme.textSecondary : Theme.textTertiary)
                    .frame(width: max(Theme.Space.xxs, g.size.width * suggestion.confidence), height: Theme.Space.xxs)
                    .frame(maxHeight: .infinity, alignment: .bottom)
            }
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
            .allowsHitTesting(false)
        }
        .onHover { hovering = $0 }
    }

    private var help: String {
        var parts = ["\(suggestion.keyword): \(suggestion.percent) confidence"]
        if suggestion.ambiguous {
            parts.append("matches more than one keyword in the list; rename or remove a synonym to accept it")
        } else if let path = suggestion.mappedPath {
            parts.append("adds “\(path)”")
        } else if !suggestion.existing {
            parts.append("new keyword under “Suggested”")
        }
        if selectionCount > 1 { parts.append("suggested for \(suggestion.images) of \(selectionCount) photos") }
        parts.append("click to accept, ⇧-click to accept all at or above the threshold")
        return parts.joined(separator: " · ")
    }
}

// MARK: - Metadata ▸ Generate caption

/// "Generate" beside the Caption / Alt text fields, and the draft's Save / Discard.
struct CaptionGenerateRow: View {
    let model: AppModel
    let understanding: UnderstandingController
    /// Editing values of the two fields (owned by the Metadata panel).
    let caption: String
    let altText: String

    var body: some View {
        let busy = understanding.activeJobs.contains { $0.tasks.contains(.caption) }
        HStack(spacing: Theme.Space.s) {
            Spacer().frame(width: Theme.Width.label)
            if understanding.draft != nil {
                Button("Save") { understanding.saveDraft(caption: caption, altText: altText) }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Save the caption and alt text as edited to the photo's XMP sidecar")
                    .accessibilityIdentifier("caption-draft-save")
                Button("Discard") { understanding.draft = nil }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .accessibilityIdentifier("caption-draft-discard")
            } else {
                Button {
                    understanding.generateCaption()
                } label: {
                    HStack(spacing: Theme.Space.xs) {
                        Image(systemName: "text.below.photo")
                        Text("Generate")
                    }
                }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(model.selectionCount > 1 || model.focusedItem == nil || busy || !understanding.isAvailable)
                .help(model.selectionCount > 1 ? "Select one photo: a caption describes a single photo"
                      : "Describe this photo on this Mac. The text fills the fields for you to edit before saving.")
                .accessibilityIdentifier("metadata-generate-caption")
                if busy {
                    ProgressView().controlSize(.small)
                }
            }
            Spacer(minLength: 0)
        }
        if understanding.draft != nil {
            HStack(spacing: Theme.Space.s) {
                Spacer().frame(width: Theme.Width.label)
                Hint("Generated on this Mac. Edit the fields, then Save.")
            }
        }
    }
}

// MARK: - Metadata ▸ Text in image

/// Read-only OCR text of the focused photo, with Detect Text and Find Similar.
struct TextInImageBlock: View {
    let model: AppModel
    let understanding: UnderstandingController
    @Bindable var library: LibraryModel

    var body: some View {
        let info = understanding.info
        let busy = understanding.activeJobs.contains { $0.tasks.contains(.ocr) }
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.s) {
                SubHeader("Text in Image")
                Spacer(minLength: Theme.Space.xs)
                if busy { ProgressView().controlSize(.small).padding(.top, Theme.Space.s) }
                Button(info?.hasOcr == true ? "Detect Again" : "Detect Text") { understanding.detectText() }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .padding(.top, Theme.Space.s)
                    .disabled(model.focusedItem == nil || busy || !understanding.isAvailable)
                    .help("Read text in the selected photos on this Mac; it becomes searchable (text:)")
                    .accessibilityIdentifier("ocr-detect")
            }
            if let info, info.hasOcr {
                if info.ocrText.isEmpty {
                    Hint("No text found.")
                } else {
                    Text(info.ocrText)
                        .font(Theme.Fonts.caption)
                        .foregroundStyle(Theme.textPrimary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(Theme.Space.s)
                        .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Theme.well))
                        .accessibilityIdentifier("ocr-text")
                    Button("Find Photos with This Text") {
                        let first = info.ocr.first?.text ?? info.ocrText
                        library.filter.text = SearchTerm.appending(SearchTerm.text(first), to: "")
                    }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .help("Search the folder for “\(info.ocr.first?.text ?? "")” in captions and text in images")
                    .accessibilityIdentifier("ocr-find")
                }
            } else {
                Hint("Text read from the photo appears here and in search.")
            }
        }
    }
}

// MARK: - Status strip

struct UnderstandingProgressBar: View {
    let understanding: UnderstandingController

    var body: some View {
        if let job = understanding.activeJobs.first {
            let queued = understanding.activeJobs.count - 1
            ProgressStrip(title: UnderstandingController.title(job.tasks), done: Int(job.done + job.failed),
                          total: Int(job.total), detail: queued > 0 ? " · \(queued) queued" : "",
                          current: job.state == .queued ? "Waiting…" : job.current) {
                Button("Stop") { understanding.cancel(job.id) }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("understanding-cancel")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("understanding-progress")
        }
    }
}

// MARK: - Settings ▸ AI

struct KeywordsCaptionsSettingsSection: View {
    let model: AppModel

    var body: some View {
        let u = model.collections.understanding
        Section("Keywords, captions and text") {
            Toggle("Suggest keywords for new photos", isOn: Binding(
                get: { u.settings.autoSuggestOnImport },
                set: { var s = u.settings; s.autoSuggestOnImport = $0; u.setSettings(s) }))
                .disabled(!u.isAvailable)
                .accessibilityIdentifier("ai-auto-suggest")
            Toggle("Write accepted suggestions to XMP sidecars", isOn: Binding(
                get: { u.settings.writeSuggestedKeywordsToXmp },
                set: { var s = u.settings; s.writeSuggestedKeywordsToXmp = $0; u.setSettings(s) }))
                .disabled(!u.isAvailable)
                .accessibilityIdentifier("ai-write-suggested-xmp")
            if !u.isAvailable {
                Hint("Open a folder to change these settings.")
            } else if let status = u.modelStatus {
                Text(status.testModels ? "Test models (--fake-captioner): colour names, no downloads."
                     : "Runs on this Mac: SigLIP keywords \(status.keywordsInstalled ? "installed" : "not installed"), "
                        + "Florence-2 captions and text \(status.captionsInstalled ? "installed" : "not installed").")
                    .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .accessibilityIdentifier("ai-caption-models")
                if !status.testModels, !(status.keywordsInstalled && status.captionsInstalled) {
                    Hint("Install with tools/fetch_siglip.py and tools/fetch_florence.py --cache \(status.cache)")
                        .textSelection(.enabled)
                }
            }
            Hint("Off: accepted suggestions stay in Tessera's catalog (searchable, kept across rescans) and other apps don't see them.")
        }
        .onAppear { u.refreshSettings() }
    }
}
