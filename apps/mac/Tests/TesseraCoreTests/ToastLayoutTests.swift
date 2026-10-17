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

    // MARK: UX-16 item 28 — a long toast wraps inside Theme.Width.toast and never clips

    /// The owner's toast: cut at both ends. It wraps to two lines at the capped width.
    static let longMessage = "1 edit could not be written · DSC02506.ARW: Original sidecars differ from the offline baseline; both copies retained"

    func testToastPlanHugsShortAndWrapsLong() {
        let short = ToastLayout.plan(message: "Opened shoot: 24 images", undoable: false, detailLines: 0)
        XCTAssertEqual(short, .init(wraps: false, messageOverflows: false, hasDetails: false))
        let long = ToastLayout.plan(message: Self.longMessage, undoable: false, detailLines: 0)
        XCTAssertTrue(long.wraps)
        XCTAssertFalse(long.messageOverflows, "two lines hold the owner's message")
        XCTAssertFalse(long.hasDetails)
        let undoable = ToastLayout.plan(message: Self.longMessage, undoable: true, detailLines: 0)
        XCTAssertTrue(undoable.wraps)
        let essay = ToastLayout.plan(message: String(repeating: Self.longMessage + " ", count: 4), undoable: false, detailLines: 0)
        XCTAssertTrue(essay.wraps)
        XCTAssertTrue(essay.messageOverflows, "beyond two lines the whole text goes behind Details")
        XCTAssertTrue(essay.hasDetails)
        XCTAssertTrue(ToastLayout.plan(message: "Export failed", undoable: false, detailLines: 2).hasDetails)
        XCTAssertLessThanOrEqual(ToastLayout.lineCount(Self.longMessage, width: Theme.Width.toast - ToastLayout.chrome(undo: false, details: false)), 2)
        XCTAssertEqual(ToastLayout.lineCount("short", width: 400), 1)
    }

    func testLongToastWrapsInsideTheCapAndStaysInTheWindow() throws {
        let size = CGSize(width: 1385, height: 868)
        let model = try ShellHarness.model(.library, scratch: try scratch())
        model.select(position: 0)
        let (window, _) = ShellHarness.window(model, size: size, dark: true)
        defer { LayoutProbeHarness.dispose(window) }
        model.documents.columnVisibility = .detailOnly
        ShellHarness.settle(window, size: size)

        model.showToast("Opened shoot: 24 images", undoable: false, seconds: 60)
        ShellHarness.settle(window, size: size)
        let short = try frame("toast", in: window)
        XCTAssertLessThan(short.width, Theme.Width.toast * 0.75, "a short toast hugs its text")
        let shortHeight = short.height

        model.toast = nil
        ShellHarness.settle(window, size: size)
        model.showToast(Self.longMessage, undoable: false, seconds: 60)
        ShellHarness.settle(window, size: size)
        let toast = try frame("toast", in: window)
        let message = try frame("toastMessage", in: window)
        let content = try XCTUnwrap(window.contentView).bounds
        XCTAssertEqual(toast.width, Theme.Width.toast, accuracy: 1, "the long toast takes the cap")
        XCTAssertTrue(content.insetBy(dx: -0.5, dy: -0.5).contains(toast), "not clipped: \(toast) in \(content)")
        XCTAssertTrue(toast.insetBy(dx: -0.5, dy: -0.5).contains(message), "the message \(message) sits inside the toast \(toast)")
        let line = Theme.NSFonts.label.ascender - Theme.NSFonts.label.descender
        XCTAssertGreaterThanOrEqual(message.height, 2 * line - 1, "two lines of text (\(message.height) pt)")
        XCTAssertGreaterThan(toast.height, shortHeight, "the toast grows for the second line rather than clipping")
        // Still above the agent row and clear of the chips (UX-15 item 20).
        let row = try frame("agentRow", in: window)
        let chips = try frame("cullQuickActions", in: window)
        XCTAssertLessThanOrEqual(toast.maxY, row.minY + 0.5)
        XCTAssertFalse(toast.intersects(chips.insetBy(dx: 0, dy: 1)))
        model.toast = nil
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
