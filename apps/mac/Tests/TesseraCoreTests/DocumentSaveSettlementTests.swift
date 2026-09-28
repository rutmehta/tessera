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
        var reply: (@MainActor (Bool) -> Void)?
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
        var finish: (@MainActor (Result<Void, Error>) -> Void)?
        defer { finish = nil; w.saveWriter = nil }
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
        var reply: (@MainActor (Bool) -> Void)?
        w.saveReplacePrompt = { _, done in reply = done }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        w.documentSaveWindowLost(id)
        reply?(true)
        XCTAssertEqual(outcomes, [.failed("Document window closed before save")])
    }
    func testWrongControllerAndIdentitylessDismissCannotCancelReplacement() throws {
        let (w, d) = try fixture()
        let other = try DocumentController(backend: StubDocumentBackend())
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        let real = try XCTUnwrap(w.saveAsRequest)
        let impostor = SaveAsRequest(id: id, doc: other, name: real.name, folder: real.folder)
        w.finishSaveAs(impostor)
        w.saveAsRequest = nil // Shell setter carries no identity; sheet callback must settle.
        XCTAssertEqual(w.saveAsRequest?.id, id)
        XCTAssertTrue(outcomes.isEmpty)
        w.saveAsSheetDidDisappear(id)
        XCTAssertEqual(outcomes, [.cancelled])
    }

    func testOldWriteCompletionPreservesNewSheetAndBlocksSameDocumentWrite() throws {
        let (w, d) = try fixture()
        let other = try DocumentController(backend: StubDocumentBackend())
        var finish: (@MainActor (Result<Void, Error>) -> Void)?
        defer { finish = nil; w.saveWriter = nil }
        w.saveWriter = { _, _, done in finish = done }
        var first: [DocumentSaveOutcome] = []
        w.saveForPreparation(d, saveAs: true) { first.append($0) }
        let r = try XCTUnwrap(w.saveAsRequest)
        w.finishSaveAs(r)
        var conflict: [DocumentSaveOutcome] = []
        w.saveForPreparation(d, saveAs: true) { conflict.append($0) }
        XCTAssertEqual(conflict, [.failed("A save for this document is still running")])
        let next = w.saveForPreparation(other, saveAs: true) { _ in }
        finish?(.success(())); finish?(.success(()))
        XCTAssertEqual(first.count, 1)
        XCTAssertEqual(w.saveAsRequest?.id, next)
        w.cancelDocumentSave(next)
    }

}
