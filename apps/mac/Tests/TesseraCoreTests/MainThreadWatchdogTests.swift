import AppKit
import Foundation
import XCTest
@testable import Tessera
@testable import TesseraCore

/// The hang watchdog (ST-2): a stalled main thread is reported with the last noted location and
/// a sampled stack; an idle one is not.
@MainActor
final class MainThreadWatchdogTests: XCTestCase {
    /// A synchronous stall of the calling (main) thread.
    nonisolated static func stallMainThread(_ seconds: TimeInterval) { Thread.sleep(forTimeInterval: seconds) }

    func testAStalledMainThreadIsReportedWithLocationAndStack() async throws {
        let watchdog = MainThreadWatchdog(interval: 0.02, threshold: 0.05)
        watchdog.start()
        defer { watchdog.stop() }
        // Let a first ping land, so the stall below is measured from a ping, not from start-up.
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertTrue(watchdog.gaps.isEmpty, "an idle main thread shows no gap")

        watchdog.note("MainThreadWatchdogTests.stall")
        Self.stallMainThread(0.25)   // the main thread, deliberately
        try await Task.sleep(for: .milliseconds(150))   // the overdue ping lands and is measured

        let gap = try XCTUnwrap(watchdog.gaps.first, "the stall is a gap")
        XCTAssertGreaterThanOrEqual(gap.duration, 0.2)
        XCTAssertEqual(gap.location, "MainThreadWatchdogTests.stall")
        #if arch(arm64)
        XCTAssertFalse(gap.stack.isEmpty, "the main thread's frames were sampled without a signal")
        XCTAssertTrue(gap.stack.contains { $0.contains("sleep") || $0.contains("nanosleep") || $0.contains("semaphore") },
                      "the sample shows where the main thread sat:\n" + gap.stack.joined(separator: "\n"))
        #endif
        XCTAssertTrue(gap.description.hasPrefix("main thread blocked"))

        watchdog.reset()
        watchdog.withoutRecording { Self.stallMainThread(0.2) }
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertTrue(watchdog.gaps.isEmpty, "a deliberate test stall is not recorded")
        XCTAssertNotNil(MemoryFootprint.current())
    }
}

/// Entering Edit and stepping through photos never stalls the main thread (ST-2): the shoot is the
/// 8-ARW sample (`TESSERA_SAMPLE_SHOOT=<folder>`) or the RAW fixtures, on the engine, in the
/// shell's own window (`ShellHarness`).
///
/// Gaps that start between a photo's open and its first final frame are reported but not
/// asserted on: that window is the engine's decode and first render, whose cost ST-1 owns and
/// which varies with the machine; every other gap is caused by our own main-thread code.
@MainActor
final class EditNavigationHangTests: XCTestCase {
    private struct Window { let start: Date; var end: Date? }

    func testEnteringEditAndSteppingThroughFivePhotosKeepsTheMainThreadFree() async throws {
        try XCTSkipIf(ProcessInfo.processInfo.environment["CI"] != nil, "needs the engine's RAW decode on a real Mac")
        LayoutProbeHarness.prepare()
        let scratch = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/hang-\(UUID().uuidString)")
        let folder = scratch.appendingPathComponent("shoot")
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        let source = ProcessInfo.processInfo.environment["TESSERA_SAMPLE_SHOOT"].map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? ShellHarness.repoRoot.appendingPathComponent("fixtures/raw")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: source, to: folder)
        for name in (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? [] where name.hasSuffix(".xmp") || name == ".edits" {
            try? FileManager.default.removeItem(at: folder.appendingPathComponent(name))
        }

        let before = MemoryFootprint.current()
        let model = AppModel()
        model.install(try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support")))
        defer { model.loadStubItems(count: 0) }
        let photos = model.library.items.count
        XCTAssertGreaterThanOrEqual(photos, 3, "the shoot has photos")
        let size = CGSize(width: 1440, height: 900)
        let (window, _) = ShellHarness.window(model, size: size, dark: true)
        defer { LayoutProbeHarness.dispose(window) }

        let watchdog = MainThreadWatchdog(interval: 0.1, threshold: 0.15)
        watchdog.start()
        defer { watchdog.stop() }
        var firstFrames: [Window] = []

        func waitForPhoto(_ expected: Int?) async throws {
            // The photo's own first frame: the engine's decode and render, reported, not asserted.
            firstFrames.append(Window(start: Date(), end: nil))
            let deadline = Date().addingTimeInterval(90)
            while Date() < deadline {
                if model.developStatus == .ready, let d = model.develop, expected.map({ d.itemID == $0 }) ?? true,
                   d.lastFrame?.isFinal == true { break }
                try await Task.sleep(for: .milliseconds(20))
            }
            firstFrames[firstFrames.count - 1].end = Date()
            XCTAssertEqual(model.developStatus, .ready, model.photoEditAvailabilityHint)
            XCTAssertTrue(model.develop?.lastFrame?.isFinal == true, "the photo rendered")
        }

        var footprints: [String] = []
        func sample(_ what: String) { footprints.append("\(what): \(MemoryFootprint.current()?.description ?? "?")") }
        sample("library open")
        model.enterPhotoEdit()
        try await waitForPhoto(model.focusedItem?.id)
        sample("photo 1")
        for step in 1...5 {
            let next = min(step, photos - 1)
            model.navigate(.right, groupwise: false, extend: false)
            try await waitForPhoto(model.visible.indices.contains(next) ? model.visible[next] : nil)
            // Let the saves of the previous photo and anything we schedule after a frame land.
            try await Task.sleep(for: .milliseconds(400))
            sample("photo \(step + 1)")
        }
        try await Task.sleep(for: .seconds(1.5))   // the lazy Try cards, if they start, run here
        let after = MemoryFootprint.current()
        // What the engine keeps once the session is closed, and what the preview caches hold.
        model.closeDevelop()
        try await Task.sleep(for: .seconds(2))
        sample("develop closed")
        model.loader.removeAll()
        try await Task.sleep(for: .milliseconds(500))
        sample("preview caches cleared")

        let gaps = watchdog.gaps
        let ours = gaps.filter { gap in
            !firstFrames.contains { w in gap.start >= w.start && gap.start <= (w.end ?? .distantFuture) }
        }
        let report = (["ST-2 hang watchdog: \(gaps.count) gap(s) over 150 ms, longest \(Int((watchdog.longest * 1000).rounded())) ms, "
                       + "latest ping \(Int((watchdog.longestLateness * 1000).rounded())) ms, "
                       + "\(ours.count) outside a photo's first frame; "
                       + "before \(before.map(\.description) ?? "?"), after \(after.map(\.description) ?? "?")"]
                      + footprints.map { "  " + $0 } + gaps.map(\.description)).joined(separator: "\n")
        print(report)
        FileHandle.standardError.write(Data((report + "\n").utf8))
        XCTAssertTrue(ours.isEmpty, "main-thread gaps caused by our own code:\n" + ours.map(\.description).joined(separator: "\n"))
    }
}
