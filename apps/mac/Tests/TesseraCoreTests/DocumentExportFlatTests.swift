import AppKit
import Foundation
import ImageIO
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Export Flat in the background (WP B5-15, perf audit P16): the engine backend's snapshot export (progress,
/// cancel, the same file as the synchronous call), and the workspace's task list (Cancel keeps the destination,
/// closing the document does not stop the export, the stub runs off the main thread too).
@MainActor
final class DocumentExportFlatTests: XCTestCase {
    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("flat-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func engineDocument(_ dir: URL, width: UInt32 = 1600, height: UInt32 = 1200) throws -> EngineDocumentBackend {
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: width, height: height, depth: .u8, profile: nil)
        let backend = try XCTUnwrap(doc as? EngineDocumentBackend)
        _ = try backend.addLayer(kind: .fill(json: #"{"kind":"solid","color":[0.9,0.4,0.1]}"#), name: "fill", parent: nil, index: nil)
        return backend
    }

    private func pixels(_ path: String) throws -> Data {
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(src, 0, nil))
        return try XCTUnwrap(image.dataProvider?.data as Data?)
    }

    private final class Log: @unchecked Sendable {
        let lock = NSLock()
        var values: [(Double, String)] = []
        func add(_ f: Double, _ p: String) { lock.withLock { values.append((f, p)) } }
    }

    func testEngineExportRunsFromASnapshotWithProgress() async throws {
        let dir = try temp()
        let backend = try engineDocument(dir)
        let sync = dir.appendingPathComponent("sync.png").path
        try backend.exportFlat(path: sync, format: .png, quality: 90, color: .srgb)
        let path = dir.appendingPathComponent("bg.png").path
        let job = try backend.beginExportFlat(path: path, format: .png, quality: 90, color: .srgb)
        // Edits after `begin` are not exported.
        let top = try XCTUnwrap(try backend.layers().first?.id)
        _ = try backend.setVisible(id: top, visible: false)
        let log = Log()
        try await Task.detached { try job.run { log.add($0, $1) } }.value
        // Same pixels (file bytes differ in the built-in profile's creation time).
        XCTAssertEqual(try pixels(path), try pixels(sync))
        let values = log.lock.withLock { log.values }
        XCTAssertEqual(values.last?.0, 1)
        XCTAssertEqual(values.last?.1, "Done")
        XCTAssertEqual(values.map(\.0), values.map(\.0).sorted())
        XCTAssertThrowsError(try backend.beginExportFlat(path: path, format: .jpeg, quality: 0, color: .srgb))
    }

