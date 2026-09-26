import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

final class BridgeTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    private func scratch() throws -> URL {
        // Never mutate the shared fixture corpus: independent copies, sidecars and catalog.
        let temp = root.appendingPathComponent("build/bridge-test-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        return temp
    }

    @MainActor func testRawFixturesPersistAcrossReopenThroughTheSession() async throws {
        let fixture = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        let temp = try scratch()
        let photos = temp.appendingPathComponent("raw")
        try FileManager.default.copyItem(at: fixture, to: photos)
        let support = temp.appendingPathComponent("support")
        let library = try EngineLibrary.scan(folder: photos, appSupport: support)
        XCTAssertEqual(library.items.count, 5)
        let cull = library.makeCullController()
        XCTAssertTrue(cull.isEngineBacked)
        let item = try XCTUnwrap(library.items.first { $0.name == "sample.dng" })
        try cull.apply(.reject, to: [item.id])
        try cull.apply(.mark(6), to: [item.id])
        let reopened = try EngineLibrary.scan(folder: photos, appSupport: support)
        let found = try XCTUnwrap(reopened.items.first { $0.url == item.url })
        let state = reopened.makeCullController()[found.id]
        XCTAssertEqual(state.decision, .reject)
        XCTAssertEqual(state.mark, 6)

        // M1-14: a cold RAW preview completes through the worker callback.
        let loader = ThumbnailLoader()
        let ref = try XCTUnwrap(found.engineImage)
        let subscription = ref.previewEvents.subscribe(imageID: ref.imageID, maxPx: 384)
        let workerReady = expectation(description: "matching engine/image/tier PreviewReady")
        let listener = Task {
            var iterator = subscription.stream.makeAsyncIterator()
            if await iterator.next() != nil { workerReady.fulfill() }
        }
        defer { subscription.cancel(); listener.cancel() }
        let ready = expectation(description: "cold RAW preview completes from worker callback")
        let request = loader.request(found, tier: .thumbnail) { image in
            XCTAssertGreaterThan(image.width, 0)
            XCTAssertTrue(loader.cached(found, tier: .thumbnail) === image,
                          "the matching item is cached before delivery")
            XCTAssertNil(loader.cached(item, tier: .thumbnail), "the old engine cannot alias the reopened item")
            ready.fulfill()
        }
        defer { request?.cancel() }
        await fulfillment(of: [workerReady, ready], timeout: 30)
        XCTAssertFalse(request?.isCancelled ?? true)
        XCTAssertNotNil(loader.cached(found, tier: .thumbnail))
        let cached = try ref.engine.embeddedPreview(imageId: ref.imageID, maxPx: 384)
        XCTAssertFalse(cached.pending)
        XCTAssertNotNil(cached.bytes)
        XCTAssertNotNil(ThumbnailLoader.render(found, tier: .thumbnail))
    }

    /// Three near-duplicate frames (A), one opposite frame (B) and a two-frame pair (C).
    private func makeGroupedFolder() throws -> (folder: URL, support: URL) {
        let temp = try scratch()
        let folder = temp.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try writeJPEG(folder.appendingPathComponent("a1.jpg"), pattern: .falling, noise: 0)
        try writeJPEG(folder.appendingPathComponent("a2.jpg"), pattern: .falling, noise: 24)   // largest: best
        try writeJPEG(folder.appendingPathComponent("a3.jpg"), pattern: .falling, noise: 6)
        try writeJPEG(folder.appendingPathComponent("b1.jpg"), pattern: .rising, noise: 0)
        try writeJPEG(folder.appendingPathComponent("c1.jpg"), pattern: .tent, noise: 0)
        try writeJPEG(folder.appendingPathComponent("c2.jpg"), pattern: .tent, noise: 12)
        return (folder, temp.appendingPathComponent("support"))
    }

