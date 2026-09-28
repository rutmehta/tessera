import Foundation
import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

// These tests replace inferred SwiftUI probe callbacks with owned native
// invocation callbacks, retaining outcome and ordering requirements.
@MainActor
final class DocumentSaveSettlementTests: XCTestCase {
    @MainActor private final class Fixture {
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
    func testLegacyHeadlessSaveAsUsesAutomaticDestinationAndReportsSuccess() throws {
        let workspace = DocumentWorkspace()
        let document = try DocumentController(backend: StubDocumentBackend())
        let folder = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
            .appendingPathComponent("legacy-headless-save-\(UUID().uuidString)", isDirectory: true)
        workspace.lastSaveFolder = folder
        XCTAssertNil(workspace.savePresenter.host)

        var writes: [URL] = []
        workspace.saveWriter = { _, url, completion in
            if let url { writes.append(url) }
            completion(.success(()))
        }
        var callbackCount = 0
        workspace.saveAs(document) { callbackCount += 1 }

        XCTAssertEqual(writes.count, 1)
        XCTAssertEqual(writes.first?.deletingLastPathComponent(), folder)
        XCTAssertEqual(writes.first?.pathExtension, SaveAsRequest.Format.tessera.rawValue)
        XCTAssertEqual(workspace.lastSaveFolder, folder)
        XCTAssertEqual(callbackCount, 1)
        XCTAssertNil(workspace.saveAsRequest, "headless compatibility must not present a sheet")
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
        w.checkedSaveWriter = { _, _, _, done in writes += 1; done(.success(.saved)) }
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
        w.checkedSaveWriter = { _, _, _, done in writes += 1; done(.success(.saved)) }
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
        w.checkedSaveWriter = { _, _, _, _ in writes += 1 }
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
            var finish: (@MainActor (Result<DocSaveAsResult, Error>) -> Void)?
            w.checkedSaveWriter = { _, _, _, done in finish = done }
            var outcomes: [DocumentSaveOutcome] = []
            let id = w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
            var request = try XCTUnwrap(w.saveAsRequest); request.folder = URL(fileURLWithPath: "/new")
            w.finishSaveAs(request); f.drain(0); w.cancelDocumentSave(id)
            XCTAssertTrue(outcomes.isEmpty)
            if success { finish?(.success(.saved)) } else { finish?(.failure(NSError(domain: "injected", code: 4))) }
            finish?(.success(.saved)); finish = nil; w.checkedSaveWriter = nil
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
        var finish: (@MainActor (Result<DocSaveAsResult, Error>) -> Void)?
        w.checkedSaveWriter = { _, _, _, done in finish = done }
        w.saveForPreparation(f.document, saveAs: true) { _ in }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest)); f.drain(0)
        var conflict: [DocumentSaveOutcome] = []
        w.saveForPreparation(f.document, saveAs: true) { conflict.append($0) }
        XCTAssertEqual(conflict, [.failed("A save for this document is still running")])
        let other = try DocumentController(backend: StubDocumentBackend())
        let next = w.saveForPreparation(other, saveAs: true) { _ in }
        finish?(.success(.saved)); finish = nil; w.checkedSaveWriter = nil
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
        w.finishSaveAs(old); f.drain(0); w.cancelDocumentSave(old.id)
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
    func testHostLossWhileWriterRunsReportsActualOutcomeWithCancelledContinuation() throws {
        let f = try Fixture(), w = f.workspace
        var finish: (@MainActor (Result<DocSaveAsResult, Error>) -> Void)?
        w.checkedSaveWriter = { _, _, _, done in finish = done }
        var outcomes: [DocumentSaveOutcome] = []
        w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        let request = try XCTUnwrap(w.saveAsRequest)
        w.finishSaveAs(request); f.drain(0)
        let binding = try XCTUnwrap(w.savePresenter.host?.bindingID)
        w.savePresenter.removeBinding(binding)
        XCTAssertTrue(outcomes.isEmpty)
        finish?(.success(.saved)); finish = nil; w.checkedSaveWriter = nil
        XCTAssertEqual(outcomes, [.saved(request.url, continuationCancelled: true)])
    }
    func testReentrantCancelledCallerSupersedesIntermediateQueuedRequest() throws {
        let f = try Fixture(), w = f.workspace
        var newest: UUID?
        w.saveForPreparation(f.document, saveAs: true) { outcome in
            if outcome == .cancelled {
                newest = w.saveForPreparation(f.document, saveAs: true) { _ in }
            }
        }
        var intermediate: [DocumentSaveOutcome] = []
        w.saveForPreparation(f.document, saveAs: true) { intermediate.append($0) }
        XCTAssertEqual(intermediate, [.cancelled])
        f.drain(0)
        XCTAssertEqual(w.saveAsRequest?.id, newest)
        XCTAssertEqual(f.sessions.count, 2)
        if let newest { w.cancelDocumentSave(newest) }
        f.drain(1)
    }

