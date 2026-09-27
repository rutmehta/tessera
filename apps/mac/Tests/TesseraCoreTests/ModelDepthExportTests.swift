import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

// MARK: - Stubs

/// `ModelDownloads` stand-in: records requests and replays a scripted event sequence per model id
/// on the calling thread (the relay hops to the main actor, like the engine's worker thread).
final class StubDownloads: ModelDownloadRequesting, @unchecked Sendable {
    private let lock = NSLock()
    private var _requests: [(id: String, version: String)] = []
    private var listeners: [String: ModelDownloadListener] = [:]
    var script: [String: [ModelDownloadEvent]] = [:]

    var requests: [(id: String, version: String)] { lock.withLock { _requests } }

    func request(id: String, version: String, listener: ModelDownloadListener) throws {
        lock.withLock {
            _requests.append((id, version))
            listeners[id] = listener
        }
        for e in script[id] ?? [] { listener.onEvent(event: e) }
    }

    /// Sends a later event for `id` (a download still in flight).
    func send(_ event: ModelDownloadEvent, to id: String) {
        lock.withLock { listeners[id] }?.onEvent(event: event)
    }
}

/// Develop session stand-in for the depth / uncorrected calls.
final class StubRenderModes: DevelopRenderModes, @unchecked Sendable {
    private let lock = NSLock()
    private var _calls: [String] = []
    var bins: [UInt64] = (0..<256).map { UInt64($0 % 16) }
    var subject: [Float] = [0.3, 0.45]
    var failure: String?

    var calls: [String] { lock.withLock { _calls } }
    private func record(_ c: String) throws {
        lock.withLock { _calls.append(c) }
        if let failure { throw BridgeError.Failure(message: failure) }
    }

    func depthHistogram() throws -> [UInt64] { try record("histogram"); return bins }
    func setRenderDepthVisualisation(enabled: Bool) throws { try record("visualize:\(enabled)") }
    func focusLensBlurOnSubject() throws -> [Float] { try record("subject"); return subject }
    func setRenderUncorrected(enabled: Bool) throws { try record("uncorrected:\(enabled)") }
    func refresh() throws { try record("refresh") }
}

@MainActor
private func waitUntil(_ what: String, timeout: TimeInterval = 5, _ condition: () -> Bool) async {
    let deadline = Date().addingTimeInterval(timeout)
    while !condition() {
        if Date() > deadline { XCTFail("timed out waiting for \(what)"); return }
        try? await Task.sleep(for: .milliseconds(5))
    }
}

private func scratchDefaults() -> UserDefaults {
    let name = "tessera-tests-\(UUID().uuidString)"
    let d = UserDefaults(suiteName: name)!
    d.removePersistentDomain(forName: name)
    return d
}

// MARK: - Model acquisition (download progress states)

@MainActor
final class ModelAcquisitionTests: XCTestCase {
    func testStatesFollowTheEngineEvents() {
        XCTAssertEqual(ModelAcquisitionState(.queued), .queued)
        XCTAssertEqual(ModelAcquisitionState(.downloading(bytes: 5, total: 10)), .downloading(bytes: 5, total: 10))
        XCTAssertEqual(ModelAcquisitionState(.ready(path: "/m")), .ready(path: "/m"))
        XCTAssertEqual(ModelAcquisitionState(.failed(reason: "offline")).failure, "offline")
        XCTAssertEqual(ModelAcquisitionState.downloading(bytes: 5, total: 10).fraction, 0.5)
        XCTAssertNil(ModelAcquisitionState.downloading(bytes: 5, total: nil).fraction, "indeterminate without a size")
        XCTAssertNil(ModelAcquisitionState.downloading(bytes: 5, total: 0).fraction)
        XCTAssertTrue(ModelAcquisitionState.queued.isBusy)
        XCTAssertFalse(ModelAcquisitionState.ready(path: "").isBusy)
        XCTAssertEqual(ModelAcquisitionState.queued.label, "Queued…")
        XCTAssertTrue(ModelAcquisitionState.downloading(bytes: 1_000_000, total: 2_000_000).label.hasPrefix("Downloading "))
        XCTAssertTrue(ModelAcquisitionState.downloading(bytes: 1_000_000, total: 2_000_000).label.contains(" of "))
        XCTAssertEqual(ModelAcquisitionState.failed(reason: "x").label, "Failed: x")
        // Several models: failure wins, then busy (bytes summed), then ready.
        XCTAssertEqual(ModelAcquisitionState.combined([.ready(path: "a"), .failed(reason: "b"), .queued]), .failed(reason: "b"))
        XCTAssertEqual(ModelAcquisitionState.combined([.downloading(bytes: 1, total: 4), .downloading(bytes: 2, total: 6), .ready(path: "")]),
                       .downloading(bytes: 3, total: 10))
        XCTAssertEqual(ModelAcquisitionState.combined([.downloading(bytes: 1, total: nil), .downloading(bytes: 2, total: 6)]),
                       .downloading(bytes: 3, total: nil))
        XCTAssertEqual(ModelAcquisitionState.combined([.queued, .ready(path: "")]), .queued)
        XCTAssertTrue(ModelAcquisitionState.combined([.ready(path: "a"), .ready(path: "b")]).isReady)
        XCTAssertEqual(ModelAcquisitionState.combined([.ready(path: "a"), .idle]), .idle)
        XCTAssertEqual(ModelRequirement.lensBlurSubject.map(\.id), ["depth/anything-v2-small", "segment/u2net", "segment/sam-encoder", "segment/sam-decoder"])
    }

