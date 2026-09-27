import Foundation
import Observation
import TesseraFFI

/// The engine calls behind Photo Merge and Enhance (M2-47). A protocol so tests can drive the
/// controller with a stubbed engine; `EnginePhotoBackend` is the real one.
public protocol PhotoJobBackend: Sendable {
    func photoMerge(imageIds: [String], options: MergeOptions, listener: PhotoJobListener) throws -> PhotoJobProtocol
    /// Synchronous and slow (decodes every source): never on the main actor.
    func mergePreview(imageIds: [String], options: MergeOptions) throws -> MergePreview
    func enhance(imageIds: [String], options: EnhanceOptions, listener: PhotoJobListener) throws -> PhotoJobProtocol
    /// Exposure facts for the spread warning; nil when unknown.
    func exposure(imageId: String) -> ExposureFacts?
}

public struct EnginePhotoBackend: PhotoJobBackend {
    public let engine: Engine
    /// Reads EXIF through the library store (`LibraryStore.metadata`), when one is open.
    public let metadata: (@Sendable (String) -> [(name: String, value: String)]?)?

    public init(engine: Engine, metadata: (@Sendable (String) -> [(name: String, value: String)]?)? = nil) {
        self.engine = engine
        self.metadata = metadata
    }

    public func photoMerge(imageIds: [String], options: MergeOptions, listener: PhotoJobListener) throws -> PhotoJobProtocol {
        try engine.photoMerge(imageIds: imageIds, options: options, listener: listener)
    }
    public func mergePreview(imageIds: [String], options: MergeOptions) throws -> MergePreview {
        try engine.mergePreview(imageIds: imageIds, options: options)
    }
    public func enhance(imageIds: [String], options: EnhanceOptions, listener: PhotoJobListener) throws -> PhotoJobProtocol {
        try engine.enhance(imageIds: imageIds, options: options, listener: listener)
    }
    public func exposure(imageId: String) -> ExposureFacts? {
        metadata?(imageId).flatMap { ExposureFacts(fields: $0) }
    }
}

/// Engine stage names (merge.rs / enhance.rs `job.progress`) in the activity strip's words.
public enum PhotoJobStage {
    public static func title(_ stage: String) -> String {
        if let model = downloadingModel(stage) { return "Downloading \(modelName(model))" }
        if stage.hasPrefix("model-ready:") {
            return "\(modelName(String(stage.dropFirst("model-ready:".count)))) ready"
        }
        switch stage {
        case "queued": return "Waiting"
        case "decode": return "Reading photos"
        case "merge": return "Merging"
        case "fill-edges": return "Filling edges"
        case "write": return "Writing DNG"
        case "denoise": return "Denoising"
        case "super-resolution": return "Super resolution"
        case "enhance": return "Enhancing"
        case "completed": return "Done"
        case "cancelled": return "Cancelled"
        case "failed": return "Failed"
        default: return stage.replacingOccurrences(of: "-", with: " ").capitalized
        }
    }

    /// The model id while its weights download ("model-download:enhance/realesrgan-x2").
    public static func downloadingModel(_ stage: String) -> String? {
        stage.hasPrefix("model-download:") ? String(stage.dropFirst("model-download:".count)) : nil
    }

    public static func modelName(_ id: String) -> String {
        let lower = id.lowercased()
        if lower == "denoise" || lower.contains("drunet") || lower.contains("denois") { return "denoise model" }
        if lower.contains("super") || lower.contains("esrgan") { return "super resolution model" }
        return id
    }
}

