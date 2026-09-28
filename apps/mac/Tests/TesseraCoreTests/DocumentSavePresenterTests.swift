import AppKit
import XCTest
@testable import Tessera

// SOURCE ONLY / UNRUN on B. Fake owns native callback and membership independently.
@MainActor
final class FakeDocumentSaveSession: DocumentSaveNativeSession {
    var canBegin = true
    var containsSheet = false
    var childActive = false
    var beginCount = 0, endCount = 0, retireCount = 0, childCancelCount = 0
    var onBegin: (() -> Void)?
    var onEnd: (() -> Void)?
    var onChildCancel: (() -> Void)?
    var completed: ((Int) -> Void)?
    var changed: (() -> Void)?
    var parentClosed: (() -> Void)?
    func begin(completed: @escaping (Int) -> Void, changed: @escaping () -> Void,
               parentClosed: @escaping () -> Void) {
        beginCount += 1; self.completed = completed; self.changed = changed; self.parentClosed = parentClosed
        containsSheet = true
        onBegin?()
    }
    func end(response: Int) { endCount += 1; onEnd?() }
    func cancelChild() { childCancelCount += 1; onChildCancel?() }
    func chooseFolder(_ folder: URL, completion: @escaping (URL?) -> Void) { completion(nil) }
    func retire() {
        retireCount += 1; completed = nil; changed = nil; parentClosed = nil
    }
    func complete(_ response: Int = NSApplication.ModalResponse.cancel.rawValue) { completed?(response) }
    func detach() { containsSheet = false; changed?() }
}

