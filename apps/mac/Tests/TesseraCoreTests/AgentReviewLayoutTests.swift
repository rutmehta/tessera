import AppKit
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

/// Real queue + current-photo preview, complementing navigation and owner-action tests.
@MainActor
final class AgentReviewLayoutTests: XCTestCase {
    func testReviewEmptyAndRealQueueAtEverySizeAndAppearance() async throws {
        try XCTSkipIf(ProcessInfo.processInfo.environment["CI"] != nil, "Uses the local background window server")
        ShellHarness.prepare()
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("review-layout-\(UUID())")
        let folder = scratch.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        let names = ["Library-return.jpg", "A-long-photo-name-with-location-and-client-reference-0123456789.jpg", "B-ready.jpg", "C-missing.jpg"]
        for (i, name) in names.enumerated() { try ShellHarness.writeJPEG(folder.appendingPathComponent(name), shade: 30 + i * 30) }
        let model = AppModel()
        model.install(try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support")))
        let preferences = model.agent.preferences
        defer { model.agent.preferences = preferences; model.closeDevelop() }
        model.enterReview()
        try check(model, state: "reviewEmpty")
        model.returnToLibrary()
        // The indexed photo disappears before the run: the queue must retain its actual failure.
        let missing = try XCTUnwrap(model.library.items.first { $0.name == "C-missing.jpg" })
        try FileManager.default.removeItem(at: folder.appendingPathComponent("C-missing.jpg"))
        let targets = model.library.items.filter { $0.name != "Library-return.jpg" }.map(\.id)
        model.agent.preferences.sceneConsistency = false
        model.agent.preferences.personConsistency = false
        model.agent.start(itemIDs: targets, provider: .scripted)
        let deadline = Date().addingTimeInterval(30)
        while model.agent.isRunning, Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertFalse(model.agent.isRunning)
        XCTAssertEqual(model.viewMode, .grid, "Run completion must not take navigation focus")
        XCTAssertEqual(model.agent.queue.count, targets.count)
        model.enterReview()
        let long = try XCTUnwrap(model.agent.queue.entries.first { $0.name.hasPrefix("A-long") })
        XCTAssertNil(long.error)
        model.selectReviewPhoto(long.imageID)
        XCTAssertNotNil(model.reviewTargetItem)
        try check(model, state: "reviewPopulatedLongName")
        try await checkReadyPreview(model)
        try checkBusyPresentation(model, entry: long)
        let failure = try XCTUnwrap(model.agent.queue.entries.first { $0.imageID == missing.engineImage?.imageID })
        XCTAssertNotNil(failure.error, "The failure state must come from the real missing-file run")
        model.selectReviewPhoto(failure.imageID)
        try check(model, state: "reviewFailedPhoto")
    }

    /// Let the real view's save barrier and loader callback run on MainActor. Cache insertion and
    /// callback delivery are synchronous in ThumbnailLoader.deliver, so observing a newly cached
    /// preview on this actor proves delivery; the background capture then checks rendered pixels.
    private func checkReadyPreview(_ model: AppModel) async throws {
        let item = try XCTUnwrap(model.reviewTargetItem)
        model.toast = nil // Same action as the visible Dismiss button; retain the prior loading captures.
        for (size, dark) in [(CGSize(width: 960, height: 600), false), (CGSize(width: 1440, height: 900), true)] {
            model.loader.invalidate(item)
            XCTAssertNil(model.loader.cached(item, tier: .preview))
            let (window, host) = ShellHarness.window(model, size: size, dark: dark)
            defer { window.orderOut(nil); window.contentViewController = nil }
            let deadline = Date().addingTimeInterval(15)
            while model.loader.cached(item, tier: .preview) == nil, Date() < deadline {
                try await Task.sleep(for: .milliseconds(20))
            }
            let loaded = try XCTUnwrap(model.loader.cached(item, tier: .preview), "Real Review view must deliver its preview before capture")
            XCTAssertGreaterThan(loaded.width, 0)
            XCTAssertGreaterThan(loaded.height, 0)
            XCTAssertNil(model.toast)
            XCTAssertFalse(NSApp.isActive)
            ShellHarness.settle(window, size: size)
            XCTAssertTrue(ShellLayoutAudit.containmentViolations(in: host, columnContent: true).isEmpty)
            if let directory = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"] {
                let tag = "reviewReady-\(Int(size.width))x\(Int(size.height))-\(dark ? "dark" : "light")"
                try ShellHarness.capture(window, to: URL(fileURLWithPath: directory).appendingPathComponent("\(tag).png"))
            }
        }
    }

