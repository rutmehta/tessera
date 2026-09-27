import XCTest
import CoreGraphics
@testable import TesseraCore

@MainActor final class ThumbnailQueueTests: XCTestCase {
    func testTwentyThousandRequestsKeepOnlyNewestBoundedBacklog() async {
        let loader = ThumbnailLoader()
        var requests: [PreviewRequest] = []
        var delivered: [Int] = []
        for index in 0..<20_000 {
            let item = PhotoItem(id: index, url: nil, name: "frame-\(index)", kind: .synthetic,
                                 captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
            if let request = loader.request(item, tier: .thumbnail, completion: { _ in delivered.append(index) }) {
                requests.append(request)
            }
        }
        let retained = requests.filter { !$0.isCancelled }.count
        print("P08 20k retained subscribers: \(retained)")
        XCTAssertLessThanOrEqual(retained, 68, "32-cell default viewport: 64 pending + 4 active")
        loader.removeAll()
        for request in requests { await request.waitForCompletion() }
        XCTAssertTrue(delivered.isEmpty)
    }

    private func item(_ id: Int) -> PhotoItem {
        PhotoItem(id: id, url: nil, name: "frame-\(id)", kind: .synthetic,
                  captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
    }

    func testLateSubscriberAfterCacheEvictionStillReceivesResult() async {
        let release = DispatchSemaphore(value: 0)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1, concurrency: 2,
                                     afterDelivery: { _ = release.wait(timeout: .now() + 10) })
        let delivered = expectation(description: "both deliveries before flight cleanup")
        delivered.expectedFulfillmentCount = 2
        var order: [Int] = []
        let first = loader.request(item(0), tier: .thumbnail) { _ in order.append(0); delivered.fulfill() }
        let second = loader.request(item(1), tier: .thumbnail) { _ in order.append(1); delivered.fulfill() }
        await fulfillment(of: [delivered], timeout: 10)
        var lateDelivered = false
        let late = loader.request(item(order.first ?? 0), tier: .thumbnail) { _ in lateDelivered = true }
        release.signal()
        release.signal()
        await first?.waitForCompletion()
        await second?.waitForCompletion()
        await late?.waitForCompletion()
        XCTAssertTrue(lateDelivered, "joining a delivered but not yet cleaned-up flight must not lose completion")
    }

    func testLoupeIsNextAdmissionAheadOfThumbnailBacklog() async {
        let loader = ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1, concurrency: 1)
        var order: [Int] = []
        let active = loader.request(item(0), tier: .thumbnail) { _ in order.append(0) }
        let thumbnail = loader.request(item(1), tier: .thumbnail) { _ in order.append(1) }
        let loupe = loader.request(item(2), tier: .preview, priority: .veryHigh) { _ in order.append(2) }
        await active?.waitForCompletion()
        await thumbnail?.waitForCompletion()
        await loupe?.waitForCompletion()
        XCTAssertEqual(order, [0, 2, 1])
    }

