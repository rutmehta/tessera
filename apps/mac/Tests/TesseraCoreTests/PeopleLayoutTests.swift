import AppKit
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor
final class PeopleLayoutTests: XCTestCase {
    func testDetailFitsWindowAndKeepsHeaderAtTop() {
        let model = AppModel()
        model.install(StubLibrary.synthetic(count: 40))
        let engine = StubPeopleEngine()
        engine.add("person", (0..<40).map { ($0, UInt32(0)) })
        model.people.install(engine)
        model.people.reload()
        model.setSource(.people)
        model.people.openDetail("person")

        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 600),
                              styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        let host = NSHostingView(rootView: ContentView(model: model))
        window.contentView = host
        window.makeKeyAndOrderFront(nil)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        host.layoutSubtreeIfNeeded()

        func descendants(_ view: NSView) -> [NSView] {
            [view] + view.subviews.flatMap(descendants)
        }
        let outlines = descendants(host).compactMap { $0 as? NSOutlineView }
        let sidebar = try? XCTUnwrap(outlines.first)
        guard let sidebar else { return }
        let frame = sidebar.convert(sidebar.bounds, to: host)
        XCTAssertLessThanOrEqual(host.bounds.height, 700,
                                 "detail must not make the hosting view taller than the window")
        XCTAssertGreaterThanOrEqual(frame.minY, 0, "detail must not push the sidebar above the window")
        XCTAssertLessThanOrEqual(frame.maxY, host.bounds.maxY + 1, "detail must not expand the content beyond the window")
        window.orderOut(nil)
    }
}
