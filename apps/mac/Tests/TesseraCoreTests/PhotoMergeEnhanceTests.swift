import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore
@testable import Tessera

/// Photo ▸ Photo Merge and Photo ▸ Enhance (WP M2-50). Option → FFI mapping, enable / disable
/// rules, progress and cancel against a stubbed engine; then one real merge of three bracketed
/// exposures (LinearRaw DNGs written here) into an HDR DNG that opens in Develop.
@MainActor
final class PhotoMergeEnhanceTests: XCTestCase {

    // MARK: Option mapping

    func testDefaultsMatchTheEngineAndHDRIgnoresPanoramaControls() {
        let s = PhotoMergeSettings(kind: .hdr)
        let o = s.options(count: 3)
        XCTAssertEqual(o, MergeOptions(kind: .hdr, autoAlign: true, autoTone: true, deghost: .medium, projection: .auto,
                                       boundaryWarp: 0, fillEdges: false, createStack: true, focalPixels: nil,
                                       bracketSizes: [], exposureValues: []),
                       "the sheet starts at the engine's MergeOptions::default")
        var hdr = PhotoMergeSettings(kind: .hdr)
        hdr.autoAlign = false
        hdr.autoTone = false
        hdr.deghost = .high
        hdr.createStack = false
        hdr.projection = .spherical      // leftovers from a panorama: not sent for HDR
        hdr.boundaryWarp = 70
        hdr.fillEdges = true
        hdr.focalPixels = 1200
        hdr.exposureValues = [0.25, 1, 4]
        let h = hdr.options(count: 3)
        XCTAssertEqual(h.kind, .hdr)
        XCTAssertFalse(h.autoAlign)
        XCTAssertFalse(h.autoTone)
        XCTAssertEqual(h.deghost, .high)
        XCTAssertFalse(h.createStack)
        XCTAssertEqual(h.projection, .auto)
        XCTAssertEqual(h.boundaryWarp, 0)
        XCTAssertFalse(h.fillEdges)
        XCTAssertNil(h.focalPixels)
        XCTAssertEqual(h.bracketSizes, [])
        XCTAssertEqual(h.exposureValues, [0.25, 1, 4])
        XCTAssertNil(hdr.problem(count: 3), "HDR does not need a focal length even if one was chosen")
        XCTAssertEqual(hdr.options(count: 4).exposureValues, [], "exposures only when there is one per photo")
    }

    func testPanoramaAndHDRPanoramaMapping() {
        var p = PhotoMergeSettings(kind: .panorama)
        p.autoAlign = false          // not a panorama control: registration is the merge
        p.deghost = .none            // not a panorama control
        p.projection = .cylindrical
        p.focalPixels = 1480.5
        p.boundaryWarp = 140
        p.fillEdges = true
        let o = p.options(count: 5)
        XCTAssertEqual(o.kind, .panorama)
        XCTAssertTrue(o.autoAlign)
        XCTAssertEqual(o.deghost, .medium)
        XCTAssertEqual(o.projection, .cylindrical)
        XCTAssertEqual(o.focalPixels, 1480.5)
        XCTAssertEqual(o.boundaryWarp, 100, "clamped to the engine's 0…100")
        XCTAssertTrue(o.fillEdges)
        XCTAssertEqual(o.bracketSizes, [])

        var hp = PhotoMergeSettings(kind: .hdrPanorama)
        hp.bracketSize = 3
        hp.deghost = .low
        hp.projection = .perspective
        let q = hp.options(count: 9)
        XCTAssertEqual(q.kind, .hdrPanorama)
        XCTAssertEqual(q.bracketSizes, [3, 3, 3], "sequential, in selection order")
        XCTAssertEqual(q.deghost, .low)
        XCTAssertEqual(q.projection, .perspective)
        XCTAssertNil(q.focalPixels)

        // Preview key ignores Create Stack (nothing the engine renders changes).
        var a = PhotoMergeSettings(kind: .hdr)
        let before = a.previewKey(count: 3)
        a.createStack = false
        XCTAssertEqual(a.previewKey(count: 3), before)
        a.deghost = .high
        XCTAssertNotEqual(a.previewKey(count: 3), before)
    }

