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

/// Edit ▸ Undo / Redo follow the People view (WP M2-44).
@MainActor
final class PeopleUndoMenuTests: XCTestCase {
    func testUndoRedoCoverPeopleEditsWhileThePeopleViewIsFrontmost() {
        let model = AppModel()
        model.install(StubLibrary.synthetic(count: 8))
        let engine = StubPeopleEngine()
        engine.add("ada", [(0, 0), (1, 0)])
        engine.add("ben", [(2, 0)])
        engine.names["ada"] = "Ada"
        model.people.install(engine)
        model.people.reload()
        model.people.selection = ["ada", "ben"]
        XCTAssertTrue(model.people.mergeSelection())

        XCTAssertEqual(model.undoMenuTitle, "Undo", "outside the People view ⌘Z addresses culling")
        model.setSource(.people)
        XCTAssertEqual(model.undoMenuTitle, "Undo Merge People")
        XCTAssertEqual(model.redoMenuTitle, "Redo")
        model.undo()
        XCTAssertTrue(engine.calls.contains("undo"))
        XCTAssertEqual(model.statusMessage, "Undo Merge People")
        XCTAssertEqual(model.people.tiles.map(\.id), ["ada", "ben"])
        XCTAssertEqual(model.redoMenuTitle, "Redo Merge People")

        // In the detail view too.
        model.people.openDetail("ada")
        model.redo()
        XCTAssertTrue(engine.calls.contains("redo"))
        XCTAssertEqual(model.statusMessage, "Redo Merge People")
        XCTAssertNil(model.people.person("ben"))
        XCTAssertEqual(model.people.detail?.members.count, 3)

        model.setSource(.all)
        XCTAssertEqual(model.undoMenuTitle, "Undo")
        let before = engine.calls.filter { $0 == "undo" }.count
        model.undo()
        XCTAssertEqual(engine.calls.filter { $0 == "undo" }.count, before, "culling undo leaves people history alone")
    }
}