    func testGroupNavigationComesFromTheSession() throws {
        let (folder, support) = try makeGroupedFolder()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let cull = library.makeCullController()
        XCTAssertEqual(library.items.count, 6)
        XCTAssertEqual(library.groups.map(\.count).sorted(), [1, 2, 3])
        // Display order keeps every group contiguous, in the session's group order.
        for (g, range) in library.groups.enumerated() {
            XCTAssertTrue(range.allSatisfy { library.items[$0].groupID == g })
        }
        let names = library.groups.map { r in Set(r.map { library.items[$0].name.prefix(1) }) }
        XCTAssertTrue(names.allSatisfy { $0.count == 1 }, "groups are a*, b*, c*: \(names)")
        let a = try XCTUnwrap(library.groups.firstIndex { $0.count == 3 })
        XCTAssertEqual(library.items[cull.bestOfGroup[a]].name, "a2.jpg")
        XCTAssertTrue(cull.isSuggestedBest(cull.bestOfGroup[a]))
        let lone = try XCTUnwrap(library.groups.firstIndex { $0.count == 1 })
        XCTAssertFalse(cull.isSuggestedBest(library.groups[lone].lowerBound), "no suggestion for singletons")

        let first = library.groups[0].lowerBound
        let last = library.groups[2]
        XCTAssertNil(try cull.navigate(.previousGroup, from: first))
        XCTAssertNil(try cull.navigate(.previousInGroup, from: first))
        XCTAssertEqual(try cull.navigate(.nextGroup, from: first), library.groups[1].lowerBound)
        XCTAssertEqual(try cull.navigate(.nextGroup, from: library.groups[1].lowerBound), last.lowerBound)
        XCTAssertNil(try cull.navigate(.nextGroup, from: last.lowerBound))
        XCTAssertEqual(try cull.navigate(.previousGroup, from: last.upperBound - 1), library.groups[1].lowerBound)
        let aRange = library.groups[a]
        XCTAssertEqual(try cull.navigate(.nextInGroup, from: aRange.lowerBound), aRange.lowerBound + 1)
        XCTAssertEqual(try cull.navigate(.previousInGroup, from: aRange.lowerBound + 1), aRange.lowerBound)
        XCTAssertNil(try cull.navigate(.nextInGroup, from: aRange.upperBound - 1))

        // The in-memory stub follows the same semantics.
        let stub = StubLibrary.synthetic(count: 40)
        let memory = stub.makeCullController()
        XCTAssertFalse(memory.isEngineBacked)
        XCTAssertNil(try memory.navigate(.previousGroup, from: 0))
        XCTAssertEqual(try memory.navigate(.nextGroup, from: 0), stub.groups[1].lowerBound)
        XCTAssertEqual(try memory.navigate(.previousGroup, from: stub.groups[2].upperBound - 1), stub.groups[1].lowerBound)
    }