    func testMergeProblemsExplainWhyMergeIsDisabled() {
        XCTAssertNotNil(PhotoMergeSettings(kind: .hdr).problem(count: 1))
        XCTAssertNil(PhotoMergeSettings(kind: .hdr).problem(count: 2))
        XCTAssertNotNil(PhotoMergeSettings(kind: .hdr).problem(count: 65), "HDR takes at most 64 frames")
        XCTAssertNil(PhotoMergeSettings(kind: .panorama).problem(count: 65))
        var curved = PhotoMergeSettings(kind: .panorama)
        curved.projection = .spherical
        XCTAssertEqual(curved.problem(count: 3), "Spherical needs the focal length in pixels")
        curved.focalPixels = 0
        XCTAssertNotNil(curved.problem(count: 3))
        curved.focalPixels = 900
        XCTAssertNil(curved.problem(count: 3))

        var hp = PhotoMergeSettings(kind: .hdrPanorama)
        XCTAssertNotNil(hp.problem(count: 3), "two brackets of two at least")
        hp.bracketSize = 3
        XCTAssertEqual(hp.problem(count: 8), "8 photos do not split into brackets of 3")
        XCTAssertNil(hp.problem(count: 6))
        XCTAssertNil(hp.bracketSizes(count: 3), "one bracket is not a panorama")
        XCTAssertEqual(PhotoMergeSettings.bracketChoices(count: 12), [2, 3, 4, 6])
        XCTAssertEqual(PhotoMergeSettings.bracketChoices(count: 9), [3])
        XCTAssertEqual(PhotoMergeSettings.bracketChoices(count: 3), [])

        var ev = PhotoMergeSettings(kind: .hdr)
        ev.exposureValues = [1, -1]
        XCTAssertNotNil(ev.problem(count: 2))
    }

    func testEnhanceOptionsMapping() {
        var e = PhotoEnhanceSettings()
        XCTAssertEqual(e.options, EnhanceOptions(denoiseAmount: 50, superResolution: false, rawDetails: false, allowModelDownload: false),
                       "denoise 50, cache-only by default")
        XCTAssertEqual(e.suffix, "-Enhanced-NR")
        e.denoiseAmount = 130
        e.superResolution = true
        e.allowModelDownload = true
        XCTAssertEqual(e.options, EnhanceOptions(denoiseAmount: 100, superResolution: true, rawDetails: false, allowModelDownload: true))
        XCTAssertEqual(e.suffix, "-Enhanced-NR-SR")
        XCTAssertEqual(e.models, ["DRUNet denoiser", "Real-ESRGAN ×2"])
        e.denoise = false
        XCTAssertNil(e.options.denoiseAmount, "Denoise off is no amount, not zero")
        XCTAssertEqual(e.suffix, "-Enhanced-SR")
        XCTAssertNil(e.problem)
        e.superResolution = false
        XCTAssertNotNil(e.problem, "the engine rejects an enhance that does nothing")
        e.denoise = true
        e.denoiseAmount = 0
        XCTAssertEqual(e.options.denoiseAmount, 0)
        XCTAssertEqual(e.models, [], "amount 0 bypasses the model")
    }

    // MARK: Enable / disable rules

