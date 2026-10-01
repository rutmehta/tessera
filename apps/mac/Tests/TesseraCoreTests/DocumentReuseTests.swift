import Foundation
import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor
final class DocumentReuseTests: XCTestCase {
    private func photo(id: Int = 0) -> PhotoItem {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return PhotoItem(id: id, url: root.appendingPathComponent("fixtures/golden/sample.png"),
                         name: "sample.png", kind: .jpeg,
                         captureDate: Date(timeIntervalSince1970: 0), pixelWidth: 2, pixelHeight: 2)
    }

    func testSecondOpenSelectsExistingDocumentWithoutThirdTab() throws {
        let workspace = DocumentWorkspace()
        workspace.engine = StubDocumentEngine()
        defer { workspace.documents.forEach { $0.close() } }
        workspace.editInLayers(photo())
        let original = try XCTUnwrap(workspace.current)
        workspace.newDocument(NewDocumentSettings())
        XCTAssertEqual(workspace.documents.count, 2)
        XCTAssertFalse(workspace.current === original)

        var outcomes: [DocumentLoadOutcome] = []
        // Library row indices can change; the source file still identifies this photo.
        workspace.editInLayers(photo(id: 42)) { outcomes.append($0) }

        XCTAssertTrue(workspace.current === original)
        XCTAssertEqual(workspace.documents.count, 2)
        XCTAssertEqual(outcomes, [.installed])
    }

    func testReusePreservesCallerOwnedNavigationAndStatus() throws {
        let app = AppModel(), workspace = DocumentWorkspace()
        workspace.app = app
        workspace.engine = StubDocumentEngine()
        defer { workspace.documents.forEach { $0.close() } }
        workspace.editInLayers(photo())
        let original = try XCTUnwrap(workspace.current)
        workspace.newDocument(NewDocumentSettings())
        app.viewMode = .grid
        app.statusMessage = "caller owns status"
        var outcomes: [DocumentLoadOutcome] = []

        workspace.editInLayers(photo(), statusPublication: .caller, activateDocument: false) {
            outcomes.append($0)
        }

        XCTAssertTrue(workspace.current === original)
        XCTAssertEqual(workspace.documents.count, 2)
        XCTAssertEqual(outcomes, [.installed])
        XCTAssertEqual(app.viewMode, .grid)
        XCTAssertEqual(app.statusMessage, "caller owns status")
    }

    func testEngineImageReusesDocumentBySourceImageID() throws {
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("reuse-\(UUID().uuidString)")
        let folder = scratch.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }
        try FileManager.default.copyItem(at: XCTUnwrap(photo().url), to: folder.appendingPathComponent("sample.png"))
        let library = try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support"))
        let item = try XCTUnwrap(library.items.first)
        let workspace = DocumentWorkspace()
        // Run real backend work synchronously for deterministic hosted coverage.
        workspace.documentLoadExecutor = { engine, body, done in done(Result { try body(engine) }) }
        defer { workspace.documents.forEach { $0.close() } }
        workspace.editInLayers(item)
        let original = try XCTUnwrap(workspace.current)
        XCTAssertEqual(original.info.sourceImageId, try XCTUnwrap(item.engineImage).imageID)
        workspace.newDocument(NewDocumentSettings())
        workspace.editInLayers(item)
        XCTAssertTrue(workspace.current === original)
        XCTAssertEqual(workspace.documents.count, 2)
    }
}