    /// Synthetic presentation only: no operation is started or represented as completed.
    /// The real entry/action tests separately prove controller busy and mutation behavior.
    private func checkBusyPresentation(_ model: AppModel, entry: AgentReviewEntry) throws {
        for dark in [true, false] {
            for size in ShellHarness.sizes {
                let content = VStack(spacing: 0) {
                    Text("Synthetic busy presentation · no engine operation")
                        .font(Theme.Fonts.caption).padding(Theme.Space.m)
                    HStack(spacing: 0) {
                        VStack {
                            ReviewDestinationRow(entry: entry, selected: true, busy: true)
                            Spacer()
                        }.frame(width: 220).padding(Theme.Space.s)
                        Spacer()
                        AgentReviewInspector(model: model, busy: true, canEdit: false)
                            .frame(width: Theme.Width.inspectorMin)
                    }
                }
                .background(Theme.panel)
                let controller = NSHostingController(rootView: content)
                let window = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled],
                                      backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
                window.contentViewController = controller
                window.orderBack(nil)
                defer { window.orderOut(nil); window.contentViewController = nil }
                controller.view.layoutSubtreeIfNeeded()
                RunLoop.main.run(until: Date().addingTimeInterval(0.1))
                let host = controller.view
                let tag = "reviewBusyPresentation-\(Int(size.width))x\(Int(size.height))-\(dark ? "dark" : "light")"
                XCTAssertFalse(NSApp.isActive, tag)
                XCTAssertTrue(model.agent.busy.isEmpty, "A presentation fixture must not fake or start an operation")
                XCTAssertFalse(model.agent.isRunning)
                XCTAssertTrue(ShellLayoutAudit.containmentViolations(in: host, columnContent: true).isEmpty, tag)
                let controls = ShellHarness.actionables(host, in: host)
                XCTAssertTrue(ShellHarness.overlaps(controls, within: host.bounds).isEmpty, tag)
                // SwiftUI's drawn buttons do not materialize an AX tree in this prohibited-activation
                // harness. The view exposes busy labels/value; screenshots cover disabled styling.
                if let directory = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"] {
                    try ShellHarness.capture(window, to: URL(fileURLWithPath: directory).appendingPathComponent("\(tag).png"))
                }
            }
        }
    }

    private func check(_ model: AppModel, state: String) throws {
        for dark in [true, false] {
            for size in ShellHarness.sizes {
                let (window, host) = ShellHarness.window(model, size: size, dark: dark)
                defer { window.orderOut(nil); window.contentViewController = nil }
                let tag = "\(state)-\(Int(size.width))x\(Int(size.height))-\(dark ? "dark" : "light")"
                XCTAssertFalse(NSApp.isActive, tag)
                XCTAssertTrue(model.isReviewing, tag)
                XCTAssertTrue(ShellLayoutAudit.containmentViolations(in: host, columnContent: true).isEmpty, tag)
                let controls = ShellHarness.actionables(window.contentView?.superview ?? host, in: host)
                let content = controls.filter { $0.view.isDescendant(of: host) }
                let top = ShellHarness.toolbarInset(window)
                let visible = CGRect(x: 0, y: top, width: host.bounds.width, height: host.bounds.height - top)
                XCTAssertTrue(ShellHarness.overlaps(content, within: visible).isEmpty, tag)
                for control in content where !(control.view is NSTableView) {
                    XCTAssertGreaterThanOrEqual(control.frame.minY, top - 1, "\(tag): \(control.label)")
                    XCTAssertLessThanOrEqual(control.frame.maxX, host.bounds.width + 1, "\(tag): \(control.label)")
                }
                if let directory = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"] {
                    try ShellHarness.capture(window, to: URL(fileURLWithPath: directory).appendingPathComponent("\(tag).png"))
                }
            }
        }
    }
}
