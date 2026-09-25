import XCTest
import TesseraFFI
@testable import TesseraCore

@MainActor final class PreviewEventsTests: XCTestCase {
    private func drain(_ requests: PreviewRequest?...) async {
        let finished = expectation(description: "specific preview tasks have finished")
        let waiter = Task {
            for request in requests { await request?.waitForCompletion() }
            finished.fulfill()
        }
        await fulfillment(of: [finished], timeout: 5)
        waiter.cancel()
    }

    func testReadyBeforeWaitIsBufferedAndFiltered() async {
        let events = PreviewEvents()
        let subscription = events.subscribe(imageID: "image", maxPx: 384)
        events.onEvent(event: .previewReady(imageId: "other", maxPx: 384))
        events.onEvent(event: .previewReady(imageId: "image", maxPx: 2560))
        events.onEvent(event: .previewReady(imageId: "image", maxPx: 384))
        subscription.cancel()
        var iterator = subscription.stream.makeAsyncIterator()
        let first: Void? = await iterator.next()
        let second: Void? = await iterator.next()
        XCTAssertNotNil(first)
        XCTAssertNil(second)
    }

    func testUnrelatedEventsDoNotWakeSubscriber() async {
        let events = PreviewEvents()
        let subscription = events.subscribe(imageID: "image", maxPx: 384)
        events.onEvent(event: .previewReady(imageId: "other", maxPx: 384))
        events.onEvent(event: .previewReady(imageId: "image", maxPx: 2560))
        subscription.cancel()
        var iterator = subscription.stream.makeAsyncIterator()
        let value: Void? = await iterator.next()
        XCTAssertNil(value)
    }

    func testCancellingOneSubscriberDoesNotRemoveAnother() async {
        let events = PreviewEvents()
        let first = events.subscribe(imageID: "image", maxPx: 384)
        let second = events.subscribe(imageID: "image", maxPx: 384)
        first.cancel()
        events.onEvent(event: .previewReady(imageId: "image", maxPx: 384))
        second.cancel()
        var firstIterator = first.stream.makeAsyncIterator()
        var secondIterator = second.stream.makeAsyncIterator()
        let cancelled: Void? = await firstIterator.next()
        let ready: Void? = await secondIterator.next()
        XCTAssertNil(cancelled)
        XCTAssertNotNil(ready)
    }

    func testCancelledTaskStopsWaitingWithoutAnEvent() async {
        let events = PreviewEvents()
        let subscription = events.subscribe(imageID: "image", maxPx: 384)
        let task = Task {
            var iterator = subscription.stream.makeAsyncIterator()
            return await iterator.next() != nil
        }
        task.cancel()
        let received = await task.value
        XCTAssertFalse(received)
        subscription.cancel()
    }

