import AppKit
import Observation
import QuartzCore
import XCTest
import TesseraFFI
@testable import TesseraCore
@testable import Tessera

@MainActor
final class FlatExportHUDTests: XCTestCase {
    private final class Changes: @unchecked Sendable {
        private let lock = NSLock()
        private var count = 0
        func changed() { lock.withLock { count += 1 } }
        var value: Int { lock.withLock { count } }
    }

    // Catches progress returning to observable state or native text/progress controls:
    // a publisher tick must change layer contents and AX values without scheduling view layout.
    func testProgressChangesLayersAndAccessibilityWithoutObservationOrLayout() throws {
        LayoutProbeHarness.prepare()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 600, height: 300),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let host = try XCTUnwrap(window.contentView)
        let ws = DocumentWorkspace()
        let hud = FlatExportProgressView(workspace: ws)
        hud.frame = NSRect(x: 0, y: 0, width: 400, height: 64)
        let task = FlatExportTask(fileName: "layers.png", documentTitle: "Layers", cancel: {})
        host.addSubview(hud)
        hud.update([task])
        hud.placeInViewport()
        hud.layoutSubtreeIfNeeded()
        let views = descendants(hud)
        for view in views { view.needsLayout = false }
        let frames = views.map(\.frame)
        let changes = Changes()
        withObservationTracking {
            _ = task.fraction; _ = task.phase
        } onChange: { changes.changed() }
        let publisher = FlatExportProgressPublisher(schedule: { _, action in
            MainActor.assumeIsolated { action() }
        }) { fraction, phase in
            task.update(fraction, phase)
        }
        publisher.receive(0.625, "Encoding")
        publisher.finish()
        XCTAssertEqual(changes.value, 0, "Progress must not invalidate SwiftUI observation")
        XCTAssertFalse(views.contains { $0 is NSTextField || $0 is NSProgressIndicator },
                       "HUD publication must not invalidate AppKit text/control layout")
        XCTAssertFalse(views.contains { $0.needsLayout })
        XCTAssertEqual(views.map(\.frame), frames)
        let text = views.flatMap { layers($0.layer) }.compactMap { ($0 as? CATextLayer)?.string as? String }
        XCTAssertTrue(text.contains("Encoding 63 %"))
        XCTAssertTrue(text.contains("Exporting layers.png"))
        let elements = accessibility(host)
        XCTAssertTrue(elements.contains { ($0.accessibilityValue() as? String) == "Encoding 63 %" })
        XCTAssertTrue(elements.contains { ($0.accessibilityValue() as? String) == "Exporting layers.png" })
        XCTAssertTrue(elements.contains { $0.accessibilityLabel() == "Cancel export of layers.png" })
        XCTAssertTrue(elements.contains { $0.accessibilityIdentifier() == "document-export-progress" })
        let group = try XCTUnwrap(elements.first { $0.accessibilityLabel() == "Export of layers.png" })
        XCTAssertEqual(group.accessibilityRole(), .group)
        for element in elements where element.accessibilityRole() == .staticText || element.accessibilityRole() == .progressIndicator || element.accessibilityRole() == .button {
            let frame = element.accessibilityFrame()
            // Unordered windows return the window from AppKit root hit-testing.
            // Reachability above uses the root tree; exercise our routing on its HUD.
            let hit = hud.accessibilityHitTest(NSPoint(x: frame.midX, y: frame.midY)) as? any NSAccessibilityProtocol
            XCTAssertEqual(hit?.accessibilityRole(), element.accessibilityRole())
            XCTAssertEqual(hit?.accessibilityLabel(), element.accessibilityLabel())
            XCTAssertEqual(hit?.accessibilityValue() as? NSObject, element.accessibilityValue() as? NSObject)
        }
        let progress = try XCTUnwrap(elements.first { $0.accessibilityRole() == .progressIndicator })
        XCTAssertEqual(progress.accessibilityLabel(), "Export progress for layers.png")
        XCTAssertEqual((progress.accessibilityValue() as? NSNumber)?.doubleValue, 0.625)
        XCTAssertEqual(hud.accessibilityIdentifier(), "document-export-progress")
        let cancel = try XCTUnwrap(views.compactMap { $0 as? NSButton }.first)
        XCTAssertEqual(cancel.accessibilityLabel(), "Cancel export of layers.png")
        ws.cancelExportFlat(task)
        XCTAssertFalse(cancel.isEnabled)
        XCTAssertTrue(accessibility(host).contains { ($0.accessibilityValue() as? String) == "Cancelling 63 %" })
    }

    func testAccessibilityNotificationsForPhaseOnlyChangesAndCancel() throws {
        LayoutProbeHarness.prepare()
        let ws = DocumentWorkspace()
        var notifications: [(AnyObject, NSAccessibility.Notification)] = []
        let hud = FlatExportProgressView(workspace: ws) { element, notification in
            notifications.append((element as AnyObject, notification))
        }
        let cancellations = Changes()
        let task = FlatExportTask(fileName: "notifications.png", documentTitle: "Notifications",
                                  cancel: { cancellations.changed() })
        hud.update([task])
        let row = try XCTUnwrap(hud.subviews.first)
        let children = try XCTUnwrap(row.accessibilityChildren())
        let phase = try XCTUnwrap(children[1] as? any NSAccessibilityProtocol)
        let progress = try XCTUnwrap(children[2] as? any NSAccessibilityProtocol)
        func expect(_ expected: [any NSAccessibilityProtocol], file: StaticString = #filePath, line: UInt = #line) {
            XCTAssertEqual(notifications.count, expected.count, file: file, line: line)
            for (actual, target) in zip(notifications, expected) {
                XCTAssertTrue(actual.0 === (target as AnyObject), file: file, line: line)
                XCTAssertEqual(actual.1, .valueChanged, file: file, line: line)
            }
            notifications.removeAll()
        }
        expect([phase, progress])
        hud.update([])
        let preparing = FlatExportTask(fileName: "reused.png", documentTitle: "Reused", cancel: {})
        hud.update([preparing])
        expect([phase, progress]) // Same Preparing 0 %, new task identity.
        hud.update([task])
        expect([phase, progress])
        task.update(0.625, "Encoding")
        expect([phase, progress])
        task.update(0.625, "Writing")
        XCTAssertEqual(phase.accessibilityValue() as? String, "Writing 63 %")
        expect([phase])
        hud.update([task]) // Republishing identical values must remain silent.
        expect([])
        task.update(0.626, "Writing") // Fraction changes within the same rounded percent.
        expect([progress])
        let cancel = try XCTUnwrap(row.subviews.compactMap { $0 as? NSButton }.first)
        cancel.performClick(nil)
        XCTAssertEqual(cancellations.value, 1)
        XCTAssertFalse(cancel.isEnabled)
        XCTAssertEqual(phase.accessibilityValue() as? String, "Cancelling 63 %")
        expect([phase])
        hud.update([task])
        expect([])

        let finishing = FlatExportTask(fileName: "finished.png", documentTitle: "Finished", cancel: {})
        hud.update([finishing]) // Exercise the reused row as well.
        expect([phase, progress])
        finishing.update(1, "Encoding")
        expect([phase, progress])
        finishing.update(1, "Complete")
        XCTAssertEqual(phase.accessibilityValue() as? String, "Complete 100 %")
        expect([phase])
        hud.update([finishing])
        expect([])
    }

    func testViewportShrinkRepositionsActiveExport() throws {
        LayoutProbeHarness.prepare()
        let ws = DocumentWorkspace()
        for flipped in [false, true] {
            let host: NSView = flipped ? FlippedHost() : NSView()
            host.frame = NSRect(x: 0, y: 0, width: 700, height: 300)
            let hud = FlatExportProgressView(workspace: ws)
            host.addSubview(hud)
            let task = FlatExportTask(fileName: "fixture.png", documentTitle: "Fixture", cancel: {})
            hud.update([task])
            hud.placeInViewport()
            task.update(0.5, "Encoding")
            host.setFrameSize(NSSize(width: 260, height: 300))
            XCTAssertTrue(host.bounds.contains(hud.frame), "HUD spilled after viewport narrowed: \(hud.frame)")
            XCTAssertEqual(hud.subviews.first?.frame.width, hud.bounds.width)
        }
    }

    private final class FlippedHost: NSView {
        override var isFlipped: Bool { true }
    }

    func testOffscreenFixtureExportAppearance() async throws {
        LayoutProbeHarness.prepare()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 440, height: 160),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("hud-fixture-\(UUID())")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let engine = try Engine.open(appSupportDir: directory.appendingPathComponent("support").path)
        let backend = try EngineDocumentEngine.for(engine).newDocument(width: 1600, height: 1200, depth: .u8, profile: nil)
        _ = try backend.addLayer(kind: .fill(json: #"{"kind":"solid","color":[0.9,0.4,0.1]}"#), name: "Fixture", parent: nil, index: nil)
        let ws = DocumentWorkspace()
        try ws.install(backend)
        let doc = try XCTUnwrap(ws.current)
        defer { ws.discard(doc) }
        ws.exportWindow = window
        var outcome: FlatExportTask.Outcome?
        let output = directory.appendingPathComponent("fixture.png")
        let task = try XCTUnwrap(ws.startExportFlat(doc, ExportFlatSettings(), to: output) { outcome = $0 })
        // Hold the main actor through both renders so completion cannot remove the row.
        // Fix the displayed fraction to make the appearance evidence reproducible.
        task.update(0.625, "Encoding")
        let hud = try XCTUnwrap(task.progressHost?.subviews.compactMap { $0 as? FlatExportProgressView }.first)
        let row = try XCTUnwrap(hud.subviews.first)
        let text = layers(row.layer).compactMap { $0 as? CATextLayer }
        XCTAssertEqual(text.count, 2)
        for layer in text {
            XCTAssertTrue(row.bounds.contains(layer.frame))
            XCTAssertTrue(layer.isGeometryFlipped, "Text must compensate for the flipped layer-backed row")
        }
        let name = try XCTUnwrap(text.first { ($0.string as? String) == "Exporting fixture.png" })
        XCTAssertLessThan(name.frame.maxY, 34)
        for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
            window.appearance = NSAppearance(named: appearance)
            hud.appearance = window.appearance
            hud.layoutSubtreeIfNeeded()
            row.viewDidChangeEffectiveAppearance()
            CATransaction.flush()
            let bitmap = try XCTUnwrap(hud.bitmapImageRepForCachingDisplay(in: hud.bounds))
            hud.cacheDisplay(in: hud.bounds, to: bitmap)
            let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
            if let directory = ProcessInfo.processInfo.environment["TESSERA_HUD_EVIDENCE"] {
                try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent("hud-\(suffix).png"))
            }
        }
        XCTAssertFalse(window.isVisible)
        for _ in 0..<1000 {
            if outcome != nil { break }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertEqual(outcome, .exported)
        XCTAssertTrue(FileManager.default.fileExists(atPath: output.path))
    }

    private func descendants(_ view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap { descendants($0) }
    }
    private func layers(_ layer: CALayer?) -> [CALayer] {
        guard let layer else { return [] }
        return [layer] + (layer.sublayers ?? []).flatMap { layers($0) }
    }
    private func accessibility(_ element: any NSAccessibilityProtocol) -> [any NSAccessibilityProtocol] {
        guard let root = NSAccessibility.unignoredDescendant(of: element) as? any NSAccessibilityProtocol else { return [] }
        return [root] + NSAccessibility.unignoredChildren(from: root.accessibilityChildren() ?? [])
            .compactMap { $0 as? any NSAccessibilityProtocol }.flatMap { accessibility($0) }
    }
}
