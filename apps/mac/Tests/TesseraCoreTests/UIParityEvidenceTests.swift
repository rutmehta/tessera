import AppKit
import WebKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// Visual parity evidence (WP UX-16): renders the real shell in Cull (burst view), Edit (panel
/// shown and hidden) and Deliver at 1440 × 900 in-process and writes window PNGs, and beside
/// each the same screen of the clickable prototype (`docs/design/redesign-2026-10/demo`) drawn
/// by a headless WKWebView at the same size (`*-prototype.png`), so the two can be compared.
/// No screen recording permission, no keystrokes, no other app touched. Skipped unless
/// `TESSERA_PARITY_EVIDENCE=<output dir>` and `TESSERA_PARITY_SHOOT=<folder of RAWs>` are set.
@MainActor
final class UIParityEvidenceTests: XCTestCase {
    private static let size = CGSize(width: 1440, height: 900)

    override func setUp() async throws {
        LayoutProbeHarness.prepare()
    }

    func testParityEvidence() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let out = env["TESSERA_PARITY_EVIDENCE"].map({ URL(fileURLWithPath: $0) }),
              let shoot = env["TESSERA_PARITY_SHOOT"].map({ URL(fileURLWithPath: $0) }) else {
            throw XCTSkip("set TESSERA_PARITY_EVIDENCE and TESSERA_PARITY_SHOOT")
        }
        let scratch = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/parity-evidence-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

        // The prototype first: the same four screens from the demo at 1440 × 900 (scale 1). The
        // demo declares no charset, so a copy with one is loaded (file URLs default to Latin-1).
        let demoSource = ShellHarness.repoRoot.appendingPathComponent("docs/design/redesign-2026-10/demo")
        let demoCopy = scratch.appendingPathComponent("demo")
        try FileManager.default.copyItem(at: demoSource, to: demoCopy)
        let demo = demoCopy.appendingPathComponent("index.html")
        try ("<meta charset=\"utf-8\">\n" + String(contentsOf: demo, encoding: .utf8)).write(to: demo, atomically: true, encoding: .utf8)
        let prototype = try await Prototype(demo: demo)
        try await prototype.shoot("setMode('cull')", to: out.appendingPathComponent("01-cull-prototype.png"))
        try await prototype.shoot("setMode('edit')", to: out.appendingPathComponent("02-edit-prototype.png"))
        try await prototype.shoot("togglePanel()", to: out.appendingPathComponent("03-edit-panel-hidden-prototype.png"))
        try await prototype.shoot("togglePanel(); setMode('deliver')", to: out.appendingPathComponent("04-deliver-prototype.png"))

        let model = AppModel()
        model.installOutputHandlers()
        model.install(try EngineLibrary.scan(folder: shoot, appSupport: scratch.appendingPathComponent("support")))
        XCTAssertFalse(model.library.items.isEmpty)
        model.cullLayout = .burst
        addTeardownBlock { UserDefaults.standard.removeObject(forKey: AppModel.cullLayoutKey) }
        // Two keeps, as a person would leave the first burst.
        for _ in 0..<2 { model.perform(.keep) }
        model.select(position: 0)

        // 1. Cull: the burst view with the triage column.
        do {
            let (window, host) = ShellHarness.window(model, size: Self.size, dark: true)
            defer { LayoutProbeHarness.dispose(window) }
            model.documents.columnVisibility = .detailOnly
            try await settle(window, seconds: 6)
            try write(window, host, to: out.appendingPathComponent("01-cull.png"))
        }

        // 2. Edit with the panel, 3. without it.
        do {
            model.selectShellMode(.edit)
            model.photoInspectorTab = .develop
            let (window, host) = ShellHarness.window(model, size: Self.size, dark: true)
            defer { LayoutProbeHarness.dispose(window) }
            let deadline = Date().addingTimeInterval(60)
            while Date() < deadline, model.developStatus != .ready || model.develop?.lastFrame?.isFinal != true {
                try await settle(window, seconds: 0.2)
            }
            XCTAssertEqual(model.developStatus, .ready, model.photoEditAvailabilityHint)
            try await settle(window, seconds: 4)
            if !model.showInspector { model.toggleInspectorPanel() }
            try await settle(window, seconds: 1.5)
            try write(window, host, to: out.appendingPathComponent("02-edit.png"))
            model.toggleInspectorPanel()
            try await settle(window, seconds: 1.5)
            try write(window, host, to: out.appendingPathComponent("03-edit-panel-hidden.png"))
            model.toggleInspectorPanel()
            model.returnFromPhotoEdit()
            try await settle(window, seconds: 1)
        }

