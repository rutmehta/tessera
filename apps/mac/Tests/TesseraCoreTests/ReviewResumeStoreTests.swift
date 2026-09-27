import Foundation
import XCTest
@testable import TesseraCore

final class ReviewResumeStoreTests: XCTestCase {
    private func scratch() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("review-resume-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func record(folder: URL, revision: UInt64 = 1,
                        state: ReviewResumeRecord.RunState = .completed) -> ReviewResumeRecord {
        ReviewResumeRecord(
            recordRevision: revision,
            libraryPath: folder.standardizedFileURL.resolvingSymlinksInPath().path,
            queueID: UUID(),
            provider: "Scripted planner",
            scope: "selection",
            sourceDescription: "Selection · 2 photos",
            state: state,
            startedAt: Date(timeIntervalSince1970: 100),
            updatedAt: Date(timeIntervalSince1970: 200),
            targets: [
                .init(imageID: "photo-a", name: "a.jpg", ordinal: 0,
                      expectedGroupID: state == .running ? nil : 42),
                .init(imageID: "photo-b", name: "b.jpg", ordinal: 1, error: "Auto edit did not produce a review result.")
            ],
            selectedImageID: "photo-b",
            anchorImageID: "photo-a"
        )
    }

    func testRoundTripsOnlySafeQueueMembershipAndCursorForCanonicalLibraryPath() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        let expected = record(folder: folder)

        try store.save(expected)
        let loaded = try XCTUnwrap(store.load(libraryFolder: folder))

        XCTAssertEqual(loaded, expected)
        let data = try Data(contentsOf: store.fileURL(for: folder))
        let json = String(decoding: data, as: UTF8.self)
        XCTAssertFalse(json.localizedCaseInsensitiveContains("instruction"))
        XCTAssertFalse(json.localizedCaseInsensitiveContains("apiKey"))
        XCTAssertFalse(json.localizedCaseInsensitiveContains("authorization"))
    }

    func testDifferentCanonicalPathDoesNotAdoptAnotherLibraryQueue() throws {
        let root = try scratch()
        let a = root.appendingPathComponent("a", isDirectory: true)
        let b = root.appendingPathComponent("b", isDirectory: true)
        try FileManager.default.createDirectory(at: a, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: b, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        try store.save(record(folder: a))

        XCTAssertNil(try store.load(libraryFolder: b))
    }

    func testStaleRevisionCannotOverwriteNewerQueueRecord() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        try store.save(record(folder: folder, revision: 2))

        XCTAssertThrowsError(try store.save(record(folder: folder, revision: 1)))
        XCTAssertEqual(try store.load(libraryFolder: folder)?.recordRevision, 2)
    }

    func testFutureSchemaIsPreservedAndReportedWithoutDecodingAsCurrent() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        let path = store.fileURL(for: folder)
        try FileManager.default.createDirectory(at: path.deletingLastPathComponent(), withIntermediateDirectories: true)
        let future = Data(#"{"schemaVersion":999,"keep":"untouched"}"#.utf8)
        try future.write(to: path)

        XCTAssertThrowsError(try store.load(libraryFolder: folder))
        XCTAssertEqual(try Data(contentsOf: path), future)
    }

    func testRunningRecordCanBeMarkedInterruptedWithoutChangingTargetsOrReplaying() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        let running = record(folder: folder, state: .running)
        try store.save(running)

        let restored = try XCTUnwrap(store.restore(libraryFolder: folder))
        XCTAssertEqual(restored.state, .interrupted)
        XCTAssertEqual(restored.targets, running.targets)
        XCTAssertEqual(restored.targets.compactMap(\.expectedGroupID), [])
    }

    func testCompletingRunningIntentDoesNotTreatItAsAnInterruptedRestore() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        let running = record(folder: folder, state: .running)
        try store.save(running)
        XCTAssertEqual(try store.load(libraryFolder: folder)?.state, .running,
                       "ordinary reads must not interrupt an in-process run")
        let completed = try running.advanced(state: .completed)

        try store.save(completed)

        XCTAssertEqual(try store.load(libraryFolder: folder)?.state, .completed)
        XCTAssertEqual(try store.load(libraryFolder: folder)?.recordRevision, completed.recordRevision)
    }

    func testFailureAdvancesNewestCursorRevisionAndKeepsCapturedTargets() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        let running = record(folder: folder, state: .running)
        try store.save(running)
        let cursorUpdate = try running.updatingCursor(selectedID: "photo-b", anchorID: "photo-a")
        try store.save(cursorUpdate)
        let failed = try cursorUpdate.advanced(state: .failed)
        try store.save(failed)

        let loaded = try XCTUnwrap(store.load(libraryFolder: folder))
        XCTAssertEqual(loaded.state, .failed)
        XCTAssertEqual(loaded.recordRevision, 3)
        XCTAssertEqual(loaded.selectedImageID, "photo-b")
        XCTAssertEqual(loaded.anchorImageID, "photo-a")
        XCTAssertEqual(loaded.targets, running.targets)
    }

    func testDuplicateTargetIdentityIsRejectedAsCorrupt() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let store = ReviewResumeStore(directory: root.appendingPathComponent("support"))
        let value = record(folder: folder)
        let duplicate = ReviewResumeRecord(
            recordRevision: value.recordRevision, libraryPath: value.libraryPath, queueID: value.queueID,
            provider: value.provider, scope: value.scope, sourceDescription: value.sourceDescription,
            state: value.state, startedAt: value.startedAt, updatedAt: value.updatedAt,
            targets: [value.targets[0], value.targets[0]])

        XCTAssertThrowsError(try store.save(duplicate))
    }

    func testRevisionCannotOverflow() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let value = ReviewResumeRecord(
            recordRevision: UInt64.max, libraryPath: ReviewResumeStore.canonicalPath(folder), queueID: UUID(),
            provider: "Scripted planner", scope: "selection", sourceDescription: "Selection",
            state: .completed, startedAt: Date(), updatedAt: Date(), targets: [])

        XCTAssertThrowsError(try value.advanced())
    }

    func testTargetOrdinalsCanBeSafelyReindexedBeforeMerging() throws {
        let root = try scratch()
        let folder = root.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var value = record(folder: folder)
        value.targets[0].ordinal = Int.max

        let reindexed = ReviewResumeRecord.reindexedTargets(value.targets)

        XCTAssertEqual(reindexed.map(\.ordinal), [0, 1])
        XCTAssertEqual(reindexed.map(\.imageID), ["photo-b", "photo-a"])
    }
}
