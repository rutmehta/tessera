import Foundation
import XCTest
@testable import Tessera
@testable import TesseraCore

// UNRUN on B: hold backend completions, no image decode or native presentation.
@MainActor
final class DocumentLoadStatusTests: XCTestCase {
    private var item: PhotoItem {
        PhotoItem(id: 0, url: URL(fileURLWithPath: "/fake/image.jpg"), name: "image", kind: .jpeg,
                  captureDate: Date(timeIntervalSince1970: 0), pixelWidth: 2, pixelHeight: 2)
    }

    func testCallerOwnedSuccessPreservesStatusAndStillInstallsAndSettles() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app
        var finish: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
        defer { finish = nil; w.documentLoadExecutor = nil }
        w.documentLoadExecutor = { _, _, done in finish = done }
        app.statusMessage = "caller start"
        var outcomes: [DocumentLoadOutcome] = []
        w.editInLayers(item, statusPublication: .caller) { outcomes.append($0) }
        XCTAssertEqual(app.statusMessage, "caller start")
        app.statusMessage = "newer owner status"
        finish?(.success(StubDocumentBackend()))
        XCTAssertEqual(app.statusMessage, "newer owner status")
        XCTAssertEqual(outcomes, [.installed])
        XCTAssertEqual(w.documents.count, 1)
        XCTAssertNil(w.opening)
    }

    func testCallerOwnedFailureDoesNotOverwriteNewerStatus() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app
        var finish: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
        defer { finish = nil; w.documentLoadExecutor = nil }
        w.documentLoadExecutor = { _, _, done in finish = done }
        var outcomes: [DocumentLoadOutcome] = []
        w.editInLayers(item, statusPublication: .caller) { outcomes.append($0) }
        app.statusMessage = "new library status"
        finish?(.failure(NSError(domain: "held failure", code: 1)))
        XCTAssertEqual(app.statusMessage, "new library status")
        XCTAssertEqual(outcomes.count, 1)
        guard case .failed = outcomes[0] else { return XCTFail("Expected failure") }
        XCTAssertNil(w.opening)
        XCTAssertTrue(w.documents.isEmpty)
    }

    func testCallerOwnedRejectionDoesNotPublishStatus() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app
        app.statusMessage = "caller owns rejection"
        var outcomes: [DocumentLoadOutcome] = []
        w.editInLayers(nil, statusPublication: .caller) { outcomes.append($0) }
        XCTAssertEqual(app.statusMessage, "caller owns rejection")
        XCTAssertEqual(outcomes, [.rejected("Edit in Layers: select a photo first")])
    }

    func testDefaultRetainsStartFailureAndRejectionMessages() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app
        var finish: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
        defer { finish = nil; w.documentLoadExecutor = nil }
        w.documentLoadExecutor = { _, _, done in finish = done }
        w.editInLayers(item)
        XCTAssertEqual(app.statusMessage, "Edit image in Layers…")
        finish?(.failure(NSError(domain: "legacy failure", code: 1)))
        XCTAssertTrue(app.statusMessage?.hasPrefix("Edit image in Layers:") == true)
        w.editInLayers(nil)
        XCTAssertEqual(app.statusMessage, "Edit in Layers: select a photo first")
    }

    func testDefaultSuccessStillPublishesLegacyMessage() {
        let app = AppModel(), w = DocumentWorkspace()
        w.app = app
        w.documentLoadExecutor = { _, _, done in done(.success(StubDocumentBackend())) }
        w.editInLayers(item)
        XCTAssertEqual(app.statusMessage, "Editing image in layers")
    }
}