/// Engine errors in the user's words, with what to do about them.
public enum PhotoJobMessages {
    public static func explain(_ message: String) -> String {
        let m = message.lowercased()
        if m.contains("missing enhancement weights") {
            let model = message.range(of: "weights ").flatMap { r in
                message[r.upperBound...].split(separator: " ").first.map(String.init)
            } ?? "model"
            return "The enhancement model \(model) is not on this Mac and downloads are off. "
                + "Turn on “Download missing models” in Photo ▸ Enhance… and try again."
        }
        if m.contains("model download") {
            return "The model could not be downloaded: \(message). Check the network connection and try again."
        }
        if m.contains("missing exposure metadata") {
            return "These photos have no exposure metadata (shutter, aperture, ISO), so HDR cannot weight them."
        }
        if m.contains("curved projection requires focal_pixels") {
            return "Spherical and Cylindrical need the lens focal length in pixels. Choose Auto or Perspective."
        }
        if m.contains("out-of-srgb") || m.contains("gamut") || m.contains("hdr/negative") {
            return "Enhance works on standard-range photos: this one has HDR or out-of-gamut values. (\(message))"
        }
        if m.contains("raw details") {
            return "Raw Details is not available: no learned demosaic model is supported."
        }
        if m.contains("photo job cancelled") { return "Cancelled" }
        return message
    }
}

/// Runs one Photo Merge or Enhance job at a time: the sheet's options, the merge preview, the
/// activity strip's progress, Cancel, and the result (published DNGs, their stack).
@MainActor @Observable
public final class PhotoJobController {
    public enum Operation: Equatable, Sendable {
        case merge(PhotoMergeKind)
        case enhance

        public var title: String {
            switch self {
            case .merge(let k): "\(k.title) merge"
            case .enhance: "Enhance"
            }
        }
    }

    public struct Progress: Equatable, Sendable {
        public var stage: String
        public var done: Int
        public var total: Int
        public var title: String { PhotoJobStage.title(stage) }
        /// Set while model weights download (stage-level: the engine reports start and ready).
        public var downloadingModel: String? { PhotoJobStage.downloadingModel(stage) }
    }

    public struct Outcome: Sendable {
        public let operation: Operation
        public let state: PhotoJobState
        public let outputs: [PhotoOutput]
        /// Engine message, as reported.
        public let error: String?
        public let stage: String?
        public var explanation: String? { error.map(PhotoJobMessages.explain) }
        public var outputIDs: [String] { outputs.map(\.imageId) }
    }

    /// Engine merge preview (≤ 512 px JPEG) for the sheet.
    public struct Preview: Equatable, Sendable {
        public var jpeg: Data?
        public var width: Int
        public var height: Int
        public var warnings: [String]
    }

    // MARK: Sheet state

    /// Which sheet is open, and for which photos.
    public var mergeSheet: PhotoMergeKind?
    public var showEnhance = false
    public private(set) var sheetImageIDs: [String] = []
    public private(set) var sheetTitle = ""
    public var mergeSettings = PhotoMergeSettings()
    public var enhanceSettings = PhotoEnhanceSettings()

    public private(set) var preview: Preview?
    public private(set) var isRenderingPreview = false
    public private(set) var previewError: String?
    /// Exposure-spread and metadata warnings for the current merge selection.
    public private(set) var advice: [String] = []

    // MARK: Job state

    public private(set) var running: Operation?
    public private(set) var runningTitle = ""
    public private(set) var progress: Progress?
    public private(set) var lastOutcome: Outcome?
    /// Last start failure (validation before the job started).
    public var startError: String?

    public var isRunning: Bool { running != nil }

    /// Called on the main actor once a job ends (completed, cancelled or failed).
    @ObservationIgnored public var onFinish: (Outcome) -> Void = { _ in }
    @ObservationIgnored public var backend: PhotoJobBackend?
    /// Debounce before a preview request (option changes arrive in bursts).
    @ObservationIgnored public var previewDelay: Duration = .milliseconds(250)

    @ObservationIgnored private var job: PhotoJobProtocol?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var previewGeneration = 0
    @ObservationIgnored private var previewTask: Task<Void, Never>?
    @ObservationIgnored private var lastPreviewKey: MergeOptions?
    @ObservationIgnored private var exposureFacts: [ExposureFacts?] = []
    @ObservationIgnored private var finishWaiters: [CheckedContinuation<Outcome, Never>] = []

    public init() {}