    func testDownloadProgressQueuedBytesReady() async {
        let stub = StubDownloads()
        var opened: [Bool] = []
        let models = ModelAcquisition(defaults: scratchDefaults()) { allow in opened.append(allow); return stub }
        XCTAssertTrue(models.allowDownloads, "Allow model downloads defaults on")
        XCTAssertEqual(models.state(.depth), .idle)
        var finished: ModelAcquisitionState?
        models.acquire([.depth]) { finished = $0 }
        XCTAssertEqual(models.state(.depth), .queued)
        XCTAssertEqual(opened, [true])
        XCTAssertEqual(stub.requests.map(\.id), ["depth/anything-v2-small"])
        XCTAssertEqual(stub.requests.first?.version, ModelRequirement.depth.version)

        stub.send(.queued, to: ModelRequirement.depth.id)
        stub.send(.downloading(bytes: 10, total: 40), to: ModelRequirement.depth.id)
        await waitUntil("bytes") { models.state(.depth) == .downloading(bytes: 10, total: 40) }
        XCTAssertEqual(models.state(.depth).fraction, 0.25)
        XCTAssertNil(finished, "not terminal yet")
        // A second acquire while running does not request again.
        models.acquire([.depth])
        XCTAssertEqual(stub.requests.count, 1)

        stub.send(.ready(path: "/cache/depth.onnx"), to: ModelRequirement.depth.id)
        await waitUntil("ready") { finished != nil }
        XCTAssertEqual(finished, .ready(path: "/cache/depth.onnx"))
        XCTAssertTrue(models.state(.depth).isReady)
        // Late progress never downgrades a terminal state; a ready model is not requested again.
        models.receive(.downloading(bytes: 1, total: 2), for: .depth)
        XCTAssertTrue(models.state(.depth).isReady)
        let again = await models.ensure([.depth])
        XCTAssertTrue(again.isReady)
        XCTAssertEqual(stub.requests.count, 1)
        XCTAssertEqual(opened, [true], "one downloader")
    }

    func testFailureCarriesTheReasonAndRetryRequestsAgain() async {
        let stub = StubDownloads()
        stub.script[AIDenoise.modelID] = [.queued, .failed(reason: "local artifact missing")]
        let models = ModelAcquisition(defaults: scratchDefaults()) { _ in stub }
        let s = await models.ensure([.cfaDenoise])
        XCTAssertEqual(s, .failed(reason: "local artifact missing"))
        XCTAssertEqual(models.state(.cfaDenoise).label, "Failed: local artifact missing")
        // A failed model is not retried implicitly; reset (Retry) requests it again.
        models.acquire([.cfaDenoise])
        XCTAssertEqual(stub.requests.count, 1)
        stub.script[AIDenoise.modelID] = [.queued, .downloading(bytes: 3, total: nil), .ready(path: "/c")]
        models.reset([.cfaDenoise])
        XCTAssertEqual(models.state(.cfaDenoise), .idle)
        let retried = await models.ensure([.cfaDenoise])
        XCTAssertTrue(retried.isReady)
        XCTAssertEqual(stub.requests.count, 2)
    }

