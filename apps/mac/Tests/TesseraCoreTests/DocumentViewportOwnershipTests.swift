import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// UNRUN source candidate. No windows or image fixtures; viewport init may create Metal resources.
@MainActor
final class DocumentViewportOwnershipTests: XCTestCase {
    private func document() throws -> DocumentController {
        let backend = try StubDocumentEngine().newDocument(width: 16, height: 16, depth: .u8, profile: nil)
        return try DocumentController(backend: backend)
    }

    func testDetachClearsCurrentOwnerAndFrameCallback() throws {
        let doc = try document()
        defer { doc.close() }
        let view = DocumentViewportView(frame: .zero)
        view.attach(doc)
        XCTAssertTrue(doc.viewport === view)
        XCTAssertNotNil(doc.onFrame)
        view.detachFromWorkspace()
        XCTAssertNil(view.controller)
        XCTAssertNil(view.workspace)
        XCTAssertNil(doc.viewport)
        XCTAssertNil(doc.onFrame)
        view.detachFromWorkspace() // idempotent SwiftUI teardown
        XCTAssertNil(doc.onFrame)
    }

    func testControllerReleasedBeforeDismantleStillClearsLocalResources() throws {
        var doc: DocumentController? = try document()
        weak var released = doc
        let view = DocumentViewportView(frame: .zero)
        view.attach(doc)
        let surface = try XCTUnwrap(DocumentSurfaces.make(width: 1, height: 1))
        view.retainSurfaceForLifecycleTest(surface)
        let ants = try XCTUnwrap(view.subviews.compactMap { $0 as? MarchingAntsView }.first)
        ants.rect = CGRect(x: 0, y: 0, width: 1, height: 1)
        XCTAssertEqual(view.retainedSurfaceCount, 1)
        XCTAssertTrue(ants.wantsAnimation)
        doc?.close()
        doc = nil
        XCTAssertNil(released)
        XCTAssertNil(view.controller, "exercise nil weak owner before teardown")

        view.detachFromWorkspace()
        XCTAssertEqual(view.retainedSurfaceCount, 0, "dismantle must release its ring before view deallocation")
        XCTAssertNil(ants.rect)
        XCTAssertFalse(ants.wantsAnimation)
        XCTAssertNil(ants.animationTimer)
        XCTAssertFalse(view.toolOverlay.wantsAnimation)
        XCTAssertNil(view.toolOverlay.animationTimer)
        view.detachFromWorkspace()
        XCTAssertEqual(view.retainedSurfaceCount, 0)
    }

    func testStaleOwnerCannotClearNewOwnerCallback() throws {
        let doc = try document()
        defer { doc.close() }
        let stale = DocumentViewportView(frame: .zero)
        let newer = DocumentViewportView(frame: .zero)
        stale.attach(doc)
        doc.viewport = newer // model a superseded owner before delayed cleanup
        doc.onFrame = { _ in }
        stale.detachFromWorkspace()
        XCTAssertNil(stale.controller)
        XCTAssertTrue(doc.viewport === newer)
        XCTAssertNotNil(doc.onFrame)
        doc.viewport = nil
        doc.onFrame = nil
    }

    func testReplacementDetachesPriorOwnerBeforeOldViewDismantles() throws {
        let doc = try document()
        defer { doc.close() }
        let old = DocumentViewportView(frame: .zero)
        let replacement = DocumentViewportView(frame: .zero)
        old.attach(doc)
        replacement.attach(doc)
        XCTAssertNil(old.controller)
        XCTAssertTrue(doc.viewport === replacement)
        XCTAssertNotNil(doc.onFrame)
        old.detachFromWorkspace()
        XCTAssertTrue(doc.viewport === replacement)
        XCTAssertNotNil(doc.onFrame, "old teardown must preserve replacement's callback")
        replacement.detachFromWorkspace()
    }
}
