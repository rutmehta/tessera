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
        w.saveSheetDetachmentObserver = { _, detached in detached(); return {} }
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
        let presented = try XCTUnwrap(w.saveAsRequest)
        XCTAssertTrue(w.saveAsPresentationWillPresent(presented.id))
        w.finishSaveAs(presented)
        w.saveAsPresentationDidDismiss(try XCTUnwrap(w.saveAsPresentationID))
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
        XCTAssertTrue(w.saveAsPresentationWillPresent(r.id))
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
        XCTAssertTrue(w.saveAsPresentationWillPresent(r.id))
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
        XCTAssertTrue(w.saveAsPresentationWillPresent(r.id))
        var next: [DocumentSaveOutcome] = []
        let nextID = w.saveForPreparation(d, saveAs: true) { next.append($0) }
        w.finishSaveAs(r); w.saveAsSheetDidDisappear(r.id)
        w.saveAsPresentationDidDismiss(r.id)
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
        let presented = try XCTUnwrap(w.saveAsRequest)
        XCTAssertTrue(w.saveAsPresentationWillPresent(presented.id))
        w.finishSaveAs(presented)
        w.saveAsPresentationDidDismiss(id)
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
        XCTAssertTrue(w.saveAsPresentationWillPresent(r.id))
        w.finishSaveAs(r)
        var conflict: [DocumentSaveOutcome] = []
        w.saveForPreparation(d, saveAs: true) { conflict.append($0) }
        XCTAssertEqual(conflict, [.failed("A save for this document is still running")])
        w.saveAsPresentationDidDismiss(r.id)
        let next = w.saveForPreparation(other, saveAs: true) { _ in }
        finish?(.success(())); finish?(.success(()))
        XCTAssertEqual(first.count, 1)
        XCTAssertEqual(w.saveAsRequest?.id, next)
        w.cancelDocumentSave(next)
    }

    func testReplacementWaitsForNativeDismissNotViewDisappearance() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var prompts = 0
        var reply: (@MainActor (Bool) -> Void)?
        w.saveReplacePrompt = { _, done in prompts += 1; reply = done }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        let presented = try XCTUnwrap(w.saveAsRequest)
        XCTAssertTrue(w.saveAsPresentationWillPresent(presented.id))
        w.finishSaveAs(presented)
        w.saveAsSheetDidDisappear(id)
        XCTAssertEqual(prompts, 0)
        XCTAssertTrue(outcomes.isEmpty)
        w.saveAsPresentationDidDismiss(id)
        w.saveAsPresentationDidDismiss(id)
        XCTAssertEqual(prompts, 1)
        reply?(false)
        XCTAssertEqual(outcomes, [.cancelled])
    }

    func testCancelDuringDismissGapCannotStartReplacementOrWrite() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var prompts = 0
        w.saveReplacePrompt = { _, _ in prompts += 1 }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        let presented = try XCTUnwrap(w.saveAsRequest)
        XCTAssertTrue(w.saveAsPresentationWillPresent(presented.id))
        w.finishSaveAs(presented)
        w.cancelDocumentSave(id)
        w.saveAsPresentationDidDismiss(id)
        XCTAssertEqual(prompts, 0)
        XCTAssertEqual(outcomes, [.cancelled])
    }

    func testSupersessionInDismissGapIgnoresStaleDismissal() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var prompts = 0
        w.saveReplacePrompt = { _, _ in prompts += 1 }
        var old: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { old.append($0) }
        let presented = try XCTUnwrap(w.saveAsRequest)
        XCTAssertTrue(w.saveAsPresentationWillPresent(presented.id))
        w.finishSaveAs(presented)
        let replacement = w.saveForPreparation(d, saveAs: true) { _ in }
        XCTAssertNil(w.saveAsRequest, "next sheet waits for old native dismissal")
        w.saveAsPresentationDidDismiss(id)
        XCTAssertEqual(w.saveAsRequest?.id, replacement)
        w.saveAsPresentationDidDismiss(id)
        XCTAssertEqual(w.saveAsRequest?.id, replacement)
        XCTAssertEqual(old, [.cancelled])
        XCTAssertEqual(prompts, 0)
        w.cancelDocumentSave(replacement)
    }

    func testWindowLossInDismissGapBlocksReplacement() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var prompts = 0
        w.saveReplacePrompt = { _, _ in prompts += 1 }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        let presented = try XCTUnwrap(w.saveAsRequest)
        XCTAssertTrue(w.saveAsPresentationWillPresent(presented.id))
        w.finishSaveAs(presented)
        w.documentSaveWindowLost(id)
        w.saveAsPresentationDidDismiss(id)
        XCTAssertEqual(prompts, 0)
        XCTAssertEqual(outcomes, [.failed("Document window closed before save")])
    }

    func testBeginCancelBeginWithoutAppearanceDoesNotQueueForever() throws {
        let (w, d) = try fixture()
        var first: [DocumentSaveOutcome] = []
        let firstID = w.saveForPreparation(d, saveAs: true) { first.append($0) }
        XCTAssertNil(w.saveAsPresentationID)
        w.cancelDocumentSave(firstID)
        let secondID = w.saveForPreparation(d, saveAs: true) { _ in }
        XCTAssertEqual(first, [.cancelled])
        XCTAssertEqual(w.saveAsRequest?.id, secondID)
        XCTAssertNil(w.saveAsPresentationID)
        w.cancelDocumentSave(secondID)
    }

    func testSupersedeUnclaimedRequestNeedsNoNativeDismissal() throws {
        let (w, d) = try fixture()
        var first: [DocumentSaveOutcome] = []
        let old = w.saveForPreparation(d, saveAs: true) { first.append($0) }
        let next = w.saveForPreparation(d, saveAs: true) { _ in }
        w.saveAsPresentationDidDismiss(old)
        XCTAssertEqual(first, [.cancelled])
        XCTAssertEqual(w.saveAsRequest?.id, next)
        XCTAssertNil(w.saveAsPresentationID)
        w.cancelDocumentSave(next)
    }

    func testClaimBeforeOnAppearRetainsNativeDismissalBarrier() throws {
        let (w, d) = try fixture()
        let old = w.saveForPreparation(d, saveAs: true) { _ in }
        XCTAssertTrue(w.saveAsPresentationWillPresent(old))
        w.cancelDocumentSave(old) // The content was claimed, but onAppear has not run.
        let next = w.saveForPreparation(d, saveAs: true) { _ in }
        XCTAssertNil(w.saveAsRequest)
        XCTAssertEqual(w.saveAsPresentationID, old)
        w.saveAsPresentationDidDismiss(old)
        XCTAssertEqual(w.saveAsRequest?.id, next)
        XCTAssertNil(w.saveAsPresentationID)
        w.cancelDocumentSave(next)
    }

    func testLateClaimOfCancelledRequestSerializesNewRequest() throws {
        let (w, d) = try fixture()
        let old = w.saveForPreparation(d, saveAs: true) { _ in }
        w.cancelDocumentSave(old)
        let next = w.saveForPreparation(d, saveAs: true) { _ in }
        XCTAssertTrue(w.saveAsPresentationWillPresent(old))
        XCTAssertNil(w.saveAsRequest)
        XCTAssertEqual(w.saveAsPresentationID, old)
        w.saveAsPresentationDidDismiss(old)
        XCTAssertEqual(w.saveAsRequest?.id, next)
        w.cancelDocumentSave(next)
    }

    func testSwiftDismissWaitsForNativeDetachmentAndIgnoresDuplicates() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var detached: (@MainActor () -> Void)?
        var removals = 0
        w.saveSheetDetachmentObserver = { _, callback in detached = callback; return { removals += 1 } }
        var prompts = 0
        w.saveReplacePrompt = { _, _ in prompts += 1 }
        let id = w.saveForPreparation(d, saveAs: true) { _ in }
        XCTAssertTrue(w.saveAsPresentationWillPresent(id))
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        XCTAssertNotNil(detached, "observe before triggering sheet dismissal")
        w.saveAsPresentationDidDismiss(id)
        XCTAssertEqual(prompts, 0)
        XCTAssertEqual(removals, 0)
        detached?(); detached?(); w.saveAsPresentationDidDismiss(id)
        XCTAssertEqual(prompts, 1)
        XCTAssertEqual(removals, 1)
        w.cancelDocumentSave(id)
    }

    func testCancelWhileNativeSheetAttachedNeverAdvancesReplacement() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var detached: (@MainActor () -> Void)?
        var removals = 0
        w.saveSheetDetachmentObserver = { _, callback in detached = callback; return { removals += 1 } }
        var prompts = 0
        w.saveReplacePrompt = { _, _ in prompts += 1 }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        XCTAssertTrue(w.saveAsPresentationWillPresent(id))
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        w.saveAsPresentationDidDismiss(id)
        w.cancelDocumentSave(id)
        detached?(); detached?()
        XCTAssertEqual(outcomes, [.cancelled])
        XCTAssertEqual(prompts, 0)
        XCTAssertEqual(removals, 1)
    }

    func testWindowLossRemovesNativeObserverAndStaleNotificationIsInert() throws {
        let (w, d) = try fixture()
        w.saveFileExists = { _ in true }
        var detached: (@MainActor () -> Void)?
        var removals = 0
        w.saveSheetDetachmentObserver = { _, callback in detached = callback; return { removals += 1 } }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(d, saveAs: true) { outcomes.append($0) }
        XCTAssertTrue(w.saveAsPresentationWillPresent(id))
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        w.saveAsPresentationDidDismiss(id)
        w.documentSaveWindowLost(id)
        detached?()
        XCTAssertEqual(outcomes, [.failed("Document window closed before save")])
        XCTAssertEqual(removals, 1)
        XCTAssertNil(w.saveAsPresentationID)
    }

}