    func testCommandRules() {
        XCTAssertFalse(PhotoCommandRules.canMerge(.hdr, selected: 1, engineBacked: true, running: false))
        XCTAssertTrue(PhotoCommandRules.canMerge(.hdr, selected: 2, engineBacked: true, running: false))
        XCTAssertTrue(PhotoCommandRules.canMerge(.panorama, selected: 2, engineBacked: true, running: false))
        XCTAssertFalse(PhotoCommandRules.canMerge(.hdrPanorama, selected: 3, engineBacked: true, running: false))
        XCTAssertTrue(PhotoCommandRules.canMerge(.hdrPanorama, selected: 4, engineBacked: true, running: false))
        XCTAssertFalse(PhotoCommandRules.canMerge(.hdr, selected: 3, engineBacked: false, running: false), "stub library")
        XCTAssertFalse(PhotoCommandRules.canMerge(.hdr, selected: 3, engineBacked: true, running: true), "one job at a time")
        XCTAssertFalse(PhotoCommandRules.canMerge(.hdr, selected: 65, engineBacked: true, running: false))
        XCTAssertFalse(PhotoCommandRules.canEnhance(selected: 0, engineBacked: true, running: false))
        XCTAssertTrue(PhotoCommandRules.canEnhance(selected: 1, engineBacked: true, running: false))
        XCTAssertTrue(PhotoCommandRules.canEnhance(selected: 40, engineBacked: true, running: false))
        XCTAssertFalse(PhotoCommandRules.canEnhance(selected: 1, engineBacked: true, running: true))
        XCTAssertFalse(PhotoCommandRules.canEnhance(selected: 1, engineBacked: false, running: false))
        XCTAssertEqual(PhotoCommandRules.mergeProblem(.hdr, selected: 1, engineBacked: true, running: false),
                       "Select 2 or more photos to merge")
    }

    func testAppModelMenuStateFollowsTheSelection() throws {
        let model = AppModel()
        model.install(StubLibrary.synthetic(count: 4))
        XCTAssertFalse(model.canPhotoMerge(.hdr), "stub items have no engine pixels")
        XCTAssertFalse(model.canEnhance)
        model.presentPhotoMerge(.hdr)
        XCTAssertNil(model.photoJobs.mergeSheet)
        XCTAssertNotNil(model.statusMessage)
    }

    // MARK: Advice, stages, messages

    func testExposureParsingAndSpreadWarnings() {
        XCTAssertEqual(ExposureFacts.number("1/250"), 0.004)
        XCTAssertEqual(ExposureFacts.number("1/250 s"), 0.004)
        XCTAssertEqual(ExposureFacts.number("0.5 s"), 0.5)
        XCTAssertEqual(ExposureFacts.number("f/2.8"), 2.8)
        XCTAssertEqual(ExposureFacts.number("F2.8"), 2.8)
        XCTAssertEqual(ExposureFacts.number("ISO 400"), 400)
        XCTAssertNil(ExposureFacts.number("n/a"))
        let facts = ExposureFacts(fields: [(name: "Make", value: "Sony"), (name: "ExposureTime", value: "1/60"),
                                           (name: "FNumber", value: "f/4"), (name: "PhotographicSensitivity", value: "200")])
        XCTAssertEqual(facts, ExposureFacts(shutter: 1.0 / 60, aperture: 4, iso: 200))
        XCTAssertNil(ExposureFacts(fields: [(name: "FNumber", value: "f/4")]), "no shutter, no facts")

        let bracket = [ExposureFacts(shutter: 1.0 / 240), ExposureFacts(shutter: 1.0 / 60), ExposureFacts(shutter: 1.0 / 15)]
        XCTAssertEqual(PhotoMergeAdvice.warnings(kind: .hdr, facts: bracket), [], "±2 EV is a good bracket")
        XCTAssertEqual(PhotoMergeAdvice.warnings(kind: .panorama, facts: bracket),
                       ["Exposures differ by 4.0 EV: for bracketed frames use HDR Panorama"])
        let flat = [ExposureFacts(shutter: 0.01), ExposureFacts(shutter: 0.01)]
        XCTAssertEqual(PhotoMergeAdvice.warnings(kind: .hdr, facts: flat),
                       ["Exposures differ by 0.0 EV: HDR adds little range. Bracket at least 1 EV apart"])
        XCTAssertEqual(PhotoMergeAdvice.warnings(kind: .panorama, facts: flat), [])
        XCTAssertEqual(PhotoMergeAdvice.warnings(kind: .hdr, facts: [nil, ExposureFacts(shutter: 1)]),
                       ["1 of 2 photos have no exposure metadata: HDR cannot weight them"])
        XCTAssertEqual(PhotoMergeAdvice.warnings(kind: .hdr, facts: [nil, nil], exposureValuesGiven: true), [])
    }

