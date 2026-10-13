import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// Deliver evidence (WP UX-15): renders the real shell in Deliver on a folder of photos, runs
/// the export through `AppModel.runDeliver` and writes window PNGs, all in-process (no screen
/// recording permission, no keystrokes, no other app touched). Skipped unless
/// `TESSERA_DELIVER_EVIDENCE=<output dir>` and `TESSERA_DELIVER_SHOOT=<folder>` are set.
@MainActor
final class DeliverEvidenceTests: XCTestCase {
    override func setUp() async throws {
        LayoutProbeHarness.prepare()
    }

    func testDeliverEvidence() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let out = env["TESSERA_DELIVER_EVIDENCE"].map({ URL(fileURLWithPath: $0) }),
              let shoot = env["TESSERA_DELIVER_SHOOT"].map({ URL(fileURLWithPath: $0) }) else {
            throw XCTSkip("set TESSERA_DELIVER_EVIDENCE and TESSERA_DELIVER_SHOOT")
        }
        let scratch = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/deliver-evidence-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

        let model = AppModel()
        model.installOutputHandlers()
        model.install(try EngineLibrary.scan(folder: shoot, appSupport: scratch.appendingPathComponent("support")))
        XCTAssertFalse(model.library.items.isEmpty)
        // Three keeps, as a person would leave Cull; then Deliver.
        for _ in 0..<3 { model.perform(.keep) }
        model.selectShellMode(.deliver)
        model.setDeliverWhat(.keeps)
        model.setDeliverHow(.web)
        model.exporter.settings.destination = scratch.appendingPathComponent("export").path
        model.exporter.settings.openInFinder = false
        let keeps = model.deliverItems.count
        XCTAssertGreaterThanOrEqual(keeps, 3)

        let (window, host) = ShellHarness.window(model, size: CGSize(width: 1440, height: 900), dark: true)
        defer { LayoutProbeHarness.dispose(window) }
        // As the app ships: the source sidebar hidden (the harness opens it for the audits).
        model.documents.columnVisibility = .detailOnly
        try await settle(window, seconds: 6)
        try write(window, host, to: out.appendingPathComponent("01-deliver-default.png"))

        model.setDeliverHow(.instagram)
        try await settle(window, seconds: 2)
        try write(window, host, to: out.appendingPathComponent("02-deliver-instagram.png"))

        // The agent field's phrase runs the export (Keeps already chosen).
        model.submitAgentInstruction("export the keeps for instagram")
        XCTAssertTrue(model.exporter.isRunning, model.statusMessage ?? "")
        let deadline = Date().addingTimeInterval(180)
        while model.deliverResult == nil, Date() < deadline {
            try await settle(window, seconds: 0.5)
        }
        let result = try XCTUnwrap(model.deliverResult)
        XCTAssertEqual(result.files.count, keeps)
        XCTAssertEqual(result.failed, 0)
        try await settle(window, seconds: 1.5)
        try write(window, host, to: out.appendingPathComponent("03-deliver-done.png"))
        print("DELIVER EVIDENCE", result.files.map(\.lastPathComponent), "→", out.path)
    }

    private func settle(_ window: NSWindow, seconds: Double) async throws {
        let end = Date().addingTimeInterval(seconds)
        while Date() < end { try await Task.sleep(nanoseconds: 100_000_000) }
        if let content = window.contentView { await LayoutProbeHarness.settleAsync(content) }
    }

    /// The window's frame view (toolbar included), drawn through `cacheDisplay` at 2× like the
    /// layout audits' OCR bitmaps.
    private func write(_ window: NSWindow, _ host: NSView, to url: URL) throws {
        let view = window.contentView?.superview ?? host
        view.displayIfNeeded()
        let bitmap = try LayoutProbeHarness.bitmap(view)
        let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
        try png.write(to: url)
    }
}