    func testMixedTiersShareViewportPendingBudget() async {
        let loader = ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1, concurrency: 1)
        loader.setViewportCapacity(2, owner: UUID())
        var requests: [PreviewRequest?] = []
        for index in 0..<5 { requests.append(loader.request(item(index), tier: .thumbnail) { _ in }) }
        requests.append(loader.request(item(6), tier: .preview) { _ in })
        requests.append(loader.request(item(7), tier: .preview) { _ in })
        XCTAssertLessThanOrEqual(loader.queueSnapshot.pending, 4)
        loader.removeAll()
        for request in requests { await request?.waitForCompletion() }
    }

    func testCancelAndImmediatelyResubscribeDoesNotOverlapSameKey() async {
        let loader = ThumbnailLoader()
        let photo = item(0)
        let old = loader.request(photo, tier: .thumbnail) { _ in XCTFail("stale subscriber") }
        old?.cancel()
        let replacement = loader.request(photo, tier: .thumbnail) { _ in }
        XCTAssertEqual(loader.queueSnapshot.active, 1, "cancelled decode still occupies the image/tier slot")
        await old?.waitForCompletion()
        await replacement?.waitForCompletion()
        XCTAssertNotNil(loader.cached(photo, tier: .thumbnail))
    }

    func testGridAndFilmstripShareOneDecodedImage() async {
        let loader = ThumbnailLoader()
        let item = PhotoItem(id: 0, url: nil, name: "shared", kind: .synthetic,
                             captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        var images: [CGImage] = []
        let first = loader.request(item, tier: .thumbnail) { images.append($0) }
        let second = loader.request(item, tier: .thumbnail) { images.append($0) }
        await first?.waitForCompletion()
        await second?.waitForCompletion()
        XCTAssertEqual(images.count, 2)
        XCTAssertTrue(images.first === images.last, "one decode must fan out to both views")
    }
    func testCancellingSharedSubscriberKeepsOtherTierAndSubscriber() async {
        let loader = ThumbnailLoader()
        let photo = item(5)
        var delivered = 0
        let cancelled = loader.request(photo, tier: .thumbnail) { _ in XCTFail("cancelled") }
        let survivor = loader.request(photo, tier: .thumbnail) { _ in delivered += 1 }
        let preview = loader.request(photo, tier: .preview) { _ in delivered += 1 }
        cancelled?.cancel()
        XCTAssertEqual(loader.queueSnapshot.subscribers, 2)
        await cancelled?.waitForCompletion()
        await survivor?.waitForCompletion()
        await preview?.waitForCompletion()
        XCTAssertEqual(delivered, 2)
        XCTAssertEqual(loader.cached(photo, tier: .thumbnail)?.width, 256)
        XCTAssertEqual(loader.cached(photo, tier: .preview)?.width, 1600)
    }

    func testPendingCancellationRemovesSubscriberSynchronously() async {
        let loader = ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1, concurrency: 1)
        let active = loader.request(item(0), tier: .thumbnail) { _ in }
        let pending = loader.request(item(1), tier: .thumbnail) { _ in XCTFail("cancelled pending") }
        XCTAssertEqual(loader.queueSnapshot.pending, 1)
        pending?.cancel()
        XCTAssertEqual(loader.queueSnapshot.pending, 0)
        XCTAssertEqual(loader.queueSnapshot.subscribers, 1)
        await pending?.waitForCompletion()
        await active?.waitForCompletion()
        XCTAssertNil(loader.cached(item(1), tier: .thumbnail))
    }

    func testReentrantInvalidationPreventsOtherSharedDeliveryAndCanReload() async {
        let loader = ThumbnailLoader()
        let photo = item(0)
        var deliveries = 0
        let completion: @MainActor @Sendable (CGImage) -> Void = { _ in
            deliveries += 1
            loader.invalidate(photo)
        }
        let first = loader.request(photo, tier: .thumbnail, completion: completion)
        let second = loader.request(photo, tier: .thumbnail, completion: completion)
        await first?.waitForCompletion()
        await second?.waitForCompletion()
        XCTAssertEqual(deliveries, 1)
        XCTAssertNil(loader.cached(photo, tier: .thumbnail))
        let reload = loader.request(photo, tier: .thumbnail) { _ in deliveries += 1 }
        await reload?.waitForCompletion()
        XCTAssertEqual(deliveries, 2)
        XCTAssertNotNil(loader.cached(photo, tier: .thumbnail))
    }

    func testViewportOwnersResizeAndReleaseBound() async {
        let loader = ThumbnailLoader()
        let grid = UUID(), strip = UUID()
        loader.setViewportCapacity(12, owner: grid)
        loader.setViewportCapacity(6, owner: strip)
        XCTAssertEqual(loader.queueSnapshot.pendingLimit, 36)
        var requests: [PreviewRequest?] = []
        for index in 0..<100 {
            requests.append(loader.request(item(index), tier: .thumbnail) { _ in })
        }
        XCTAssertEqual(loader.queueSnapshot.pending, 36)
        loader.setViewportCapacity(2, owner: grid)
        XCTAssertEqual(loader.queueSnapshot.pending, 16)
        loader.removeViewport(owner: strip)
        XCTAssertEqual(loader.queueSnapshot.pending, 4)
        loader.setViewportCapacity(0, owner: grid)
        XCTAssertEqual(loader.queueSnapshot.pending, 0)
        loader.removeAll()
        for request in requests { await request?.waitForCompletion() }
    }

    func testTwentyThousandReversalDeliversOnlyFinalViewport() async {
        let loader = ThumbnailLoader()
        let owner = UUID()
        loader.setViewportCapacity(24, owner: owner)
        var viewport: [PreviewRequest] = []
        var deliveries: [Int] = []
        var maxPending = 0, maxActive = 0, maxSubscribers = 0
        let trace = PerformanceTrace(enabled: true)
        let span = trace.begin("thumbnail_20k_reversal")
        // Forward to the end, then reverse to the start without yielding the delivery actor.
        for index in Array(0..<20_000) + Array((0..<20_000).reversed()) {
            if viewport.count == 24 { viewport.removeFirst().cancel() }
            if let request = loader.request(item(index), tier: .thumbnail, completion: { _ in deliveries.append(index) }) {
                viewport.append(request)
            }
            let state = loader.queueSnapshot
            maxPending = max(maxPending, state.pending)
            maxActive = max(maxActive, state.active)
            maxSubscribers = max(maxSubscribers, state.subscribers)
        }
        for request in viewport { await request.waitForCompletion() }
        XCTAssertEqual(Set(deliveries), Set(0..<24))
        XCTAssertEqual(deliveries.count, 24)
        XCTAssertLessThanOrEqual(maxPending, 48)
        XCTAssertLessThanOrEqual(maxActive, 4)
        XCTAssertLessThanOrEqual(maxSubscribers, 24)
        XCTAssertEqual(loader.queueSnapshot.pending, 0)
        trace.end(span)
        XCTAssertEqual(trace.snapshot().dropped, 0)
        let milliseconds = trace.snapshot().events.last?.durationMs ?? -1
        print("P08 20k forward/reverse: peak pending=\(maxPending), active=\(maxActive), subscribers=\(maxSubscribers), delivered=\(deliveries.count), traceMs=\(milliseconds)")
    }

}
