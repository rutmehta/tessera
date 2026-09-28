import Foundation
import XCTest
@testable import Tessera
@testable import TesseraCore

// UNRUN: fake executor owns the completion, with no native image decode or GPU.
@MainActor
final class DocumentLoadSettlementTests: XCTestCase {
    private func item(url: URL? = URL(fileURLWithPath: "/fake/image.jpg")) -> PhotoItem {
        PhotoItem(id: 0, url: url, name: "image", kind: .jpeg, captureDate: Date(timeIntervalSince1970: 0),
                  pixelWidth: 2, pixelHeight: 2)
    }

    func testDelayedBackendCompletionDoesNotReleaseAtDispatch() throws {
        let w = DocumentWorkspace()
        var finish: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
        defer { finish = nil; w.documentLoadExecutor = nil }
        w.documentLoadExecutor = { _, _, done in finish = done }
        var outcomes: [DocumentLoadOutcome] = []
        w.editInLayers(item()) { outcomes.append($0) }
        XCTAssertTrue(outcomes.isEmpty)
        XCTAssertTrue(w.documents.isEmpty)
        let backend = StubDocumentBackend()
        finish?(.success(backend)); finish?(.success(backend))
        XCTAssertEqual(outcomes, [.installed])
        XCTAssertEqual(w.documents.count, 1)
    }

    func testBackendFailureSettlesOnceWithoutInstall() {
        let w = DocumentWorkspace()
        var finish: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
        defer { finish = nil; w.documentLoadExecutor = nil }
        w.documentLoadExecutor = { _, _, done in finish = done }
        var outcomes: [DocumentLoadOutcome] = []
        w.editInLayers(item()) { outcomes.append($0) }
        finish?(.failure(NSError(domain: "load", code: 7)))
        finish?(.failure(NSError(domain: "load", code: 8)))
        XCTAssertEqual(outcomes.count, 1)
        guard case .failed = outcomes.first else { return XCTFail("Expected failure") }
        XCTAssertTrue(w.documents.isEmpty)
    }

    func testEarlyRejectsSettleWithoutExecutor() {
        let w = DocumentWorkspace()
        var calls = 0
        w.documentLoadExecutor = { _, _, _ in calls += 1 }
        var outcomes: [DocumentLoadOutcome] = []
        w.editInLayers(nil) { outcomes.append($0) }
        w.editInLayers(item(url: nil)) { outcomes.append($0) }
        XCTAssertEqual(outcomes.count, 2)
        for outcome in outcomes {
            guard case .rejected = outcome else { return XCTFail("Expected rejection") }
        }
        XCTAssertEqual(calls, 0)
    }

    func testReleasedWorkspaceStillSettlesAfterBackendReturns() {
        var workspace: DocumentWorkspace? = DocumentWorkspace()
        weak var weakWorkspace = workspace
        var finish: (@MainActor (Result<any DocumentBackend, Error>) -> Void)?
        workspace?.documentLoadExecutor = { _, _, done in finish = done }
        var outcomes: [DocumentLoadOutcome] = []
        workspace?.editInLayers(item()) { outcomes.append($0) }
        workspace = nil
        XCTAssertNil(weakWorkspace)
        XCTAssertTrue(outcomes.isEmpty)
        finish?(.success(StubDocumentBackend()))
        XCTAssertEqual(outcomes, [.workspaceReleased])
        finish = nil
    }
}
