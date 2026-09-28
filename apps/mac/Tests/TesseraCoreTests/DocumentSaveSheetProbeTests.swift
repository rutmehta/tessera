import AppKit
import XCTest
@testable import Tessera

// UNRUN on B. Hidden AppKit window only; no sheet, backend save, timer or GPU work.
@MainActor
final class DocumentSaveSheetProbeTests: XCTestCase {
    func testReusedAttachedViewImmediatelyReportsUpdatedOwnerAndRequest() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 16, height: 16),
                              styleMask: [], backing: .buffered, defer: true)
        let view = DocumentSaveSheetWindowProbe.ProbeView()
        window.contentView = view
        let firstOwner = DocumentWorkspace(), secondOwner = DocumentWorkspace()
        let firstID = UUID(), secondID = UUID()
        var reports: [(ObjectIdentifier, UUID, ObjectIdentifier)] = []
        let report: @MainActor (DocumentWorkspace, UUID, NSWindow) -> Void = { owner, id, window in
            reports.append((ObjectIdentifier(owner), id, ObjectIdentifier(window)))
        }
        var first = DocumentSaveSheetWindowProbe(workspace: firstOwner, requestID: firstID)
        first.reportWindow = report
        first.refreshCapture(on: view)
        XCTAssertEqual(reports.count, 1)
        XCTAssertEqual(reports.last?.1, firstID)
        var second = DocumentSaveSheetWindowProbe(workspace: secondOwner, requestID: secondID)
        second.reportWindow = report
        second.refreshCapture(on: view) // updateNSView path, no new window attachment event.
        XCTAssertEqual(reports.count, 2)
        XCTAssertEqual(reports.last?.0, ObjectIdentifier(secondOwner))
        XCTAssertEqual(reports.last?.1, secondID)
        XCTAssertEqual(reports.last?.2, ObjectIdentifier(window))
        view.viewDidMoveToWindow()
        XCTAssertEqual(reports.count, 3)
        XCTAssertEqual(reports.last?.1, secondID)
        view.capture = nil
        window.contentView = nil
    }

    func testCaptureDoesNotKeepFormerWorkspaceAlive() {
        let view = DocumentSaveSheetWindowProbe.ProbeView()
        weak var observed: DocumentWorkspace?
        do {
            let owner = DocumentWorkspace()
            observed = owner
            DocumentSaveSheetWindowProbe(workspace: owner, requestID: UUID()).refreshCapture(on: view)
        }
        XCTAssertNil(observed)
        view.capture = nil
    }
}
