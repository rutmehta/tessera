import CryptoKit
import Foundation
import XCTest
@testable import TesseraCore

/// Real bridge qualification on the repository's Sony ARW fixture
/// (`fixtures/raw/sony-arw.ARW`; `TESSERA_SMART_PREVIEW_RAW` overrides it); no
/// NSApplication, window, or GUI harness. Every write targets a disposable photo
/// COPY or its isolated support directory.
@MainActor
final class SmartPreviewNativeWorkflowTests: XCTestCase {
    /// `TESSERA_SMART_PREVIEW_RAW` when set, else the repository's Sony ARW fixture.
    /// Absence skips visibly, or fails when `TESSERA_REQUIRE_RAW_FIXTURES` is set.
    private func sonyFixture() throws -> URL {
        let environment = ProcessInfo.processInfo.environment
        if let path = environment["TESSERA_SMART_PREVIEW_RAW"], !path.isEmpty {
            return URL(fileURLWithPath: path)
        }
        let fixture = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("../../fixtures/raw/sony-arw.ARW").standardizedFileURL
        if FileManager.default.fileExists(atPath: fixture.path) { return fixture }
        let reason = "fixtures/raw/sony-arw.ARW is absent (run fixtures/fetch.sh or set TESSERA_SMART_PREVIEW_RAW)"
        if environment["TESSERA_REQUIRE_RAW_FIXTURES"] != nil {
            struct MissingFixture: Error {}
            XCTFail("\(reason); TESSERA_REQUIRE_RAW_FIXTURES is set")
            throw MissingFixture()
        }
        throw XCTSkip(reason)
    }

    private func sha256(_ url: URL) throws -> String {
        let file = try FileHandle(forReadingFrom: url)
        defer { try? file.close() }
        var hash = SHA256()
        while let bytes = try file.read(upToCount: 65_536), !bytes.isEmpty {
            hash.update(data: bytes)
        }
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }

    private func files(_ folder: URL) throws -> [String: String] {
        // Relative names avoid /var versus /private/var spelling differences.
        let enumerator = try XCTUnwrap(FileManager.default.enumerator(atPath: folder.path))
        var result: [String: String] = [:]
        for case let relative as String in enumerator {
            let url = folder.appendingPathComponent(relative)
            if try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true {
                result[relative] = try sha256(url)
            }
        }
        return result
    }

    private func nextFrame(_ controller: DevelopController, after generation: UInt64) async throws -> DevelopFrame {
        let deadline = Date().addingTimeInterval(60)
        while Date() < deadline {
            if let frame = controller.lastFrame, frame.isFinal, frame.generation > generation { return frame }
            try await Task.sleep(for: .milliseconds(10))
        }
        struct FrameTimeout: Error {}
        XCTFail("No final proxy frame within 60 seconds")
        throw FrameTimeout()
    }

    private func prepareOnline(folder: URL, support: URL) async throws -> (folder: URL, imageID: String) {
        let library = try EngineLibrary.open(folder: folder, appSupport: support)
        XCTAssertEqual(library.accessMode, .originalFolder)
        XCTAssertFalse(library.isReadOnly)
        let item = try XCTUnwrap(library.items.only)
        let imageID = try XCTUnwrap(item.engineImage?.imageID)
        let canonical = try XCTUnwrap(library.folder)
        // Use the actual edit/commit/save path so settings and history remain consistent.
        let original = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        do {
            original.apply(patch: ["denoise": ["method": ["kind": "off"]],
                                   "tone": ["exposure": 0.25]], interactive: false)
            XCTAssertTrue(original.commit(label: "Smart Preview build baseline"))
            try (await original.close()).get()
        } catch {
            _ = await original.close()
            throw error
        }
        let built = try await SmartPreviewAPI.live(engine: library.engine).build(imageID)
        XCTAssertEqual(built.state, .ready)
        XCTAssertFalse(built.dirty)
        XCTAssertTrue(built.originalAvailable)
        // This qualification targets the repository's Sony ARW fixture.
        XCTAssertEqual(built.width, 1640)
        XCTAssertEqual(built.height, 1092)
        return (canonical, imageID)
    }

