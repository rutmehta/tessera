import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// A bounded real-photo complement to the empty/stub workspace layout matrix.
@MainActor
final class WorkspaceReadyPhotoTests: XCTestCase {
    func testRealRAWDevelopAndMasksRemainReadyAcrossInspectorTabs() async throws {
        ShellHarness.prepare()
        let scratch = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/workspace-ready-\(UUID().uuidString)")
        let folder = scratch.appendingPathComponent("raw")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        let fixture = ShellHarness.repoRoot.appendingPathComponent("fixtures/raw/sony-arw.ARW")
        try FileManager.default.copyItem(at: fixture, to: folder.appendingPathComponent(fixture.lastPathComponent))

        // These are the production controllers; their shared model must match the root view.
        let model = AppModel.shared
        let tools = DevelopTools.shared
        let masks = MaskTools.shared
        model.install(try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support")))
        defer { model.loadStubItems(count: 0) }
        model.enterPhotoEdit()
        model.photoInspectorTab = .develop
        let size = CGSize(width: 1440, height: 900)
        let (window, host) = ShellHarness.window(model, size: size, dark: true)
        defer { window.orderOut(nil); window.contentViewController = nil }
        let deadline = Date().addingTimeInterval(60)
        while Date() < deadline, model.developStatus != .ready || model.develop?.lastFrame?.isFinal != true {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertEqual(model.developStatus, .ready, model.photoEditAvailabilityHint)
        let controller = try XCTUnwrap(model.develop)
        XCTAssertTrue(controller.lastFrame?.isFinal == true)
        XCTAssertEqual(model.workspaceScope, "Editing 1 photo · RAW")
        XCTAssertEqual(model.targetIDs.count, 1)
        XCTAssertEqual(controller.itemID, model.editTarget?.id)
        XCTAssertTrue(tools.develop === controller)
        XCTAssertTrue(masks.develop === controller)

        func checkLayoutAndEnabledSliders(_ name: String) throws {
            ShellHarness.settle(window, size: size)
            XCTAssertFalse(NSApp.isActive)
            XCTAssertTrue(ShellLayoutAudit.containmentViolations(in: host, columnContent: true).isEmpty)
            let controls = ShellHarness.actionables(host, in: host)
            let sliders = controls.compactMap { $0.view as? ValueSlider }
            XCTAssertFalse(sliders.isEmpty, "\(name) has real adjustment controls")
            XCTAssertTrue(sliders.contains(where: { $0.isEnabled }), "\(name) controls are enabled for the ready RAW")
            let visible = CGRect(x: 0, y: ShellHarness.toolbarInset(window), width: host.bounds.width,
                                 height: host.bounds.height - ShellHarness.toolbarInset(window))
            XCTAssertTrue(ShellHarness.overlaps(controls, within: visible).isEmpty)
            if let directory = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"] {
                try ShellHarness.capture(window, to: URL(fileURLWithPath: directory).appendingPathComponent("\(name)-1440x900-dark.png"))
            }
        }
        try checkLayoutAndEnabledSliders("photoEditReadyRAWDevelop")
        let mask = try XCTUnwrap(controller.addMask(LinearGradientShape(start: (0.25, 0.2), end: (0.75, 0.8)).json))
        masks.refresh()
        masks.select(mask)
        let history = controller.history
        model.photoInspectorTab = .masks
        try checkLayoutAndEnabledSliders("photoEditReadyRAWMasks")
        XCTAssertEqual(masks.selected?.id, mask)
        XCTAssertTrue(model.develop === controller, "Inspector tabs keep the same photo session")
        XCTAssertEqual(controller.history.headLabel, history.headLabel, "Tab changes do not create photo history")
        model.returnToLibrary()
        XCTAssertFalse(masks.active)
        model.closeDevelop()
        await controller.close()
    }
}