    func testKeepBestRejectRestIsOneUndoableStepThatPersists() throws {
        let (folder, support) = try makeGroupedFolder()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let cull = library.makeCullController()
        let a = try XCTUnwrap(library.groups.firstIndex { $0.count == 3 })
        XCTAssertFalse(cull.canUndo)
        let (best, change) = try cull.keepBestRejectRest(group: a)
        XCTAssertEqual(library.items[best].name, "a2.jpg")
        XCTAssertEqual(Set(change.ids), Set(library.groups[a]))
        for id in library.groups[a] {
            XCTAssertEqual(cull[id].decision, id == best ? .keep : .reject)
        }
        XCTAssertEqual(cull.counts.keep, 1)
        XCTAssertEqual(cull.counts.reject, 2)
        XCTAssertTrue(cull.canUndo)

        let undone = try XCTUnwrap(try cull.undo())
        XCTAssertEqual(Set(undone.ids), Set(library.groups[a]))
        XCTAssertTrue(library.groups[a].allSatisfy { cull[$0].decision == .undecided })
        XCTAssertFalse(cull.canUndo)
        XCTAssertNil(try cull.undo())
        XCTAssertNotNil(try cull.redo())
        XCTAssertEqual(cull.counts.reject, 2)

        // "Choose this" in compare: keep one, reject the other, one step.
        let c = try XCTUnwrap(library.groups.firstIndex { $0.count == 2 })
        let pair = Array(library.groups[c])
        try cull.decide([(pair[1], .keep), (pair[0], .reject)])
        XCTAssertEqual(cull[pair[1]].decision, .keep)
        XCTAssertEqual(cull[pair[0]].decision, .reject)
        _ = try cull.undo()
        XCTAssertEqual(cull[pair[1]].decision, .undecided)
        XCTAssertEqual(cull[pair[0]].decision, .undecided)
        XCTAssertEqual(cull.counts.reject, 2, "the keep-best step is untouched")

        // Decisions are in the sidecars: a fresh session sees them without history.
        let reopened = try EngineLibrary.scan(folder: folder, appSupport: support).makeCullController()
        XCTAssertEqual(reopened.counts.keep, 1)
        XCTAssertEqual(reopened.counts.reject, 2)
        XCTAssertFalse(reopened.canUndo)
    }

    func testBasketAlbumsSafeDeleteAndDefectSweep() throws {
        let (folder, support) = try makeGroupedFolder()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support, basketTarget: "Portfolio")
        let cull = library.makeCullController()
        XCTAssertEqual(cull.basketTarget, "Portfolio")
        XCTAssertEqual(cull.albums.map(\.name), ["Portfolio"], "target shown before it exists")
        try cull.apply(.toggleBasket, to: [0, 1])
        XCTAssertEqual(cull.counts.basket, 2)
        XCTAssertEqual(cull.members(ofAlbum: "Portfolio"), [0, 1])
        XCTAssertEqual(cull.statuses[0].albums, ["Portfolio"])
        XCTAssertEqual(cull.statuses[0].phase, .unedited)

        // Safe delete in an album: membership only.
        try cull.removeFromAlbum("Portfolio", ids: [0])
        XCTAssertFalse(cull[0].inBasket)
        XCTAssertTrue(FileManager.default.fileExists(atPath: library.items[0].url!.path))
        XCTAssertEqual(cull.members(ofAlbum: "Portfolio"), [1])
        _ = try cull.undo()
        XCTAssertTrue(cull[0].inBasket)

        try cull.setBasketTarget("Print")
        XCTAssertEqual(cull.counts.basket, 0, "membership follows the new target")
        try cull.apply(.toggleBasket, to: [2])
        XCTAssertEqual(Set(cull.albums.map(\.name)), ["Portfolio", "Print"])
        XCTAssertEqual(Set(cull.statuses[2].albums), ["Print"])