    private func editOffline(_ library: EngineLibrary, imageID: String) async throws {
        let item = try XCTUnwrap(library.items.only)
        let reference = try XCTUnwrap(item.engineImage)
        let snapshot = try await SmartPreviewAPI.live(engine: library.engine).info(imageID)
        XCTAssertEqual(snapshot.state, .originalOffline)
        XCTAssertFalse(snapshot.originalAvailable)
        XCTAssertThrowsError(try SmartPreviewRouting.route(snapshot, preferPreview: false))
        let controller = try await SmartPreviewRouting.open(snapshot, preferPreview: true) { source in
            try await DevelopController.open(reference, itemID: item.id, source: source)
        }
        do {
            XCTAssertEqual(controller.sourceRoute, .smartPreview)
            XCTAssertEqual(controller.value(.exposure), 0.25, accuracy: 0.0001)
            try controller.attachSurfaces(viewWidth: 640, viewHeight: 480, count: 2)
            let baseline = try await nextFrame(controller, after: 0)
            let before = try XCTUnwrap(controller.histogram).luminance
            controller.set(.exposure, 1.25, interactive: false)
            let edited = try await nextFrame(controller, after: baseline.generation)
            XCTAssertGreaterThan(edited.width, 0)
            XCTAssertNotEqual(try XCTUnwrap(controller.histogram).luminance, before)
            controller.set(.temperature, 4200, interactive: false)
            _ = try await nextFrame(controller, after: edited.generation)
            XCTAssertTrue(controller.commit(label: "Offline Smart Preview edit"))
            try (await controller.close()).get()
        } catch {
            _ = await controller.close()
            throw error
        }
    }

    /// Separate acceptance check: a failure here must not mask the Develop workflow.
    func testActualCachedThumbnailAfterOriginalDisconnect() async throws {
        let fixture = try sonyFixture()
        let sourceHash = try sha256(fixture)
        defer { XCTAssertEqual(try? sha256(fixture), sourceHash) }
        let fm = FileManager.default
        let scratch = fm.temporaryDirectory.appendingPathComponent("smart-preview-thumbnail-\(UUID().uuidString)")
        let photos = scratch.appendingPathComponent("photos", isDirectory: true)
        let hidden = scratch.appendingPathComponent("disconnected-copy", isDirectory: true)
        let support = scratch.appendingPathComponent("support", isDirectory: true)
        try fm.createDirectory(at: photos, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: scratch) }
        try fm.copyItem(at: fixture, to: photos.appendingPathComponent(fixture.lastPathComponent))
        let prepared = try await prepareOnline(folder: photos, support: support)
        // Populate the ordinary thumbnail cache through the real EngineImageReference.
        let online = try EngineLibrary.open(folder: prepared.folder, appSupport: support)
        let item = try XCTUnwrap(online.items.only)
        let reference = try XCTUnwrap(item.engineImage)
        let deadline = Date().addingTimeInterval(60)
        var cachedBytes: Data?
        while Date() < deadline {
            let response = try reference.engine.embeddedPreview(imageId: reference.imageID, maxPx: 384)
            if let bytes = response.bytes { cachedBytes = bytes; break }
            try await Task.sleep(for: .milliseconds(10))
        }
        let bytes = try XCTUnwrap(cachedBytes, "Online thumbnail did not become ready")
        XCTAssertFalse(bytes.isEmpty)
        XCTAssertNotNil(ThumbnailLoader.render(item, tier: .thumbnail))
        let baseline = try files(photos)
        try fm.moveItem(at: photos, to: hidden)
        defer {
            XCTAssertFalse(fm.fileExists(atPath: photos.path))
            XCTAssertEqual(try? files(hidden), baseline)
        }
        let offline = try EngineLibrary.open(folder: prepared.folder, appSupport: support)
        let cachedItem = try XCTUnwrap(offline.items.only)
        XCTAssertTrue(offline.isReadOnly)
        // Positive product assertion, intentionally not an expected-error assertion.
        // Current native request_raw stats the absent original before cache lookup.
        let offlineDeadline = Date().addingTimeInterval(60)
        var delivered = false
        while Date() < offlineDeadline {
            if ThumbnailLoader.render(cachedItem, tier: .thumbnail) != nil {
                delivered = true
                break
            }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertTrue(delivered,
                      "An offline Smart Preview thumbnail must become available within 60 seconds")
    }

