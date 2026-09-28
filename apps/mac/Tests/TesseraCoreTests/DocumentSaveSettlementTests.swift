import Foundation
import XCTest
@testable import Tessera
@testable import TesseraCore

// UNRUN on B. Injected continuations avoid native sheets, disk writes and GPU.
@MainActor
final class DocumentSaveSettlementTests: XCTestCase {
    private func fixture() throws -> (DocumentWorkspace, DocumentController) {
        let w = DocumentWorkspace()
        let d = try DocumentController(backend: StubDocumentBackend())
        w.saveHasWindow = { true }
        w.saveFileExists = { _ in false }
        return (w, d)
    }

    func testMissingWindowFailsClosed() throws {
        let (w, d) = try fixture()
        w.saveHasWindow = { false }
        var outcomes: [DocumentSaveOutcome] = []
        var writes = 0
        w.saveWriter = { _, _, done in writes += 1; done(.success(())) }
        w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        XCTAssertEqual(outcomes, [.failed("Save requires a document window")])
        XCTAssertEqual(writes, 0)
        XCTAssertFalse(d.isClosed)
    }

    func testCancelAndDismissSettleOnlyOnce() throws {
        let (w, d) = try fixture()
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        w.cancelDocumentSave(id)
        w.saveAsSheetDidDisappear(id)
        w.cancelDocumentSave(id)
        XCTAssertEqual(outcomes, [.cancelled])
        XCTAssertNil(w.saveAsRequest)
    }

    func testReplaceCancelAndDuplicateAcceptanceCannotWrite() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var reply: ((Bool) -> Void)?
        w.saveReplacePrompt = { _, done in reply = done }
        var writes = 0
        w.saveWriter = { _, _, done in writes += 1; done(.success(())) }
        var outcomes: [DocumentSaveOutcome] = []
        w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        reply?(false); reply?(true)
        XCTAssertEqual(writes, 0)
        XCTAssertEqual(outcomes, [.cancelled])
    }

    func testWriteFailureDoesNotChangeLastFolderOrReportSuccess() throws {
        let (w, d) = try fixture()
        let old = URL(fileURLWithPath: "/old")
        w.lastSaveFolder = old
        w.saveWriter = { _, _, done in done(.failure(NSError(domain: "injected", code: 4))) }
        var outcomes: [DocumentSaveOutcome] = []
        w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        var r = try XCTUnwrap(w.saveAsRequest)
        r.folder = URL(fileURLWithPath: "/new")
        w.finishSaveAs(r)
        XCTAssertEqual(outcomes.count, 1)
        guard case .failed = outcomes[0] else { return XCTFail("Expected genuine write failure") }
        XCTAssertEqual(w.lastSaveFolder, old)
        XCTAssertFalse(d.isClosed)
    }

    func testAdmittedWriteSurvivesCancelAndSettlesActualOutcomeOnce() throws {
        let (w, d) = try fixture()
        var finish: ((Result<Void, Error>) -> Void)?
        var writes = 0
        w.saveWriter = { _, _, done in writes += 1; finish = done }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        let r = try XCTUnwrap(w.saveAsRequest)
        w.finishSaveAs(r); w.finishSaveAs(r)
        w.cancelDocumentSave(id)
        XCTAssertTrue(outcomes.isEmpty)
        finish?(.success(())); finish?(.success(()))
        XCTAssertEqual(writes, 1)
        XCTAssertEqual(outcomes, [.saved(r.url, continuationCancelled: true)])
        XCTAssertEqual(w.lastSaveFolder, r.folder)
        XCTAssertFalse(d.isClosed)
    }

    func testSupersededPromptCannotAffectReplacement() throws {
        let (w, d) = try fixture()
        var old: [DocumentSaveOutcome] = []
        w.saveForPreparation(d, saveAs: true) { old.append($0) }
        let r = try XCTUnwrap(w.saveAsRequest)
        var next: [DocumentSaveOutcome] = []
        let nextID = w.saveForPreparation(d, saveAs: true) { next.append($0) }
        w.finishSaveAs(r); w.saveAsSheetDidDisappear(r.id)
        XCTAssertEqual(old, [.cancelled])
        XCTAssertEqual(w.saveAsRequest?.id, nextID)
        XCTAssertTrue(next.isEmpty)
        w.cancelDocumentSave(nextID)
    }

    func testLostWindowWhileReplacingSettlesFailureAndIgnoresLateApproval() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var reply: ((Bool) -> Void)?
        w.saveReplacePrompt = { _, done in reply = done }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        w.documentSaveWindowLost(id)
        reply?(true)
        XCTAssertEqual(outcomes, [.failed("Document window closed before save")])
    }
}
