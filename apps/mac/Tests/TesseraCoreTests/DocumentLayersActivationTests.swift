import Foundation
import XCTest
@testable import Tessera
@testable import TesseraCore

// UNRUN on B. Held executor and stub backend; no decode, native sheet or GPU.
@MainActor
final class DocumentLayersActivationTests: XCTestCase {
    private var item: PhotoItem {
        PhotoItem(id: 0, url: URL(fileURLWithPath: "/fake/image.jpg"), name: "image", kind: .jpeg,
                  captureDate: Date(timeIntervalSince1970: 0), pixelWidth: 2, pixelHeight: 2)
    }

    func testHeldCallerOwnedLoadInstallsWithoutPublishingModeAndSettlesOnce() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app; w.engine = StubDocumentEngine.shared
        app.viewMode = .grid
        var finish: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
        defer { finish = nil; w.documentLoadExecutor = nil }
        w.documentLoadExecutor = { _, _, done in finish = done }
        var outcomes: [DocumentLoadOutcome] = []
        w.editInLayers(item, statusPublication: .caller, activateDocument: false) { outcomes.append($0) }
        XCTAssertTrue(outcomes.isEmpty)
        app.statusMessage = "newer navigation status"
        let backend = StubDocumentBackend()
        finish?(.success(backend)); finish?(.success(backend))
        XCTAssertEqual(outcomes, [.installed])
        XCTAssertEqual(w.documents.count, 1)
        XCTAssertTrue(w.current?.backend === backend)
        XCTAssertEqual(app.viewMode, .grid)
        XCTAssertEqual(app.statusMessage, "newer navigation status")
        XCTAssertNil(w.opening)
    }

    func testDefaultActivationIsIndependentOfCallerOwnedStatus() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app; w.engine = StubDocumentEngine.shared
        app.statusMessage = "caller status"
        w.documentLoadExecutor = { _, _, done in done(.success(StubDocumentBackend())) }
        w.editInLayers(item, statusPublication: .caller)
        XCTAssertEqual(app.viewMode, .document)
        XCTAssertEqual(app.statusMessage, "caller status")
    }

    func testSuppressedActivationStillAllowsWorkspaceStatus() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app; w.engine = StubDocumentEngine.shared
        app.viewMode = .grid
        w.documentLoadExecutor = { _, _, done in done(.success(StubDocumentBackend())) }
        w.editInLayers(item, activateDocument: false)
        XCTAssertEqual(app.viewMode, .grid)
        XCTAssertEqual(app.statusMessage, "Editing image in layers")
        XCTAssertNotNil(w.current)
    }

    func testExistingBackendInstallSelectsInternallyWithoutModePublication() throws {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app
        let first = StubDocumentBackend(), second = StubDocumentBackend()
        try w.install(first, activateDocument: false)
        try w.install(second, activateDocument: false)
        try w.install(first, activateDocument: false)
        XCTAssertEqual(w.documents.count, 2)
        XCTAssertTrue(w.current?.backend === first)
        XCTAssertEqual(app.viewMode, .grid)
        w.select(try XCTUnwrap(w.documents.last))
        XCTAssertEqual(app.viewMode, .document)
    }

    func testOpenRetainsDefaultModePublication() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app; w.engine = StubDocumentEngine.shared
        w.documentLoadExecutor = { _, _, done in done(.success(StubDocumentBackend())) }
        w.open(URL(fileURLWithPath: "/fake/image.jpg"))
        XCTAssertEqual(app.viewMode, .document)
        XCTAssertEqual(app.statusMessage, "Opened image.jpg")
    }
}
