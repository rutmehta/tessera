import Foundation
import XCTest
@testable import TesseraCore

private final class ControlledPreviewWorker: @unchecked Sendable {
    let entered = DispatchSemaphore(value: 0)
    let release = DispatchSemaphore(value: 0)
    private let lock = NSLock()
    private var calls = 0

    func pauseFirstRender() {
        let first = lock.withLock { () -> Bool in
            calls += 1
            return calls == 1
        }
        if first {
            entered.signal()
            _ = release.wait(timeout: .now() + 10)
        }
    }

    func waitUntilEntered() async -> Bool {
        await Task.detached { self.entered.wait(timeout: .now() + 5) == .success }.value
    }
}

@MainActor
final class ThumbnailFlightDrainTests: XCTestCase {
    private func item(_ id: Int = 0) -> PhotoItem {
        PhotoItem(id: id, url: nil, name: "drain-\(id)", kind: .synthetic,
                  captureDate: .distantPast, pixelWidth: 64, pixelHeight: 48)
    }

    private func loader(_ control: ControlledPreviewWorker) -> ThumbnailLoader {
        ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1, concurrency: 1,
                        beforeRender: { control.pauseFirstRender() })
    }

    func testInvalidateDetachesSubscriberButWaitsForRunningWorkerDrain() async throws {
        let control = ControlledPreviewWorker()
        let loader = loader(control)
        let photo = item()
        let request = try XCTUnwrap(loader.request(photo, tier: .preview) { _ in XCTFail("stale delivery") })
        defer { control.release.signal() }
        let entered = await control.waitUntilEntered()
        XCTAssertTrue(entered)

        loader.invalidate(photo)
        await request.waitForCompletion()
        XCTAssertFalse(request.isFlightDrained)
        control.release.signal()
        await request.waitForFlightDrain()
        XCTAssertTrue(request.isFlightDrained)
    }

    func testCancelledCoalescedSubscriberSharesRunningDrain() async throws {
        let control = ControlledPreviewWorker()
        let loader = loader(control)
        let photo = item()
        let first = try XCTUnwrap(loader.request(photo, tier: .preview) { _ in XCTFail("cancelled") })
        defer { control.release.signal() }
        let entered = await control.waitUntilEntered()
        XCTAssertTrue(entered)
        var delivered = false
        let second = try XCTUnwrap(loader.request(photo, tier: .preview) { _ in delivered = true })

        first.cancel()
        await first.waitForCompletion()
        XCTAssertFalse(first.isFlightDrained)
        control.release.signal()
        await first.waitForFlightDrain()
        await second.waitForFlightDrain()
        XCTAssertTrue(delivered)
    }

    func testCancelledPendingReplacementWaitsForOlderSameKeyWorker() async throws {
        let control = ControlledPreviewWorker()
        let loader = loader(control)
        let photo = item()
        let old = try XCTUnwrap(loader.request(photo, tier: .preview) { _ in XCTFail("invalidated") })
        defer { control.release.signal() }
        let entered = await control.waitUntilEntered()
        XCTAssertTrue(entered)
        loader.invalidate(photo)
        let replacement = try XCTUnwrap(loader.request(photo, tier: .preview) { _ in XCTFail("cancelled") })
        replacement.cancel()
        await replacement.waitForCompletion()
        XCTAssertFalse(replacement.isFlightDrained)
        control.release.signal()
        await replacement.waitForFlightDrain()
        await old.waitForFlightDrain()
        XCTAssertTrue(replacement.isFlightDrained)
    }

    func testPendingOnlyCancellationDrainsImmediately() async throws {
        let control = ControlledPreviewWorker()
        let loader = loader(control)
        let active = try XCTUnwrap(loader.request(item(0), tier: .thumbnail) { _ in })
        defer { control.release.signal() }
        let entered = await control.waitUntilEntered()
        XCTAssertTrue(entered)
        let pending = try XCTUnwrap(loader.request(item(1), tier: .thumbnail) { _ in XCTFail("cancelled") })
        pending.cancel()
        await pending.waitForFlightDrain()
        XCTAssertTrue(pending.isFlightDrained)
        XCTAssertFalse(active.isFlightDrained)
        control.release.signal()
        await active.waitForFlightDrain()
    }

    func testRemoveAllAndLoaderReleaseDoNotStrandRunningDrain() async throws {
        let control = ControlledPreviewWorker()
        var loader: ThumbnailLoader? = loader(control)
        let request = try XCTUnwrap(loader?.request(item(), tier: .preview) { _ in XCTFail("cancelled") })
        defer { control.release.signal() }
        let entered = await control.waitUntilEntered()
        XCTAssertTrue(entered)
        loader?.removeAll()
        loader = nil
        await request.waitForCompletion()
        XCTAssertFalse(request.isFlightDrained)
        control.release.signal()
        await request.waitForFlightDrain()
        XCTAssertTrue(request.isFlightDrained)
    }

    func testCachedHitNeedsNoFlightHandle() async throws {
        let loader = ThumbnailLoader()
        let photo = item()
        let first = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in })
        await first.waitForFlightDrain()
        var delivered = false
        let hit = loader.request(photo, tier: .thumbnail) { _ in delivered = true }
        XCTAssertNil(hit)
        XCTAssertTrue(delivered)
    }
}