    func testStageTitlesAndErrorMessages() {
        XCTAssertEqual(PhotoJobStage.title("decode"), "Reading photos")
        XCTAssertEqual(PhotoJobStage.title("merge"), "Merging")
        XCTAssertEqual(PhotoJobStage.title("fill-edges"), "Filling edges")
        XCTAssertEqual(PhotoJobStage.title("write"), "Writing DNG")
        XCTAssertEqual(PhotoJobStage.title("model-download:enhance/realesrgan-x2"), "Downloading super resolution model")
        XCTAssertEqual(PhotoJobStage.downloadingModel("model-download:enhance/drunet"), "enhance/drunet")
        XCTAssertNil(PhotoJobStage.downloadingModel("denoise"))
        XCTAssertEqual(PhotoJobStage.title("model-ready:denoise"), "denoise model ready")
        let missing = PhotoJobMessages.explain(
            "missing enhancement weights enhance/drunet@1.0 (offline); set allow_model_download=true to download")
        XCTAssertTrue(missing.contains("enhance/drunet@1.0"), missing)
        XCTAssertTrue(missing.contains("Download missing models"), missing)
        XCTAssertTrue(PhotoJobMessages.explain("missing exposure metadata; supply exposure_values").contains("no exposure metadata"))
        XCTAssertEqual(PhotoJobMessages.explain("something else"), "something else")
    }

    // MARK: Controller against a stubbed engine

    func testMergeRunsWithPreviewProgressAndCompletion() async throws {
        let backend = StubPhotoBackend()
        backend.exposures = ["a": ExposureFacts(shutter: 0.01), "b": ExposureFacts(shutter: 0.01)]
        let jobs = PhotoJobController()
        jobs.backend = backend
        jobs.previewDelay = .milliseconds(1)
        var finished: [PhotoJobController.Outcome] = []
        jobs.onFinish = { finished.append($0) }

        jobs.presentMerge(.hdr, imageIDs: ["a", "b"], title: "a and 1 more")
        XCTAssertEqual(jobs.mergeSheet, .hdr)
        XCTAssertTrue(jobs.isRenderingPreview)
        await jobs.previewSettled()
        XCTAssertEqual(jobs.preview?.width, 64)
        XCTAssertEqual(jobs.preview?.warnings, ["Insufficient overlap between frames 1 and 2"])
        XCTAssertEqual(backend.previewCalls.count, 1)
        XCTAssertEqual(backend.previewCalls.first?.0, ["a", "b"])
        try await waitUntil { !jobs.advice.isEmpty }
        XCTAssertEqual(jobs.advice.count, 1, "equal exposures: little HDR range")

        // Option changes re-run the preview; Create Stack does not.
        jobs.mergeSettings.createStack = false
        jobs.settingsChanged()
        await jobs.previewSettled()
        XCTAssertEqual(backend.previewCalls.count, 1)
        jobs.mergeSettings.deghost = .high
        jobs.settingsChanged()
        await jobs.previewSettled()
        XCTAssertEqual(backend.previewCalls.count, 2)
        XCTAssertEqual(backend.previewCalls.last?.1.deghost, .high)

        XCTAssertTrue(jobs.startMerge())
        XCTAssertTrue(jobs.isRunning)
        XCTAssertEqual(jobs.running, .merge(.hdr))
        XCTAssertEqual(backend.mergeCalls.count, 1)
        XCTAssertEqual(backend.mergeCalls[0].1.deghost, .high)
        XCTAssertFalse(backend.mergeCalls[0].1.createStack)
        XCTAssertFalse(jobs.startMerge(), "one job at a time")

        backend.emit(stage: "decode", done: 1, total: 2)
        try await waitUntil { jobs.progress?.stage == "decode" }
        XCTAssertEqual(jobs.progress?.done, 1)
        XCTAssertEqual(jobs.progress?.total, 2)
        XCTAssertEqual(jobs.progress?.title, "Reading photos")

        backend.job.complete(outputs: [PhotoOutput(imageId: "m", path: "/tmp/a-HDR.dng", sourceIds: ["a", "b"])])
        let outcome = await jobs.waitForFinish()
        XCTAssertEqual(outcome?.state, .completed)
        XCTAssertEqual(outcome?.outputIDs, ["m"])
        XCTAssertFalse(jobs.isRunning)
        XCTAssertNil(jobs.progress)
        XCTAssertEqual(finished.count, 1)
    }