        // Real scores (M3-11): smooth, grain-free gradients measure as soft; grainy frames do not.
        XCTAssertEqual(try cull.defectSweep(DefectRule.defaults), [], "nothing measured yet")
        let pass = library.analyze(Array(library.items.indices), faces: false)
        XCTAssertEqual(pass.analyzed, library.items.count)
        XCTAssertEqual(pass.errors, [])
        XCTAssertEqual(library.analyze(Array(library.items.indices), faces: false).analyzed, 0, "already analysed")
        // Every frame's measured sharpness (a threshold above any value lists them all).
        var everything = DefectRule.focus
        everything.threshold = 1.01
        let sharpness = Dictionary(uniqueKeysWithValues: try cull.defectSweep([everything]).map { finding in
            (library.items[finding.item].name, Double(finding.reasons[0].split(separator: " ")[2])!)
        })
        XCTAssertEqual(sharpness.count, 6)
        XCTAssertLessThan(sharpness["a1.jpg"]!, sharpness["a2.jpg"]!, "grain measures sharper than a smooth gradient")
        let found = try cull.defectSweep(DefectRule.defaults)
        let names = Set(found.map { library.items[$0.item].name })
        XCTAssertTrue(names.isSuperset(of: ["a1.jpg", "b1.jpg", "c1.jpg"]), "\(names) \(sharpness)")
        XCTAssertTrue(found[0].reasons[0].hasPrefix("Missed focus"))
        XCTAssertTrue(found.allSatisfy { cull[$0.item].decision == .undecided }, "the sweep is review-only")
        var highlightsOnly = DefectRule.defaults
        for i in highlightsOnly.indices where highlightsOnly[i].signal != "highlight_clipping" { highlightsOnly[i].enabled = false }
        XCTAssertEqual(try cull.defectSweep(highlightsOnly), [], "no clipped frames")
        try cull.apply(.reject, to: found.map(\.item))
        XCTAssertEqual(cull.counts.reject, found.count)
        _ = try cull.undo()
        XCTAssertEqual(cull.counts.reject, 0)
        XCTAssertEqual(try StubLibrary.synthetic(count: 3).makeCullController().defectSweep(DefectRule.defaults), [])
    }

    func testDeleteFromDiskTrashesFileAndLeavesTheQueue() throws {
        let (folder, support) = try makeGroupedFolder()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let cull = library.makeCullController()
        let victim = try XCTUnwrap(library.items.first { $0.name == "b1.jpg" })
        try cull.apply(.toggleBasket, to: [victim.id])
        try cull.apply(.reject, to: [victim.id])
        let bin = folder.deletingLastPathComponent().appendingPathComponent("trash")
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        var binned: [String] = []
        let trashed = try cull.moveToTrash([victim.id]) { url in
            binned.append(url.lastPathComponent)
            try FileManager.default.moveItem(at: url, to: bin.appendingPathComponent(url.lastPathComponent))
        }
        XCTAssertEqual(binned, ["b1.jpg", "b1.jpg.xmp", "b1.json"], "file, XMP and recipe sidecar")
        XCTAssertEqual(trashed, [victim.url!])
        XCTAssertFalse(FileManager.default.fileExists(atPath: victim.url!.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: victim.url!.path + ".xmp"))
        XCTAssertTrue(cull.members(ofAlbum: cull.basketTarget).isEmpty, "removed from albums first")
        let reopened = try EngineLibrary.scan(folder: folder, appSupport: support)
        XCTAssertEqual(reopened.items.count, 5)
        XCTAssertFalse(reopened.items.contains { $0.name == "b1.jpg" })
    }

    func testRestoreDoesNotCreateUndoHistory() {
        let store = CullStore(states: [CullState(decision: .keep, grade: 2), CullState(decision: .reject)])
        XCTAssertEqual(store.counts.keep, 1)
        XCTAssertEqual(store.counts.reject, 1)
        XCTAssertFalse(store.canUndo)
    }

    // MARK: Fixtures

    private enum Pattern { case falling, rising, tent }

    /// dHash-distinct luminance patterns; `noise` adds deterministic grain (larger file).
    private func writeJPEG(_ url: URL, pattern: Pattern, noise: Int) throws {
        let w = 240, h = 160
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        var seed: UInt32 = 12345
        for y in 0..<h {
            for x in 0..<w {
                let base: Int = switch pattern {
                case .falling: 230 - x * 200 / w
                case .rising: 30 + x * 200 / w
                case .tent: x < w / 2 ? 30 + x * 360 / w : 390 - x * 360 / w
                }
                seed = seed &* 1_664_525 &+ 1_013_904_223
                let n = noise == 0 ? 0 : Int(seed >> 24) % (noise * 2 + 1) - noise
                let v = UInt8(clamping: base + n)
                let i = (y * w + x) * 4
                pixels[i] = v; pixels[i + 1] = v; pixels[i + 2] = UInt8(clamping: Int(v) / 2 + y / 2); pixels[i + 3] = 255
            }
        }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                          provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.9] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
    }
}