    func testCancelledEngineExportKeepsTheDestination() async throws {
        let dir = try temp()
        let backend = try engineDocument(dir, width: 4000, height: 3000)
        let url = dir.appendingPathComponent("kept.tif")
        try Data("previous".utf8).write(to: url)
        let job = try backend.beginExportFlat(path: url.path, format: .tiff, quality: 90, color: .displayP3)
        do {
            try await Task.detached { try job.run { f, _ in if f >= 0.2 { job.cancel() } } }.value
            XCTFail("a cancelled export throws")
        } catch {}
        XCTAssertTrue(job.isCancelled)
        XCTAssertEqual(try Data(contentsOf: url), Data("previous".utf8))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted(), ["kept.tif", "support"])
    }

    func testWorkspaceSnapshotExcludesEditCommittedAfterConfirm() async throws {
        let dir = try temp()
        let backend = try engineDocument(dir, width: 32, height: 24)
        let ws = DocumentWorkspace()
        try ws.install(backend)
        let doc = try XCTUnwrap(ws.current)
        defer { ws.discard(doc) }
        let before = dir.appendingPathComponent("before.png").path
        let edited = dir.appendingPathComponent("edited.png").path
        let output = dir.appendingPathComponent("confirmed.png")
        try backend.exportFlat(path: before, format: .png, quality: 90, color: .srgb)
        let worker = DispatchSemaphore(value: 0)
        ws.exportWorkerWillRun = { worker.wait() }
        let trace = PerformanceTrace(enabled: true)
        ws.exportTrace = trace
        var outcome: FlatExportTask.Outcome?
        do {
            defer { worker.signal() }
            _ = try XCTUnwrap(ws.startExportFlat(doc, ExportFlatSettings(format: .png, quality: 90, color: .srgb),
                                               to: output) { outcome = $0 })
            // The worker cannot acquire a snapshot before this committed edit.
            let snapshots = trace.snapshot().events.filter { $0.name == "export_flat_snapshot_start" }
            XCTAssertEqual(snapshots.count, 1, "Confirm must acquire the snapshot before returning")
            XCTAssertTrue(snapshots.allSatisfy(\.mainThread))
            let layer = try XCTUnwrap(try backend.layers().first?.id)
            _ = try backend.setVisible(id: layer, visible: false)
            try backend.exportFlat(path: edited, format: .png, quality: 90, color: .srgb)
        }
        let finished = await waitFor { outcome != nil }
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .exported)
        XCTAssertNotEqual(try pixels(before), try pixels(edited))
        XCTAssertEqual(try pixels(output.path), try pixels(before))
        let snapshot = try XCTUnwrap(trace.snapshot().events.first { $0.name == "export_flat_snapshot_end" && $0.durationMs != nil })
        let milliseconds = try XCTUnwrap(snapshot.durationMs)
        print("B5-48 confirm snapshot: \(milliseconds * 1000) us (main=\(snapshot.mainThread))")
        if ProcessInfo.processInfo.environment["TESSERA_FILTER_PERF"] != nil {
            XCTAssertLessThan(milliseconds, 20, "Confirm snapshot must remain a short publication read")
        }
    }

    private final class ProgressScheduler: @unchecked Sendable {
        private let lock = NSLock()
        private var time: TimeInterval = 0
        private var pending: [(TimeInterval, @MainActor @Sendable () -> Void)] = []
        var now: TimeInterval { lock.withLock { time } }
        func enqueue(_ delay: TimeInterval, _ action: @escaping @MainActor @Sendable () -> Void) {
            lock.withLock { pending.append((time + delay, action)) }
        }
        @MainActor func advance(to instant: TimeInterval) -> Int {
            let ready = lock.withLock {
                time = instant
                let ready = pending.filter { $0.0 <= instant }
                pending.removeAll { $0.0 <= instant }
                return ready
            }
            for (_, action) in ready { action() }
            return ready.count
        }
    }

    func testDuplicateProgressWakesMainAtMostOncePerInterval() {
        let scheduler = ProgressScheduler()
        var publications = 0
        let publisher = FlatExportProgressPublisher(now: { scheduler.now },
            schedule: { scheduler.enqueue($0, $1) }) { _, _ in publications += 1 }
        publisher.receive(0.5, "Rendering")
        XCTAssertEqual(scheduler.advance(to: 0), 1)
        // Start after the previous publication's throttle interval expired. Drain
        // every callback, so a pending callback cannot hide repeated main wakes.
        var wakes = 0
        for index in 0..<100 {
            let time = 1 + Double(index) * 0.0009
            wakes += scheduler.advance(to: time)
            publisher.receive(0.5001, "Rendering") // same displayed percentage
            wakes += scheduler.advance(to: time)
        }
        XCTAssertLessThanOrEqual(wakes, 1)
        XCTAssertEqual(publications, 1)
        publisher.receive(0.6, "Encoding")
        _ = scheduler.advance(to: 2)
        XCTAssertEqual(publications, 2)
        publisher.finish()
        publisher.receive(1, "Done")
        _ = scheduler.advance(to: 3)
        XCTAssertEqual(publications, 2)
    }

    func testSnapshotExportSurvivesCloseBeforeWorkerStarts() async throws {
        let dir = try temp()
        let backend = try engineDocument(dir)
        let output = dir.appendingPathComponent("reserved.png")
        let request = try backend.beginExportFlat(path: output.path, format: .png, quality: 90, color: .srgb)
        backend.close()
        // Deliberately start the worker only after close, avoiding a scheduling-dependent test.
        try await Task.detached { try request.run { _, _ in } }.value
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(output as CFURL, nil))
        XCTAssertEqual(CGImageSourceCreateImageAtIndex(source, 0, nil)?.width, 1600)
        XCTAssertThrowsError(try backend.beginExportFlat(path: output.path, format: .png, quality: 90, color: .srgb))
    }

    func testSnapshotExportCancelBeforeWorkerStartsKeepsDestination() async throws {
        let dir = try temp()
        let backend = try engineDocument(dir)
        let output = dir.appendingPathComponent("kept.png")
        let original = Data("previous".utf8)
        try original.write(to: output)
        let request = try backend.beginExportFlat(path: output.path, format: .png, quality: 90, color: .srgb)
        request.cancel()
        backend.close()
        let job = request
        XCTAssertTrue(job.isCancelled)
        do {
            try await Task.detached { try job.run { _, _ in } }.value
            XCTFail("Cancel before worker starts must prevent the write")
        } catch {}
        XCTAssertEqual(try Data(contentsOf: output), original)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted(), ["kept.png", "support"])
    }

    private func waitFor(_ timeout: Double = 60, _ condition: () -> Bool) async -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > end { return false }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return true
    }

    func testWorkspaceExportIsABackgroundTaskThatSurvivesClose() async throws {
        let dir = try temp()
        let ws = DocumentWorkspace()
        try ws.install(try engineDocument(dir))
        let doc = try XCTUnwrap(ws.current)
        let url = dir.appendingPathComponent("out.jpg")
        var outcome: FlatExportTask.Outcome?
        let task = try XCTUnwrap(ws.startExportFlat(doc, ExportFlatSettings(format: .jpeg, quality: 80, color: .srgb), to: url) {
            outcome = $0
        })
        // Returned before the export finished; it is listed until then.
        XCTAssertTrue(ws.flatExports.contains { $0 === task })
        ws.discard(doc)
        let finished = await waitFor { outcome != nil }
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .exported)
        XCTAssertTrue(ws.flatExports.isEmpty)
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        XCTAssertEqual(CGImageSourceCreateImageAtIndex(src, 0, nil)?.width, 1600)
    }

    func testWorkspaceCancelKeepsTheDestination() async throws {
        let dir = try temp()
        let ws = DocumentWorkspace()
        try ws.install(try engineDocument(dir, width: 6000, height: 4000))
        let doc = try XCTUnwrap(ws.current)
        let url = dir.appendingPathComponent("kept.png")
        try Data("previous".utf8).write(to: url)
        var outcome: FlatExportTask.Outcome?
        let task = try XCTUnwrap(ws.startExportFlat(doc, ExportFlatSettings(format: .png, quality: 90, color: .srgb), to: url) {
            outcome = $0
        })
        ws.cancelExportFlat(task)
        XCTAssertTrue(task.cancelling)
        let finished = await waitFor { outcome != nil }
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .cancelled)
        XCTAssertEqual(try Data(contentsOf: url), Data("previous".utf8))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted(), ["kept.png", "support"])
    }

    /// Blocks the main thread (so the export's completion cannot be delivered) until `url` exists.
    private func blockMainUntilWritten(_ url: URL, timeout: Double = 60) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while !FileManager.default.fileExists(atPath: url.path) {
            if Date() > end { return false }
            Thread.sleep(forTimeInterval: 0.005)
        }
        return true
    }

    /// A Cancel that arrives after the last checkpoint, once the file is already in place, must report the
    /// export as done: the destination was written, so "cancelled" would be wrong (A review of B5-15).
    func testCancelAfterTheFileIsWrittenReportsExported() async throws {
        let dir = try temp()
        let ws = DocumentWorkspace()
        try ws.install(try engineDocument(dir))
        let doc = try XCTUnwrap(ws.current)
        let url = dir.appendingPathComponent("late.png")
        var outcome: FlatExportTask.Outcome?
        let task = try XCTUnwrap(ws.startExportFlat(doc, ExportFlatSettings(format: .png, quality: 90, color: .srgb), to: url) {
            outcome = $0
        })
        XCTAssertTrue(blockMainUntilWritten(url))
        ws.cancelExportFlat(task)
        XCTAssertTrue(task.cancelling)
        let finished = await waitFor { outcome != nil }
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .exported)
        XCTAssertTrue(ws.flatExports.isEmpty)
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        XCTAssertEqual(CGImageSourceCreateImageAtIndex(src, 0, nil)?.width, 1600)
    }

    func testStubCancelAfterTheFileIsWrittenReportsExported() async throws {
        let dir = try temp()
        let ws = DocumentWorkspace()
        try ws.install(StubDocumentBackend(sampleWidth: 300, height: 200))
        let doc = try XCTUnwrap(ws.current)
        let url = dir.appendingPathComponent("stub-late.png")
        var outcome: FlatExportTask.Outcome?
        let task = try XCTUnwrap(ws.startExportFlat(doc, ExportFlatSettings(format: .png, quality: 90, color: .srgb), to: url) {
            outcome = $0
        })
        XCTAssertTrue(blockMainUntilWritten(url))
        ws.cancelExportFlat(task)
        let finished = await waitFor { outcome != nil }
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .exported)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
    }

    func testStubBackendExportsOffTheMainThread() async throws {
        let dir = try temp()
        let ws = DocumentWorkspace()
        try ws.install(StubDocumentBackend(sampleWidth: 300, height: 200))
        let doc = try XCTUnwrap(ws.current)
        let url = dir.appendingPathComponent("stub.png")
        var outcome: FlatExportTask.Outcome?
        XCTAssertNotNil(ws.startExportFlat(doc, ExportFlatSettings(format: .png, quality: 90, color: .srgb), to: url) { outcome = $0 })
        let finished = await waitFor { outcome != nil }
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .exported)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
    }

    /// Progress must not resize the canvas (and cause a new render + whole-window SwiftUI layout)
    /// when it appears or disappears. The window is never ordered or activated.
    func testExportProgressPreservesDocumentLayoutAndRunsWorkOffMain() async throws {
        _ = NSApplication.shared
        let dir = try temp()
        let ws = DocumentWorkspace()
        try ws.install(try engineDocument(dir, width: 1600, height: 1200))
        let window = NSWindow(contentRect: NSRect(x: 40, y: 40, width: 1000, height: 700),
                              styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        ws.exportWindow = window
        let trace = PerformanceTrace(enabled: true)
        ws.exportTrace = trace
        window.contentView?.layoutSubtreeIfNeeded()
        let before = window.contentLayoutRect
        var outcome: FlatExportTask.Outcome?
        let task = try XCTUnwrap(ws.startExportFlat(try XCTUnwrap(ws.current), ExportFlatSettings(),
                                                   to: dir.appendingPathComponent("layout.png")) { outcome = $0 })
        window.contentView?.layoutSubtreeIfNeeded()
        XCTAssertEqual(window.contentLayoutRect, before, "Export progress must not change document layout")
        task.update(0.5, "Encoding")
        window.contentView?.layoutSubtreeIfNeeded()
        XCTAssertEqual(window.contentLayoutRect, before)
        let finished = await waitFor { outcome != nil }
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .exported)
        window.contentView?.layoutSubtreeIfNeeded()
        XCTAssertEqual(window.contentLayoutRect, before)
        // A loose wall-time guard for loaded CI; the strict check is worker isolation below.
        let events = trace.snapshot().events
        let setup = try XCTUnwrap(events.first { $0.name == "export_flat_setup_end" })
        XCTAssertTrue(setup.mainThread)
        XCTAssertLessThan(try XCTUnwrap(setup.durationMs), 100)
        for name in ["export_flat_progress_end", "export_flat_completion_end"] {
            let updates = events.filter { $0.name == name }
            XCTAssertFalse(updates.isEmpty, "Missing timing coverage for \(name)")
            for event in updates {
                XCTAssertTrue(event.mainThread)
                XCTAssertLessThan(try XCTUnwrap(event.durationMs), 100,
                                  "UI publication must remain short even on a loaded test host")
            }
        }
        let work = events.filter { $0.name == "export_flat_work_start" }
        XCTAssertEqual(work.count, 1)
        XCTAssertTrue(work.allSatisfy { !$0.mainThread }, "Rendering, encoding and writing must never run on main")
    }

    /// B5-40: the same developed 18 MP RAW + Gaussian smart filter as FilterSelfTest.
    /// Requires confirm-time snapshot acquisition on main and catches a callback storm from tile progress.
    func testSmartFilterFixtureExportBoundsMainSpansAndCoalescesProgress() async throws {
        _ = NSApplication.shared
        let dir = try temp()
        let photos = dir.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: photos, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: ShellHarness.repoRoot.appendingPathComponent("fixtures/raw/sample.dng"),
                                        to: photos.appendingPathComponent("sample.dng"))
        let library = try await Task.detached {
            try EngineLibrary.scan(folder: photos, appSupport: dir.appendingPathComponent("support"))
        }.value
        defer { withExtendedLifetime(library) {} }
        let image = try XCTUnwrap(library.items.first?.engineImage)
        let backend = try await Task.detached {
            let doc = try EngineDocumentEngine.for(library.engine)
                .openDocumentFromImage(imageId: image.imageID, developed: true)
            let engine = try XCTUnwrap(doc as? EngineDocumentBackend)
            let layer = try XCTUnwrap(try engine.layers().first { $0.kind == .pixel }?.id)
            _ = try engine.convertForSmartFilters(layer: layer)
            _ = try engine.applyFilter(layer: layer, filterJson: #"{"id":"gaussian_blur","params":{"radius":8}}"#)
            return engine
        }.value
        let measured = ProcessInfo.processInfo.environment["TESSERA_EXPORT_BASELINES"] != nil
        let model = AppModel()
        let ws = model.documents
        try ws.install(backend)
        let doc = try XCTUnwrap(ws.current)
        defer { ws.discard(doc) }
        XCTAssertEqual(doc.info.width, 5212)
        XCTAssertEqual(doc.info.height, 3468)
        let window = measured ? SelfTestHost.makeWindow(model: model) : NSWindow(contentRect: NSRect(x: 40, y: 40, width: 1000, height: 700),
                              styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        ws.exportWindow = window
        window.contentView?.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(200))
        let trace = PerformanceTrace(enabled: true)
        ws.exportTrace = trace
        let spans = MainThreadSpans(trace: trace)
        var controls: [[String: Any]] = []
        func load1() -> Double {
            var loads = [Double](repeating: 0, count: 3)
            _ = getloadavg(&loads, 3)
            return loads[0]
        }
        func sample(_ label: String, _ busy: [Double], _ seconds: Double, _ load: [Double]) -> [String: Any] {
            let sorted = busy.sorted()
            return ["scenario": label, "seconds": seconds, "count": busy.count,
                    "busyMaxMs": busy.max() ?? 0, "busyTotalMs": busy.reduce(0, +),
                    "busyMsPerSecond": busy.reduce(0, +) / seconds,
                    "busyP95Ms": sorted.isEmpty ? 0 : sorted[Int(Double(sorted.count - 1) * 0.95)],
                    "load1": load]
        }
        if measured {
            XCTAssertFalse(window.canBecomeKey)
            XCTAssertFalse(window.isVisible)
            // Settle hosted document construction before either control. No foreground window.
            try await Task.sleep(for: .seconds(3))
            for editing in [false, true] {
                let layer = try XCTUnwrap(doc.layers.first?.id)
                doc.select(layer)
                let load = load1(), began = Date()
                spans.start()
                for tick in 0..<180 {
                    // The same controller path used by the Layers opacity slider, one commit on release.
                    if editing { doc.setOpacity(100 - Double(tick % 60), final: tick == 179) }
                    try await Task.sleep(for: .milliseconds(17))
                }
                let busy = spans.stop()
                controls.append(sample(editing ? "edit" : "idle", busy, Date().timeIntervalSince(began), [load, load1()]))
                if editing { doc.setOpacity(100, final: true) }
                try await Task.sleep(for: .seconds(3))
            }
        }
        var outcome: FlatExportTask.Outcome?
        let exportEventOffset = trace.snapshot().events.count
        let exportLoad = load1(), exportBegan = Date()
        spans.start()
        let output = dir.appendingPathComponent("smart.png")
        let task = ws.startExportFlat(doc, ExportFlatSettings(), to: output) { outcome = $0 }
        let hud = task?.progressHost?.subviews.first {
            $0.accessibilityIdentifier() == "document-export-progress"
        }
        let finished = await waitFor(300) { outcome != nil }
        let busy = spans.stop()
        if measured {
            controls.append(sample("export", busy, Date().timeIntervalSince(exportBegan), [exportLoad, load1()]))
            if let path = ProcessInfo.processInfo.environment["TESSERA_EXPORT_TEST_TRACE"] {
                let data = try JSONSerialization.data(withJSONObject: controls, options: [.prettyPrinted, .sortedKeys])
                try data.write(to: URL(fileURLWithPath: path + ".baselines.json"))
            }
            print("B5-47 BASELINES", controls)
        }
        XCTAssertNotNil(task)
        XCTAssertNotNil(hud, "The measured export must have its HUD attached to the captured host")
        XCTAssertTrue(finished)
        XCTAssertEqual(outcome, .exported)
        XCTAssertTrue(FileManager.default.fileExists(atPath: output.path))
        // Reuse FilterSelfTest.perfMode's opt-in. Whole-runloop wall time is
        // sensitive to unrelated work and scheduling; measure it on a quiet host.
        let measureTiming = ProcessInfo.processInfo.environment["TESSERA_FILTER_PERF"] != nil
        if measureTiming {
            XCTAssertLessThan(try XCTUnwrap(busy.max()), 20,
                              "18 MP fixture: no main-thread busy span may exceed the loose 20 ms regression bound")
        }
        // Controls share the trace for diagnostics; only export events have export timing bounds.
        let events = Array(trace.snapshot().events.dropFirst(exportEventOffset))
        let snapshots = events.filter { $0.name == "export_flat_snapshot_start" }
        XCTAssertEqual(snapshots.count, 1)
        XCTAssertTrue(snapshots.allSatisfy { $0.mainThread }, "The publication snapshot must run on main at confirm")
        let progress = events.filter { $0.name == "export_flat_progress_start" }
        XCTAssertGreaterThan(progress.count, 1, "The real fixture must exercise progress publication")
        for (previous, next) in zip(progress, progress.dropFirst()) {
            XCTAssertGreaterThanOrEqual(next.time - previous.time, 0.095,
                                        "Progress must coalesce to at most 10 Hz, including phase changes")
        }
        // Both named export spans and whole-runloop samples use the quiet-host opt-in.
        // The sibling B5-33 test keeps its loose 100 ms bounds enabled by default.
        if measureTiming {
            let timedEvents = events.filter { $0.mainThread && $0.durationMs != nil }
            for event in timedEvents {
                XCTAssertLessThan(event.durationMs!, 20, "Main export span: \(event.name)")
            }
            print("TESSERA_FILTER_PERF: evaluated 20 ms export-event bound for \(timedEvents.count) events")
        } else {
            print("TESSERA_FILTER_PERF unset: skipped 20 ms export-event bound")
        }
        if let path = ProcessInfo.processInfo.environment["TESSERA_EXPORT_TEST_TRACE"] {
            try await Task.detached { try trace.write(to: URL(fileURLWithPath: path)) }.value
            var rows: [String] = []
            func visit(_ view: NSView, depth: Int) {
                rows.append(String(repeating: "  ", count: depth) + String(describing: type(of: view)))
                for child in view.subviews { visit(child, depth: depth + 1) }
            }
            if let hud { visit(hud, depth: 0) }
            try rows.joined(separator: "\n").write(toFile: path + ".views.txt", atomically: true, encoding: .utf8)
        }
    }

}
