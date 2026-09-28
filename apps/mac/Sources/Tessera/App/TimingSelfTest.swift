import AppKit
import TesseraCore
import SwiftUI

/// Keeps the visible presentation capability run explicitly separate from the background audit.
enum TimingSelfTestLaunchMode: Equatable {
    case notRequested
    case background
    case visible
    case conflictingArguments

    var shouldStartUpdater: Bool { self == .notRequested }

    static func resolve(arguments: [String]) -> Self {
        let requestsTest = arguments.contains("--timing-selftest") || arguments.contains("--timing-grid-only")
        let requestsVisible = arguments.contains("--timing-visible")
        let requestsBackground = arguments.contains("--nonactivating")
        guard requestsTest else { return requestsVisible ? .conflictingArguments : .notRequested }
        if requestsVisible && requestsBackground { return .conflictingArguments }
        if requestsBackground { return .background }
        if requestsVisible { return .visible }
        // Preserve the former no-op behavior for a timing test without an explicit host mode.
        return .notRequested
    }
}

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

/// Isolated diagnostic run. The script copies its fixture so self-test edits never touch originals.
@MainActor
func runTimingSelfTest(model: AppModel, mode: TimingSelfTestLaunchMode) {
    let args = ProcessInfo.processInfo.arguments
    guard mode == .background || mode == .visible,
          let path = PerformanceTrace.outputPath else { return }
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
        if mode == .visible {
            writeVisibleWindowReceipt(nextTo: URL(fileURLWithPath: path))
        }
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

private struct VisibleWindowReceipt: Encodable {
    struct Rect: Encodable {
        let x: Double
        let y: Double
        let width: Double
        let height: Double
    }
    let processID: Int32
    let bundleID: String
    let activationPolicy: String
    let appActive: Bool
    let windowNumber: Int
    let windowTitle: String
    let isRegularWindow: Bool
    let isVisible: Bool
    let isKeyWindow: Bool
    let occlusionVisible: Bool
    let contentScreenFrame: Rect
}

@MainActor
private func writeVisibleWindowReceipt(nextTo traceURL: URL) {
    guard let window = NSApp.keyWindow,
          !(window is NSPanel),
          let content = window.contentView else {
        FileHandle.standardError.write(Data("timing-visible: no key regular window at completion\n".utf8))
        return
    }
    let contentInWindow = content.convert(content.bounds, to: nil)
    let frame = window.convertToScreen(contentInWindow)
    let receipt = VisibleWindowReceipt(
        processID: ProcessInfo.processInfo.processIdentifier,
        bundleID: Bundle.main.bundleIdentifier ?? "",
        activationPolicy: NSApp.activationPolicy() == .regular ? "regular" : "nonregular",
        appActive: NSApp.isActive,
        windowNumber: window.windowNumber,
        windowTitle: window.title,
        isRegularWindow: !(window is NSPanel),
        isVisible: window.isVisible,
        isKeyWindow: window.isKeyWindow,
        occlusionVisible: window.occlusionState.contains(.visible),
        contentScreenFrame: .init(x: Double(frame.origin.x), y: Double(frame.origin.y),
                                  width: Double(frame.width), height: Double(frame.height)))
    do {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        try encoder.encode(receipt).write(to: traceURL.appendingPathExtension("window.json"), options: .atomic)
    } catch {
        FileHandle.standardError.write(Data("timing-visible: window receipt write failed: \(error)\n".utf8))
    }
}
