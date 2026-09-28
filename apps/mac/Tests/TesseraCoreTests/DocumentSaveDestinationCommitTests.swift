import Darwin
import Foundation
import XCTest
@testable import TesseraCore

// SOURCE ONLY / UNRUN on B. Real tiny files, deterministic per-call barriers.
final class DocumentSaveDestinationCommitTests: XCTestCase {
    private final class Results: @unchecked Sendable {
        private let lock = NSLock()
        private var values: [DocSaveAsResult] = []
        private var failures: [String] = []
        func run(_ body: () throws -> DocSaveAsResult) {
            do { let result = try body(); lock.lock(); values.append(result); lock.unlock() }
            catch { lock.lock(); failures.append(String(describing: error)); lock.unlock() }
        }
        func read() -> ([DocSaveAsResult], [String]) { lock.lock(); defer { lock.unlock() }; return (values, failures) }
    }
    private func directory() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("checked-stub-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }
    func testRealCollisionLeavesSentinelAndMarkersThenRetryAndLegacyReplaceWork() throws {
        let dir = try directory(), url = dir.appendingPathComponent("out.tessera-doc")
        let doc = StubDocumentBackend(sampleWidth: 4, height: 4)
        _ = try doc.setOpacity(id: 2, value: 0.4, interactive: false)
        let before = try doc.info(), sentinel = Data("sentinel".utf8)
        try sentinel.write(to: url)
        let identity = try FileManager.default.attributesOfItem(atPath: url.path)[.systemFileNumber] as? NSNumber
        XCTAssertEqual(try doc.saveAs(path: url.path, intent: .createIfAbsent), .destinationExists)
        XCTAssertEqual(try Data(contentsOf: url), sentinel)
        XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: url.path)[.systemFileNumber] as? NSNumber, identity)
        XCTAssertEqual(try doc.info().path, before.path); XCTAssertEqual(try doc.info().title, before.title)
        XCTAssertTrue(try doc.info().dirty)
        let next = dir.appendingPathComponent("next.tessera-doc")
        XCTAssertEqual(try doc.saveAs(path: next.path, intent: .createIfAbsent), .saved)
        XCTAssertFalse(try doc.info().dirty)
        XCTAssertEqual(try doc.saveAs(path: url.path, intent: .replaceConfirmed), .saved)
        try doc.saveAs(path: url.path) // Existing legacy replacing control.
        try doc.save()
        XCTAssertNotEqual(try Data(contentsOf: url), sentinel)
        XCTAssertThrowsError(try doc.saveAs(path: dir.appendingPathComponent("out.psd").path, intent: .createIfAbsent))
        XCTAssertEqual(Set(try FileManager.default.contentsOfDirectory(atPath: dir.path)), ["out.tessera-doc", "next.tessera-doc"])
    }
    func testLateFileAppearanceAndUnsafeEntriesNeverClobber() throws {
        let dir = try directory(), target = dir.appendingPathComponent("race.tessera-doc")
        let sentinel = Data("late".utf8)
        let doc = StubDocumentBackend(sampleWidth: 4, height: 4)
        XCTAssertEqual(try doc.saveForTesting(path: target.path, intent: .createIfAbsent, beforeCommit: { _ in
            try sentinel.write(to: target)
        }), .destinationExists)
        XCTAssertEqual(try Data(contentsOf: target), sentinel)
        for name in ["directory.tessera-doc", "symlink.tessera-doc"] {
            let url = dir.appendingPathComponent(name)
            if name.hasPrefix("directory") { try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false) }
            else { try FileManager.default.createSymbolicLink(atPath: url.path, withDestinationPath: dir.appendingPathComponent("missing").path) }
            XCTAssertEqual(try doc.saveAs(path: url.path, intent: .createIfAbsent), .destinationExists)
        }
        XCTAssertThrowsError(try doc.saveAs(path: dir.appendingPathComponent("missing/out.tessera-doc").path, intent: .createIfAbsent))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).count, 3)
    }
    func testConcurrentSameNamePublishesExactlyOneAndUsesUniqueStaging() throws {
        let dir = try directory(), target = dir.appendingPathComponent("race.tessera-doc")
        let ready = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0), group = DispatchGroup()
        let results = Results()
        for _ in 0..<2 {
            let doc = StubDocumentBackend(sampleWidth: 4, height: 4)
            group.enter()
            DispatchQueue.global().async {
                defer { group.leave() }
                results.run { try doc.saveForTesting(path: target.path, intent: .createIfAbsent, beforeCommit: { _ in
                    ready.signal()
                    guard release.wait(timeout: .now() + 5) == .success else { throw NSError(domain: "barrier", code: 1) }
                }) }
            }
        }
        defer { release.signal(); release.signal() }
        XCTAssertEqual(ready.wait(timeout: .now() + 5), .success)
        XCTAssertEqual(ready.wait(timeout: .now() + 5), .success)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).count, 2, "two exclusively reserved stages")
        release.signal(); release.signal()
        XCTAssertEqual(group.wait(timeout: .now() + 5), .success)
        let (values, failures) = results.read()
        XCTAssertTrue(failures.isEmpty, "\(failures)")
        XCTAssertEqual(values.filter { $0 == .saved }.count, 1)
        XCTAssertEqual(values.filter { $0 == .destinationExists }.count, 1)
        _ = try StubDocumentEngine().openDocument(path: target.path)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path), ["race.tessera-doc"])
    }
    func testCapturedHeadLeavesLaterEditDirtyAndOrdinarySaveReadsPathAfterGate() throws {
        let dir = try directory(), a = dir.appendingPathComponent("a.tessera-doc"), b = dir.appendingPathComponent("b.tessera-doc")
        let doc = StubDocumentBackend(sampleWidth: 4, height: 4)
        try doc.saveAs(path: a.path)
        let oldA = try Data(contentsOf: a)
        _ = try doc.setOpacity(id: 2, value: 0.4, interactive: false)
        let snapshotReady = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0), group = DispatchGroup()
        let result = Results()
        group.enter()
        DispatchQueue.global().async {
            defer { group.leave() }
            result.run { try doc.saveForTesting(path: b.path, intent: .createIfAbsent, beforeCommit: { _ in
                snapshotReady.signal()
                guard release.wait(timeout: .now() + 5) == .success else { throw NSError(domain: "barrier", code: 2) }
            }) }
        }
        defer { release.signal() }
        XCTAssertEqual(snapshotReady.wait(timeout: .now() + 5), .success)
        _ = try doc.setOpacity(id: 2, value: 0.8, interactive: false)
        release.signal()
        XCTAssertEqual(group.wait(timeout: .now() + 5), .success)
        XCTAssertTrue(result.read().1.isEmpty)
        XCTAssertTrue(try doc.info().dirty)
        let captured = try StubDocumentEngine().openDocument(path: b.path)
        XCTAssertEqual(try captured.layer(id: 2).opacity, 0.4, accuracy: 0.001)
        XCTAssertEqual(try Data(contentsOf: a), oldA)

        // Hold a second Save As while an ordinary Save reaches the save gate.
        let c = dir.appendingPathComponent("c.tessera-doc"), ready = DispatchSemaphore(value: 0)
        let allow = DispatchSemaphore(value: 0), queued = DispatchSemaphore(value: 0)
        let oldB = try Data(contentsOf: b)
        group.enter()
        DispatchQueue.global().async {
            defer { group.leave() }
            result.run { try doc.saveForTesting(path: c.path, intent: .createIfAbsent, beforeCommit: { _ in
                ready.signal()
                guard allow.wait(timeout: .now() + 5) == .success else { throw NSError(domain: "barrier", code: 3) }
            }) }
        }
        defer { allow.signal() }
        XCTAssertEqual(ready.wait(timeout: .now() + 5), .success)
        _ = try doc.setOpacity(id: 2, value: 0.6, interactive: false)
        group.enter()
        DispatchQueue.global().async {
            defer { group.leave() }
            result.run { try doc.saveForTesting(willAcquireSaveGate: { queued.signal() }) }
        }
        XCTAssertEqual(queued.wait(timeout: .now() + 5), .success)
        allow.signal()
        XCTAssertEqual(group.wait(timeout: .now() + 5), .success)
        XCTAssertTrue(result.read().1.isEmpty)
        XCTAssertEqual(try Data(contentsOf: b), oldB, "ordinary Save must not resolve stale path before the gate")
        let saved = try StubDocumentEngine().openDocument(path: c.path)
        XCTAssertEqual(try saved.layer(id: 2).opacity, 0.6, accuracy: 0.001)
        XCTAssertFalse(try doc.info().dirty)
    }
    func testExclusiveRenameFallbackAndPostCommitCleanupFailureKeepActualResult() throws {
        let dir = try directory(), url = dir.appendingPathComponent("out.tessera-doc"), bytes = Data("file".utf8)
        let hooks = DocumentSaveDestinationCommit.Hooks(exclusiveRename: { _, _ in errno = ENOSYS; return -1 },
            unlinkStage: { _ in errno = EACCES; return -1 })
        XCTAssertEqual(try DocumentSaveDestinationCommit.write(bytes, to: url, intent: .createIfAbsent, hooks: hooks), .saved)
        XCTAssertEqual(try Data(contentsOf: url), bytes)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).count, 2, "failed cleanup is not hidden")
        XCTAssertEqual(try DocumentSaveDestinationCommit.write(Data("other".utf8), to: url, intent: .createIfAbsent), .destinationExists)
        XCTAssertEqual(try Data(contentsOf: url), bytes)
    }
    // SOURCE ONLY / UNRUN: both unsupported exclusive-rename results must
    // exercise the real no-clobber link fallback when a destination already exists.
    func testForcedExclusiveRenameFallbackCollisionPreservesSentinelAndRemovesStage() throws {
        for unsupported in [ENOSYS, EINVAL] {
            let dir = try directory(), url = dir.appendingPathComponent("sentinel.tessera-doc")
            let sentinel = Data("existing destination".utf8)
            try sentinel.write(to: url)
            let inode = try XCTUnwrap(FileManager.default.attributesOfItem(atPath: url.path)[.systemFileNumber] as? NSNumber)
            let invoked = DispatchSemaphore(value: 0)
            let hooks = DocumentSaveDestinationCommit.Hooks(exclusiveRename: { _, _ in
                invoked.signal()
                errno = unsupported
                return -1
            })
            XCTAssertEqual(try DocumentSaveDestinationCommit.write(Data("must not replace".utf8), to: url,
                               intent: .createIfAbsent, hooks: hooks), .destinationExists)
            XCTAssertEqual(invoked.wait(timeout: .now()), .success, "forced fallback hook must run")
            XCTAssertEqual(invoked.wait(timeout: .now()), .timedOut, "exclusive rename attempted once")
            XCTAssertEqual(try Data(contentsOf: url), sentinel)
            XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: url.path)[.systemFileNumber] as? NSNumber, inode)
            XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path), [url.lastPathComponent],
                           "the failed attempt must remove its own stage")
        }
    }

    func testSuccessfulRenameDoesNotCleanupReusedOldStageName() throws {
        let dir = try directory(), url = dir.appendingPathComponent("out.tessera-doc")
        let hooks = DocumentSaveDestinationCommit.Hooks(afterCommit: { stage in try Data("unowned".utf8).write(to: stage) })
        XCTAssertEqual(try DocumentSaveDestinationCommit.write(Data("owned".utf8), to: url, intent: .replaceConfirmed, hooks: hooks), .saved)
        let other = try FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil).filter { $0 != url }
        XCTAssertEqual(other.count, 1)
        XCTAssertEqual(try Data(contentsOf: XCTUnwrap(other.first)), Data("unowned".utf8))
    }
    func testReplacedStagingNameIsNeitherPublishedNorDeleted() throws {
        let dir = try directory(), target = dir.appendingPathComponent("out.tessera-doc")
        let retained = dir.appendingPathComponent("original-stage")
        let hooks = DocumentSaveDestinationCommit.Hooks(beforeCommit: { stage in
            try FileManager.default.moveItem(at: stage, to: retained)
            try Data("foreign".utf8).write(to: stage)
        })
        XCTAssertThrowsError(try DocumentSaveDestinationCommit.write(Data("ours".utf8), to: target,
                                                                     intent: .createIfAbsent, hooks: hooks))
        XCTAssertFalse(FileManager.default.fileExists(atPath: target.path))
        let entries = try FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        XCTAssertEqual(entries.count, 2)
        let foreign = try XCTUnwrap(entries.first { $0 != retained })
        XCTAssertEqual(try Data(contentsOf: foreign), Data("foreign".utf8))
    }
    func testConfirmedReplaceStillReplacesPathChangedAfterConfirmation() throws {
        let dir = try directory(), target = dir.appendingPathComponent("out.tessera-doc")
        try Data("initial".utf8).write(to: target)
        let hooks = DocumentSaveDestinationCommit.Hooks(beforeCommit: { _ in
            try Data("changed externally".utf8).write(to: target, options: .atomic)
        })
        XCTAssertEqual(try DocumentSaveDestinationCommit.write(Data("confirmed".utf8), to: target,
                            intent: .replaceConfirmed, hooks: hooks), .saved)
        XCTAssertEqual(try Data(contentsOf: target), Data("confirmed".utf8))
    }

}
