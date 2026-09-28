import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Source-only on B. Fake thumbnail bytes/events; native proxy validation is A's gate.
@MainActor
final class SmartPreviewThumbnailTests: XCTestCase {
    private func fixture() throws -> (Engine, PreviewEvents) {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("proxy-thumbs-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        return (try Engine.open(appSupportDir: root.path), PreviewEvents())
    }
    private func item(_ engine: Engine, _ events: PreviewEvents, _ source: EnginePreviewSource,
                      _ api: EngineThumbnailAPI) -> PhotoItem {
        let ref = EngineImageReference(engine: engine, imageID: "same", previewEvents: events,
                                       previewSource: source, thumbnailAPI: api)
        return PhotoItem(id: 0, url: nil, name: "offline.raw", kind: .raw, captureDate: .distantPast,
                         pixelWidth: 2, pixelHeight: 2, engineImage: ref)
    }
    private func png() throws -> Data {
        let context = try XCTUnwrap(CGContext(data: nil, width: 2, height: 2, bitsPerComponent: 8,
            bytesPerRow: 8, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let image = try XCTUnwrap(context.makeImage())
        let data = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return data as Data
    }
    func testTransientProxyErrorRetriesWithinSameFlightThenDelivers() async throws {
        let (engine, events) = try fixture()
        let bytes = try png()
        let calls = ThumbnailCallLog()
        let waiting = expectation(description: "retry backoff entered")
        let clock = ThumbnailRetryClock(waiting: waiting)
        let api = EngineThumbnailAPI(original: { _, _ in XCTFail("No Original fallback"); throw ThumbnailProbeError.failed },
            smartPreview: { _, _ in
                if calls.append("proxy") == 1 { throw ThumbnailProbeError.failed }
                return PreviewResponse(bytes: bytes, pending: false)
            })
        let photo = item(engine, events, .smartPreview, api)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1024, previewCostLimit: 1024,
                                     proxyRetryWait: { await clock.pause($0) })
        let ready = expectation(description: "retried thumbnail")
        let request = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in ready.fulfill() })
        await fulfillment(of: [waiting], timeout: 5)
        XCTAssertEqual(calls.values.count, 1)
        XCTAssertEqual(loader.queueSnapshot.active, 1, "backoff retains the existing flight slot")
        XCTAssertFalse(request.isFlightDrained)
        await clock.resume()
        await fulfillment(of: [ready], timeout: 5)
        await request.waitForFlightDrain()
        XCTAssertEqual(calls.values.count, 2)
        XCTAssertNotNil(loader.cached(photo, tier: .thumbnail))
    }