    // MARK: Sheets

    /// Opens a merge sheet for `imageIDs` (selection order).
    public func presentMerge(_ kind: PhotoMergeKind, imageIDs: [String], title: String) {
        sheetImageIDs = imageIDs
        sheetTitle = title
        mergeSettings.kind = kind
        if kind == .hdrPanorama, mergeSettings.bracketSizes(count: imageIDs.count) == nil {
            let choices = PhotoMergeSettings.bracketChoices(count: imageIDs.count)
            mergeSettings.bracketSize = choices.contains(3) ? 3 : (choices.first ?? 3)
        }
        mergeSettings.exposureValues = []
        preview = nil
        previewError = nil
        lastPreviewKey = nil
        startError = nil
        advice = []
        exposureFacts = []
        mergeSheet = kind
        loadExposureFacts()
        requestPreview(immediately: true)
    }

    public func presentEnhance(imageIDs: [String], title: String) {
        sheetImageIDs = imageIDs
        sheetTitle = title
        startError = nil
        showEnhance = true
    }

    public func dismissSheets() {
        mergeSheet = nil
        showEnhance = false
        previewTask?.cancel()
        previewGeneration += 1
        isRenderingPreview = false
    }

    /// Why Merge is disabled in the sheet, or nil.
    public var mergeProblem: String? {
        if isRunning { return "A photo merge or enhance is already running" }
        if backend == nil { return "Photo Merge needs a folder opened on the engine" }
        return mergeSettings.problem(count: sheetImageIDs.count)
    }

    public var enhanceProblem: String? {
        if isRunning { return "A photo merge or enhance is already running" }
        if backend == nil { return "Enhance needs a folder opened on the engine" }
        if sheetImageIDs.isEmpty { return "Select a photo to enhance" }
        return enhanceSettings.problem
    }

    // MARK: Preview

    /// The sheet's options changed: re-run the engine preview (debounced) if the engine would
    /// see something different.
    public func settingsChanged() {
        advice = PhotoMergeAdvice.warnings(kind: mergeSettings.kind, facts: exposureFacts,
                                           exposureValuesGiven: !mergeSettings.exposureValues.isEmpty)
        requestPreview(immediately: false)
    }

    public func requestPreview(immediately: Bool) {
        guard mergeSheet != nil, let backend else { return }
        let count = sheetImageIDs.count
        if let problem = mergeSettings.problem(count: count) {
            previewTask?.cancel()
            previewGeneration += 1
            isRenderingPreview = false
            previewError = problem
            preview = nil
            lastPreviewKey = nil
            return
        }
        let key = mergeSettings.previewKey(count: count)
        guard key != lastPreviewKey else { return }
        lastPreviewKey = key
        previewTask?.cancel()
        previewGeneration += 1
        let generation = previewGeneration
        let ids = sheetImageIDs
        let delay = immediately ? Duration.zero : previewDelay
        isRenderingPreview = true
        previewError = nil
        previewTask = Task { [weak self] in
            if delay > .zero { try? await Task.sleep(for: delay) }
            guard !Task.isCancelled else { return }
            let result = await Task.detached(priority: .userInitiated) {
                Result { try backend.mergePreview(imageIds: ids, options: key) }
            }.value
            guard let self, generation == self.previewGeneration else { return }
            self.isRenderingPreview = false
            switch result {
            case .success(let p):
                self.preview = Preview(jpeg: p.bytes.map { Data($0) }, width: Int(p.width), height: Int(p.height),
                                       warnings: p.warnings)
                self.previewError = nil
            case .failure(let error):
                self.preview = nil
                self.previewError = PhotoJobMessages.explain(Self.message(error))
            }
        }
    }

    /// Resolves once the preview in flight (if any) has landed. Tests.
    public func previewSettled() async {
        while isRenderingPreview { try? await Task.sleep(for: .milliseconds(10)) }
    }