    func testActualSwiftBridgeOfflineLibraryRenderSaveReopenAndReconnect() async throws {
        let fixture = try sonyFixture()
        let sourceHash = try sha256(fixture)
        XCTAssertEqual(sourceHash, "bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8")
        defer { XCTAssertEqual(try? sha256(fixture), sourceHash, "Source fixture must remain byte-identical") }
        let fm = FileManager.default
        let scratch = fm.temporaryDirectory.appendingPathComponent("smart-preview-native-\(UUID().uuidString)")
        let photos = scratch.appendingPathComponent("photos", isDirectory: true)
        let hidden = scratch.appendingPathComponent("disconnected-copy", isDirectory: true)
        let support = scratch.appendingPathComponent("support", isDirectory: true)
        try fm.createDirectory(at: photos, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: scratch) }
        let filename = fixture.lastPathComponent
        try fm.copyItem(at: fixture, to: photos.appendingPathComponent(filename))
        let prepared = try await prepareOnline(folder: photos, support: support)
        let baselineFiles = try files(photos)
        try fm.moveItem(at: photos, to: hidden)

        // Production router, new native Engine, actual cached factory; no injected closure.
        let cached = try EngineLibrary.open(folder: prepared.folder, appSupport: support)
        XCTAssertEqual(cached.accessMode, .cachedSmartPreviews)
        XCTAssertTrue(cached.isReadOnly)
        XCTAssertEqual(cached.imageIDs, [prepared.imageID])
        XCTAssertTrue(cached.subfolders.isEmpty)
        let item = try XCTUnwrap(cached.items.only)
        let cull = cached.makeCullController()
        XCTAssertTrue(cull.statuses[item.id].isCachedDeclaration)
        XCTAssertThrowsError(try cull.apply(.grade(1), to: [item.id]))
        XCTAssertThrowsError(try cached.session.gradeImages(imageIds: [prepared.imageID], grade: 1))
        let delta = try cached.session.syncChanges()
        _ = cached.apply(delta)
        XCTAssertEqual(cached.imageIDs, [prepared.imageID])
        try await editOffline(cached, imageID: prepared.imageID)
        XCTAssertFalse(fm.fileExists(atPath: photos.path), "Proxy save must not recreate originals")
        XCTAssertEqual(try files(hidden), baselineFiles, "Offline edits must not alter hidden source/sidecars")

        let restarted = try EngineLibrary.open(folder: prepared.folder, appSupport: support)
        XCTAssertTrue(restarted.isReadOnly)
        XCTAssertEqual(restarted.imageIDs, [prepared.imageID])
        let live = SmartPreviewAPI.live(engine: restarted.engine)
        let pending = try await live.info(prepared.imageID)
        XCTAssertTrue(pending.dirty)
        XCTAssertFalse(pending.originalAvailable)
        let again = try XCTUnwrap(restarted.items.only)
        let restored = try await DevelopController.open(try XCTUnwrap(again.engineImage), itemID: again.id, source: .smartPreview)
        do {
            XCTAssertEqual(restored.value(.exposure), 1.25, accuracy: 0.0001)
            XCTAssertEqual(restored.value(.temperature), 4200, accuracy: 0.1)
            try restored.attachSurfaces(viewWidth: 640, viewHeight: 480, count: 2)
            _ = try await nextFrame(restored, after: 0)
            try (await restored.close()).get()
        } catch {
            _ = await restored.close()
            throw error
        }
        XCTAssertFalse(fm.fileExists(atPath: photos.path))
        XCTAssertEqual(try files(hidden), baselineFiles)
        try fm.moveItem(at: hidden, to: photos)
        let synced = try await live.synchronize(prepared.imageID)
        XCTAssertFalse(synced.dirty)
        XCTAssertTrue(synced.originalAvailable)
        XCTAssertEqual(synced.state, .ready)
        let online = try EngineLibrary.open(folder: prepared.folder, appSupport: support)
        XCTAssertFalse(online.isReadOnly)
        XCTAssertEqual(online.imageIDs, [prepared.imageID])
        let originalItem = try XCTUnwrap(online.items.only)
        let original = try await DevelopController.open(try XCTUnwrap(originalItem.engineImage), itemID: originalItem.id)
        do {
            XCTAssertEqual(original.sourceRoute, .original)
            XCTAssertEqual(original.value(.exposure), 1.25, accuracy: 0.0001)
            XCTAssertEqual(original.value(.temperature), 4200, accuracy: 0.1)
            try (await original.close()).get()
        } catch {
            _ = await original.close()
            throw error
        }
        XCTAssertEqual(try sha256(photos.appendingPathComponent(filename)), sourceHash)
    }
}

private extension Array {
    var only: Element? { count == 1 ? first : nil }
}
