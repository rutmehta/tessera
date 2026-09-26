import Foundation
import Observation
import TesseraCore
import TesseraFFI

/// A generated caption waiting in the Metadata panel: editable, saved only on Save.
struct CaptionDraft: Equatable {
    let imageID: String
    var caption: String
    var altText: String
}

/// Keyword suggestions, captions / alt text and text in images (docs/09 §1, §3; WP M3-15).
/// Models run on this Mac in the engine's background jobs; this controller starts them, polls
/// their progress for the status strip and refreshes the Keywords and Metadata panels.
/// Suggestions change nothing until accepted; generated captions are drafts until saved.
@MainActor @Observable
final class UnderstandingController {
    @ObservationIgnored weak var library: LibraryModel?

    /// Suggested keywords for the selection (Keywords panel ▸ Suggested).
    var chips = SuggestionChips()
    /// Cached caption / alt text / OCR of the focused photo.
    private(set) var info: ImageUnderstandingInfo?
    /// Active and recently finished jobs (status strip shows the active ones).
    private(set) var jobs: [UnderstandingJobInfo] = []
    /// Generated caption offered in the Metadata panel.
    var draft: CaptionDraft?
    private(set) var settings = AiMetadataSettings(autoSuggestOnImport: false, writeSuggestedKeywordsToXmp: false)
    private(set) var modelStatus: UnderstandingModelStatus?

    @ObservationIgnored private var poll: Task<Void, Never>?
    @ObservationIgnored private var captionWanted: String?
    @ObservationIgnored private var lastProgress: [UInt64: UInt32] = [:]
    @ObservationIgnored private var announced: Set<UInt64> = []

    private var engine: Engine? { library?.app?.engineLibrary?.engine }
    private var catalog: LibraryCatalog? { library?.catalog }

    var isAvailable: Bool { engine != nil && catalog != nil }
    var activeJobs: [UnderstandingJobInfo] { jobs.filter { $0.state == .queued || $0.state == .running } }

    // MARK: Lifecycle

    /// A folder opened: settings, the hidden `--fake-captioner` aid, then auto-suggest.
    func install() {
        poll?.cancel()
        poll = nil
        chips = SuggestionChips(threshold: chips.threshold)
        info = nil
        draft = nil
        captionWanted = nil
        guard let engine, let catalog else { return }
        if ProcessInfo.processInfo.arguments.contains("--fake-captioner") {
            try? engine.useTestUnderstanding()
        }
        if let s = try? engine.aiMetadataSettings() { settings = s }
        modelStatus = try? engine.understandingModelStatus()
        refreshJobs()
        do {
            if try engine.autoSuggest(imageIds: catalog.imageIDs(for: Array(library?.app?.library.items.indices ?? 0..<0))) != nil {
                startPolling()
            }
        } catch {
            library?.app?.statusMessage = "Keyword suggestions: \(error.localizedDescription)"
        }
    }

    /// Focus or selection changed (or a panel needs fresh data).
    func reload() {
        guard let engine, let catalog, let app = library?.app, let item = app.focusedItem,
              let id = catalog.imageID(of: item.id) else {
            info = nil
            chips.replace(with: [])
            return
        }
        info = try? engine.imageUnderstanding(imageId: id)
        if let d = draft, d.imageID != id { draft = nil }
        let ids = catalog.imageIDs(for: Array(app.targetIDs.prefix(500)))
        let fresh = (try? catalog.store.suggestions(imageIds: ids.isEmpty ? [id] : ids)) ?? []
        chips.replace(with: fresh.map(SuggestedKeyword.init))
    }

    // MARK: Jobs

    /// Keywords panel ▸ Suggest for Selection.
    func suggestForSelection() {
        start("Suggest keywords") { engine, ids in try engine.suggestKeywords(imageIds: ids) }
    }

    /// Metadata panel ▸ Detect Text.
    func detectText() {
        start("Detect text") { engine, ids in try engine.ocr(imageIds: ids) }
    }

    /// Metadata panel ▸ Generate: the focused photo only (a caption describes one photo). A
    /// cached caption fills the fields at once; otherwise they fill when the job finishes.
    func generateCaption() {
        guard let engine, let catalog, let app = library?.app, let item = app.focusedItem,
              let id = catalog.imageID(of: item.id) else { return }
        if let info, info.imageId == id, info.hasCaption {
            draft = CaptionDraft(imageID: id, caption: info.caption, altText: info.altText)
            return
        }
        do {
            _ = try engine.caption(imageIds: [id])
            captionWanted = id
            app.statusMessage = "Describing \(item.url?.lastPathComponent ?? "the photo") on this Mac…"
            startPolling()
        } catch {
            app.statusMessage = "Generate caption failed: \(error.localizedDescription)"
        }
    }

    private func start(_ verb: String, _ submit: (Engine, [String]) throws -> UInt64) {
        guard let engine, let catalog, let app = library?.app else { return }
        let ids = catalog.imageIDs(for: app.targetIDs)
        guard !ids.isEmpty else { app.statusMessage = "Select photos first"; return }
        do {
            _ = try submit(engine, ids)
            startPolling()
        } catch {
            app.statusMessage = "\(verb) failed: \(error.localizedDescription)"
        }
    }