    func testRemoveAllPreventsLateDeliveryAndCacheRepopulation() async {
        let loader = ThumbnailLoader()
        let item = PhotoItem(id: 0, url: nil, name: "old", kind: .synthetic,
                             captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        var deliveries = 0
        let request = loader.request(item, tier: .thumbnail) { _ in deliveries += 1 }
        loader.removeAll()
        XCTAssertTrue(request?.isCancelled ?? false)
        await drain(request)
        XCTAssertEqual(deliveries, 0)
        XCTAssertNil(loader.cached(item, tier: .thumbnail))
    }

    func testInvalidationCancelsInFlightRequestsForBothTiersOnlyForThatItem() async {
        let loader = ThumbnailLoader()
        let item = PhotoItem(id: 0, url: nil, name: "edited", kind: .synthetic,
                             captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        let other = PhotoItem(id: 0, url: nil, name: "other", kind: .synthetic,
                              captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        var staleDeliveries = 0
        let thumbnail = loader.request(item, tier: .thumbnail) { _ in staleDeliveries += 1 }
        let preview = loader.request(item, tier: .preview) { _ in staleDeliveries += 1 }
        let ready = expectation(description: "unrelated item still completes")
        let unrelated = loader.request(other, tier: .thumbnail) { _ in ready.fulfill() }
        // Still on the main actor: no completion can have run before invalidation.
        loader.invalidate(item)
        XCTAssertTrue(thumbnail?.isCancelled ?? false)
        XCTAssertTrue(preview?.isCancelled ?? false)
        XCTAssertFalse(unrelated?.isCancelled ?? true)
        await fulfillment(of: [ready], timeout: 5)
        await drain(thumbnail, preview, unrelated)
        XCTAssertEqual(staleDeliveries, 0)
        XCTAssertNil(loader.cached(item, tier: .thumbnail))
        XCTAssertNil(loader.cached(item, tier: .preview))
        XCTAssertNotNil(loader.cached(other, tier: .thumbnail))
    }

    func testOverBudgetImageIsCachedBeforeDelivery() async {
        // A single image larger than the budget must remain available at delivery.
        // NSCache may evict that newly inserted entry before the callback runs.
        let loader = ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1)
        let item = PhotoItem(id: 0, url: nil, name: "over-budget", kind: .synthetic,
                             captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        let ready = expectation(description: "over-budget preview delivered")
        let request = loader.request(item, tier: .thumbnail) { image in
            XCTAssertTrue(loader.cached(item, tier: .thumbnail) === image)
            ready.fulfill()
        }
        await fulfillment(of: [ready], timeout: 5)
        await drain(request)
        XCTAssertNotNil(loader.cached(item, tier: .thumbnail))
    }

    func testCacheKeysRemainRetrievableAcrossManyRequests() async {
        let loader = ThumbnailLoader()
        for index in 0..<256 {
            let item = PhotoItem(id: 0, url: nil, name: "item-\(index)", kind: .synthetic,
                                 captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
            let ready = expectation(description: "preview \(index)")
            _ = loader.request(item, tier: .thumbnail) { _ in
                XCTAssertNotNil(loader.cached(item, tier: .thumbnail), "cache miss inside callback: \(index)")
                ready.fulfill()
            }
            await fulfillment(of: [ready], timeout: 5)
            XCTAssertNotNil(loader.cached(item, tier: .thumbnail), "cache miss after callback: \(index)")
        }
    }

    func testConcurrentDeliveriesPublishBeforeCallbackAndEvictOlderImages() async {
        let loader = ThumbnailLoader(thumbnailCostLimit: 1, previewCostLimit: 1)
        let ready = expectation(description: "each specific image is cached at delivery")
        ready.expectedFulfillmentCount = 16
        var items: [PhotoItem] = []
        var last: PhotoItem?
        for index in 0..<16 {
            let item = PhotoItem(id: 0, url: nil, name: "concurrent-\(index)", kind: .synthetic,
                                 captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
            items.append(item)
            _ = loader.request(item, tier: .thumbnail) { image in
                XCTAssertTrue(loader.cached(item, tier: .thumbnail) === image)
                last = item
                ready.fulfill()
            }
        }
        await fulfillment(of: [ready], timeout: 5)
        XCTAssertNotNil(last)
        for item in items {
            XCTAssertEqual(loader.cached(item, tier: .thumbnail) != nil, item == last)
        }
    }

    func testCacheDoesNotAliasReusedDenseItemIDs() async {
        let loader = ThumbnailLoader()
        let first = PhotoItem(id: 0, url: nil, name: "first", kind: .synthetic,
                              captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        let second = PhotoItem(id: 0, url: nil, name: "second", kind: .synthetic,
                               captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        let ready = expectation(description: "synthetic preview")
        let request = loader.request(first, tier: .thumbnail) { image in
            XCTAssertTrue(loader.cached(first, tier: .thumbnail) === image)
            XCTAssertNil(loader.cached(second, tier: .thumbnail))
            ready.fulfill()
        }
        await fulfillment(of: [ready], timeout: 5)
        await drain(request)
        XCTAssertNotNil(loader.cached(first, tier: .thumbnail))
        XCTAssertNil(loader.cached(second, tier: .thumbnail))

        let secondReady = expectation(description: "reused dense id gets its own completion")
        let secondRequest = loader.request(second, tier: .thumbnail) { image in
            XCTAssertTrue(loader.cached(second, tier: .thumbnail) === image)
            XCTAssertFalse(loader.cached(first, tier: .thumbnail) === image)
            secondReady.fulfill()
        }
        XCTAssertNotNil(secondRequest, "a reused dense id must not be a cache hit")
        await fulfillment(of: [secondReady], timeout: 5)
        await drain(secondRequest)
        XCTAssertNotNil(loader.cached(first, tier: .thumbnail))
        XCTAssertNotNil(loader.cached(second, tier: .thumbnail))
    }
}