    func testCancelStopsTheJobAndReportsCancelled() async throws {
        let backend = StubPhotoBackend()
        let jobs = PhotoJobController()
        jobs.backend = backend
        jobs.presentEnhance(imageIDs: ["a", "b", "c"], title: "three")
        jobs.enhanceSettings.superResolution = true
        jobs.enhanceSettings.allowModelDownload = true
        XCTAssertNil(jobs.enhanceProblem)
        XCTAssertTrue(jobs.startEnhance())
        XCTAssertEqual(backend.enhanceCalls.first?.1,
                       EnhanceOptions(denoiseAmount: 50, superResolution: true, rawDetails: false, allowModelDownload: true))
        backend.emit(stage: "model-download:enhance/realesrgan-x2", done: 0, total: 1)
        try await waitUntil { jobs.progress?.downloadingModel != nil }
        XCTAssertEqual(jobs.progress?.title, "Downloading super resolution model")
        jobs.cancel()
        XCTAssertTrue(backend.job.cancelRequested)
        let outcome = await jobs.waitForFinish()
        XCTAssertEqual(outcome?.state, .cancelled)
        XCTAssertEqual(outcome?.explanation, "Cancelled")
        XCTAssertFalse(jobs.isRunning)
        // A late progress event from the worker does not resurrect the strip.
        backend.emit(stage: "enhance", done: 1, total: 3)
        try await Task.sleep(for: .milliseconds(30))
        XCTAssertNil(jobs.progress)
    }

    func testFailuresAreExplained() async throws {
        let backend = StubPhotoBackend()
        let jobs = PhotoJobController()
        jobs.backend = backend
        jobs.presentEnhance(imageIDs: ["a"], title: "a")
        backend.startError = BridgeError.Failure(message: "select denoise or super resolution")
        XCTAssertFalse(jobs.startEnhance())
        XCTAssertEqual(jobs.startError, "select denoise or super resolution")
        XCTAssertFalse(jobs.isRunning)

        backend.startError = nil
        XCTAssertTrue(jobs.startEnhance())
        backend.job.fail(stage: "model-download:enhance/drunet",
                         message: "missing enhancement weights enhance/drunet@1 (offline); set allow_model_download=true to download")
        let outcome = await jobs.waitForFinish()
        XCTAssertEqual(outcome?.state, .failed)
        XCTAssertTrue(outcome?.explanation?.contains("Download missing models") == true)
        XCTAssertEqual(jobs.lastOutcome?.operation, .enhance)

        // No backend: nothing starts.
        let idle = PhotoJobController()
        idle.presentEnhance(imageIDs: ["a"], title: "a")
        XCTAssertNotNil(idle.enhanceProblem)
        XCTAssertFalse(idle.startEnhance())
    }

    // MARK: Real engine

