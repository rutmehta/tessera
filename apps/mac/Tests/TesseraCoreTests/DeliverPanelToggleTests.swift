import AppKit
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

/// UX-16 items 34 and 35 on the real shell (`ShellHarness`, in-process, no keystrokes):
/// after Cull → Edit → Deliver the toolbar's Panel toggle shows Deliver's What · How · Where
/// column and nothing else presents an empty pane; the Cull · Edit · Deliver control keeps every
/// title whole at 2000 pt.
@MainActor
final class DeliverPanelToggleTests: XCTestCase {
    override func setUp() async throws {
        LayoutProbeHarness.prepare()
        ShellLayoutProbe.isEnabled = true
        ShellLayoutProbe.frames = [:]
    }

    override func tearDown() async throws {
        ShellLayoutProbe.isEnabled = false
        ShellLayoutProbe.frames = [:]
        for mode in ShellMode.allCases { UserDefaults.standard.removeObject(forKey: "Shell.panel." + mode.rawValue) }
    }

    private func frame(_ name: String) throws -> CGRect {
        try XCTUnwrap(ShellLayoutProbe.frames[name], "\(name) not laid out: \(ShellLayoutProbe.frames.keys.sorted())")
    }

    /// The split view items of every `NSSplitViewController` in the window (the navigation split
    /// view's sidebar and detail, and the system inspector when SwiftUI presents one), with their
    /// behaviour, collapsed state and width. An expanded inspector item in Deliver is the bug.
    private func splitItems(_ window: NSWindow) -> [(behavior: NSSplitViewItem.Behavior, collapsed: Bool, width: CGFloat)] {
        var controllers: [NSSplitViewController] = []
        func walk(_ vc: NSViewController) {
            if let split = vc as? NSSplitViewController { controllers.append(split) }
            vc.children.forEach(walk)
        }
        if let root = window.contentViewController { walk(root) }
        func walkViews(_ v: NSView) {
            if let split = v as? NSSplitView, let c = split.delegate as? NSSplitViewController, !controllers.contains(where: { $0 === c }) {
                controllers.append(c)
            }
            v.subviews.forEach(walkViews)
        }
        if let content = window.contentView { walkViews(content) }
        return controllers.flatMap { c in
            c.splitViewItems.map { ($0.behavior, $0.isCollapsed, $0.viewController.view.frame.width) }
        }
    }