    func testPersistentProxyFailureHasBoundedCallsAndNoPendingPlaceholderFlight() async throws {
        let (engine, events) = try fixture()
        let calls = ThumbnailCallLog()
        let waits = ThumbnailCallLog()
        let api = EngineThumbnailAPI(original: { _, _ in XCTFail("No Original fallback"); throw ThumbnailProbeError.failed },
            smartPreview: { _, _ in calls.append("proxy"); throw ThumbnailProbeError.failed })
        let photo = item(engine, events, .smartPreview, api)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1024, previewCostLimit: 1024,
                                     proxyRetryWait: { waits.append(String($0)) })
        let request = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in XCTFail("Failure has no pixels") })
        await request.waitForFlightDrain()
        XCTAssertEqual(calls.values.count, 4)
        XCTAssertEqual(waits.values, ["250000000", "500000000", "1000000000"])
        XCTAssertEqual(loader.queueSnapshot.active, 0)
        XCTAssertEqual(loader.queueSnapshot.pending, 0)
        XCTAssertNil(loader.cached(photo, tier: .thumbnail))
    }

    func testSupersessionDuringRetrySuppressesOldRetryAndDeliversReplacement() async throws {
        let (engine, events) = try fixture()
        let bytes = try png()
        let calls = ThumbnailCallLog()
        let waiting = expectation(description: "old flight is in retry backoff")
        let clock = ThumbnailRetryClock(waiting: waiting)
        let api = EngineThumbnailAPI(original: { _, _ in XCTFail("No Original fallback"); throw ThumbnailProbeError.failed },
            smartPreview: { _, _ in
                if calls.append("proxy") == 1 { throw ThumbnailProbeError.failed }
                return PreviewResponse(bytes: bytes, pending: false)
            })
        let photo = item(engine, events, .smartPreview, api)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1024, previewCostLimit: 1024,
                                     proxyRetryWait: { await clock.pause($0) })
        let old = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in XCTFail("stale subscriber") })
        await fulfillment(of: [waiting], timeout: 5)
        loader.invalidate(photo)
        await old.waitForCompletion()
        XCTAssertTrue(old.isCancelled)
        XCTAssertFalse(old.isFlightDrained)
        let ready = expectation(description: "replacement only")
        let replacement = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in ready.fulfill() })
        XCTAssertEqual(calls.values.count, 1)
        // Deliberately non-cancellation-aware test clock: production must recheck
        // cancellation after even an injected wait returns normally.
        await clock.resume()
        await fulfillment(of: [ready], timeout: 5)
        await old.waitForFlightDrain()
        await replacement.waitForFlightDrain()
        XCTAssertEqual(calls.values.count, 2, "old flight must not issue another native call")
        XCTAssertNotNil(loader.cached(photo, tier: .thumbnail))
    }

    func testCancelledRetryWaitTerminatesWithoutAnotherNativeCall() async throws {
        let (engine, events) = try fixture()
        let calls = ThumbnailCallLog()
        let api = EngineThumbnailAPI(original: { _, _ in throw ThumbnailProbeError.failed },
            smartPreview: { _, _ in calls.append("proxy"); throw ThumbnailProbeError.failed })
        let photo = item(engine, events, .smartPreview, api)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1024, previewCostLimit: 1024,
                                     proxyRetryWait: { _ in throw CancellationError() })
        let request = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in XCTFail("cancelled wait") })
        await request.waitForFlightDrain()
        XCTAssertEqual(calls.values.count, 1)
        XCTAssertNil(loader.cached(photo, tier: .thumbnail))
    }

    func testOriginalErrorsRemainTerminalWithoutRetry() async throws {
        let (engine, events) = try fixture()
        let calls = ThumbnailCallLog()
        let api = EngineThumbnailAPI(original: { _, _ in calls.append("original"); throw ThumbnailProbeError.failed },
            smartPreview: { _, _ in XCTFail("No proxy fallback"); throw ThumbnailProbeError.failed })
        let photo = item(engine, events, .original, api)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1024, previewCostLimit: 1024,
                                     proxyRetryWait: { _ in XCTFail("Original must not retry") })
        let request = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in XCTFail("failed original") })
        await request.waitForFlightDrain()
        XCTAssertEqual(calls.values, ["original"])
        XCTAssertNil(loader.cached(photo, tier: .thumbnail))
    }

    func testExplicitRoleChoosesExactlyOneAPIAndNeverFallsBack() throws {
        let (engine, events) = try fixture()
        let bytes = try png()
        let calls = ThumbnailCallLog()
        let api = EngineThumbnailAPI(original: { id, px in
            calls.append("original:\(id):\(px)"); return PreviewResponse(bytes: bytes, pending: false)
        }, smartPreview: { id, px in
            calls.append("proxy:\(id):\(px)"); throw ThumbnailProbeError.failed
        })
        let original = item(engine, events, .original, api)
        let proxy = item(engine, events, .smartPreview, api)
        XCTAssertNotEqual(ThumbnailLoader.key(original), ThumbnailLoader.key(proxy))
        XCTAssertNotNil(ThumbnailLoader.render(original, tier: .thumbnail))
        XCTAssertNil(ThumbnailLoader.render(proxy, tier: .preview))
        XCTAssertEqual(calls.values, ["original:same:384", "proxy:same:2560"])
    }
    func testOriginalCacheCannotSatisfySameImageProxyRequest() async throws {
        let (engine, events) = try fixture()
        let bytes = try png()
        let api = EngineThumbnailAPI(original: { _, _ in PreviewResponse(bytes: bytes, pending: false) },
            smartPreview: { _, _ in throw ThumbnailProbeError.failed })
        let original = item(engine, events, .original, api)
        let proxy = item(engine, events, .smartPreview, api)
        let loader = ThumbnailLoader()
        let ready = expectation(description: "original cached")
        let first = try XCTUnwrap(loader.request(original, tier: .thumbnail) { _ in ready.fulfill() })
        await fulfillment(of: [ready], timeout: 5)
        await first.waitForFlightDrain()
        XCTAssertNotNil(loader.cached(original, tier: .thumbnail))
        XCTAssertNil(loader.cached(proxy, tier: .thumbnail))
        let failed = try XCTUnwrap(loader.request(proxy, tier: .thumbnail) { _ in XCTFail("No original-cache fallback") })
        await failed.waitForFlightDrain()
        XCTAssertNil(loader.cached(proxy, tier: .thumbnail))
        XCTAssertNotNil(loader.cached(original, tier: .thumbnail))
    }
    func testPendingProxyUsesBufferedReadyEventAndKeepsSource() async throws {
        let (engine, events) = try fixture()
        let bytes = try png()
        let calls = ThumbnailCallLog()
        let api = EngineThumbnailAPI(original: { _, _ in XCTFail("No original route"); throw ThumbnailProbeError.failed },
            smartPreview: { id, px in
                let count = calls.append("proxy")
                if count == 1 {
                    // Arrives before await; subscription must already exist.
                    events.onEvent(event: .previewReady(imageId: id, maxPx: px))
                    return PreviewResponse(bytes: nil, pending: true)
                }
                return PreviewResponse(bytes: bytes, pending: false)
            })
        let photo = item(engine, events, .smartPreview, api)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1024, previewCostLimit: 1024,
                                     proxyRetryWait: { _ in XCTFail("Successful pending path must use events") })
        let ready = expectation(description: "proxy ready")
        let request = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in ready.fulfill() })
        await fulfillment(of: [ready], timeout: 5)
        await request.waitForFlightDrain()
        XCTAssertEqual(calls.values, ["proxy", "proxy"])
        XCTAssertNotNil(loader.cached(photo, tier: .thumbnail))
    }
    func testInvalidatedProxyWorkerCannotRepopulateCacheOrDeliverOldResult() async throws {
        let (engine, events) = try fixture()
        let bytes = try png()
        let gate = ThumbnailReadGate()
        let api = EngineThumbnailAPI(original: { _, _ in XCTFail("No original route"); throw ThumbnailProbeError.failed },
            smartPreview: { _, _ in gate.pauseFirst(); return PreviewResponse(bytes: bytes, pending: false) })
        let photo = item(engine, events, .smartPreview, api)
        let loader = ThumbnailLoader()
        let old = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in XCTFail("stale result") })
        defer { gate.release.signal() }
        let entered = await gate.waitForEntry()
        XCTAssertTrue(entered)
        loader.invalidate(photo) // same path used after local save/rebuild/discard
        XCTAssertTrue(old.isCancelled)
        XCTAssertNil(loader.cached(photo, tier: .thumbnail))
        let ready = expectation(description: "replacement")
        let replacement = try XCTUnwrap(loader.request(photo, tier: .thumbnail) { _ in ready.fulfill() })
        XCTAssertFalse(old.isFlightDrained)
        gate.release.signal()
        await fulfillment(of: [ready], timeout: 5)
        await old.waitForFlightDrain()
        await replacement.waitForFlightDrain()
        XCTAssertEqual(gate.count, 2)
        XCTAssertNotNil(loader.cached(photo, tier: .thumbnail))
    }
    func testReconnectDropsProxyFlightAndDoesNotShareOriginalCache() async throws {
        let (engine, events) = try fixture()
        let bytes = try png()
        let gate = ThumbnailReadGate()
        let api = EngineThumbnailAPI(original: { _, _ in PreviewResponse(bytes: bytes, pending: false) },
            smartPreview: { _, _ in gate.pauseFirst(); return PreviewResponse(bytes: bytes, pending: false) })
        let proxy = item(engine, events, .smartPreview, api)
        let original = item(engine, events, .original, api)
        let loader = ThumbnailLoader()
        let old = try XCTUnwrap(loader.request(proxy, tier: .thumbnail) { _ in XCTFail("old source delivered") })
        defer { gate.release.signal() }
        let entered = await gate.waitForEntry()
        XCTAssertTrue(entered)
        loader.removeAll() // AppModel.install does this before replacing the library
        let ready = expectation(description: "original route")
        let current = try XCTUnwrap(loader.request(original, tier: .thumbnail) { _ in ready.fulfill() })
        await fulfillment(of: [ready], timeout: 5)
        gate.release.signal()
        await old.waitForFlightDrain()
        await current.waitForFlightDrain()
        XCTAssertNil(loader.cached(proxy, tier: .thumbnail))
        XCTAssertNotNil(loader.cached(original, tier: .thumbnail))
    }
}