    /// Three bracketed exposures of one synthetic scene (LinearRaw float DNGs, the engine's
    /// interchange format) → HDR merge through the app → a new DNG, selected in the grid,
    /// stacked with its sources, that opens in Develop.
    func testRealEngineMergesThreeBracketsIntoAnHDRDngThatOpensInDevelop() async throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let temp = root.appendingPathComponent("build/photo-merge-test-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        let folder = temp.appendingPathComponent("brackets")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let (w, h) = (192, 128)
        let exposures = [0.25, 1.0, 4.0]
        for (i, t) in exposures.enumerated() {
            let pixels = LinearDNG.scene(width: w, height: h, exposure: Float(t))
            try LinearDNG.write(to: folder.appendingPathComponent("bracket-\(i + 1).dng"), width: w, height: h, rgb: pixels)
        }
        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        XCTAssertEqual(library.items.count, 3, "the three DNGs are indexed")

        let model = AppModel()
        model.install(library)
        model.select(position: 0)
        XCTAssertFalse(model.canPhotoMerge(.hdr), "one photo")
        XCTAssertTrue(model.canEnhance)
        model.selectAll()
        XCTAssertTrue(model.canPhotoMerge(.hdr))
        XCTAssertTrue(model.canPhotoMerge(.panorama))
        XCTAssertFalse(model.canPhotoMerge(.hdrPanorama))
        model.presentPhotoMerge(.hdr)
        let jobs = model.photoJobs
        XCTAssertEqual(jobs.mergeSheet, .hdr)
        XCTAssertEqual(jobs.sheetImageIDs.count, 3)
        // These DNGs carry no EXIF exposure: say what each bracket was (in grid order).
        let order = jobs.sheetImageIDs.map { id in library.items[library.itemOfImage[id]!].name }
        jobs.mergeSettings.exposureValues = order.map { exposures[Int(String($0.dropFirst("bracket-".count).prefix(1)))! - 1] }
        jobs.settingsChanged()
        await jobs.previewSettled()
        XCTAssertNotNil(jobs.preview?.jpeg, "engine preview: \(jobs.previewError ?? "-") \(jobs.preview?.warnings ?? [])")
        XCTAssertLessThanOrEqual(max(jobs.preview?.width ?? 0, jobs.preview?.height ?? 0), 512)

        XCTAssertTrue(jobs.startMerge(), jobs.startError ?? "")
        XCTAssertFalse(model.canPhotoMerge(.hdr), "disabled while running")
        let finished = await jobs.waitForFinish()
        let outcome = try XCTUnwrap(finished)
        XCTAssertEqual(outcome.state, .completed, outcome.error ?? "")
        let output = try XCTUnwrap(outcome.outputs.first)
        XCTAssertTrue(output.path.hasSuffix("-HDR.dng"), output.path)
        XCTAssertTrue(FileManager.default.fileExists(atPath: output.path))
        XCTAssertEqual(Set(output.sourceIds), Set(jobs.sheetImageIDs))
        let stack = try library.engine.photoStack(imageId: output.imageId)
        XCTAssertEqual(stack.first, output.imageId, "derived image first")
        XCTAssertEqual(stack.count, 4, "stacked with its three sources")

        // The change feed brings the DNG into the grid, selected.
        try await waitUntil(seconds: 30) { model.focusedItem?.engineImage?.imageID == output.imageId }
        let merged = try XCTUnwrap(model.focusedItem)
        XCTAssertEqual(model.selectionCount, 1)
        XCTAssertEqual(model.visibleCount, 4)

        // It opens in Develop.
        model.viewMode = .loupe
        model.openDevelop(for: merged)
        try await waitUntil(seconds: 30) { model.developStatus != .loading && model.developStatus != .none }
        XCTAssertEqual(model.developStatus, .ready, "\(model.developStatus)")
        XCTAssertEqual(model.develop?.itemID, merged.id)
        XCTAssertEqual(model.develop?.info.width, UInt32(w))
        model.closeDevelop()
    }

