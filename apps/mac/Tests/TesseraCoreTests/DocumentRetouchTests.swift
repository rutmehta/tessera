import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-09: Remove tool options, distraction review, the Neural Filters sheet's outputs, error
/// presentation, the selection-outline generation counter, and the engine / stub adapters.
final class DocumentRetouchTests: XCTestCase {
    // MARK: Tool options

    func testRemoveOptionsMapToEngineParams() {
        var o = RemoveToolOptions()
        XCTAssertEqual(o.engine, .auto)
        XCTAssertEqual(o.paramsJson, #"{"dilation":2}"#)
        o.dilation = 100
        XCTAssertEqual(o.paramsJson, #"{"dilation":64}"#, "clamped to the engine's 0…64")
        o.dilation = -3
        XCTAssertEqual(o.paramsJson, #"{"dilation":0}"#)
        o.setSize(0)
        XCTAssertEqual(o.size, 1)
        o.setSize(.nan)
        XCTAssertEqual(o.size, 60)
        o.setSize(90)
        o.bracket(larger: true)
        XCTAssertEqual(o.size, 100)
        o.bracket(larger: false)
        XCTAssertEqual(o.size, 90)
        XCTAssertEqual(RemoveEngine.allCases.map(\.title), ["Auto", "PatchMatch", "LaMa"])
        XCTAssertEqual(RemoveEngine.lama.ffi, .lama)
        XCTAssertEqual(RemoveEngine.patchMatch.ffi, .patchMatch)
        XCTAssertEqual(RemoveEngine.auto.ffi, .auto)
    }

    // MARK: Suggestion review

    private func scan() -> DistractionScanResult {
        DistractionScanResult(candidates: [
            DistractionCandidate(id: 0, kind: .wire, bounds: CanvasRect(x: 10, y: 20, width: 40, height: 2), pixels: 80),
            DistractionCandidate(id: 1, kind: .faceBox, bounds: CanvasRect(x: 0, y: 0, width: 100, height: 100), pixels: 10_000),
        ], faces: "supplied", limitation: "Geometric proxies")
    }

    func testSuggestionReviewStartsAcceptedAndAppliesOnlyAccepted() {
        var r = DistractionReview(layer: 7, scan: scan())
        XCTAssertEqual(r.acceptedIds, [0, 1])
        XCTAssertEqual(r.summary, "2 of 2 suggestions selected")
        r.toggle(1)
        XCTAssertEqual(r.acceptedIds, [0])
        XCTAssertFalse(r.isAccepted(1))
        r.toggle(99)
        XCTAssertEqual(r.acceptedIds, [0], "unknown ids are ignored")
        // A click inside both picks the smaller (the wire inside the face box).
        XCTAssertEqual(r.hit(CanvasPoint(x: 20, y: 21))?.id, 0)
        XCTAssertEqual(r.hit(CanvasPoint(x: 90, y: 90))?.id, 1)
        XCTAssertNil(r.hit(CanvasPoint(x: 200, y: 200)))
        r.setAll(false)
        XCTAssertFalse(r.canApply)
        r.setAll(true)
        XCTAssertTrue(r.canApply)
        let empty = DistractionReview(layer: 7, scan: DistractionScanResult(candidates: [], faces: "none", limitation: ""))
        XCTAssertEqual(empty.summary, "Nothing found")
        XCTAssertFalse(empty.canApply)
    }

    // MARK: Neural sheet

    private var specs: [NeuralFilterSpec] { NeuralFilterSpec.engineCatalogue }

    func testNeuralCatalogueComesFromTheEngine() {
        XCTAssertEqual(specs.map(\.kind), [.skinSmoothing, .colorize, .jpegArtifactRemoval])
        XCTAssertEqual(specs.map(\.requiresWeights), [false, true, true])
        XCTAssertEqual(specs[0].controls.map(\.key), ["blur", "smoothness"])
        XCTAssertEqual(specs[2].controls.first?.key, "strength")
        XCTAssertEqual(NeuralKind(filterId: "neural/colorize"), .colorize)
        XCTAssertNil(NeuralKind(filterId: "gaussian_blur"))
    }

    func testNeuralSheetDestinationsFollowTheLayer() {
        var pixel = NeuralSheetState(specs: specs, layerKind: .pixel, hasSelection: false)
        XCTAssertEqual(pixel.output, .currentLayer)
        XCTAssertEqual(NeuralOutput.allCases.filter(pixel.allowed), [.currentLayer, .newLayer, .smartFilter])
        let selected = NeuralSheetState(specs: specs, layerKind: .pixel, hasSelection: true)
        XCTAssertFalse(selected.allowed(.smartFilter), "smart retouch filters are not masked by a selection")
        XCTAssertNotNil(selected.reason(.smartFilter))
        let smart = NeuralSheetState(specs: specs, layerKind: .smartObject, hasSelection: false)
        XCTAssertEqual(smart.output, .smartFilter)
        XCTAssertFalse(smart.allowed(.newLayer))
        XCTAssertEqual(smart.reason(.newLayer), "New layer output needs a pixel layer")
        let group = NeuralSheetState(specs: specs, layerKind: .group, hasSelection: false)
        XCTAssertTrue(NeuralOutput.allCases.filter(group.allowed).isEmpty)

        // Values: clamped, per filter, into the params JSON; reset restores defaults.
        pixel.kind = .jpegArtifactRemoval
        let strength = pixel.spec!.controls[0]
        pixel.set(strength, 3)
        XCTAssertEqual(pixel.value(strength), 1)
        XCTAssertEqual(pixel.paramsJson, #"{"strength":1}"#)
        pixel.reset()
        XCTAssertEqual(pixel.paramsJson, #"{"strength":0.5}"#)
        pixel.kind = .skinSmoothing
        XCTAssertEqual(pixel.paramsJson, #"{"blur":4,"smoothness":0.5}"#, "no faces: the engine finds them")
    }

    func testReEditingANeuralSmartFilterKeepsItsFacesAndOutput() {
        let json = #"{"id":"neural/skin_smoothing","params":{"blur":2,"faces":[[10,12,30,40]],"smoothness":0.25}}"#
        let s = NeuralSheetState(specs: specs, layerKind: .smartObject, hasSelection: true, kind: .skinSmoothing,
                                 smartIndex: 0, filterJson: json)
        XCTAssertEqual(s.output, .smartFilter)
        XCTAssertEqual(NeuralOutput.allCases.filter(s.allowed), [.smartFilter])
        XCTAssertEqual(s.faces, [[10, 12, 30, 40]])
        XCTAssertEqual(s.value(s.spec!.controls[0]), 2)
        XCTAssertEqual(s.filterJson,
                       #"{"id":"neural/skin_smoothing","params":{"blur":2,"faces":[[10,12,30,40]],"smoothness":0.25}}"#)
    }

    // MARK: Errors

    func testMissingWeightsAreNamedWithTheirSource() {
        let msg = "Colorize needs the filters/ddcolor model weights, which are not installed. Tessera never downloads "
            + "weights on its own: the file comes from https://huggingface.co/x/ddcolor.onnx and is expected, "
            + "hash-verified, at /tmp/cache/abc.onnx"
        let p = RetouchErrorPresentation(operation: "Colorize", message: msg)
        XCTAssertTrue(p.isMissingWeights)
        XCTAssertEqual(p.modelId, "filters/ddcolor")
        XCTAssertEqual(p.sourceURL, "https://huggingface.co/x/ddcolor.onnx")
        XCTAssertEqual(p.title, "Colorize needs a model that is not installed")
        XCTAssertEqual(p.detail, msg)
        XCTAssertTrue(p.statusLine.contains("filters/ddcolor"))

        let c = RetouchErrorPresentation(operation: "Remove", message: "cancelled")
        XCTAssertTrue(c.isCancel)
        XCTAssertEqual(c.statusLine, "Remove cancelled")
        let s = RetouchErrorPresentation(operation: "Content-Aware Fill", message: "make a selection first")
        XCTAssertEqual(s.title, "Content-Aware Fill needs a selection")
        let o = RetouchErrorPresentation(operation: "Remove", message: "the layer's pixels are locked")
        XCTAssertEqual(o.title, "Remove failed")
        XCTAssertFalse(o.isMissingWeights)
    }

    // MARK: Outline generations

    func testOutlineGenerationDropsStaleResults() {
        var g = OutlineRequestGate()
        let first = g.begin()
        XCTAssertTrue(g.accepts(first))
        // The selection is cleared while `first` is in flight: the clear starts a generation too.
        _ = g.begin()
        XCTAssertFalse(g.accepts(first), "a result from before the clear is stale")
        let third = g.begin()
        XCTAssertTrue(g.accepts(third))
        XCTAssertFalse(g.accepts(third - 1))
    }

    // MARK: Adapters

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-retouch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testEngineRetouchThroughTheAdapter() throws {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: 64, height: 48, depth: .u8, profile: nil)
        defer { doc.close() }
        let r = try XCTUnwrap(doc as? any DocumentRetouchBackend)
        let tools = try XCTUnwrap(doc as? any DocumentToolsBackend)
        let layer = try doc.layers()[0].id
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.8, g: 0.4, b: 0.2)), opacity: 1)

        // Models are listed from the local cache; nothing is installed in a fresh support dir.
        let models = try r.retouchModels()
        XCTAssertEqual(Set(models.map(\.modelId)), ["remove/lama", "filters/ddcolor", "enhance/drunet-color", "opencv/yunet", "opencv/sface"])
        XCTAssertTrue(models.allSatisfy { !$0.installed && $0.sourceURL.hasPrefix("https://") })

        XCTAssertThrowsError(try r.contentAwareFill(layer: layer, paramsJson: "{}"), "needs a selection")
        _ = try tools.selectMarquee(.rect, rect: CGRect(x: 20, y: 10, width: 10, height: 10), feather: 0, antialias: false, op: .replace)
        let before = try doc.historyItems().count
        let o = try r.removeSelection(layer: layer, engine: .auto, paramsJson: #"{"dilation":0}"#)
        XCTAssertEqual(o.backend, "PatchMatch")
        XCTAssertTrue(o.note?.contains("remove/lama") ?? false)
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Remove")
        XCTAssertThrowsError(try r.removeSelection(layer: layer, engine: .lama, paramsJson: "{}")) { e in
            XCTAssertTrue(e.localizedDescription.contains("remove/lama"))
        }

        // A stroke: one node.
        _ = try doc.clearSelection()
        try r.beginRemoveStroke(layer: layer, size: 8, engine: .patchMatch)
        let dirty = try r.removeStrokePoints([CanvasPoint(x: 10, y: 10), CanvasPoint(x: 30, y: 12)])
        XCTAssertNotNil(dirty)
        let n = try doc.historyItems().count
        _ = try r.endRemoveStroke(paramsJson: "{}")
        XCTAssertEqual(try doc.historyItems().count, n + 1)

        // Neural: the missing model is named; skin smoothing to a new layer is one node.
        XCTAssertThrowsError(try r.neuralFilter(layer: layer, kind: .colorize, paramsJson: "{}", output: .currentLayer)) { e in
            XCTAssertTrue(RetouchErrorPresentation(operation: "Colorize", message: e.localizedDescription).isMissingWeights)
        }
        let m = try doc.historyItems().count
        _ = try r.neuralFilter(layer: layer, kind: .skinSmoothing, paramsJson: #"{"faces":[[5,5,20,20]]}"#, output: .newLayer)
        XCTAssertEqual(try doc.historyItems().count, m + 1)
        XCTAssertEqual(try doc.layers().count, 2)
    }

    func testTheStubNeedsTheEngine() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 32, height: 32, depth: .u8, profile: nil)
        defer { doc.close() }
        let r = try XCTUnwrap(doc as? any DocumentRetouchBackend)
        XCTAssertEqual(r.neuralFilterSpecs().count, 3)
        XCTAssertThrowsError(try r.contentAwareFill(layer: 1, paramsJson: "{}"))
        XCTAssertThrowsError(try r.neuralFilter(layer: 1, kind: .skinSmoothing, paramsJson: "{}", output: .newLayer))
        XCTAssertEqual(try r.retouchModels(), [])
    }

    // MARK: Cancel never waits (B5-09b)

    /// A fake slow engine call: blocks until released and ignores cancel (a coarse PatchMatch).
    private final class SlowEngine: @unchecked Sendable {
        private let gate = DispatchSemaphore(value: 0)
        private let lock = NSLock()
        private var _calls = 0
        private var _stops = 0
        var calls: Int { lock.withLock { _calls } }
        var stops: Int { lock.withLock { _stops } }
        func run() { lock.withLock { _calls += 1 }; gate.wait() }
        /// The engine's cancel: here it blocks for a while too, like a lock held by the running job.
        func stop() { lock.withLock { _stops += 1 }; Thread.sleep(forTimeInterval: 0.3) }
        func release() { gate.signal() }
    }

    @MainActor private func waitFor(_ what: String, timeout: TimeInterval = 5, _ condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > deadline { XCTFail("timed out waiting for \(what)"); return }
            try? await Task.sleep(for: .milliseconds(5))
        }
    }

    @MainActor func testJobsCancelAbandonsWithoutWaitingAndRefusesUntilStopped() async {
        let jobs = RetouchJobs()
        let slow = SlowEngine()
        var ends: [String] = []
        XCTAssertNil(jobs.start("Removing…", operation: "Remove", { slow.run(); return 1 }) { (end: RetouchJobs.End<Int>) in
            switch end {
            case .finished: ends.append("finished")
            case .discarded(let r): ends.append((try? r.get()).map { "discarded \($0)" } ?? "discarded error")
            }
        })
        XCTAssertTrue(jobs.isBusy)
        XCTAssertEqual(jobs.running?.operation, "Remove")
        XCTAssertNotNil(jobs.start("Filling…", operation: "Content-Aware Fill", { 2 }) { (_: RetouchJobs.End<Int>) in XCTFail() },
                        "one job at a time")
        await waitFor("the slow call started") { slow.calls == 1 }

        let t = Date()
        let job = jobs.cancel { slow.stop() }
        XCTAssertLessThan(Date().timeIntervalSince(t), 0.1, "Cancel does not wait for the engine's cancel")
        XCTAssertEqual(job?.operation, "Remove")
        XCTAssertFalse(jobs.isBusy, "the bar is idle at once")
        XCTAssertEqual(jobs.abandoned?.operation, "Remove")
        XCTAssertNil(jobs.cancel { XCTFail("nothing is running") })
        await waitFor("the engine was asked to stop") { slow.stops == 1 }

        // While the abandoned call still runs, a new job is refused with a clear message and runs nothing.
        var ran = false
        let refusal = jobs.start("Removing…", operation: "Remove", { 3 }) { (_: RetouchJobs.End<Int>) in ran = true }
        XCTAssertTrue(refusal?.contains("still stopping") ?? false, refusal ?? "nil")
        XCTAssertEqual(jobs.refusal, refusal)

        // The late result is handed back as discarded, never as finished.
        slow.release()
        await waitFor("the abandoned call returned") { !ends.isEmpty }
        XCTAssertEqual(ends, ["discarded 1"])
        XCTAssertNil(jobs.abandoned)
        XCTAssertNil(jobs.refusal)
        XCTAssertFalse(ran)

        // Then the next job runs normally.
        XCTAssertNil(jobs.start("Filling…", operation: "Content-Aware Fill", { 4 }) { (end: RetouchJobs.End<Int>) in
            if case .finished(.success(let v)) = end { ends.append("finished \(v)") }
        })
        await waitFor("the next job") { ends.count == 2 }
        XCTAssertEqual(ends.last, "finished 4")
    }

    /// The Remove tool over a fake slow backend: Cancel returns the bar to idle immediately; a new Remove is
    /// refused while the abandoned one runs; its late result (committed by the "engine" after the cancel) is
    /// discarded and reverted, so History and layers end as they were.
    @MainActor func testCancelledRemoveNeverReachesHistory() async throws {
        let backend = try StubDocumentEngine.shared.newDocument(width: 32, height: 32, depth: .u8, profile: nil)
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let r = DocumentRetouch.shared
        let slow = SlowEngine()
        let head = doc.info.historyHead
        let layers = doc.layers.count
        var discarded: Result<RetouchOutcome, Error>?
        var finished: Result<RetouchOutcome, Error>?
        r.onDiscarded = { discarded = $0 }
        r.onFinished = { finished = $0 }
        defer { r.onDiscarded = nil; r.onFinished = nil }

        r.start("Removing…", doc) {
            slow.run()
            // The engine finishes and commits although Cancel was pressed.
            let c = try backend.addLayer(kind: .pixel, name: "late", parent: nil, index: nil)
            return RetouchOutcome(change: c, backend: "PatchMatch", note: nil, millis: 1)
        }
        XCTAssertEqual(r.busy, "Removing…")
        await waitFor("the slow call started") { slow.calls == 1 }
        let t = Date()
        r.cancel()
        XCTAssertLessThan(Date().timeIntervalSince(t), 0.1)
        XCTAssertNil(r.busy, "Cancel returns the bar to idle immediately")
        XCTAssertNotNil(r.jobs.abandoned)

        let second = SlowEngine()
        r.start("Removing…", doc) { second.run(); throw DocumentError.invalid("must not run") }
        XCTAssertNil(r.busy)
        XCTAssertTrue(r.notice?.contains("still stopping") ?? false, r.notice ?? "nil")
        XCTAssertEqual(second.calls, 0)

        slow.release()
        await waitFor("the late result") { discarded != nil }
        XCTAssertNil(finished, "a cancelled job never finishes into history")
        XCTAssertEqual(doc.info.historyHead, head, "the step the engine committed late is undone")
        XCTAssertEqual(doc.layers.count, layers)
        XCTAssertNil(r.jobs.abandoned)
        XCTAssertNil(r.notice, "the refusal clears once the job has stopped")
    }

    // MARK: Edit ▸ Content-Aware Fill (B5-09b)

    func testContentAwareFillIsEnabledWithAMarqueeAndDisabledWithoutASelection() {
        XCTAssertTrue(RetouchMenuState.contentAwareFillEnabled(layerKind: .pixel, hasSelection: true, jobRunning: false))
        XCTAssertTrue(RetouchMenuState.contentAwareFillEnabled(layerKind: .smartObject, hasSelection: true, jobRunning: false))
        XCTAssertFalse(RetouchMenuState.contentAwareFillEnabled(layerKind: .pixel, hasSelection: false, jobRunning: false))
        XCTAssertFalse(RetouchMenuState.contentAwareFillEnabled(layerKind: .adjustment, hasSelection: true, jobRunning: false))
        XCTAssertFalse(RetouchMenuState.contentAwareFillEnabled(layerKind: nil, hasSelection: true, jobRunning: false))
        XCTAssertFalse(RetouchMenuState.contentAwareFillEnabled(layerKind: .pixel, hasSelection: true, jobRunning: true))
    }

    /// The menu item's inputs from a real document: a marquee on the pixel layer enables it; Deselect disables
    /// it; a cancelled job that is still stopping does not keep it disabled.
    @MainActor func testContentAwareFillMenuFollowsTheDocumentSelection() async throws {
        let backend = try StubDocumentEngine.shared.newDocument(width: 64, height: 48, depth: .u8, profile: nil)
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let tools = try XCTUnwrap(backend as? any DocumentToolsBackend)
        let layer = try XCTUnwrap(doc.layers.first { $0.kind == .pixel })
        doc.select(layer.id)
        let jobs = RetouchJobs()
        func enabled() -> Bool {
            RetouchMenuState.contentAwareFillEnabled(layerKind: doc.primary?.kind, hasSelection: doc.marquee != nil, jobRunning: jobs.isBusy)
        }
        XCTAssertFalse(enabled(), "no selection")
        _ = doc.run("Marquee") {
            try tools.selectMarquee(.rect, rect: CGRect(x: 10, y: 10, width: 12, height: 8), feather: 0, antialias: false, op: .replace)
        }
        XCTAssertNotNil(doc.marquee)
        XCTAssertTrue(enabled(), "a marquee on a pixel layer")
        let slow = SlowEngine()
        jobs.start("Removing…", operation: "Remove", { slow.run() }) { (_: RetouchJobs.End<Void>) in }
        XCTAssertFalse(enabled(), "an apply is running")
        jobs.cancel { }
        XCTAssertTrue(enabled(), "after Cancel, even while the engine is still stopping")
        slow.release()
        await waitFor("stopped") { jobs.abandoned == nil }
        _ = doc.run("Deselect") { try doc.backend.clearSelection() }
        XCTAssertFalse(enabled(), "deselected")
    }

    // MARK: Model downloads (B5-09b)

    private final class Downloads: ModelDownloadRequesting, @unchecked Sendable {
        private let lock = NSLock()
        private var _requests: [(id: String, version: String)] = []
        private var listeners: [String: ModelDownloadListener] = [:]
        var requests: [(id: String, version: String)] { lock.withLock { _requests } }
        func request(id: String, version: String, listener: ModelDownloadListener) throws {
            lock.withLock { _requests.append((id, version)); listeners[id] = listener }
        }
        func send(_ e: ModelDownloadEvent, to id: String) { lock.withLock { listeners[id] }?.onEvent(event: e) }
    }

    private func scratchDefaults() -> UserDefaults {
        let name = "tessera-retouch-tests-\(UUID().uuidString)"
        let d = UserDefaults(suiteName: name)!
        d.removePersistentDomain(forName: name)
        addTeardownBlock { UserDefaults(suiteName: name)?.removePersistentDomain(forName: name) }
        return d
    }

    private let lama = RetouchModelInfo(modelId: "remove/lama", usedBy: "Remove (LaMa)", installed: false,
                                        cachePath: "/support/models/cache/1f.onnx", sourceURL: "https://huggingface.co/x/lama.onnx",
                                        version: "c3c0c9e468934d62e79c329e35d82dd09ff8c444")

    @MainActor func testDownloadAllowedShowsProgressThenRunsTheOperation() async {
        let stub = Downloads()
        let d = RetouchModelDownloads(acquisition: ModelAcquisition(defaults: scratchDefaults()) { allow in
            XCTAssertTrue(allow); return stub
        })
        XCTAssertEqual(RetouchModelDownloads.modelId(for: .lama), "remove/lama")
        XCTAssertNil(RetouchModelDownloads.modelId(for: .auto), "Auto falls back to PatchMatch: nothing to download")
        XCTAssertEqual(RetouchModelDownloads.modelId(for: .colorize), "filters/ddcolor")
        XCTAssertEqual(RetouchModelDownloads.modelId(for: .jpegArtifactRemoval), "enhance/drunet-color")
        XCTAssertNil(RetouchModelDownloads.modelId(for: .skinSmoothing))
        XCTAssertEqual(d.phase(lama), .available(lama))
        XCTAssertTrue(stub.requests.isEmpty, "nothing downloads before it is asked for")

        var runs: [Result<Void, RetouchModelDownloadError>] = []
        XCTAssertEqual(d.request(lama, for: "Remove") { runs.append($0) }, .started)
        XCTAssertEqual(stub.requests.map(\.id), ["remove/lama"])
        XCTAssertEqual(stub.requests.first?.version, lama.version, "the engine's pinned version")
        XCTAssertEqual(d.waiting, ["remove/lama": "Remove"])
        stub.send(.queued, to: lama.modelId)
        stub.send(.downloading(bytes: 52, total: 208), to: lama.modelId)
        await waitFor("progress") { d.phase(lama).isDownloading && d.phase(lama).progress.fraction == 0.25 }
        XCTAssertEqual(d.line(lama)?.hasSuffix("Remove runs when it is ready"), true, d.line(lama) ?? "")
        XCTAssertTrue(runs.isEmpty, "not before the download completes")
        // A second request while downloading joins it (no second download).
        d.request(lama, for: "Remove") { runs.append($0) }
        XCTAssertEqual(stub.requests.count, 1)

        stub.send(.ready(path: lama.cachePath), to: lama.modelId)
        await waitFor("the operation") { runs.count == 2 }
        XCTAssertTrue(runs.allSatisfy { if case .success = $0 { true } else { false } })
        XCTAssertTrue(d.waiting.isEmpty)
        var installed = lama
        installed.installed = true
        XCTAssertEqual(d.phase(installed), .installed)
        XCTAssertEqual(d.request(installed, for: "Remove") { _ in XCTFail("ready: the caller runs it") }, .ready)
        XCTAssertEqual(stub.requests.count, 1)
    }

    @MainActor func testDownloadsOffNeverRequestsAndSaysSo() {
        let stub = Downloads()
        let acquisition = ModelAcquisition(defaults: scratchDefaults()) { _ in stub }
        acquisition.allowDownloads = false
        let d = RetouchModelDownloads(acquisition: acquisition)
        XCTAssertEqual(d.phase(lama), .downloadsOff(lama))
        XCTAssertTrue(d.line(lama)?.contains("model downloads are off (Settings ▸ AI)") ?? false)
        XCTAssertEqual(d.request(lama, for: "Remove") { _ in XCTFail("never runs") }, .downloadsOff)
        XCTAssertTrue(stub.requests.isEmpty, "never downloads silently")
        XCTAssertTrue(d.waiting.isEmpty)
        // Switching the setting on makes it available (the Settings ▸ AI toggle is the shared preference).
        acquisition.allowDownloads = true
        XCTAssertEqual(d.phase(lama), .available(lama))
    }

    @MainActor func testDownloadFailureIsReportedAndRetryRequestsAgain() async {
        let stub = Downloads()
        let d = RetouchModelDownloads(acquisition: ModelAcquisition(defaults: scratchDefaults()) { _ in stub })
        var result: Result<Void, RetouchModelDownloadError>?
        d.request(lama, for: "Colorize") { result = $0 }
        stub.send(.failed(reason: "HTTP 503"), to: lama.modelId)
        await waitFor("the failure") { result != nil }
        guard case .failure(let e)? = result else { return XCTFail("\(String(describing: result))") }
        XCTAssertEqual(e.reason, "HTTP 503")
        XCTAssertEqual(d.phase(lama), .failed(lama, "HTTP 503"))
        XCTAssertEqual(d.phase(lama).progress, .failed(reason: "HTTP 503"))
        XCTAssertTrue(d.waiting.isEmpty)
        // Retry (a new request) downloads again.
        result = nil
        XCTAssertEqual(d.request(lama, for: "Colorize") { result = $0 }, .started)
        XCTAssertEqual(stub.requests.count, 2)
        stub.send(.ready(path: "/c"), to: lama.modelId)
        await waitFor("ready") { result != nil }
        if case .success? = result {} else { XCTFail("\(String(describing: result))") }
        // Downloaded, yet the engine does not report it installed: shown as a problem, not as installed.
        if case .failed(_, let why) = d.phase(lama) { XCTAssertTrue(why.contains(lama.cachePath)) } else { XCTFail("\(d.phase(lama))") }
    }

    @MainActor func testForgettingAWaitingOperationKeepsTheDownload() async {
        let stub = Downloads()
        let d = RetouchModelDownloads(acquisition: ModelAcquisition(defaults: scratchDefaults()) { _ in stub })
        d.request(lama, for: "Remove") { _ in XCTFail("forgotten") }
        d.forgetWaiting(lama.modelId)
        XCTAssertTrue(d.waiting.isEmpty)
        stub.send(.ready(path: "/c"), to: lama.modelId)
        await waitFor("ready") { d.acquisition.state(RetouchModelDownloads.requirement(lama)).isReady }
    }
}
