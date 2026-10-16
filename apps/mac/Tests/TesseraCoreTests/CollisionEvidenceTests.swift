import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// UX-15 collision evidence (FOLLOWUPS items 18–25): renders the real shell on a folder of
/// RAWs through the states the owner's screenshots located (a toast over Cull's chips, Edit
/// while the session opens, the Edit → Deliver switch, the grouped grid, the library panel in
/// Cull) and writes window PNGs, all in-process: no screen recording, no keystrokes, no other
/// app touched. Skipped unless `TESSERA_UX15_EVIDENCE=<output dir>` and
/// `TESSERA_UX15_SHOOT=<folder of a few copied RAWs>` are set.
@MainActor
final class CollisionEvidenceTests: XCTestCase {
    private static let size = CGSize(width: 1385, height: 868)

    override func setUp() async throws {
        LayoutProbeHarness.prepare()
    }

    func testCollisionEvidence() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let out = env["TESSERA_UX15_EVIDENCE"].map({ URL(fileURLWithPath: $0) }),
              let shoot = env["TESSERA_UX15_SHOOT"].map({ URL(fileURLWithPath: $0) }) else {
            throw XCTSkip("set TESSERA_UX15_EVIDENCE and TESSERA_UX15_SHOOT")
        }
        let scratch = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/ux15-evidence-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        addTeardownBlock { UserDefaults.standard.removeObject(forKey: AppModel.cullLayoutKey) }

        let model = AppModel()
        model.installOutputHandlers()
        model.install(try EngineLibrary.scan(folder: shoot, appSupport: scratch.appendingPathComponent("support")))
        XCTAssertFalse(model.library.items.isEmpty)
        model.cullLayout = .burst
        model.select(position: 0)

        let (window, host) = ShellHarness.window(model, size: Self.size, dark: true)
        defer { LayoutProbeHarness.dispose(window) }
        model.documents.columnVisibility = .detailOnly
        if !model.showInspector { model.toggleInspectorPanel() }
        try await settle(window, seconds: 4)

        // 18–20: the toast the folder open shows, over Cull's quick-action chips (item 20).
        model.showToast("Opened \(model.library.title): \(model.library.items.count.formatted()) images", undoable: false, seconds: 60)
        try await settle(window, seconds: 1)
        try write(window, host, to: out.appendingPathComponent("01-cull-toast.png"))
        model.toast = nil

        // 23: the grouped grid (the glyph and the Review button on the burst rows).
        model.cullLayout = .grid
        try await settle(window, seconds: 2)
        try write(window, host, to: out.appendingPathComponent("02-cull-grid.png"))
        model.cullLayout = .burst

        // 22: the library panel beside the loupe in Cull.
        model.requestLibraryViewMode(.loupe)
        try await settle(window, seconds: 2)
        try write(window, host, to: out.appendingPathComponent("03-cull-loupe-panel.png"))
        model.requestLibraryViewMode(.grid)
        try await settle(window, seconds: 1)

        // 19: Edit as soon as the mode switches, while the engine opens the session.
        model.selectShellMode(.edit)
        model.photoInspectorTab = .develop
        try await settle(window, seconds: 0.4)
        try write(window, host, to: out.appendingPathComponent("04-edit-opening.png"))
        let deadline = Date().addingTimeInterval(60)
        while Date() < deadline, model.developStatus != .ready || model.develop?.lastFrame?.isFinal != true {
            try await settle(window, seconds: 0.2)
        }
        XCTAssertEqual(model.developStatus, .ready, model.photoEditAvailabilityHint)
        try await settle(window, seconds: 2)
        try write(window, host, to: out.appendingPathComponent("05-edit-ready.png"))

        // 18: Deliver chosen with the session open: nothing of Edit may draw under Deliver.
        model.selectShellMode(.deliver)
        model.setDeliverWhat(.shoot)
        model.exporter.settings.destination = scratch.appendingPathComponent("export").path
        try await settle(window, seconds: 0.3)
        try write(window, host, to: out.appendingPathComponent("06-deliver-switching.png"))
        let settled = Date().addingTimeInterval(30)
        while Date() < settled, model.isPhotoEditing || !model.deliverMode {
            try await settle(window, seconds: 0.2)
        }
        try await settle(window, seconds: 2)
        try write(window, host, to: out.appendingPathComponent("07-deliver-settled.png"))
        XCTAssertTrue(model.deliverMode)
        XCTAssertFalse(model.isPhotoEditing)

        model.selectShellMode(.cull)
        try await settle(window, seconds: 1)
        model.closeDevelop()
        print("UX-15 EVIDENCE →", out.path)
    }

    private func settle(_ window: NSWindow, seconds: Double) async throws {
        let end = Date().addingTimeInterval(seconds)
        while Date() < end { try await Task.sleep(nanoseconds: 100_000_000) }
        if let content = window.contentView { await LayoutProbeHarness.settleAsync(content) }
    }

    /// The window as the screen shows it when `screencapture -l` is allowed (the Metal loupe
    /// draws only there); otherwise the frame view through `cacheDisplay` at 2×, like the
    /// Deliver evidence.
    private func write(_ window: NSWindow, _ host: NSView, to url: URL) throws {
        let view = window.contentView?.superview ?? host
        view.displayIfNeeded()
        let shot = Process()
        shot.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        shot.arguments = ["-l", String(window.windowNumber), "-x", "-o", url.path]
        shot.standardError = FileHandle.nullDevice
        try shot.run()
        shot.waitUntilExit()
        if shot.terminationStatus == 0, let size = try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int, size > 10_000 {
            return
        }
        try? FileManager.default.removeItem(at: url)
        let bitmap = try LayoutProbeHarness.bitmap(view)
        let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
        try png.write(to: url)
    }
}