    func testAllowDownloadsSettingPersistsAndReopensTheDownloader() async {
        let defaults = scratchDefaults()
        let stub = StubDownloads()
        stub.script[ModelRequirement.depth.id] = [.queued, .failed(reason: "model is not cached")]
        var opened: [Bool] = []
        let models = ModelAcquisition(defaults: defaults) { allow in opened.append(allow); return stub }
        models.allowDownloads = false
        XCTAssertEqual(defaults.object(forKey: ModelAcquisition.allowDownloadsKey) as? Bool, false)
        let s = await models.ensure([.depth])
        XCTAssertEqual(opened, [false], "the engine gets allow_downloads = false")
        XCTAssertEqual(s.failure, "model is not cached (model downloads are off in Settings ▸ AI)")
        // Turning downloads back on forgets the failure and reopens with allow_downloads = true.
        stub.script[ModelRequirement.depth.id] = [.ready(path: "/c")]
        models.allowDownloads = true
        XCTAssertEqual(models.state(.depth), .idle)
        let ok = await models.ensure([.depth])
        XCTAssertTrue(ok.isReady)
        XCTAssertEqual(opened, [false, true])
        XCTAssertTrue(ModelAcquisition(defaults: defaults) { _ in stub }.allowDownloads, "persisted as on again")
    }

    func testDownloaderThatCannotOpenFailsWithTheReason() async {
        let models = ModelAcquisition(defaults: scratchDefaults()) { _ in throw ModelAcquisitionError.noManifest("/x/models.toml") }
        let s = await models.ensure(ModelRequirement.lensBlurSubject)
        XCTAssertEqual(s.failure, "No model catalog at /x/models.toml (open a photo in Develop first)")
        XCTAssertTrue(ModelRequirement.lensBlurSubject.allSatisfy { models.state($0).failure != nil })
    }

    func testStandardPathsUseTheSupportModelsFolder() async throws {
        let support = FileManager.default.temporaryDirectory.appendingPathComponent("tessera-models-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: support) }
        let models = ModelAcquisition.standard(support: support, environment: [:], defaults: scratchDefaults())
        let s = await models.ensure([.depth])
        XCTAssertEqual(s.failure, ModelAcquisitionError.noManifest(support.appendingPathComponent("models/models.toml").path).errorDescription)
        XCTAssertTrue(FileManager.default.fileExists(atPath: support.appendingPathComponent("models/cache").path), "cache folder created")
    }
}

// MARK: - Lens Blur depth: histogram binding, Visualize Depth, Subject

@MainActor
final class LensBlurDepthModelTests: XCTestCase {
    private func make(depth: [ModelDownloadEvent] = [.queued, .ready(path: "/c/depth")],
                      segmentation: [ModelDownloadEvent] = [.ready(path: "/c/seg")]) -> (LensBlurDepthModel, StubDownloads) {
        let stub = StubDownloads()
        stub.script[ModelRequirement.depth.id] = depth
        for r in ModelRequirement.subjectSegmentation { stub.script[r.id] = segmentation }
        return (LensBlurDepthModel(models: ModelAcquisition(defaults: scratchDefaults()) { _ in stub }), stub)
    }

    func testHistogramBindsTheEngineBinsNormalised() async {
        let (depth, stub) = make()
        let session = StubRenderModes()
        session.bins = (0..<256).map { $0 == 10 ? 400 : UInt64($0) }
        depth.bind(session)
        XCTAssertNil(depth.histogram)
        await depth.refreshHistogram()
        XCTAssertEqual(stub.requests.map(\.id), [ModelRequirement.depth.id], "weights first")
        XCTAssertEqual(session.calls, ["histogram"])
        let h = try? XCTUnwrap(depth.histogram)
        XCTAssertEqual(h?.count, 256, "256 near → far bins")
        XCTAssertEqual(h?[10], 1)
        XCTAssertEqual(h?[200] ?? 0, 0.5, accuracy: 1e-12)
        XCTAssertEqual(h?[0], 0)
        XCTAssertNil(depth.busy)
        XCTAssertNil(depth.error)
        XCTAssertEqual(LensBlurDepthModel.normalize([0, 0]), [0, 0])
        // Another photo: the histogram is cleared until estimated again.
        depth.bind(StubRenderModes())
        XCTAssertNil(depth.histogram)
    }

    func testMissingWeightsAreAnInlineErrorAndNoEngineCall() async {
        let (depth, _) = make(depth: [.queued, .failed(reason: "depth model is not cached")])
        let session = StubRenderModes()
        depth.bind(session)
        await depth.refreshHistogram()
        XCTAssertEqual(depth.error, "depth model is not cached")
        XCTAssertNil(depth.histogram)
        await depth.setVisualize(true)
        XCTAssertFalse(depth.visualize)
        XCTAssertEqual(depth.weights.failure, "depth model is not cached")
        XCTAssertTrue(session.calls.isEmpty, "\(session.calls)")
        // Engine failures (e.g. an unsupported process version) are shown too.
        let (ok, _) = make()
        let failing = StubRenderModes()
        failing.failure = "depth input requires the native process"
        ok.bind(failing)
        await ok.refreshHistogram()
        XCTAssertEqual(ok.error, "depth input requires the native process")
    }

