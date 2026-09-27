import AppKit
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

/// The shell never places a subtree outside the window, respects the toolbar's safe area and keeps
/// actionable controls apart (WP M2-56, layout audit D01/D02 and Machine B's toolbar overlap), in
/// library, RAW-folder and layered-document states at 960 × 600 (declared minimum), 1280 × 800,
/// 1440 × 900 and 1728 × 1117, light and dark. Background-safe: see `ShellHarness`.
///
/// `TESSERA_LAYOUT_CAPTURE=<dir>` also saves each window as `<dir>/<state>-<w>x<h>-<appearance>.png`.
@MainActor
final class ShellLayoutTests: XCTestCase {
    override func setUp() async throws {
        ShellHarness.prepare()
    }

    private func scratch() throws -> URL {
        let dir = ShellHarness.repoRoot.appendingPathComponent("apps/mac/build/shell-layout-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testShellContainedAtEverySizeStateAndAppearance() throws {
        let captureDir = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"].map { URL(fileURLWithPath: $0) }
        var failures: [String] = []
        var checked = 0
        var documentColumnOverflow: [String] = []
        for state in ShellHarness.State.allCases {
            let model = try ShellHarness.model(state, scratch: try scratch())
            for dark in [true, false] {
                for size in ShellHarness.sizes {
                    let (window, host) = ShellHarness.window(model, size: size, dark: dark)
                    defer { window.orderOut(nil); window.contentViewController = nil }
                    let tag = "\(state.rawValue)-\(Int(size.width))x\(Int(size.height))-\(dark ? "dark" : "light")"
                    XCTAssertFalse(NSApp.isActive, "the harness never activates")
                    // 1. Root containment: no split / hosting subtree outside the window's content.
                    for v in ShellLayoutAudit.containmentViolations(in: host) { failures.append("\(tag) containment: \(v)") }
                    // Column content (scroll views inside a column) too, except the document inspector,
                    // whose stacked panels are Machine B's (tools/orchestrate/wp/M2-56/DOCUMENT-HANDOFF.md).
                    let inner = ShellLayoutAudit.containmentViolations(in: host, columnContent: true)
                    if state == .document { documentColumnOverflow += inner.map { "\(tag): \($0)" } }
                    else { for v in inner { failures.append("\(tag) column content: \(v)") } }
                    // 2. The window keeps the requested size (clamped only to the declared minimum
                    //    content size): content never forces it larger.
                    let top = ShellHarness.toolbarInset(window)
                    if top < 28 { failures.append("\(tag) toolbar inset \(top) (no unified toolbar?)") }
                    let expected = CGSize(width: max(size.width, ShellBudget.minWindow.width),
                                          height: max(size.height, ShellBudget.minWindow.height + top))
                    if abs(window.frame.width - expected.width) > 1 || abs(window.frame.height - expected.height) > 1 {
                        failures.append("\(tag) window is \(NSStringFromSize(window.frame.size)), expected \(NSStringFromSize(expected))")
                    }
                    // 3. The toolbar's safe area: no content control under the toolbar or under a
                    //    toolbar item (Machine B: clicks meant for Character ▸ Style hit Auto Edit).
                    let all = ShellHarness.actionables(window.contentView?.superview ?? host, in: host)
                    let content = all.filter { $0.view.isDescendant(of: host) }
                    let toolbar = all.filter { !$0.view.isDescendant(of: host) && $0.frame.maxY <= top + 1 }
                    if content.count < 2 { failures.append("\(tag) only \(content.count) content controls found") }
                    for e in content where !(e.view is NSTableView) && e.frame.minY < top - 1 && e.frame.maxY > 0 {
                        failures.append("\(tag) under the toolbar: \(e.role) '\(e.label)' \(NSStringFromRect(e.frame)) top=\(top)")
                    }
                    for e in content {
                        for t in toolbar where e.frame.intersection(t.frame).width > 1 && e.frame.intersection(t.frame).height > 1 {
                            failures.append("\(tag) '\(e.label)' under toolbar item '\(t.label)'")
                        }
                    }
                    for e in content where e.frame.maxX > host.bounds.width + 1 || e.frame.minX < -1 || e.frame.maxY > host.bounds.height + 1 {
                        failures.append("\(tag) outside the window: \(e.role) '\(e.label)' \(NSStringFromRect(e.frame))")
                    }
                    // 4. No two actionable siblings overlap.
                    let visible = CGRect(x: 0, y: top, width: host.bounds.width, height: host.bounds.height - top)
                    for o in ShellHarness.overlaps(content, within: visible) { failures.append("\(tag) overlap: \(o)") }
                    if let captureDir { try ShellHarness.capture(window, to: captureDir.appendingPathComponent("\(tag).png")) }
                    checked += 1
                }
            }
        }
        XCTAssertEqual(checked, 24)
        if !documentColumnOverflow.isEmpty {
            XCTExpectFailure("document inspector panels overflow their column (handed off to Machine B, L1)", strict: false) {
                XCTFail(documentColumnOverflow.joined(separator: "\n"))
            }
        }
        XCTAssert(failures.isEmpty, "\(failures.count) layout failures:\n" + failures.joined(separator: "\n"))
    }
}
