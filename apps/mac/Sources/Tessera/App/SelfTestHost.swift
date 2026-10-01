import AppKit
import SwiftUI

/// One host for in-app document and Library/Develop self-tests launched in the background (WP B5-selftest-window).
///
/// `--nonactivating` (accessory policy) does not instantiate SwiftUI's `Window` scene, so no document view ever
/// appears, and the self-tests that start from it (or from a panel, sheet or menu inside it) never run. When any
/// document self-test is requested together with `--nonactivating`, `launch(model:)`:
///  1. hosts the real `ContentView` in a regular window that can never become key or main and is ordered behind
///     every other app's windows (never front, never activating the app). A regular window, not a panel: AppKit
///     does not count panels, so closing a sheet or alert would count as "the last window closed" and quit mid-run;
///  2. starts every requested self-test directly, instead of waiting for its view, panel or menu to appear, so a
///     background launch needs no `--new-document`;
///  3. opens a blank document for the self-tests that only run "once a document is open" (channels, text) when
///     neither `--new-document` nor `--open-document` was given.
/// Self-tests call `raiseForCapture` instead of ordering their window front themselves: in the background it is a
/// no-op (the capture scripts use `screencapture -l <window-id>`, which works behind other windows).
///
/// Launch recipe: `open -g -n --stderr <log> Tessera.app --args --nonactivating --app-dir <dir> --folder <dir>
/// <self-test flag>` (environment-driven ones via `open --env TESSERA_…_SELFTEST=<dir>`).
@MainActor
enum SelfTestHost {
    /// Background audit launch (M2-53): accessory policy, never activated, nothing ordered front.
    static let isBackground = CommandLine.arguments.contains("--nonactivating")

    /// Argument flags (`--flag <dir>` or `--flag=<dir>`) that start a document self-test.
    static let argumentFlags = [
        "--document-selftest", "--tools-selftest", "--filter-selftest", "--styles-selftest", "--retouch-selftest",
        "--vector-selftest", "--transform-selftest", "--liquify-selftest", "--channel-paint-selftest",
        "--camera-raw-selftest", "--adaptive-wide-angle-selftest",   // B5-20
    ]
    /// Library/Develop entry points accept bare boolean flags only.
    static let libraryFlags = ["--develop-selftest", "--develop-panels-selftest", "--hdr-selftest", "--masks-selftest"]

    /// Environment variables that start a document self-test.
    static let environmentKeys = ["TESSERA_CHANNELS_SELFTEST", "TESSERA_TEXT_SELFTEST", "TESSERA_STACK_SELFTEST"]
    /// Self-tests that wait for an already-open document instead of opening their own.
    private static let needsOpenDocument = ["TESSERA_CHANNELS_SELFTEST", "TESSERA_TEXT_SELFTEST"]

    static func requested(_ args: [String] = CommandLine.arguments,
                          environment: [String: String] = ProcessInfo.processInfo.environment) -> Bool {
        args.contains { a in argumentFlags.contains { a == $0 || a.hasPrefix($0 + "=") } }
            || args.contains(where: libraryFlags.contains)
            || environmentKeys.contains { environment[$0] != nil }
    }

    private static var window: NSWindow?
    private static var launched = false

    /// The host window: real content, but never key or main, so it can never take keyboard focus from another app.
    private final class HostWindow: NSWindow {
        override var canBecomeKey: Bool { false }
        override var canBecomeMain: Bool { false }
    }