    func testVisualizeToggleDrivesTheSessionOverlay() async {
        let (depth, stub) = make()
        let session = StubRenderModes()
        depth.bind(session)
        await depth.setVisualize(true)
        XCTAssertTrue(depth.visualize)
        XCTAssertEqual(stub.requests.count, 1)
        await depth.setVisualize(false)
        XCTAssertFalse(depth.visualize)
        await depth.setVisualize(true)
        XCTAssertEqual(session.calls, ["visualize:true", "visualize:false", "visualize:true"])
        XCTAssertEqual(stub.requests.count, 1, "ready weights are not requested again")
        // Switching photos turns the old session's overlay off.
        let next = StubRenderModes()
        depth.bind(next)
        XCTAssertEqual(session.calls.last, "visualize:false")
        XCTAssertFalse(depth.visualize)
        XCTAssertTrue(next.calls.isEmpty)
    }

    func testSubjectAcquiresSegmentationAndAppliesTheEngineRange() async {
        let (depth, stub) = make()
        let session = StubRenderModes()
        session.subject = [0.3, 0.45]
        depth.bind(session)
        var applied: [FocalRange] = []
        let r = await depth.focusOnSubject { applied.append($0) }
        let expected = FocalRange(near: Double(Float(0.3)), far: Double(Float(0.45)))
        XCTAssertEqual(r, expected)
        XCTAssertEqual(applied, [expected], "applied once (one history step in the panel)")
        XCTAssertEqual(Set(stub.requests.map(\.id)), Set(ModelRequirement.lensBlurSubject.map(\.id)))
        XCTAssertEqual(session.calls, ["subject", "histogram"], "subject also refreshes the histogram")
        XCTAssertNotNil(depth.histogram)
        XCTAssertNil(depth.busy)

        // Missing segmentation weights: inline error, the focal range is untouched.
        let (blocked, _) = make(segmentation: [.failed(reason: "segmentation weights missing")])
        let s2 = StubRenderModes()
        blocked.bind(s2)
        var calls = 0
        let none = await blocked.focusOnSubject { _ in calls += 1 }
        XCTAssertNil(none)
        XCTAssertEqual(calls, 0)
        XCTAssertEqual(blocked.error, "segmentation weights missing")
        XCTAssertTrue(s2.calls.isEmpty)

        // The engine finding no subject is an error, not an applied range.
        let (noSubject, _) = make()
        let s3 = StubRenderModes()
        s3.failure = "no subject found"
        noSubject.bind(s3)
        let failed = await noSubject.focusOnSubject { _ in calls += 1 }
        XCTAssertNil(failed)
        XCTAssertEqual(calls, 0)
        XCTAssertEqual(noSubject.error, "no subject found")
    }
}

// MARK: - Guided Upright: uncorrected view on enter / exit

@MainActor
final class UncorrectedPlacementTests: XCTestCase {
    func testGuidedEnterAndExitToggleTheUncorrectedView() {
        let p = UncorrectedPlacement()
        let a = StubRenderModes(), b = StubRenderModes()
        XCTAssertFalse(p.isActive)
        p.enter(a)
        XCTAssertTrue(p.isActive)
        p.enter(a)
        XCTAssertEqual(a.calls, ["uncorrected:true"], "entering twice is one call")
        p.exit()
        XCTAssertEqual(a.calls, ["uncorrected:true", "uncorrected:false"])
        XCTAssertFalse(p.isActive)
        p.exit()
        XCTAssertEqual(a.calls.count, 2, "exit is idempotent")
        // Moving to another session restores the first.
        p.enter(a)
        p.enter(b)
        XCTAssertEqual(a.calls.suffix(2), ["uncorrected:true", "uncorrected:false"])
        XCTAssertEqual(b.calls, ["uncorrected:true"])
        // A closed session is forgotten without a call.
        p.abandon()
        p.exit()
        XCTAssertEqual(b.calls, ["uncorrected:true"])
        // Failures are reported, and the tool does not think the view is uncorrected.
        var failures: [String] = []
        p.onFailure = { failures.append($0) }
        let broken = StubRenderModes()
        broken.failure = "session closed"
        p.enter(broken)
        XCTAssertFalse(p.isActive)
        XCTAssertEqual(failures, ["session closed"])
    }
}

// MARK: - Export warnings

final class ExportWarningsTests: XCTestCase {
    private func item(_ name: String, _ path: String?, error: String? = nil) -> ExportItemResult {
        ExportItemResult(imageId: name, name: name, outputPath: path, error: error)
    }