    private func waitUntil(seconds: Double = 5, _ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition() {
            if Date() > deadline { XCTFail("condition not met within \(seconds) s"); return }
            try await Task.sleep(for: .milliseconds(10))
        }
    }
}

// MARK: - Stub engine

final class StubPhotoJob: PhotoJobProtocol, @unchecked Sendable {
    private let condition = NSCondition()
    private var current = PhotoJobStatus(state: .running, outputs: [], error: nil)
    private(set) var cancelRequested = false

    func cancel() {
        condition.lock()
        cancelRequested = true
        condition.unlock()
        settle(.cancelled, outputs: [], error: PhotoJobError(stage: "decode", message: "photo job cancelled"))
    }
    func status() -> PhotoJobStatus {
        condition.lock(); defer { condition.unlock() }
        return current
    }
    func wait() -> PhotoJobStatus {
        condition.lock(); defer { condition.unlock() }
        while current.state == .running { condition.wait() }
        return current
    }
    func complete(outputs: [PhotoOutput]) { settle(.completed, outputs: outputs, error: nil) }
    func fail(stage: String, message: String) { settle(.failed, outputs: [], error: PhotoJobError(stage: stage, message: message)) }

    private func settle(_ state: PhotoJobState, outputs: [PhotoOutput], error: PhotoJobError?) {
        condition.lock()
        if current.state == .running { current = PhotoJobStatus(state: state, outputs: outputs, error: error) }
        condition.broadcast()
        condition.unlock()
    }
}

final class StubPhotoBackend: PhotoJobBackend, @unchecked Sendable {
    private let lock = NSLock()
    private var _previewCalls: [([String], MergeOptions)] = []
    private(set) var mergeCalls: [([String], MergeOptions)] = []
    private(set) var enhanceCalls: [([String], EnhanceOptions)] = []
    private(set) var job = StubPhotoJob()
    private var listener: PhotoJobListener?
    var startError: Error?
    var exposures: [String: ExposureFacts] = [:]

    var previewCalls: [([String], MergeOptions)] { lock.withLock { _previewCalls } }

    func photoMerge(imageIds: [String], options: MergeOptions, listener: PhotoJobListener) throws -> PhotoJobProtocol {
        if let startError { throw startError }
        mergeCalls.append((imageIds, options))
        return begin(listener)
    }
    func mergePreview(imageIds: [String], options: MergeOptions) throws -> MergePreview {
        lock.withLock { _previewCalls.append((imageIds, options)) }
        return MergePreview(bytes: Data([0xFF, 0xD8]), width: 64, height: 48, warnings: ["Insufficient overlap between frames 1 and 2"])
    }
    func enhance(imageIds: [String], options: EnhanceOptions, listener: PhotoJobListener) throws -> PhotoJobProtocol {
        if let startError { throw startError }
        enhanceCalls.append((imageIds, options))
        return begin(listener)
    }
    func exposure(imageId: String) -> ExposureFacts? { lock.withLock { exposures[imageId] } }

    private func begin(_ listener: PhotoJobListener) -> PhotoJobProtocol {
        job = StubPhotoJob()
        self.listener = listener
        return job
    }

    /// Like the engine: from a worker thread.
    func emit(stage: String, done: UInt32, total: UInt32) {
        let listener = self.listener
        let status = job.status()
        Thread.detachNewThread {
            listener?.onProgress(progress: PhotoProgress(stage: stage, done: done, total: total,
                                                        status: PhotoJobStatus(state: .running, outputs: [], error: status.error)))
        }
    }
}

// MARK: - LinearRaw DNG writer (the engine's bounded interchange: one IFD, one float32 strip)

