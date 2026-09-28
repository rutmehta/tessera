import Foundation
import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

// SOURCE ONLY / UNRUN. These tests replace inferred SwiftUI probe callbacks with
// owned native invocation callbacks, retaining outcome and ordering requirements.
@MainActor
final class DocumentSaveSettlementTests: XCTestCase {
    private final class Fixture {
        let workspace = DocumentWorkspace()
        let document: DocumentController
        let windowIdentity = NSObject()
        var sessions: [FakeDocumentSaveSession] = []
        var tokens: [DocumentSavePresentationToken] = []
        init() throws {
            document = try DocumentController(backend: StubDocumentBackend())
            workspace.saveHasWindow = { true }
            workspace.saveFileExists = { _ in false }
            let binding = UUID()
            workspace.savePresenter.registerBinding(binding)
            workspace.savePresenter.updateBinding(binding, windowID: ObjectIdentifier(windowIdentity)) { [weak self] token, _, _ in
                guard let self else { return nil }
                let session = FakeDocumentSaveSession()
                sessions.append(session); tokens.append(token)
                return session
            }
        }
        func drain(_ index: Int, response: Int = NSApplication.ModalResponse.cancel.rawValue) {
            sessions[index].complete(response); sessions[index].detach()
        }
    }
    func testMissingWindowFailsClosed() throws {
        let f = try Fixture(), w = f.workspace
        w.saveHasWindow = { false }
        var outcomes: [DocumentSaveOutcome] = []
        w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        XCTAssertEqual(outcomes, [.failed("Save requires a document window")]); XCTAssertTrue(f.sessions.isEmpty)
    }
    func testKnownGapEntireAttachmentAfterCancelWithoutAnyObservationNeedsProgress() throws {
        // Historical e51/unseen-lifetime regression: no content appearance, update,
        // disappearance or teardown is delivered. Only owned completion + detach.
        let f = try Fixture(), w = f.workspace
        var outcomes: [DocumentSaveOutcome] = []
        let old = w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        w.cancelDocumentSave(old)
        let next = w.saveForPreparation(f.document, saveAs: true) { _ in }
        XCTAssertEqual(f.sessions.count, 1)
        f.sessions[0].changed?() // Parent-only event cannot release the invocation.
        XCTAssertEqual(f.sessions.count, 1)
        f.drain(0)
        XCTAssertEqual(f.sessions.count, 2)
        XCTAssertEqual(w.saveAsRequest?.id, next)
        XCTAssertEqual(outcomes, [.cancelled])
        w.cancelDocumentSave(old); w.cancelDocumentSave(next); f.drain(1)
        XCTAssertEqual(outcomes, [.cancelled])
    }
    func testSaveWaitsForCompletionAndMembershipThenWritesOnce() throws {
        let f = try Fixture(), w = f.workspace
        var writes = 0, outcomes: [DocumentSaveOutcome] = []
        w.saveWriter = { _, _, done in writes += 1; done(.success(())) }
        w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        let request = try XCTUnwrap(w.saveAsRequest)
        w.finishSaveAs(request); w.finishSaveAs(request)
        f.sessions[0].complete(); XCTAssertEqual(writes, 0)
        f.sessions[0].detach(); XCTAssertEqual(writes, 1)
        XCTAssertEqual(outcomes, [.saved(request.url, continuationCancelled: false)])
        XCTAssertEqual(w.lastSaveFolder, request.folder)
    }
    func testReplaceOwnsDistinctTokenAndWaitsForItsPhysicalDrainBeforeWrite() throws {
        let f = try Fixture(), w = f.workspace
        w.saveFileExists = { _ in true }
        var writes = 0
        w.saveWriter = { _, _, done in writes += 1; done(.success(())) }
        w.saveForPreparation(f.document, saveAs: true) { _ in }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        f.sessions[0].detach(); XCTAssertEqual(f.sessions.count, 1)
        f.sessions[0].complete(); XCTAssertEqual(f.sessions.count, 2)
        XCTAssertEqual(f.tokens[0].requestID, f.tokens[1].requestID)
        XCTAssertNotEqual(f.tokens[0].presentationID, f.tokens[1].presentationID)
        f.sessions[1].complete(NSApplication.ModalResponse.alertFirstButtonReturn.rawValue)
        XCTAssertEqual(writes, 0)
        f.sessions[1].detach(); XCTAssertEqual(writes, 1)
    }
    func testReplaceCancelAndStaleApprovalCannotWrite() throws {
        let f = try Fixture(), w = f.workspace
        w.saveFileExists = { _ in true }
        var writes = 0, outcomes: [DocumentSaveOutcome] = []
        w.saveWriter = { _, _, _ in writes += 1 }
        w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest)); f.drain(0)
        let stale = f.sessions[1].completed
        f.drain(1, response: NSApplication.ModalResponse.alertSecondButtonReturn.rawValue)
        stale?(NSApplication.ModalResponse.alertFirstButtonReturn.rawValue)
        XCTAssertEqual(writes, 0); XCTAssertEqual(outcomes, [.cancelled])
    }
    func testCancelDuringFormDrainCannotStartReplacementOrWrite() throws {
        let f = try Fixture(), w = f.workspace
        w.saveFileExists = { _ in true }
        var outcomes: [DocumentSaveOutcome] = []
        let id = w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest)); w.cancelDocumentSave(id); f.drain(0)
        XCTAssertEqual(f.sessions.count, 1); XCTAssertEqual(outcomes, [.cancelled])
    }
    func testWriteFailurePreservesFolderAndAdmittedWriteDrainsThroughCancellation() throws {
        for success in [false, true] {
            let f = try Fixture(), w = f.workspace
            let old = URL(fileURLWithPath: "/old")
            w.lastSaveFolder = old
            var finish: (@MainActor (Result<Void, Error>) -> Void)?
            w.saveWriter = { _, _, done in finish = done }
            var outcomes: [DocumentSaveOutcome] = []
            let id = w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
            var request = try XCTUnwrap(w.saveAsRequest); request.folder = URL(fileURLWithPath: "/new")
            w.finishSaveAs(request); f.drain(0); w.cancelDocumentSave(id)
            XCTAssertTrue(outcomes.isEmpty)
            if success { finish?(.success(())) } else { finish?(.failure(NSError(domain: "injected", code: 4))) }
            finish?(.success(())); finish = nil; w.saveWriter = nil
            XCTAssertEqual(outcomes.count, 1)
            if success {
                XCTAssertEqual(outcomes, [.saved(request.url, continuationCancelled: true)])
                XCTAssertEqual(w.lastSaveFolder, request.folder)
            } else {
                guard case .failed = outcomes[0] else { return XCTFail("Expected write failure") }
                XCTAssertEqual(w.lastSaveFolder, old)
            }
            XCTAssertFalse(f.document.isClosed)
        }
    }
    func testOldWriteCannotOverwriteNewPromptOrAdmitDuplicateDocumentWrite() throws {
        let f = try Fixture(), w = f.workspace
        var finish: (@MainActor (Result<Void, Error>) -> Void)?
        w.saveWriter = { _, _, done in finish = done }
        w.saveForPreparation(f.document, saveAs: true) { _ in }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest)); f.drain(0)
        var conflict: [DocumentSaveOutcome] = []
        w.saveForPreparation(f.document, saveAs: true) { conflict.append($0) }
        XCTAssertEqual(conflict, [.failed("A save for this document is still running")])
        let other = try DocumentController(backend: StubDocumentBackend())
        let next = w.saveForPreparation(other, saveAs: true) { _ in }
        finish?(.success(())); finish = nil; w.saveWriter = nil
        XCTAssertEqual(w.saveAsRequest?.id, next)
        w.cancelDocumentSave(next); f.drain(1)
    }
    func testWrongControllerAndStaleActionsCannotAffectSuccessor() throws {
        let f = try Fixture(), w = f.workspace
        w.saveForPreparation(f.document, saveAs: true) { _ in }
        let old = try XCTUnwrap(w.saveAsRequest)
        let other = try DocumentController(backend: StubDocumentBackend())
        w.finishSaveAs(SaveAsRequest(id: old.id, doc: other, name: old.name, folder: old.folder))
        XCTAssertEqual(f.sessions[0].endCount, 0)
        let next = w.saveForPreparation(other, saveAs: true) { _ in }
        w.finishSaveAs(old); f.drain(0); w.cancelDocumentSave(old)
        XCTAssertEqual(w.saveAsRequest?.id, next)
        w.cancelDocumentSave(next); f.drain(1)
    }
    func testParentCloseFailsQueuedSuccessorAndKeepsNativeDrain() throws {
        let f = try Fixture(), w = f.workspace
        w.saveForPreparation(f.document, saveAs: true) { _ in }
        var outcomes: [DocumentSaveOutcome] = []
        w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        f.sessions[0].parentClosed?()
        XCTAssertEqual(outcomes, [.failed("Document window closed before save")])
        XCTAssertTrue(w.savePresenter.isBusy)
        f.drain(0); XCTAssertEqual(f.sessions.count, 1)
    }
}
