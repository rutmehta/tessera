import Foundation
import Synchronization
import XCTest
@testable import TesseraCore

/// `EngineCall` (ST-2): engine calls from the UI run on a worker, a `Busy` refusal is retried after
/// a short delay, every other error is thrown at once.
final class EngineCallTests: XCTestCase {
    private struct Busy: LocalizedError { var errorDescription: String? { "Busy: catalog lock held by a worker" } }
    private struct Broken: LocalizedError { var errorDescription: String? { "sidecar unreadable" } }

    func testBusyIsRetriedOffTheMainThreadUntilTheEngineAnswers() async throws {
        let attempts = Mutex(0)
        let value = try await EngineCall.detached(delay: .milliseconds(5)) { () -> String in
            let n = attempts.withLock { $0 += 1; return $0 }
            XCTAssertFalse(Thread.isMainThread, "the call runs on a worker")
            if n < 3 { throw Busy() }
            return "answer"
        }
        XCTAssertEqual(value, "answer")
        XCTAssertEqual(attempts.withLock { $0 }, 3)
    }

    func testOtherErrorsAreThrownAtOnce() async {
        let attempts = Mutex(0)
        do {
            _ = try await EngineCall.detached(delay: .milliseconds(5)) { () -> Int in
                attempts.withLock { $0 += 1 }
                throw Broken()
            }
            XCTFail("expected the error")
        } catch {
            XCTAssertEqual(error.localizedDescription, "sidecar unreadable")
        }
        XCTAssertEqual(attempts.withLock { $0 }, 1)
        XCTAssertFalse(EngineCall.isBusy(Broken()))
        XCTAssertTrue(EngineCall.isBusy(Busy()))
    }

    func testBusyBeyondTheAttemptBudgetIsReportedAndOptionalSwallowsIt() async {
        let attempts = Mutex(0)
        do {
            _ = try await EngineCall.detached(attempts: 3, delay: .milliseconds(1)) { () -> Int in
                attempts.withLock { $0 += 1 }
                throw Busy()
            }
            XCTFail("expected the Busy error after the budget")
        } catch {
            XCTAssertTrue(EngineCall.isBusy(error))
        }
        XCTAssertEqual(attempts.withLock { $0 }, 3)
        let none: Int? = await EngineCall.optional { throw Broken() }
        XCTAssertNil(none)
    }
}