    private func loadExposureFacts() {
        guard let backend else { return }
        let ids = sheetImageIDs
        Task { [weak self] in
            let facts = await Task.detached(priority: .utility) { ids.map { backend.exposure(imageId: $0) } }.value
            guard let self, self.mergeSheet != nil, self.sheetImageIDs == ids else { return }
            self.exposureFacts = facts
            self.advice = PhotoMergeAdvice.warnings(kind: self.mergeSettings.kind, facts: facts,
                                                    exposureValuesGiven: !self.mergeSettings.exposureValues.isEmpty)
        }
    }

    // MARK: Jobs

    /// Starts the merge for the sheet's photos. False (with `startError`) when it cannot start.
    @discardableResult
    public func startMerge() -> Bool {
        guard mergeProblem == nil, let backend else {
            startError = mergeProblem
            return false
        }
        let kind = mergeSettings.kind
        let ids = sheetImageIDs
        let options = mergeSettings.options(count: ids.count)
        return start(.merge(kind), title: "\(kind.title) · \(sheetTitle)") { listener in
            try backend.photoMerge(imageIds: ids, options: options, listener: listener)
        }
    }

    @discardableResult
    public func startEnhance() -> Bool {
        guard enhanceProblem == nil, let backend else {
            startError = enhanceProblem
            return false
        }
        let ids = sheetImageIDs
        let options = enhanceSettings.options
        return start(.enhance, title: "Enhance · \(sheetTitle)") { listener in
            try backend.enhance(imageIds: ids, options: options, listener: listener)
        }
    }

    public func cancel() { job?.cancel() }

    /// Resolves with the outcome of the job running now (immediately with the last one if idle).
    public func waitForFinish() async -> Outcome? {
        guard isRunning else { return lastOutcome }
        return await withCheckedContinuation { finishWaiters.append($0) }
    }

    private func start(_ operation: Operation, title: String,
                       launch: (PhotoJobListener) throws -> PhotoJobProtocol) -> Bool {
        guard !isRunning else { startError = "A photo merge or enhance is already running"; return false }
        generation += 1
        let generation = generation
        let relay = PhotoJobRelay { [weak self] p in
            Task { @MainActor in self?.receive(p, generation: generation) }
        }
        let job: PhotoJobProtocol
        do {
            job = try launch(relay)
        } catch {
            startError = PhotoJobMessages.explain(Self.message(error))
            return false
        }
        self.job = job
        startError = nil
        running = operation
        runningTitle = title
        progress = Progress(stage: "queued", done: 0, total: 0)
        Task { [weak self] in
            // Blocking wait off the main actor (the engine forbids it on the main thread).
            let status = await Task.detached(priority: .userInitiated) { job.wait() }.value
            self?.finish(status, operation: operation, generation: generation)
        }
        return true
    }

    private func receive(_ p: PhotoProgress, generation: Int) {
        guard generation == self.generation, isRunning, p.status.state == .running else { return }
        progress = Progress(stage: p.stage, done: Int(p.done), total: Int(p.total))
    }

    private func finish(_ status: PhotoJobStatus, operation: Operation, generation: Int) {
        guard generation == self.generation else { return }
        let outcome = Outcome(operation: operation, state: status.state, outputs: status.outputs,
                              error: status.error?.message, stage: status.error?.stage)
        job = nil
        running = nil
        progress = nil
        lastOutcome = outcome
        onFinish(outcome)
        let waiters = finishWaiters
        finishWaiters = []
        waiters.forEach { $0.resume(returning: outcome) }
    }

    nonisolated static func message(_ error: Error) -> String {
        if let e = error as? BridgeError, case .Failure(let message) = e { return message }
        return error.localizedDescription
    }
}

/// Engine worker thread → main actor.
final class PhotoJobRelay: PhotoJobListener, @unchecked Sendable {
    private let handler: @Sendable (PhotoProgress) -> Void
    init(_ handler: @escaping @Sendable (PhotoProgress) -> Void) { self.handler = handler }
    func onProgress(progress: PhotoProgress) { handler(progress) }
}