private enum ThumbnailProbeError: Error { case failed }
private final class ThumbnailCallLog: @unchecked Sendable {
    private let lock = NSLock()
    private var entries: [String] = []
    @discardableResult func append(_ value: String) -> Int {
        lock.withLock { entries.append(value); return entries.count }
    }
    var values: [String] { lock.withLock { entries } }
}
private final class ThumbnailReadGate: @unchecked Sendable {
    private let lock = NSLock()
    private let entered = DispatchSemaphore(value: 0)
    let release = DispatchSemaphore(value: 0)
    private var calls = 0
    var count: Int { lock.withLock { calls } }
    func pauseFirst() {
        let first = lock.withLock { calls += 1; return calls == 1 }
        if first { entered.signal(); _ = release.wait(timeout: .now() + 10) }
    }
    func waitForEntry() async -> Bool {
        await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                continuation.resume(returning: self.entered.wait(timeout: .now() + 5) == .success)
            }
        }
    }
}

/// Ordering uses a continuation, not elapsed time; production receives Task.sleep.
private actor ThumbnailRetryClock {
    let waiting: XCTestExpectation
    private var continuation: CheckedContinuation<Void, Never>?
    init(waiting: XCTestExpectation) { self.waiting = waiting }
    func pause(_ nanoseconds: UInt64) async {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            waiting.fulfill()
        }
    }
    func resume() { continuation?.resume(); continuation = nil }
}
