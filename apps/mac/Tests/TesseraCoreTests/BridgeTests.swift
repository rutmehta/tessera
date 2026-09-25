import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

final class BridgeTests: XCTestCase {
    func testRawFixturesPersistAcrossReopen() throws {
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
        let item = try XCTUnwrap(library.items.first)
        try library.persist(CullState(decision: .reject), for: item)
        let reopened = try EngineLibrary.scan(folder: photos, appSupport: support)
        let found = try XCTUnwrap(reopened.items.first { $0.url == item.url })
        XCTAssertEqual(reopened.initialState(for: found).decision, .reject)
        XCTAssertNotNil(ThumbnailLoader.render(found, tier: .thumbnail))
        let ref = try XCTUnwrap(found.engineImage)
        let events = BridgeEvents()
        ref.engine.setEventListener(listener: events)
        _ = try ref.engine.indexFolder(path: photos.path)
        _ = try ref.engine.embeddedPreview(imageId: ref.imageID, maxPx: 128)
        XCTAssertEqual(events.count, 3)
        ref.engine.setEventListener(listener: nil)

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

private final class BridgeEvents: EngineEventListener, @unchecked Sendable {
    private let lock = NSLock()
    private var events: [EngineEvent] = []
    var count: Int { lock.withLock { events.count } }
    func onEvent(event: EngineEvent) { lock.withLock { events.append(event) } }
}