    // SOURCE ONLY / UNRUN: typed destination intent through the real writer seam.
    func testAbsentIntentSurvivesLateAppearanceAndConflictDoesNotAdvanceState() throws {
        let f = try Fixture(), w = f.workspace, model = AppModel()
        w.app = model
        let oldFolder = URL(fileURLWithPath: "/old")
        w.lastSaveFolder = oldFolder
        let oldInfo = f.document.info
        var intents: [DocSaveDestinationIntent] = [], outcomes: [DocumentSaveOutcome] = []
        var finish: (@MainActor (Result<DocSaveAsResult, Error>) -> Void)?
        w.checkedSaveWriter = { _, _, intent, done in intents.append(intent); finish = done }
        let id = w.saveForPreparation(f.document, saveAs: true) { outcomes.append($0) }
        var request = try XCTUnwrap(w.saveAsRequest); request.folder = URL(fileURLWithPath: "/new")
        w.finishSaveAs(request)
        w.saveFileExists = { _ in true }
        f.sessions[0].complete(); XCTAssertTrue(intents.isEmpty)
        f.sessions[0].detach(); XCTAssertEqual(intents, [.createIfAbsent])
        w.cancelDocumentSave(id)
        XCTAssertTrue(outcomes.isEmpty)
        finish?(.success(.destinationExists)); finish?(.success(.saved))
        finish = nil; w.checkedSaveWriter = nil
        XCTAssertEqual(outcomes, [.destinationConflict(request.url)])
        XCTAssertEqual(w.lastSaveFolder, oldFolder)
        XCTAssertEqual(f.document.info.path, oldInfo.path)
        XCTAssertEqual(f.document.info.title, oldInfo.title)
        XCTAssertEqual(f.document.info.dirty, oldInfo.dirty)
        XCTAssertEqual(model.statusMessage?.contains("appeared"), true)
        XCTAssertNotEqual(model.statusMessage?.hasPrefix("Saved"), true)
        XCTAssertEqual(f.sessions.count, 1, "conflict cannot auto-upgrade to Replace")
    }
    func testOnlyAffirmativeMatchingReplaceDrainGrantsReplacementIntent() throws {
        let f = try Fixture(), w = f.workspace
        w.saveFileExists = { _ in true }
        var intents: [DocSaveDestinationIntent] = []
        w.checkedSaveWriter = { _, _, intent, done in intents.append(intent); done(.success(.saved)) }
        w.saveForPreparation(f.document, saveAs: true) { _ in }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest))
        let oldCompletion = f.sessions[0].completed
        f.drain(0)
        oldCompletion?(NSApplication.ModalResponse.alertFirstButtonReturn.rawValue)
        XCTAssertTrue(intents.isEmpty)
        f.sessions[1].complete(NSApplication.ModalResponse.alertFirstButtonReturn.rawValue)
        XCTAssertTrue(intents.isEmpty)
        f.sessions[1].detach()
        XCTAssertEqual(intents, [.replaceConfirmed])
    }
    func testSupersededWriterConflictPreservesNewStatusAndNeverRunsThen() throws {
        let f = try Fixture(), w = f.workspace, model = AppModel()
        w.app = model
        var finish: (@MainActor (Result<DocSaveAsResult, Error>) -> Void)?
        w.checkedSaveWriter = { _, _, _, done in finish = done }
        var thenCount = 0
        w.saveAs(f.document) { thenCount += 1 }
        w.finishSaveAs(try XCTUnwrap(w.saveAsRequest)); f.drain(0)
        let other = try DocumentController(backend: StubDocumentBackend())
        let next = w.saveForPreparation(other, saveAs: true) { _ in }
        model.statusMessage = "Newer request"
        finish?(.success(.destinationExists)); finish = nil; w.checkedSaveWriter = nil
        XCTAssertEqual(model.statusMessage, "Newer request")
        XCTAssertEqual(thenCount, 0)
        XCTAssertEqual(w.saveAsRequest?.id, next)
        w.cancelDocumentSave(next); f.drain(1)
    }

}
