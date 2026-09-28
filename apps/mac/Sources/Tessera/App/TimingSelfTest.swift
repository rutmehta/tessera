import AppKit
import QuartzCore
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
                if mode == .visible {
                    finished = events.contains { $0.name == "measurement_end" || $0.name == "qualification_failed" }
                    if finished { break }
                } else {
                    let inputs = events.filter { $0.name == "input" }.count
                    if inputs >= 121 { finished = true; break }
                }
            }
            try? await Task.sleep(for: .milliseconds(100))
        }
        // Give actual drawable-presented callbacks time to arrive; an occluded window can have none.
        try? await Task.sleep(for: .seconds(2))
        let qualified = gridOnly || PerformanceTrace.shared.snapshot().events.contains { $0.name == "measurement_end" }
        if mode == .visible {
            PerformanceTrace.shared.record(finished && qualified
                                           ? "selftest_complete" : "selftest_qualification_failed")
        } else {
            PerformanceTrace.shared.record(finished ? "selftest_complete" : "selftest_timeout")
        }
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

@MainActor
func runQualifiedVisibleTimingInputs(model: AppModel, controller: DevelopController) async -> Bool {
    let args = ProcessInfo.processInfo.arguments
    func fail(_ reason: String) -> Bool {
        PerformanceTrace.shared.record("qualification_failed", session: controller.timingSession)
        FileHandle.standardError.write(Data("timing-visible qualification failed: \(reason)\n".utf8))
        return false
    }
    func value(after flag: String) -> String? {
        guard let index = args.firstIndex(of: flag), index + 1 < args.count else { return nil }
        return args[index + 1]
    }
    guard let directory = value(after: "--timing-control-dir"),
          let nonce = value(after: "--timing-nonce"), !nonce.isEmpty else {
        return fail("visible timing control directory or nonce is missing")
    }
    let control = URL(fileURLWithPath: directory, isDirectory: true)
    let readyURL = control.appendingPathComponent("ready.json")
    let startURL = control.appendingPathComponent("start.json")

    func currentIdentity(time: Double) -> TimingVisibleHandshake? {
        guard let window = NSApp.keyWindow, !(window is NSPanel), window.isVisible,
              window.isKeyWindow, NSApp.isActive, window.occlusionState.contains(.visible),
              NSApp.activationPolicy() == .regular,
              let bundleID = Bundle.main.bundleIdentifier,
              let application = NSRunningApplication(processIdentifier: ProcessInfo.processInfo.processIdentifier),
              let launchDate = application.launchDate else { return nil }
        let bundleURL = Bundle.main.bundleURL.standardizedFileURL.path
        return TimingVisibleHandshake(nonce: nonce,
            pid: ProcessInfo.processInfo.processIdentifier, bundleID: bundleID,
            bundleURL: bundleURL, launchDate: launchDate.timeIntervalSince1970,
            windowNumber: window.windowNumber, session: controller.timingSession, time: time)
    }

    func write(_ record: TimingVisibleHandshake, to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        try encoder.encode(record).write(to: url, options: .atomic)
    }

    // The initial visible drawable is a warmup condition, outside the measured interval.
    let readyDeadline = CACurrentMediaTime() + 20
    var identity: TimingVisibleHandshake?
    while CACurrentMediaTime() < readyDeadline {
        guard model.develop === controller else { return fail("Develop session changed before readiness") }
        let events = PerformanceTrace.shared.snapshot().events
        let hasPositiveBaseline = events.contains {
            $0.name == "drawable_presented" && $0.session == controller.timingSession && $0.time.isFinite && $0.time > 0
        }
        if hasPositiveBaseline, let current = currentIdentity(time: CACurrentMediaTime()) {
            identity = current
            break
        }
        try? await Task.sleep(for: .milliseconds(50))
    }
    guard let ready = identity else { return fail("no positive same-session presentation in a visible regular window") }
    do { try write(ready, to: readyURL) }
    catch { return fail("could not publish ready record: \(error)") }

    var protocolState = TimingVisibleIntervalProtocol(ready: ready)
    let permitDeadline = ready.time + protocolState.timeout
    var permitAccepted = false
    while CACurrentMediaTime() <= permitDeadline {
        guard model.develop === controller,
              let current = currentIdentity(time: CACurrentMediaTime()), current.identifiesSameRun(as: ready) else {
            return fail("visible app, window, process, or Develop session changed while waiting for permit")
        }
        if FileManager.default.fileExists(atPath: startURL.path) {
            do {
                let permit = try JSONDecoder().decode(TimingVisibleHandshake.self, from: Data(contentsOf: startURL))
                let now = CACurrentMediaTime()
                guard protocolState.acceptStart(permit, now: now, visible: true) else {
                    return fail("start permit was stale, duplicated, expired, or had a mismatched identity")
                }
                PerformanceTrace.shared.record("measurement_start", session: controller.timingSession, time: now)
                permitAccepted = true
                break
            } catch {
                return fail("start permit could not be decoded: \(error)")
            }
        }
        protocolState.expire(now: CACurrentMediaTime())
        try? await Task.sleep(for: .milliseconds(50))
    }
    guard permitAccepted else { return fail("timed out waiting for matching start permit") }

    for index in 0...120 {
        guard model.develop === controller,
              let current = currentIdentity(time: CACurrentMediaTime()), current.identifiesSameRun(as: ready) else {
            return fail("app visibility or Develop identity changed during the fixed input sequence")
        }
        PerformanceTrace.shared.record("app_visibility_check", session: controller.timingSession,
                                       span: "scripted-input-\(index + 1)", time: CACurrentMediaTime())
        model.setAdjustment(.exposure, 1.5 * Double(index) / 120, final: index == 120, for: controller.itemID)
        // Preserve the existing timing self-test's explicit flush aid. Results describe this
        // scripted app-input path, not an OS mouse gesture or display-link-coalesced user drag.
        controller.flushPending()
        if index < 120 { try? await Task.sleep(for: .milliseconds(16)) }
    }
    guard let measurementStart = protocolState.measurementStart else {
        return fail("protocol state lost its measurement start")
    }
    let sessionInputs = PerformanceTrace.shared.snapshot().events.filter {
        $0.name == "input" && $0.session == controller.timingSession && $0.input != nil
            && $0.time >= measurementStart
    }
    let inputIDs = sessionInputs.compactMap(\.input).sorted()
    guard let firstInput = inputIDs.first, inputIDs.count == 121,
          inputIDs == Array(firstInput..<(firstInput + 121)) else {
        return fail("fixed scripted sequence did not produce exactly 121 consecutive input IDs")
    }
    let finalInput = inputIDs.last!
    PerformanceTrace.shared.record("input_sequence_complete", session: controller.timingSession,
                                   input: finalInput, time: CACurrentMediaTime())

    let presentationDeadline = CACurrentMediaTime() + 20
    var finalPresented = false
    while CACurrentMediaTime() < presentationDeadline {
        guard model.develop === controller,
              let current = currentIdentity(time: CACurrentMediaTime()), current.identifiesSameRun(as: ready) else {
            return fail("app visibility or Develop identity changed while draining final presentation")
        }
        PerformanceTrace.shared.record("app_visibility_check", session: controller.timingSession,
                                       input: finalInput, time: CACurrentMediaTime())
        finalPresented = PerformanceTrace.shared.snapshot().events.contains {
            $0.name == "drawable_presented" && $0.session == controller.timingSession
                && $0.input == finalInput && $0.time.isFinite && $0.time > 0
        }
        if finalPresented { break }
        try? await Task.sleep(for: .milliseconds(50))
    }
    guard finalPresented else { return fail("final input had no positive actual drawable presentation before timeout") }
    let end = CACurrentMediaTime()
    protocolState.expire(now: end)
    let endIdentity = TimingVisibleHandshake(nonce: ready.nonce, pid: ready.pid, bundleID: ready.bundleID,
        bundleURL: ready.bundleURL, launchDate: ready.launchDate, windowNumber: ready.windowNumber,
        session: ready.session, time: end)
    guard protocolState.phase == .measuring,
          protocolState.end(now: end, identity: endIdentity, visible: true) else {
        return fail("qualified interval could not end exactly once")
    }
    PerformanceTrace.shared.record("measurement_end", session: controller.timingSession, time: end)
    return true
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
