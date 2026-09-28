import AppKit
import TesseraCore
import SwiftUI

/// Accessory launches may not instantiate SwiftUI's Window scene. Host the same real content,
/// but never allow this diagnostic window to become key/main or move ahead of another app.
@MainActor
final class BackgroundAuditWindow: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    init(model: AppModel) {
        super.init(contentRect: NSRect(x: 40, y: 40, width: 1440, height: 900),
                   styleMask: [.titled, .resizable, .nonactivatingPanel], backing: .buffered, defer: false)
        isReleasedWhenClosed = false
        isFloatingPanel = false
        hidesOnDeactivate = false
        level = .normal
        title = "Tessera background audit"
        contentView = NSHostingView(rootView: ContentView(model: model)
            .frame(minWidth: 960, minHeight: 600))
    }
}

/// Background-only diagnostic run. The script copies its fixture so self-test edits never touch originals.
@MainActor
func runTimingSelfTest(model: AppModel) {
    let args = ProcessInfo.processInfo.arguments
    guard args.contains("--nonactivating"), let path = PerformanceTrace.outputPath else { return }
    let gridOnly = args.contains("--timing-grid-only")
    let environment = ProcessInfo.processInfo.environment
    let verbose = environment["TESSERA_DOC_FRAME_LOG"] != nil || environment["TESSERA_SELFTEST_VERBOSE"] == "1"
    Task { @MainActor in
        PerformanceTrace.shared.record("selftest_start")
        if verbose { PerformanceTrace.shared.record("frame_logging_enabled") }
        var finished = false
        for _ in 0..<1200 {
            if verbose { break }
            let events = PerformanceTrace.shared.snapshot().events
            let gridAppeared = events.contains { $0.name == "grid_appeared" }
            if gridOnly {
                if gridAppeared, !model.library.items.isEmpty { finished = true; break }
            } else {
                if gridAppeared, !model.isLoading, !model.library.items.isEmpty, model.viewMode != .loupe {
                    model.requestViewMode(.loupe)
                }
                let inputs = events.filter { $0.name == "input" }.count
                if inputs >= 121 { finished = true; break }
            }
            try? await Task.sleep(for: .milliseconds(100))
        }
        // Give actual drawable-presented callbacks time to arrive; an occluded window can have none.
        try? await Task.sleep(for: .seconds(2))
        PerformanceTrace.shared.record(finished ? "selftest_complete" : "selftest_timeout")
        if !finished {
            let state = "timing-selftest timeout: loading=\(model.isLoading) items=\(model.library.items.count) visible=\(model.visibleCount) mode=\(model.viewMode) focus=\(String(describing: model.focus)) develop=\(model.developStatus) windows=\(NSApp.windows.count)\n"
            FileHandle.standardError.write(Data(state.utf8))
        }
        do {
            try await Task.detached(priority: .utility) {
                try PerformanceTrace.shared.write(to: URL(fileURLWithPath: path))
            }.value
        } catch {
            FileHandle.standardError.write(Data("Timing trace write failed: \(error)\n".utf8))
        }
        NSApp.terminate(nil)
    }
}
