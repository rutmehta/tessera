import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

@MainActor
final class DocumentSaveSheetProbeTests: XCTestCase {
    func testClosingCapturedParentDuringRealFolderChooserDrainsNativeSheet() async throws {
        let presenter = DocumentSavePresenter()
        let parent = LayoutProbeHarness.window(contentRect: NSRect(x: 20, y: 20, width: 240, height: 120),
                              styleMask: [.titled], backing: .buffered, defer: false)
        parent.isReleasedWhenClosed = false
        parent.title = "Document Save Parent Probe"

        let bridge = DocumentSaveParentBridge.ParentView(presenter: presenter)
        parent.contentView = bridge
        parent.orderFrontRegardless()
        defer {
            bridge.shutdown()
            if parent.isVisible { parent.close() }
        }
        XCTAssertTrue(bridge.window === parent)
        XCTAssertEqual(presenter.host?.bindingID, bridge.bindingID)

        var nativeSession: AppKitDocumentSaveSession?
        presenter.updateBinding(bridge.bindingID, windowID: ObjectIdentifier(parent)) { _, content, actions in
            guard case .form = content else { return nil }
            let session = AppKitDocumentSaveSession(parent: parent, content: content, actions: actions)
            nativeSession = session
            return session
        }

        let document = try DocumentController(backend: StubDocumentBackend())
        let request = SaveAsRequest(doc: document, name: "Parent loss probe.psd", folder: FileManager.default.temporaryDirectory)
        let token = DocumentSavePresentationToken(requestID: request.id)
        var lostCount = 0
        var drainedCount = 0
        let drained = expectation(description: "real beginSheet completion and membership drain")
        presenter.present(token, content: .form(request), actions: .inert) { event in
            switch event {
            case .hostLost(let eventToken):
                XCTAssertEqual(eventToken, token)
                lostCount += 1
            case .drained(let eventToken, _):
                XCTAssertEqual(eventToken, token)
                drainedCount += 1
                drained.fulfill()
            case .failed(let eventToken, let message):
                XCTFail("Native save sheet failed: \(eventToken.requestID) \(message)")
            }
        }

        let session = try XCTUnwrap(nativeSession)
        XCTAssertTrue(session.containsSheet, "The actual Save As sheet must be attached before the chooser starts")
        let capturedSheet = try XCTUnwrap(parent.attachedSheet)
        var parentCloseRanInsideModalMode = false
        var childWasActiveAtParentClose = false
        var folderCompletionCount = 0
        let closeParent = Timer(timeInterval: 0.15, repeats: false) { _ in
            MainActor.assumeIsolated {
                parentCloseRanInsideModalMode = NSApp.modalWindow != nil
                childWasActiveAtParentClose = session.childActive
                parent.close()
            }
        }
        RunLoop.main.add(closeParent, forMode: .modalPanel)
        RunLoop.main.add(closeParent, forMode: .common)
        defer { closeParent.invalidate() }

        presenter.chooseFolder(token, folder: FileManager.default.temporaryDirectory) { _ in
            folderCompletionCount += 1
        }

        XCTAssertTrue(parentCloseRanInsideModalMode, "The close action must run from NSOpenPanel's nested modal loop")
        XCTAssertTrue(childWasActiveAtParentClose, "The captured parent must close while the real child panel is active")
        XCTAssertFalse(session.childActive, "The actual runModal call must return before the native sheet can drain")
        XCTAssertEqual(folderCompletionCount, 0, "Parent loss suppresses the folder-selection callback")
        XCTAssertEqual(lostCount, 1)
        await fulfillment(of: [drained], timeout: 5)
        XCTAssertEqual(drainedCount, 1, "Presenter drain joins the real beginSheet completion and membership clearance")
        XCTAssertFalse(parent.attachedSheet === capturedSheet)
        XCTAssertFalse(parent.sheets.contains { $0 === capturedSheet })
        XCTAssertFalse(session.containsSheet, "The captured sheet must leave native parent membership")
        XCTAssertFalse(presenter.isBusy)
        XCTAssertNil(presenter.host)
    }

    // The parent bridge is identity-only. Reuse must refresh the current window;
    // an old incarnation, even for that very same window, cannot mutate the host.
    func testStaleSameWindowBridgeTeardownDoesNotShutdownNewBinding() {
        let presenter = DocumentSavePresenter(), window = NSObject()
        let first = UUID(), second = UUID(), session = FakeDocumentSaveSession()
        presenter.registerBinding(first)
        presenter.updateBinding(first, windowID: ObjectIdentifier(window)) { _, _, _ in session }
        presenter.registerBinding(second)
        presenter.updateBinding(second, windowID: ObjectIdentifier(window)) { _, _, _ in session }
        let token = DocumentSavePresentationToken(requestID: UUID())
        presenter.present(token, content: .test, actions: .inert) { _ in }
        presenter.removeBinding(first)
        presenter.updateBinding(first, windowID: ObjectIdentifier(window)) { _, _, _ in nil }
        XCTAssertEqual(session.endCount, 0)
        XCTAssertEqual(presenter.host?.bindingID, second)
        presenter.removeBinding(second)
        XCTAssertEqual(session.endCount, 1)
        session.complete(); session.detach()
        XCTAssertFalse(presenter.isBusy)
    }

    func testDismantlingOldAttachedBridgeCannotClearNewBridgeOnSameWindow() throws {
        let presenter = DocumentSavePresenter()
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 80, height: 60),
                              styleMask: [], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false

        let oldBridge = DocumentSaveParentBridge.ParentView(presenter: presenter)
        window.contentView = oldBridge
        XCTAssertTrue(oldBridge.window === window)
        XCTAssertEqual(presenter.host?.bindingID, oldBridge.bindingID)

        let newBridge = DocumentSaveParentBridge.ParentView(presenter: presenter)
        window.contentView = newBridge
        XCTAssertTrue(newBridge.window === window)
        XCTAssertEqual(presenter.host?.bindingID, newBridge.bindingID)
        XCTAssertEqual(presenter.host?.windowID, ObjectIdentifier(window))

        DocumentSaveParentBridge.dismantleNSView(oldBridge, coordinator: ())
        XCTAssertEqual(presenter.host?.bindingID, newBridge.bindingID)
        XCTAssertEqual(presenter.host?.windowID, ObjectIdentifier(window))

        newBridge.shutdown()
        XCTAssertNil(presenter.host)
        window.close()
    }
}