    /// Pixels inside `rect` (window coordinates, y down) that are not the canvas colour. The
    /// window image also goes to `TESSERA_UX16_EVIDENCE` when set, for a look.
    private func inkedPixels(in rect: CGRect, of host: NSView, window: NSWindow, step: Int) throws -> Int {
        let rep = try LayoutProbeHarness.bitmap(host)
        if let dir = ProcessInfo.processInfo.environment["TESSERA_UX16_EVIDENCE"] {
            let url = URL(fileURLWithPath: dir).appendingPathComponent("deliver-toggle-step\(step).png")
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try rep.representation(using: .png, properties: [:])?.write(to: url)
        }
        let scale = CGFloat(rep.pixelsWide) / host.bounds.width
        let canvas = Theme.Palette.canvas.usingColorSpace(.sRGB) ?? Theme.Palette.canvas
        var inked = 0
        let x0 = Int(rect.minX * scale), x1 = Int(rect.maxX * scale)
        let y0 = Int(rect.minY * scale), y1 = Int(rect.maxY * scale)
        var y = y0
        while y < min(y1, rep.pixelsHigh) {
            var x = x0
            while x < min(x1, rep.pixelsWide) {
                if let c = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB),
                   abs(c.redComponent - canvas.redComponent) + abs(c.greenComponent - canvas.greenComponent)
                    + abs(c.blueComponent - canvas.blueComponent) > 0.12 {
                    inked += 1
                }
                x += 2
            }
            y += 2
        }
        return inked
    }

    private func settle(_ window: NSWindow, size: CGSize, seconds: Double) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            ShellHarness.settle(window, size: size)
            try await Task.sleep(for: .milliseconds(50))
        }
    }

    func testPanelToggleInDeliverAfterEditShowsTheDeliverColumn() async throws {
        let size = CGSize(width: 2000, height: 1257)
        let scratch = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/deliver-toggle-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        // A real folder on the engine: Edit opens a develop session, and Deliver's switch goes
        // through the asynchronous save-and-close, as the owner's did.
        let model = try ShellHarness.model(.library, scratch: scratch)
        model.select(position: 1)
        let (window, host) = ShellHarness.window(model, size: size, dark: true)
        defer { LayoutProbeHarness.dispose(window) }
        model.documents.columnVisibility = .detailOnly
        ShellHarness.settle(window, size: size)
        let content = try XCTUnwrap(window.contentView).bounds

        // Cull → Edit → Deliver.
        model.selectShellMode(.edit)
        let opened = Date().addingTimeInterval(60)
        while model.developStatus != .ready, Date() < opened { try await settle(window, size: size, seconds: 0.1) }
        XCTAssertEqual(model.developStatus, .ready, "the session opened (\(model.developStatus))")
        XCTAssertTrue(model.isPhotoEditing)
        model.selectShellMode(.deliver)
        let delivered = Date().addingTimeInterval(60)
        while !(model.deliverMode && !model.isPhotoEditing), Date() < delivered { try await settle(window, size: size, seconds: 0.1) }
        try await settle(window, size: size, seconds: 0.5)
        XCTAssertTrue(model.deliverMode)
        XCTAssertFalse(model.isPhotoEditing)

        for step in 0..<3 {
            // As restored, then hidden, then shown again by the toolbar's Panel toggle.
            if step > 0 {
                model.toggleInspectorPanel()
                try await settle(window, size: size, seconds: 0.5)
            }
            let shown = model.showInspector
            let workspace = try frame("deliverWorkspace")
            XCTAssertEqual(workspace.width, content.width, accuracy: 1,
                           "step \(step): Deliver spans the window; nothing else takes a column (\(workspace) in \(content))")
            let items = splitItems(window)
            let inspectors = items.filter { $0.behavior == .inspector && !$0.collapsed && $0.width > 1 }
            XCTAssertTrue(inspectors.isEmpty, "step \(step): the system inspector is not presented in Deliver: \(items)")
            if shown {
                let column = try frame("deliverColumn")
                XCTAssertGreaterThan(column.width, 0, "step \(step)")
                XCTAssertEqual(column.minX, workspace.minX, accuracy: 1, "step \(step): the column is Deliver's own, on the left")
                XCTAssertTrue(workspace.insetBy(dx: -0.5, dy: -0.5).contains(column), "step \(step): \(column) in \(workspace)")
                // Drawn, not only laid out: the column's titles and rows put ink on the canvas
                // (an AppKit view left over from Edit drawing on top would leave it blank).
                let inked = try inkedPixels(in: column, of: host, window: window, step: step)
                XCTAssertGreaterThan(inked, 2000, "step \(step): the column shows What · How · Where (\(inked) inked pixels)")
            } else if let column = ShellLayoutProbe.frames["deliverColumn"] {
                XCTAssertFalse(workspace.intersects(column.insetBy(dx: 1, dy: 1)), "step \(step): hidden column \(column) off the workspace")
            }
            ShellLayoutProbe.frames["deliverColumn"] = nil
        }
        XCTAssertTrue(model.showInspector, "shown · hidden · shown")
    }

    func testModeControlKeepsEveryTitleWholeAt2000Points() throws {
        let size = CGSize(width: 2000, height: 1257)
        let model = AppModel()
        model.loadStubItems(count: 6)
        model.selectShellMode(.deliver)
        let (window, _) = ShellHarness.window(model, size: size, dark: true)
        defer { LayoutProbeHarness.dispose(window) }
        model.documents.columnVisibility = .detailOnly
        ShellHarness.settle(window, size: size)
        let control = try frame("modeControl")
        // Three titles at their own width (the chosen one semibold) with the segment padding,
        // the segment spacing and the track inset: narrower than that, one truncates.
        let titles = ShellMode.allCases.map(\.title)
        let text = titles.reduce(CGFloat(0)) { $0 + ToastLayout.textWidth($1, NSFont.systemFont(ofSize: 13, weight: .semibold)) }
        let needed = text + CGFloat(titles.count) * 2 * Theme.Space.l + CGFloat(titles.count - 1) * Theme.Space.xxs
        XCTAssertGreaterThanOrEqual(control.width, needed - 1, "the control \(control) holds its titles (\(needed) pt)")
        XCTAssertLessThan(control.width, needed + Theme.Space.xl, "and hugs them")
        XCTAssertTrue(titles.contains("Deliver"))
    }
}