@MainActor
final class DocumentSavePresenterTests: XCTestCase {
    private func configured(_ driver: FakeDocumentSaveSession) -> (DocumentSavePresenter, NSObject, UUID) {
        let presenter = DocumentSavePresenter(), window = NSObject(), binding = UUID()
        presenter.registerBinding(binding)
        presenter.updateBinding(binding, windowID: ObjectIdentifier(window)) { _, _, _ in driver }
        return (presenter, window, binding)
    }
    private func start(_ p: DocumentSavePresenter, events: @escaping (DocumentSavePresentationEvent) -> Void = { _ in }) -> DocumentSavePresentationToken {
        let token = DocumentSavePresentationToken(requestID: UUID())
        p.present(token, content: .test, actions: .inert, events: events)
        return token
    }
    func testWhollyUnseenAttachmentWithLiveViewWaitsCompletionAndDetachInEitherOrder() {
        for completionFirst in [false, true] {
            let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
            var drained = 0
            let token = start(p) { if case .drained = $0 { drained += 1 } }
            p.end(token)
            XCTAssertEqual(d.endCount, 1)
            if completionFirst { d.complete() } else { d.detach() }
            XCTAssertTrue(p.isBusy); XCTAssertEqual(drained, 0)
            if completionFirst { d.detach() } else { d.complete() }
            XCTAssertFalse(p.isBusy); XCTAssertEqual(drained, 1); XCTAssertEqual(d.retireCount, 1)
            withExtendedLifetime(window) {}
        }
    }
    func testCompletionWithCapturedSheetStillNativeQueuedCannotRelease() {
        let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
        let token = start(p)
        // containsSheet also represents membership behind an unrelated top sheet.
        p.end(token); d.complete()
        XCTAssertTrue(p.isBusy); XCTAssertEqual(d.endCount, 1)
        d.changed?() // Unrelated parent's event is not a detach proof.
        XCTAssertTrue(p.isBusy)
        d.detach(); XCTAssertFalse(p.isBusy)
        withExtendedLifetime(window) {}
    }
    func testReentrantCancelDuringBeginAndCompletionDuringEndAreIdempotent() {
        let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
        let token = DocumentSavePresentationToken(requestID: UUID())
        d.onBegin = { p.end(token); XCTAssertEqual(d.endCount, 0) }
        d.onEnd = { d.complete(); d.detach() }
        var drained = 0
        p.present(token, content: .test, actions: .inert) { if case .drained = $0 { drained += 1 } }
        XCTAssertEqual(d.beginCount, 1); XCTAssertEqual(d.endCount, 1)
        XCTAssertEqual(drained, 1); XCTAssertFalse(p.isBusy)
        p.end(token); XCTAssertEqual(d.endCount, 1)
        d.onBegin = nil; d.onEnd = nil
        withExtendedLifetime(window) {}
    }
    func testCancelBeforeBeginFromFactoryNeverBeginsNativeSession() {
        let p = DocumentSavePresenter(), window = NSObject(), binding = UUID(), d = FakeDocumentSaveSession()
        let token = DocumentSavePresentationToken(requestID: UUID())
        p.registerBinding(binding)
        p.updateBinding(binding, windowID: ObjectIdentifier(window)) { _, _, _ in p.end(token); return d }
        p.present(token, content: .test, actions: .inert) { _ in }
        XCTAssertEqual(d.beginCount, 0); XCTAssertEqual(d.endCount, 0)
        XCTAssertFalse(p.isBusy); XCTAssertEqual(d.retireCount, 1)
    }
    func testSameWindowNewGenerationRejectsOldUpdateAndTeardown() {
        let d = FakeDocumentSaveSession(); let (p, window, old) = configured(d)
        let newer = UUID(), newDriver = FakeDocumentSaveSession()
        p.registerBinding(newer)
        p.updateBinding(newer, windowID: ObjectIdentifier(window)) { _, _, _ in newDriver }
        let token = start(p)
        p.updateBinding(old, windowID: ObjectIdentifier(window)) { _, _, _ in d }
        p.removeBinding(old)
        XCTAssertEqual(p.host?.bindingID, newer); XCTAssertTrue(p.isBusy)
        XCTAssertEqual(newDriver.endCount, 0); XCTAssertEqual(d.beginCount, 0)
        p.removeBinding(newer)
        XCTAssertEqual(newDriver.endCount, 1)
        newDriver.complete(); newDriver.detach()
        XCTAssertFalse(p.isBusy); p.end(token)
    }
    func testHeldChildCancellationDefersParentEndIncludingReentrantChildReturn() {
        for reentrant in [false, true] {
            let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
            let token = start(p); d.childActive = true
            if reentrant { d.onChildCancel = { d.childActive = false; d.changed?() } }
            p.end(token); p.end(token)
            XCTAssertEqual(d.childCancelCount, 1)
            if !reentrant {
                XCTAssertEqual(d.endCount, 0)
                d.childActive = false; d.changed?()
            }
            XCTAssertEqual(d.endCount, 1)
            d.complete(); d.detach(); XCTAssertEqual(d.retireCount, 1)
            d.onChildCancel = nil; withExtendedLifetime(window) {}
        }
    }
    func testParentCloseSettlesLossButNativeMembershipStillMustDrain() {
        let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
        var lost = 0, drained = 0
        _ = start(p) { event in
            if case .hostLost = event { lost += 1 }
            if case .drained = event { drained += 1 }
        }
        d.parentClosed?(); d.parentClosed?()
        XCTAssertEqual(lost, 1); XCTAssertEqual(d.endCount, 1)
        d.complete(); XCTAssertEqual(drained, 0)
        d.detach(); XCTAssertEqual(drained, 1); XCTAssertEqual(d.retireCount, 1)
        withExtendedLifetime(window) {}
    }
    func testStaleCompletionCannotRetireSuccessorAndUnrelatedAdmissionFailsClosed() {
        let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
        _ = start(p); let staleCompletion = d.completed, staleChange = d.changed
        d.complete(); d.detach()
        let second = start(p)
        staleCompletion?(0); staleChange?()
        XCTAssertTrue(p.isBusy)
        p.end(second); d.complete(); d.detach()
        d.canBegin = false
        var failures = 0
        _ = start(p) { if case .failed = $0 { failures += 1 } }
        XCTAssertEqual(failures, 1); XCTAssertFalse(p.isBusy)
        withExtendedLifetime(window) {}
    }
    func testDuplicateCompletionBeforeDetachPreservesFirstResponse() {
        let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
        var responses: [Int] = []
        _ = start(p) { if case .drained(_, let response) = $0 { responses.append(response) } }
        d.complete(42); d.complete(99)
        XCTAssertTrue(p.isBusy)
        d.detach(); XCTAssertEqual(responses, [42])
        withExtendedLifetime(window) {}
    }
    func testNewBindingMayWaitForOldDrainButOldCloseCannotRemoveNewHost() {
        let oldDriver = FakeDocumentSaveSession(); let (p, window, _) = configured(oldDriver)
        _ = start(p)
        let oldClose = oldDriver.parentClosed
        let nextBinding = UUID(), nextDriver = FakeDocumentSaveSession()
        p.registerBinding(nextBinding)
        p.updateBinding(nextBinding, windowID: ObjectIdentifier(window)) { _, _, _ in nextDriver }
        oldClose?()
        XCTAssertEqual(p.host?.bindingID, nextBinding)
        oldDriver.complete(); oldDriver.detach()
        let next = start(p)
        oldClose?()
        XCTAssertEqual(nextDriver.endCount, 0)
        XCTAssertEqual(p.host?.bindingID, nextBinding)
        p.end(next); nextDriver.complete(); nextDriver.detach()
    }
    func testCancelThenWholeNativeLifetimeBeforeBeginReturnsStillDrains() {
        let d = FakeDocumentSaveSession(); let (p, window, _) = configured(d)
        let token = DocumentSavePresentationToken(requestID: UUID())
        var drained = 0
        d.onBegin = { p.end(token); d.complete(); d.detach(); XCTAssertTrue(p.isBusy) }
        p.present(token, content: .test, actions: .inert) { if case .drained = $0 { drained += 1 } }
        XCTAssertFalse(p.isBusy); XCTAssertEqual(drained, 1)
        XCTAssertEqual(d.endCount, 0, "The native invocation already ended; do not end it twice")
        d.onBegin = nil; withExtendedLifetime(window) {}
    }

}