    func testWarningFilesAreReadAndSurfacedInTheToast() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("tessera-warn-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let a = dir.appendingPathComponent("a.jpg").path, b = dir.appendingPathComponent("b.jpg").path
        try "Lens Blur skipped: depth model is not cached\n\n".write(toFile: ExportWarnings.path(for: a), atomically: true, encoding: .utf8)
        XCTAssertEqual(ExportWarnings.path(for: a), a + ".tessera-warnings.txt")
        let report = ExportReport(destination: dir.path, items: [item("A.ARW", a), item("B.ARW", b), item("C.ARW", nil, error: "decode failed")],
                                  exported: 2, failed: 1, cancelled: false, seconds: 1.25)
        let w = ExportWarnings.read(report)
        XCTAssertEqual(w.items, [.init(name: "A.ARW", warnings: ["Lens Blur skipped: depth model is not cached"])])
        XCTAssertEqual(w.lines, ["A.ARW: Lens Blur skipped: depth model is not cached"])
        let toast = ExportWarnings.toastLines(report, warnings: w)
        XCTAssertEqual(toast.headline, "Exported 2 photos to \(dir.lastPathComponent); 1 failed; 1 with warnings")
        XCTAssertEqual(toast.details, ["C.ARW: decode failed", "A.ARW: Lens Blur skipped: depth model is not cached"])

        let clean = ExportReport(destination: dir.path, items: [item("B.ARW", b)], exported: 1, failed: 0, cancelled: false, seconds: 0.5)
        XCTAssertTrue(ExportWarnings.read(clean).isEmpty)
        XCTAssertEqual(ExportWarnings.toastLines(clean, warnings: ExportWarnings.read(clean)).headline,
                       "Exported 1 photo to \(dir.lastPathComponent) in 0.5 s")
        XCTAssertEqual(ExportWarnings.toastLines(ExportReport(destination: dir.path, items: [], exported: 0, failed: 0, cancelled: false, seconds: 0),
                                                 warnings: ExportWarnings()).headline, "Nothing was exported")
    }
}

/// The real engine: Lens Blur on a photo whose depth weights are not cached exports anyway, with
/// the engine's warning beside the file; `ExportWarnings` reads it for the completion toast.
@MainActor
final class LensBlurExportWarningTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    func testLensBlurWithoutDepthWeightsExportsWithAWarning() async throws {
        try XCTSkipIf(ProcessInfo.processInfo.environment["TESSERA_DEPTH_MODELS"] != nil, "depth weights supplied")
        let fixtures = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        let raw = try XCTUnwrap(try FileManager.default.contentsOfDirectory(at: fixtures, includingPropertiesForKeys: nil)
            .first { $0.pathExtension.lowercased() == "arw" }, "fetch fixtures/raw first")
        let temp = root.appendingPathComponent("build/lensblur-warning-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("raw")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        try FileManager.default.copyItem(at: raw, to: folder.appendingPathComponent(raw.lastPathComponent))
        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let item = try XCTUnwrap(library.items.first)
        let ref = try XCTUnwrap(item.engineImage)

        let c = try await DevelopController.open(ref, itemID: item.id)
        c.apply(patch: LensBlurControls.applyPatch(true), interactive: false)
        XCTAssertTrue(c.commit(label: "Lens Blur On"))
        await c.close()

        let out = temp.appendingPathComponent("out")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let options = "{\"format\":\"png\",\"resize\":{\"mode\":\"long_edge\",\"long_edge\":320},\"metadata\":\"none\",\"destination\":\"" + out.path + "\"}"
        let engine = ref.engine
        let target = ExportTarget.images(imageIds: [ref.imageID])
        let report: ExportReport = try await Task.detached {
            try engine.exportBatch(target: target, settingsJson: options, listener: nil, cancel: nil)
        }.value
        XCTAssertEqual(report.exported, 1, "\(report.items.map { $0.error ?? "" })")
        let warnings = ExportWarnings.read(report)
        XCTAssertTrue(warnings.lines.contains { $0.contains("Lens Blur skipped") }, "\(warnings)")
        let toast = ExportWarnings.toastLines(report, warnings: warnings)
        XCTAssertTrue(toast.headline.hasSuffix("; 1 with warnings"), toast.headline)
        print("lensblur-export-warning: \(toast.details)")
    }
}
