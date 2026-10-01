import AppKit
import Observation
import QuartzCore
import XCTest
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
        _ = NSApplication.shared
        let ws = DocumentWorkspace()
        let hud = FlatExportProgressView(workspace: ws)
        hud.frame = NSRect(x: 0, y: 0, width: 400, height: 64)
        let task = FlatExportTask(fileName: "layers.png", documentTitle: "Layers", cancel: {})
        hud.update([task])
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
        let elements = accessibility(hud)
        XCTAssertTrue(elements.contains { ($0.accessibilityValue() as? String) == "Encoding 63 %" })
        let progress = try XCTUnwrap(elements.first { $0.accessibilityRole() == .progressIndicator })
        XCTAssertEqual(progress.accessibilityLabel(), "Export progress for layers.png")
        XCTAssertEqual((progress.accessibilityValue() as? NSNumber)?.doubleValue, 0.625)
        XCTAssertEqual(hud.accessibilityIdentifier(), "document-export-progress")
        let cancel = try XCTUnwrap(views.compactMap { $0 as? NSButton }.first)
        XCTAssertEqual(cancel.accessibilityLabel(), "Cancel export of layers.png")
        ws.cancelExportFlat(task)
        XCTAssertFalse(cancel.isEnabled)
        XCTAssertTrue(accessibility(hud).contains { ($0.accessibilityValue() as? String) == "Cancelling 63 %" })
    }

    private func descendants(_ view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap { descendants($0) }
    }
    private func layers(_ layer: CALayer?) -> [CALayer] {
        guard let layer else { return [] }
        return [layer] + (layer.sublayers ?? []).flatMap { layers($0) }
    }
    private func accessibility(_ element: any NSAccessibilityProtocol) -> [any NSAccessibilityProtocol] {
        [element] + (element.accessibilityChildren() ?? []).compactMap { $0 as? any NSAccessibilityProtocol }
            .flatMap { accessibility($0) }
    }
}