    func cancel(_ job: UInt64) {
        try? engine?.cancelUnderstandingJob(id: job)
        refreshJobs()
    }

    private func refreshJobs() {
        guard let engine, let fresh = try? engine.understandingJobs() else { return }
        if fresh != jobs { jobs = fresh }
    }

    private func startPolling() {
        refreshJobs()
        guard poll == nil else { return }
        poll = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(300))
                guard let self, !Task.isCancelled else { return }
                if !self.tick() { self.poll = nil; return }
            }
        }
    }

    /// One poll: returns whether any job is still active.
    private func tick() -> Bool {
        refreshJobs()
        var changed = false
        for job in jobs {
            let progress = job.done + job.failed
            if lastProgress[job.id] != progress { lastProgress[job.id] = progress; changed = true }
            let finished = job.state != .queued && job.state != .running
            if finished, !announced.contains(job.id) {
                announced.insert(job.id)
                changed = true
                announce(job)
            }
        }
        if changed {
            reload()
            if let wanted = captionWanted, let info, info.imageId == wanted, info.hasCaption {
                draft = CaptionDraft(imageID: wanted, caption: info.caption, altText: info.altText)
                captionWanted = nil
            }
            // Keywords accepted elsewhere and captions/OCR feed search: refresh counts.
            library?.cullDidChange(albums: false)
        }
        return !activeJobs.isEmpty
    }

    private func announce(_ job: UnderstandingJobInfo) {
        guard let app = library?.app else { return }
        let what = Self.title(job.tasks)
        switch job.state {
        case .failed:
            app.statusMessage = "\(what) failed: \(job.error ?? "unknown error")"
            if job.tasks.contains(.caption) { captionWanted = nil }
        case .cancelled:
            app.statusMessage = "\(what) stopped after \(job.done) photo\(job.done == 1 ? "" : "s")"
        case .succeeded:
            if job.failed > 0 {
                app.statusMessage = "\(what): \(job.done) done, \(job.failed) failed (\(job.error ?? ""))"
            } else if job.total == 0 {
                app.statusMessage = "\(what): already up to date"
            } else {
                app.statusMessage = "\(what): \(job.done) photo\(job.done == 1 ? "" : "s")"
            }
        case .queued, .running:
            break
        }
    }

    static func title(_ tasks: [UnderstandingTask]) -> String {
        let names = tasks.map { t -> String in
            switch t {
            case .keywords: "keyword suggestions"
            case .caption: "captions"
            case .ocr: "text in images"
            }
        }
        return names.joined(separator: ", ").capitalizedSentence
    }

    // MARK: Suggestions

    /// Click (one chip) or ⇧-click (every chip at or above the threshold).
    func accept(_ keyword: String, all: Bool) {
        let names = all ? chips.acceptAllAboveThreshold() : chips.accept(keyword)
        guard !names.isEmpty, let catalog, let app = library?.app else { return }
        let ids = catalog.imageIDs(for: app.targetIDs)
        do {
            let paths = try catalog.store.acceptSuggestions(imageIds: ids, keywords: names)
            let shown = paths.map { $0.replacingOccurrences(of: "|", with: " › ") }
            let what = shown.count == 1 ? "“\(shown[0])”" : "\(shown.count) suggested keywords"
            let target = settings.writeSuggestedKeywordsToXmp ? "XMP sidecars" : "the catalog (XMP off)"
            app.statusMessage = shown.isEmpty ? "Nothing to add" : "Added \(what) to \(target)"
        } catch {
            app.statusMessage = "Accept suggestion failed: \(error.localizedDescription)"
        }
        library?.keywordsDidChange()
        reload()
    }

    func reject(_ keyword: String) {
        let names = chips.reject(keyword)
        guard !names.isEmpty, let catalog, let app = library?.app else { return }
        do {
            try catalog.store.rejectSuggestions(imageIds: catalog.imageIDs(for: app.targetIDs), keywords: names)
        } catch {
            app.statusMessage = "Reject suggestion failed: \(error.localizedDescription)"
        }
        reload()
    }

    // MARK: Captions

    /// Saves the draft (as edited) to the selection's XMP: the Caption and Alt text fields.
    func saveDraft(caption: String, altText: String) {
        guard let d = draft, let catalog, let app = library?.app,
              let item = catalog.items(for: [d.imageID]).first else { draft = nil; return }
        library?.saveIPTC(IptcEdit(title: nil, caption: caption, copyright: nil, creator: nil, keywords: nil,
                                   altText: altText), items: [item])
        draft = nil
        app.statusMessage = "Saved caption and alt text to \(app.library.items[item].url?.lastPathComponent ?? "the photo")"
    }

    // MARK: Settings ▸ AI

    func setSettings(_ s: AiMetadataSettings) {
        guard let engine else { settings = s; return }
        do {
            try engine.setAiMetadataSettings(settings: s)
            settings = s
        } catch {
            library?.app?.statusMessage = "Settings: \(error.localizedDescription)"
        }
    }

    func refreshSettings() {
        guard let engine else { return }
        if let s = try? engine.aiMetadataSettings() { settings = s }
        modelStatus = try? engine.understandingModelStatus()
    }
}
