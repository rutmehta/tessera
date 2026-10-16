import AppKit
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

/// UX-15 (item 20): the toast sits one row up from the quick-action chips, in a band every mode
/// keeps clear, so it never overlaps the chips, the burst view's actions or Deliver's status
/// line. Measured on the real shell through `ShellLayoutProbe` (the rows are SwiftUI; the
/// AppKit tree has no view for them). Background-safe: see `ShellHarness`.
@MainActor
final class ToastLayoutTests: XCTestCase {
    override func setUp() async throws {
        LayoutProbeHarness.prepare()
        ShellLayoutProbe.isEnabled = true
        ShellLayoutProbe.frames = [:]
    }

    override func tearDown() async throws {
        ShellLayoutProbe.isEnabled = false
        ShellLayoutProbe.frames = [:]
        UserDefaults.standard.removeObject(forKey: AppModel.cullLayoutKey)
    }

    private func scratch() throws -> URL {
        let dir = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/toast-layout-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    /// Frames in SwiftUI's global space: the window's content, y down from the top.
    private func frame(_ name: String, in window: NSWindow) throws -> CGRect {
        let rect = try XCTUnwrap(ShellLayoutProbe.frames[name], "\(name) not laid out: \(ShellLayoutProbe.frames.keys.sorted())")
        let content = try XCTUnwrap(window.contentView).bounds
        XCTAssertTrue(content.contains(rect), "\(name) \(NSStringFromRect(rect)) inside the window \(NSStringFromRect(content))")
        return rect
    }

    func testToastSitsAboveTheChipsAndBelowTheBurstActionsInCull() throws {
        for size in [CGSize(width: 1385, height: 868), ShellBudget.minWindow] {
            let model = try ShellHarness.model(.library, scratch: try scratch())
            model.select(position: 0)
            let (window, _) = ShellHarness.window(model, size: size, dark: true)
            defer { LayoutProbeHarness.dispose(window) }
            model.documents.columnVisibility = .detailOnly
            ShellHarness.settle(window, size: size)
            model.showToast("Opened shoot: 24 images", undoable: false, seconds: 60)
            ShellHarness.settle(window, size: size)
            let toast = try frame("toast", in: window)
            let chips = try frame("cullQuickActions", in: window)
            let row = try frame("agentRow", in: window)
            let actions = try frame("burstActions", in: window)
            let tag = "\(Int(size.width))x\(Int(size.height))"
            XCTAssertGreaterThan(toast.height, 0, tag)
            XCTAssertLessThanOrEqual(toast.maxY, chips.minY - Theme.Space.s + 0.5, "\(tag): the toast is one row up from the chips")
            XCTAssertLessThanOrEqual(toast.maxY, row.minY + 0.5, "\(tag): the toast is above the agent row")
            XCTAssertGreaterThanOrEqual(toast.minY, actions.maxY - 0.5, "\(tag): the toast is below the burst's action buttons")
            XCTAssertFalse(toast.intersects(actions.insetBy(dx: 0, dy: 1)), "\(tag): toast \(toast) over burst actions \(actions)")
            XCTAssertFalse(toast.intersects(chips.insetBy(dx: 0, dy: 1)), "\(tag): toast \(toast) over chips \(chips)")
            model.toast = nil
        }
    }

    func testToastClearsDeliverStatusLine() throws {
        let size = CGSize(width: 1385, height: 868)
        let model = try ShellHarness.model(.deliver, scratch: try scratch())
        let (window, _) = ShellHarness.window(model, size: size, dark: true)
        defer { LayoutProbeHarness.dispose(window) }
        model.documents.columnVisibility = .detailOnly
        ShellHarness.settle(window, size: size)
        model.showToast("An export is already running", undoable: false, seconds: 60)
        ShellHarness.settle(window, size: size)
        let toast = try frame("toast", in: window)
        let status = try frame("deliverStatus", in: window)
        let row = try frame("agentRow", in: window)
        XCTAssertLessThanOrEqual(toast.maxY, row.minY + 0.5)
        XCTAssertGreaterThanOrEqual(toast.minY, status.maxY - 0.5, "toast \(toast) over Deliver's status line \(status)")
        model.toast = nil
    }

    func testTheBandIsTheToastAndItsGap() {
        XCTAssertEqual(Theme.Height.toastBand, Theme.Height.toast + Theme.Space.s)
        XCTAssertGreaterThanOrEqual(CullWorkspace.gridBottomInset, Theme.Height.agentField + Theme.Height.large + Theme.Height.toastBand)
    }
}