enum LinearDNG {
    /// A textured HDR scene (0.02…12 linear) seen at `exposure`, clipped at 1 like a sensor.
    static func scene(width: Int, height: Int, exposure: Float) -> [Float] {
        var out = [Float](repeating: 0, count: width * height * 3)
        for y in 0..<height {
            for x in 0..<width {
                let fx = Float(x) / Float(width), fy = Float(y) / Float(height)
                var l = 0.02 * powf(600, fx)                                  // a 9-stop ramp
                if ((x / 16) + (y / 16)) % 2 == 0 { l *= 0.5 }               // texture for alignment
                let dx = fx - 0.7, dy = fy - 0.35
                if dx * dx + dy * dy < 0.01 { l = 12 }                       // a bright "sun"
                let rgb: [Float] = [l, l * (0.8 + 0.2 * fy), l * (0.7 + 0.3 * (1 - fy))]
                for c in 0..<3 { out[(y * width + x) * 3 + c] = min(rgb[c] * exposure * 0.25, 1) }
            }
        }
        return out
    }

    static func write(to url: URL, width: Int, height: Int, rgb: [Float]) throws {
        precondition(rgb.count == width * height * 3)
        var data = Data()
        func u16(_ v: UInt16) { var le = v.littleEndian; withUnsafeBytes(of: &le) { data.append(contentsOf: $0) } }
        func u32(_ v: UInt32) { var le = v.littleEndian; withUnsafeBytes(of: &le) { data.append(contentsOf: $0) } }
        func i32(_ v: Int32) { u32(UInt32(bitPattern: v)) }
        // XYZ (D65) → camera: camera space = linear sRGB primaries.
        let matrix: [Double] = [3.2406, -1.5372, -0.4986, -0.9689, 1.8758, 0.0415, 0.0557, -0.2040, 1.0570]
        let entries = 16
        let ifdSize = 2 + entries * 12 + 4
        var extra = 8 + ifdSize
        func reserve(_ n: Int) -> UInt32 { defer { extra += n + (n % 2) }; return UInt32(extra) }
        let bitsOffset = reserve(6), formatOffset = reserve(6), whiteOffset = reserve(12)
        let matrixOffset = reserve(72), neutralOffset = reserve(24)
        let stripOffset = UInt32(extra)
        let stripBytes = UInt32(rgb.count * 4)

        data.append(contentsOf: [0x49, 0x49]); u16(42); u32(8)
        u16(UInt16(entries))
        func entry(_ tag: UInt16, _ type: UInt16, _ count: UInt32, _ value: UInt32, short: Bool = false) {
            u16(tag); u16(type); u32(count)
            if short { u16(UInt16(value)); u16(0) } else { u32(value) }
        }
        entry(256, 4, 1, UInt32(width))
        entry(257, 4, 1, UInt32(height))
        entry(258, 3, 3, bitsOffset)
        entry(259, 3, 1, 1, short: true)
        entry(262, 3, 1, 34892, short: true)
        entry(273, 4, 1, stripOffset)
        entry(277, 3, 1, 3, short: true)
        entry(278, 4, 1, UInt32(height))
        entry(279, 4, 1, stripBytes)
        entry(284, 3, 1, 1, short: true)
        entry(339, 3, 3, formatOffset)
        u16(50706); u16(1); u32(4); data.append(contentsOf: [1, 4, 0, 0])
        entry(50717, 4, 3, whiteOffset)
        entry(50721, 10, 9, matrixOffset)
        entry(50728, 5, 3, neutralOffset)
        entry(50778, 3, 1, 21, short: true)
        u32(0)
        for _ in 0..<3 { u16(32) }          // BitsPerSample
        for _ in 0..<3 { u16(3) }           // SampleFormat: IEEE float
        for _ in 0..<3 { u32(1) }           // WhiteLevel
        for v in matrix { i32(Int32((v * 10000).rounded())); i32(10000) }
        for _ in 0..<3 { u32(1); u32(1) }   // AsShotNeutral
        precondition(data.count == Int(stripOffset))
        for v in rgb { u32(v.bitPattern) }
        try data.write(to: url)
    }
}
