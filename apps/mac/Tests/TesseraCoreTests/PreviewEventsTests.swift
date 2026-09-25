import XCTest
import TesseraFFI
@testable import TesseraCore

@MainActor final class PreviewEventsTests: XCTestCase {
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
        let unexpected = expectation(description: "cancelled delivery")
        unexpected.isInverted = true
        let request = loader.request(item, tier: .thumbnail) { _ in unexpected.fulfill() }
        loader.removeAll()
        XCTAssertTrue(request?.isCancelled ?? false)
        await fulfillment(of: [unexpected], timeout: 0.2)
        XCTAssertNil(loader.cached(item, tier: .thumbnail))
    }

    func testCacheDoesNotAliasReusedDenseItemIDs() async {
        let loader = ThumbnailLoader()
        let first = PhotoItem(id: 0, url: nil, name: "first", kind: .synthetic,
                              captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        let second = PhotoItem(id: 0, url: nil, name: "second", kind: .synthetic,
                               captureDate: .distantPast, pixelWidth: 100, pixelHeight: 100)
        let ready = expectation(description: "synthetic preview")
        _ = loader.request(first, tier: .thumbnail) { _ in ready.fulfill() }
        await fulfillment(of: [ready], timeout: 5)
        XCTAssertNotNil(loader.cached(first, tier: .thumbnail))
        XCTAssertNil(loader.cached(second, tier: .thumbnail))
    }
}