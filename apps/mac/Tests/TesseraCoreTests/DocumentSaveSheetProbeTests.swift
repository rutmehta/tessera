import AppKit
import XCTest
@testable import Tessera

@MainActor
final class DocumentSaveSheetProbeTests: XCTestCase {
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
}
