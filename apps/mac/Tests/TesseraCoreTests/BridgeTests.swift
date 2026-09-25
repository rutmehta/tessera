import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

final class BridgeTests: XCTestCase {
    @MainActor func testRawFixturesPersistAcrossReopen() async throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        let fixture = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        // Never mutate the shared fixture corpus. Real RAW bytes, independent sidecars/catalog.
        let temp = root.appendingPathComponent("build/bridge-test-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temp) }
        let photos = temp.appendingPathComponent("raw")
        try FileManager.default.copyItem(at: fixture, to: photos)
        let support = temp.appendingPathComponent("support")
        let library = try EngineLibrary.scan(folder: photos, appSupport: support)
        XCTAssertEqual(library.items.count, 5)
        let item = try XCTUnwrap(library.items.first { $0.name == "sample.dng" })
        try library.persist(CullState(decision: .reject), for: item)
        let reopened = try EngineLibrary.scan(folder: photos, appSupport: support)
        let found = try XCTUnwrap(reopened.items.first { $0.url == item.url })
        XCTAssertEqual(reopened.initialState(for: found).decision, .reject)
        let loader = ThumbnailLoader()
        let ready = expectation(description: "cold RAW preview completes from worker callback")
        let request = loader.request(found, tier: .thumbnail) { image in
            XCTAssertGreaterThan(image.width, 0)
            ready.fulfill()
        }
        await fulfillment(of: [ready], timeout: 30)
        XCTAssertFalse(request?.isCancelled ?? true)
        XCTAssertNotNil(loader.cached(found, tier: .thumbnail))
        let ref = try XCTUnwrap(found.engineImage)
        let cached = try ref.engine.embeddedPreview(imageId: ref.imageID, maxPx: 384)
        XCTAssertFalse(cached.pending)
        XCTAssertNotNil(cached.bytes)
        XCTAssertNotNil(ThumbnailLoader.render(found, tier: .thumbnail))

        var cull = CullStore(count: reopened.items.count)
        for action: CullAction in [.keep, .grade(3), .mark(6), .reject, .undecided] {
            cull.apply(action, to: [found.id])
            try reopened.persist(cull[found.id], for: found)
            let next = try EngineLibrary.scan(folder: photos, appSupport: support)
            let nextItem = try XCTUnwrap(next.items.first { $0.url == found.url })
            XCTAssertEqual(next.initialState(for: nextItem), cull[found.id])
        }
        for _ in 0..<2 {
            _ = cull.undo()
            try reopened.persist(cull[found.id], for: found)
        }
        let afterUndo = try EngineLibrary.scan(folder: photos, appSupport: support)
        let undoItem = try XCTUnwrap(afterUndo.items.first { $0.url == found.url })
        XCTAssertEqual(afterUndo.initialState(for: undoItem), cull[found.id])
    }

    func testRestoreDoesNotCreateUndoHistory() {
        let store = CullStore(states: [CullState(decision: .keep, grade: 2), CullState(decision: .reject)])
        XCTAssertEqual(store.counts.keep, 1)
        XCTAssertEqual(store.counts.reject, 1)
        XCTAssertFalse(store.canUndo)
    }
}