    /// Called once from `applicationDidFinishLaunching`. Does nothing unless a document self-test is requested with
    /// `--nonactivating` (foreground runs keep starting from the document view as before).
    static func launch(model: AppModel) {
        guard isBackground, requested(), !launched else { return }
        launched = true
        // After SwiftUI has had its chance to open the `Window` scene (when it does, that window is used).
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            MainActor.assumeIsolated {
                ensureWindow(model: model)
                startSelfTests(model: model)
            }
        }
    }

    /// Hosts the app's content in a background window unless a visible regular window already exists.
    static func ensureWindow(model: AppModel) {
        guard isBackground, window == nil,
              !NSApp.windows.contains(where: { !($0 is NSPanel) && $0.isVisible }) else { return }
        let w = makeWindow(model: model)
        w.order(.below, relativeTo: 0)   // behind everything; never orderFront, never makeKey
        window = w
        log("host window \(w.windowNumber)")
    }

    /// The actual host, also exercised unordered by the resize regression test.
    static func makeWindow(model: AppModel) -> NSWindow {
        let w = HostWindow(contentRect: NSRect(x: 40, y: 40, width: 1440, height: 900), styleMask: [.titled, .resizable],
                           backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        w.title = "Tessera self-test"
        w.contentView = NSHostingView(rootView: ContentView.root(model: model))
        return w
    }

    private static func startSelfTests(model: AppModel) {
        let ws = model.documents
        let args = CommandLine.arguments
        let env = ProcessInfo.processInfo.environment
        startLibrarySelfTests(model: model, args: args)
        if needsOpenDocument.contains(where: { env[$0] != nil }), !args.contains("--new-document"),
           !args.contains("--open-document"), ws.current == nil, ws.opening == nil {
            ws.newDocument(ws.newSettings)
        }
        // Each entry point is idempotent: the view / panel / menu that normally starts it later is a no-op.
        TransformSelfTest.startIfRequested(ws)
        VectorSelfTest.startIfRequested()
        ChannelPaintSelfTest.startIfRequested()
        CameraRawSelfTest.startIfRequested()
        AdaptiveWideAngleSelfTest.startIfRequested()   // B5-20
        RetouchSelfTest.startIfRequested()
        LiquifySelfTest.startIfRequested()
        FilterSelfTest.startIfRequested()
        StylesSelfTest.startIfRequested()
        ChannelsSelfTest.startIfRequested(ws)
        TextSelfTest.startIfRequested(ws)
        if env["TESSERA_STACK_SELFTEST"] != nil { DocumentStack.shared.attach(model) }   // starts StackSelfTest
    }

    private static func startLibrarySelfTests(model: AppModel, args: [String]) {
        let requested = libraryFlags.filter(args.contains)
        guard !requested.isEmpty else { return }
        // Register the existing tool observers even when no inspector has appeared.
        _ = DevelopTools.shared
        _ = MaskTools.shared
        Task { @MainActor in
            for _ in 0..<120 {
                if !model.isLoading, model.canEnterPhotoEdit, let item = model.focusedItem,
                   item.engineImage != nil {
                    for flag in requested {
                        logLibraryResult(String(flag.dropFirst(2)), "N/A key-window keyboard routing (never-key host; controller paths exercised)")
                    }
                    model.enterPhotoEdit()
                    if model.develop == nil, model.developStatus != .loading { model.openDevelop(for: item) }
                    return
                }
                try? await Task.sleep(for: .milliseconds(500))
            }
            for flag in requested {
                let name = String(flag.dropFirst(2))
                logLibraryResult(name, "FAIL no indexed photo became available")
                finishLibraryTest(name, failures: 1)
            }
        }
    }

    static func logLibraryResult(_ name: String, _ result: String) {
        FileHandle.standardError.write(Data("\(name): \(result)\n".utf8))
    }

    static func finishLibraryTest(_ name: String, failures: Int) {
        logLibraryResult(name, "done, \(failures) failures")
    }

    /// For a step's screenshot: in a foreground run, puts `window` above other apps' windows (without activating);
    /// in the background, leaves it where it is.
    static func raiseForCapture(_ window: NSWindow, floating: Bool = true) {
        guard !isBackground else { return }
        if floating { window.level = .floating }
        window.orderFrontRegardless()
    }

    private static func log(_ s: String) { FileHandle.standardError.write(Data("selftest-host: \(s)\n".utf8)) }
}
