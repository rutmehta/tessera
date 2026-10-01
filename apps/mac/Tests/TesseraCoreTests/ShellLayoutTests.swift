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

    func testLayoutHarnessUsesTimerDrivenProgressAnimations() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 600),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let progress = NSProgressIndicator(frame: NSRect(x: 0, y: 0, width: 20, height: 20))
        progress.usesThreadedAnimation = true
        window.contentView?.addSubview(progress)
        ShellHarness.settle(window, size: CGSize(width: 960, height: 600))
        XCTAssertFalse(progress.usesThreadedAnimation,
                       "Many background layout windows must not exhaust dispatch workers with animation threads")
    }

    func testShellContainedAtEverySizeStateAndAppearance() throws {
        try XCTSkipIf(ProcessInfo.processInfo.environment["CI"] != nil, "CI runners have a smaller virtual screen; windows and screen-derived budgets are clamped. Runs locally.")
        let captureDir = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"].map { URL(fileURLWithPath: $0) }
        var failures: [String] = []
        var checked = 0
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
                    // Column content (scroll views inside a column) too, the document inspector included
                    // (the reconciled B5-16 candidate must pass the former expected failure).
                    for v in ShellLayoutAudit.containmentViolations(in: host, columnContent: true) {
                        failures.append("\(tag) column content: \(v)")
                    }
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
                    // The empty, preview-only Masks tab has one native control: its fixed
                    // inspector picker. SwiftUI text/buttons are not in this native traversal.
                    let minimumNativeControls = state == .photoEditMasks ? 1 : 2
                    if content.count < minimumNativeControls { failures.append("\(tag) only \(content.count) content controls found") }
                    if state == .photoEditMasks {
                        XCTAssertTrue(model.isPhotoEditing)
                        XCTAssertEqual(model.photoInspectorTab, .masks)
                        XCTAssertNotNil(model.editTarget)
                    }
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
        XCTAssertEqual(checked, ShellHarness.State.allCases.count * 8)
        XCTAssert(failures.isEmpty, "\(failures.count) layout failures:\n" + failures.joined(separator: "\n"))
    }

    /// WP B5-16: the document window at every size with every inspector sub-tab (Stack · Properties ·
    /// Channels) and History expanded and collapsed, with snapshots and history rows present: root and
    /// column containment, no overlapping actionable siblings, nothing under the toolbar, and the
    /// inspector's regions (tab bar, tab content, History header / body / New Snapshot row, the Layers
    /// and Channels footers) stacked in order inside the window with the budget's minimums.
    func testDocumentInspectorEveryTabAndHistoryStateAtEverySize() throws {
        let captureDir = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"].map { URL(fileURLWithPath: $0) }
        let model = try ShellHarness.model(.document, scratch: try scratch())
        let ws = model.documents
        let doc = try XCTUnwrap(ws.current)
        for i in 0..<12 { doc.addAdjustment(i.isMultiple(of: 2) ? .exposure : .curves) }   // history rows and layers
        for n in 1...6 { doc.snapshot(named: "Snapshot \(n)") }
        let savedTab = ws.inspectorTab
        let historyKey = "InspectorPanel.History"
        let savedHistory = UserDefaults.standard.object(forKey: historyKey)
        DocumentInspectorProbe.isEnabled = true
        defer {
            DocumentInspectorProbe.isEnabled = false
            ws.inspectorTab = savedTab
            if let savedHistory { UserDefaults.standard.set(savedHistory, forKey: historyKey) }
            else { UserDefaults.standard.removeObject(forKey: historyKey) }
        }
        let budget = DocumentInspector.budget
        XCTAssertLessThanOrEqual(budget.minimumColumn(historyExpanded: true), 548, "fits a 960 × 600 window's column")
        var failures: [String] = []
        var checked = 0
        for size in ShellHarness.sizes {
            for tab in DocumentInspectorTab.allCases {
                for history in [true, false] {
                    ws.inspectorTab = tab
                    UserDefaults.standard.set(history, forKey: historyKey)
                    DocumentInspectorProbe.frames = [:]
                    let (window, host) = ShellHarness.window(model, size: size, dark: true)
                    defer { window.orderOut(nil); window.contentViewController = nil }
                    let tag = "document-\(Int(size.width))x\(Int(size.height))-\(tab.rawValue)-history-\(history ? "open" : "closed")"
                    XCTAssertFalse(NSApp.isActive, "the harness never activates")
                    for v in ShellLayoutAudit.containmentViolations(in: host, columnContent: true) { failures.append("\(tag) containment: \(v)") }
                    let top = ShellHarness.toolbarInset(window)
                    let all = ShellHarness.actionables(window.contentView?.superview ?? host, in: host)
                    let content = all.filter { $0.view.isDescendant(of: host) }
                    for e in content where !(e.view is NSTableView) && e.frame.minY < top - 1 && e.frame.maxY > 0 {
                        failures.append("\(tag) under the toolbar: \(e.role) '\(e.label)' \(NSStringFromRect(e.frame))")
                    }
                    for e in content where e.frame.maxX > host.bounds.width + 1 || e.frame.minX < -1 || e.frame.maxY > host.bounds.height + 1 {
                        failures.append("\(tag) outside the window: \(e.role) '\(e.label)' \(NSStringFromRect(e.frame))")
                    }
                    let visible = CGRect(x: 0, y: top, width: host.bounds.width, height: host.bounds.height - top)
                    for o in ShellHarness.overlaps(content, within: visible) { failures.append("\(tag) overlap: \(o)") }
                    // The inspector's own regions, top to bottom.
                    let f = DocumentInspectorProbe.frames
                    var order = ["tabBar", "tabContent", "historyHeader"]
                    if history { order += ["historyBody"] }
                    for name in order where f[name] == nil { failures.append("\(tag) region \(name) not laid out") }
                    let regions = order.compactMap { name in f[name].map { (name, $0) } }
                    for (name, r) in regions {
                        if r.minY < top - 1 { failures.append("\(tag) \(name) under the toolbar: \(NSStringFromRect(r)) top=\(top)") }
                        if r.maxY > host.bounds.height + 1 || r.maxX > host.bounds.width + 1 {
                            failures.append("\(tag) \(name) outside the window: \(NSStringFromRect(r)) in \(NSStringFromSize(host.bounds.size))")
                        }
                    }
                    for (a, b) in zip(regions, regions.dropFirst()) where b.1.minY < a.1.maxY - 1 {
                        failures.append("\(tag) \(b.0) overlaps \(a.0): \(NSStringFromRect(b.1)) / \(NSStringFromRect(a.1))")
                    }
                    if let c = f["tabContent"], c.height < budget.tabMinimum - 1 {
                        failures.append("\(tag) tab content \(c.height) below its minimum \(budget.tabMinimum)")
                    }
                    if history, let body = f["historyBody"] {
                        if body.height < budget.historyMinimum - 1 {
                            failures.append("\(tag) History body \(body.height) below its minimum \(budget.historyMinimum)")
                        }
                        // The New Snapshot row sits whole at the bottom of the History body.
                        if let footer = f["historyFooter"], footer.maxY > body.maxY + 1 || footer.minY < body.minY - 1 {
                            failures.append("\(tag) New Snapshot row \(NSStringFromRect(footer)) outside History \(NSStringFromRect(body))")
                        } else if f["historyFooter"] == nil {
                            failures.append("\(tag) New Snapshot row not laid out")
                        }
                    }
                    // The tab's own footer is whole inside the tab content.
                    let footerName: String? = switch tab { case .stack: "layersFooter"; case .channels: "channelsFooter"; case .properties: nil }
                    if let footerName, let c = f["tabContent"] {
                        if let footer = f[footerName] {
                            if footer.maxY > c.maxY + 1 || footer.minY < c.minY - 1 {
                                failures.append("\(tag) \(footerName) \(NSStringFromRect(footer)) clipped by the tab \(NSStringFromRect(c))")
                            }
                        } else {
                            failures.append("\(tag) \(footerName) not laid out")
                        }
                    }
                    // Required controls of the tab are present (Stack: Opacity and Fill; the outline).
                    if tab == .stack {
                        for id in ["document.layers.opacity", "document.layers.fill"] where !content.contains(where: { $0.view.accessibilityIdentifier() == id }) {
                            failures.append("\(tag) \(id) not visible")
                        }
                    }
                    if let captureDir {
                        try ShellHarness.capture(window, to: captureDir.appendingPathComponent("inspector-\(tag).png"))
                    }
                    checked += 1
                }
            }
        }
        XCTAssertEqual(checked, ShellHarness.sizes.count * DocumentInspectorTab.allCases.count * 2)
        XCTAssert(failures.isEmpty, "\(failures.count) inspector layout failures:\n" + failures.joined(separator: "\n"))
    }

    /// WP B5-16 (H8): the toolbar's tab strip shows at most three tabs; more documents only add the
    /// "+n" overflow menu, so its width stops growing. Checks 960 × 600 and 1280 × 800 with eight
    /// documents open (root containment and the toolbar checks of the main test still apply).
    func testManyDocumentTabsStayCapped() throws {
        let captureDir = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"].map { URL(fileURLWithPath: $0) }
        let model = try ShellHarness.model(.document, scratch: try scratch())
        let ws = model.documents
        func stripWidth(compact: Bool = false) -> CGFloat {
            NSHostingController(rootView: DocumentTabs(workspace: ws).environment(\.toolbarCompact, compact))
                .sizeThatFits(in: CGSize(width: 4000, height: 40)).width
        }
        var widths: [Int: CGFloat] = [1: stripWidth()]
        let compactOne = stripWidth(compact: true)
        while ws.documents.count < 8 {
            ws.newDocument(ws.newSettings)
            widths[ws.documents.count] = stripWidth()
        }
        // A compact toolbar (below 1280 pt) keeps one tab plus the menu.
        XCTAssertLessThan(stripWidth(compact: true) - compactOne, Theme.Height.large * 2, "compact: one tab and the menu")
        XCTAssertGreaterThan(widths[3]!, widths[2]!, "a third tab still adds width")
        let menu = widths[4]! - widths[3]!
        XCTAssertGreaterThan(menu, 0, "the overflow menu appears with a fourth document")
        XCTAssertLessThan(menu, Theme.Height.large * 2, "the overflow menu is small")
        for n in 5...8 { XCTAssertEqual(widths[n]!, widths[4]!, accuracy: 8, "\(n) documents: the strip stops growing") }
        // The current document is always a visible tab, including the first after switching back.
        if let first = ws.documents.first { ws.select(first) }
        XCTAssertEqual(DocumentTabStrip.visible(count: ws.documents.count, current: 0), 0..<3)
        for size in [CGSize(width: 960, height: 600), CGSize(width: 1280, height: 800)] {
            let (window, host) = ShellHarness.window(model, size: size, dark: true)
            defer { window.orderOut(nil); window.contentViewController = nil }
            XCTAssertEqual(ShellLayoutAudit.containmentViolations(in: host, columnContent: true), [])
            if let captureDir {
                try ShellHarness.capture(window, to: captureDir.appendingPathComponent("tabs-8-documents-\(Int(size.width))x\(Int(size.height)).png"))
            }
        }
    }

    /// WP B5-16: ⌃1 / ⌃2 / ⌃3 reach the inspector's tabs through the window's key equivalents.
    func testInspectorTabShortcuts() throws {
        let model = try ShellHarness.model(.document, scratch: try scratch())
        let ws = model.documents
        let saved = ws.inspectorTab
        defer { ws.inspectorTab = saved }
        let (window, _) = ShellHarness.window(model, size: CGSize(width: 1280, height: 800), dark: true)
        defer { window.orderOut(nil); window.contentViewController = nil }
        let codes: [Character: UInt16] = ["1": 18, "2": 19, "3": 20]
        for tab in [DocumentInspectorTab.properties, .channels, .stack] {
            let c = String(tab.shortcutDigit)
            let e = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .control,
                                                   timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                                   context: nil, characters: c, charactersIgnoringModifiers: c, isARepeat: false,
                                                   keyCode: codes[tab.shortcutDigit] ?? 0))
            XCTAssertTrue(window.performKeyEquivalent(with: e), "⌃\(c) handled")
            RunLoop.main.run(until: Date().addingTimeInterval(0.2))
            XCTAssertEqual(ws.inspectorTab, tab, "⌃\(c)")
        }
    }
}