        // 4. Deliver: the keeps as web JPEGs.
        do {
            model.selectShellMode(.deliver)
            model.setDeliverWhat(.keeps)
            model.setDeliverHow(.web)
            model.exporter.settings.destination = scratch.appendingPathComponent("export").path
            let (window, host) = ShellHarness.window(model, size: Self.size, dark: true)
            defer { LayoutProbeHarness.dispose(window) }
            model.documents.columnVisibility = .detailOnly
            try await settle(window, seconds: 4)
            try write(window, host, to: out.appendingPathComponent("04-deliver.png"))
        }
        model.closeDevelop()
        print("PARITY EVIDENCE →", out.path)
    }

    private func settle(_ window: NSWindow, seconds: Double) async throws {
        let end = Date().addingTimeInterval(seconds)
        while Date() < end { try await Task.sleep(nanoseconds: 100_000_000) }
        if let content = window.contentView { await LayoutProbeHarness.settleAsync(content) }
    }

    /// The window as the screen shows it (`screencapture -l`, the window only, never the screen):
    /// the Metal loupe and macOS 26's inspector material do not draw through `cacheDisplay`.
    private func write(_ window: NSWindow, _ host: NSView, to url: URL) throws {
        host.displayIfNeeded()
        let shot = Process()
        shot.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        shot.arguments = ["-l", String(window.windowNumber), "-x", "-o", url.path]
        try shot.run()
        shot.waitUntilExit()
        XCTAssertEqual(shot.terminationStatus, 0, "Window capture failed: \(url.lastPathComponent)")
    }

    /// The demo in a WKWebView of the stage's size, in a background window (a web view only
    /// draws inside a window); `shoot` runs one of the demo's own handlers, waits a beat for its
    /// transitions, and snapshots at 2×.
    @MainActor
    private final class Prototype: NSObject, WKNavigationDelegate {
        private let web: WKWebView
        private let window: NSWindow
        private var loaded: CheckedContinuation<Void, Error>?

        init(demo: URL) async throws {
            let configuration = WKWebViewConfiguration()
            web = WKWebView(frame: NSRect(origin: .zero, size: UIParityEvidenceTests.size), configuration: configuration)
            window = LayoutProbeHarness.window(contentRect: NSRect(origin: .zero, size: UIParityEvidenceTests.size),
                                               styleMask: [.titled, .fullSizeContentView], backing: .buffered, defer: false)
            super.init()
            window.contentView = web
            window.orderBack(nil)
            web.navigationDelegate = self
            try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                loaded = c
                web.loadFileURL(demo, allowingReadAccessTo: demo.deletingLastPathComponent())
            }
            // Web fonts (if the network allows) and the first layout.
            try await Task.sleep(for: .seconds(2))
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            loaded?.resume()
            loaded = nil
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            loaded?.resume(throwing: error)
            loaded = nil
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            loaded?.resume(throwing: error)
            loaded = nil
        }

        func shoot(_ script: String, to url: URL) async throws {
            _ = try await web.evaluateJavaScript(script + "; 0")
            try await Task.sleep(for: .milliseconds(600))
            let configuration = WKSnapshotConfiguration()
            configuration.rect = CGRect(origin: .zero, size: UIParityEvidenceTests.size)
            configuration.snapshotWidth = NSNumber(value: Double(UIParityEvidenceTests.size.width))
            let image = try await web.takeSnapshot(configuration: configuration)
            let tiff = try XCTUnwrap(image.tiffRepresentation)
            let bitmap = try XCTUnwrap(NSBitmapImageRep(data: tiff))
            let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
            try png.write(to: url)
        }
    }
}
