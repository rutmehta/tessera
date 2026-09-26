import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore
@testable import Tessera

/// M2-28: catalog changes (new frames, imports, rescans, deletes) reach the open library in
/// place: selection, undo history and active filters survive, and views update without a reload.
final class IncrementalLibraryTests: XCTestCase {
    private func scratch() throws -> URL {
        let temp = FileManager.default.temporaryDirectory.appendingPathComponent("incremental-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        return temp
    }

    /// Small JPEGs (no capture time), each a different picture.
    private func photos(_ names: [String], in folder: URL) throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for name in names {
            try jpeg(folder.appendingPathComponent(name), shade: UInt8(truncatingIfNeeded: name.hashValue))
        }
    }

    private func jpeg(_ url: URL, shade: UInt8) throws {
        let w = 64, h = 48
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        for y in 0..<h { for x in 0..<w {
            let i = (y * w + x) * 4
            pixels[i] = shade &+ UInt8(x * 3); pixels[i + 1] = UInt8(y * 4) &+ shade; pixels[i + 2] = UInt8((x ^ y) & 0xFF)
            pixels[i + 3] = 255
        } }
        let ctx = try XCTUnwrap(CGContext(data: &pixels, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let image = try XCTUnwrap(ctx.makeImage())
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
    }

    @MainActor private func spin(timeout: TimeInterval = 30, until condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition(), Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.02)) }
        return condition()
    }

    /// Pulls the catalog into the model and waits for it to land.
    @MainActor private func sync(_ app: AppModel) {
        var done = false
        app.syncLibrary { done = true }
        XCTAssertTrue(spin { done }, "library pull finished")
    }

    private func sync(_ lib: EngineLibrary, _ cull: CullController) throws -> LibraryUpdate {
        let update = try XCTUnwrap(lib.apply(try lib.session.syncChanges()))
        cull.libraryDidUpdate(update)
        return update
    }

    private func key(_ lib: EngineLibrary, _ name: String) throws -> String {
        try XCTUnwrap(lib.items.first { $0.name == name }.map { lib.imageIDs[$0.id] })
    }

    // MARK: TesseraCore: the library and the cull controller

    func testLibraryInsertsUpdatesAndRemovesInPlaceKeepingUndo() throws {
        let temp = try scratch()
        let folder = temp.appendingPathComponent("shoot")
        try photos(["b.jpg", "c.jpg", "d.jpg"], in: folder)
        let lib = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let cull = lib.makeCullController()
        let c = try key(lib, "c.jpg")
        try cull.apply(.keep, to: [try XCTUnwrap(lib.itemOfImage[c])])
        XCTAssertTrue(cull.canUndo)

        // Insert: two new files join at their queue positions; ids are remapped, state follows.
        try photos(["a.jpg", "e.jpg"], in: folder)
        _ = try lib.engine.indexFolder(path: folder.path)
        let inserted = try sync(lib, cull)
        XCTAssertEqual(inserted.inserted.count, 2)
        XCTAssertEqual(lib.items.count, 5)
        XCTAssertEqual(lib.items.map(\.id), Array(lib.items.indices), "ids stay dense display positions")
        XCTAssertEqual(lib.imageIDs, try lib.session.groups().flatMap(\.images), "display order is group order")
        for item in lib.items { XCTAssertTrue(lib.groups[item.groupID].contains(item.id)) }
        let cItem = try XCTUnwrap(lib.itemOfImage[c])
        XCTAssertEqual(cull[cItem].decision, .keep)
        XCTAssertEqual(cull.counts.keep, 1)
        XCTAssertEqual(cull.counts.undecided, 4)
        XCTAssertTrue(cull.canUndo, "the session and its history are the same")

        // Update: a decision written outside the session (another window, the agent) shows up.
        let b = try key(lib, "b.jpg")
        try lib.engine.setSelection(imageId: b, selection: Selection(decision: .reject, grade: nil, mark: nil))
        let updated = try sync(lib, cull)
        XCTAssertTrue(updated.inserted.isEmpty && updated.removed.isEmpty)
        let bItem = try XCTUnwrap(lib.itemOfImage[b])
        XCTAssertTrue(updated.updated.contains { $0.id == bItem && $0.fields.selection })
        XCTAssertEqual(cull[bItem].decision, .reject)

        // Remove: a file deleted from disk leaves; undo still addresses c.
        let d = try key(lib, "d.jpg")
        try FileManager.default.removeItem(at: folder.appendingPathComponent("d.jpg"))
        XCTAssertEqual(try lib.engine.forgetMissing(imageIds: [d, c]), 1, "only the missing file")
        let removed = try sync(lib, cull)
        XCTAssertEqual(removed.removed, [d])
        XCTAssertEqual(lib.items.count, 4)
        XCTAssertNil(lib.itemOfImage[d])
        let undone = try XCTUnwrap(cull.undo())
        XCTAssertEqual(undone.ids, [try XCTUnwrap(lib.itemOfImage[c])])
        XCTAssertEqual(cull[try XCTUnwrap(lib.itemOfImage[c])].decision, .undecided)
        XCTAssertTrue(try lib.session.syncChanges().added.isEmpty, "nothing pending")
    }

    // MARK: The app model and the grid

    private final class Counter: LibraryObserver {
        var reloads = 0
        var updates = 0
        func libraryDidReload() { reloads += 1 }
        func libraryDidUpdate(_ change: VisibleChange) { updates += 1 }
        func itemsDidChange(_ positions: IndexSet) {}
        func selectionDidChange(scrollToFocus: Bool) {}
    }

    @MainActor func testAppModelKeepsSelectionFilterAndGridAcrossUpdates() throws {
        _ = NSApplication.shared
        let temp = try scratch()
        let folder = temp.appendingPathComponent("shoot")
        try photos(["b.jpg", "d.jpg", "f.jpg", "h.jpg"], in: folder)
        let lib = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let app = AppModel()
        app.install(lib)
        sync(app)
        let counter = Counter()
        app.addObserver(counter)
        let grid = BrowserController(model: app, style: .grid)
        grid.scrollView.frame = NSRect(x: 0, y: 0, width: 900, height: 700)
        grid.libraryDidReload()

        // A decision (undo step), then a filter that the decided photo still passes.
        app.select(position: 0)
        app.perform(.reject)
        XCTAssertTrue(app.canUndo)
        var filter = LibraryFilter()
        filter.decisions = ["undecided", "reject"]
        let chosen = counter.reloads
        app.collections.filter = filter
        XCTAssertTrue(spin { counter.reloads > chosen }, "filter applied (when chosen: one reload)")
        XCTAssertEqual(app.visibleCount, 4)
        app.setSelectionFromUI(IndexSet([1, 3]), clicked: 3)
        let selected = Set(app.selection.map { lib.imageIDs[app.visibleIDs[$0]] })
        let focused = app.focusedItem?.engineImage?.imageID
        let reloads = counter.reloads

        // New files arrive (an import or a rescan): inserted in place, nothing else moves.
        try photos(["a.jpg", "c.jpg", "e.jpg"], in: folder)
        _ = try lib.engine.indexFolder(path: folder.path)
        sync(app)
        XCTAssertEqual(lib.items.count, 7)
        XCTAssertEqual(app.visibleCount, 7, "new undecided photos pass the active filter")
        XCTAssertEqual(Set(app.selection.map { lib.imageIDs[app.visibleIDs[$0]] }), selected)
        XCTAssertEqual(app.focusedItem?.engineImage?.imageID, focused)
        XCTAssertEqual(app.collections.filter, filter)
        XCTAssertEqual(counter.reloads, reloads, "no reload")
        XCTAssertEqual(counter.updates, 1)
        XCTAssertEqual(grid.collectionView.numberOfItems(inSection: 0), app.visibleCount)
        XCTAssertTrue(app.canUndo)

        // A photo kept elsewhere stays visible: filters apply when chosen, not live.
        let e = try key(lib, "e.jpg")
        try lib.engine.setSelection(imageId: e, selection: Selection(decision: .keep, grade: nil, mark: nil))
        sync(app)
        XCTAssertEqual(app.visibleCount, 7)
        XCTAssertEqual(app.cull[try XCTUnwrap(lib.itemOfImage[e])].decision, .keep)
        XCTAssertEqual(app.counts.keep, 1)

        // A deleted file leaves the grid; the selection keeps the rest.
        let doomed = try XCTUnwrap(app.selection.map { app.visibleIDs[$0] }.first)
        let doomedKey = lib.imageIDs[doomed]
        try FileManager.default.removeItem(at: try XCTUnwrap(lib.items[doomed].url))
        _ = try lib.engine.forgetMissing(imageIds: [doomedKey])
        sync(app)
        XCTAssertEqual(app.visibleCount, 6)
        XCTAssertEqual(Set(app.selection.map { lib.imageIDs[app.visibleIDs[$0]] }), selected.subtracting([doomedKey]))
        XCTAssertEqual(grid.collectionView.numberOfItems(inSection: 0), 6)
        XCTAssertEqual(counter.reloads, reloads)

        // The reject from before every update undoes on the same photo.
        app.undo()
        XCTAssertEqual(app.counts.reject, 0)
        XCTAssertFalse(app.canUndo)
    }

    // MARK: Tethered capture

    /// The test camera shoots 20 frames into the open library while a filter is active and an
    /// undo step is pending: every frame joins in place; the history and the filter survive.
    @MainActor func testTwentyTetheredFramesKeepUndoAndFilters() throws {
        _ = NSApplication.shared
        let temp = try scratch()
        let folder = temp.appendingPathComponent("studio")
        try photos(["existing1.jpg", "existing2.jpg"], in: folder)
        let card = temp.appendingPathComponent("card")
        try FileManager.default.createDirectory(at: card, withIntermediateDirectories: true)
        for n in 1...20 { try jpeg(card.appendingPathComponent(String(format: "SAMPLE_%04d.jpg", n)), shade: UInt8(n * 11)) }
        let lib = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let app = AppModel()
        app.install(lib)
        sync(app)
        let counter = Counter()
        app.addObserver(counter)
        let tether = TetherController(arguments: ["Tessera", "--fake-tether", card.path, "--fake-tether-interval", "0"])
        tether.app = app
        tether.sessionName = "Studio test"
        tether.template = "studio_{sequence}.{ext}"
        tether.insertToken("{original}")
        XCTAssertEqual(tether.template, "studio_{sequence}_{original}.{ext}")
        tether.connect()
        XCTAssertTrue(spin { tether.connected }, tether.error ?? "connected")
        let album = try XCTUnwrap(tether.sessionAlbum)
        XCTAssertEqual(app.source, .album(album))

        // Two frames, a reject on the first (the pending undo step), then a filter.
        tether.capture()
        tether.capture()
        XCTAssertTrue(spin { app.visibleCount == 2 }, "first frames in the album view")
        app.select(position: 0)
        let rejected = try XCTUnwrap(app.focusedItem?.engineImage?.imageID)
        app.autoAdvance = false
        app.perform(.reject)
        XCTAssertTrue(app.canUndo)
        var filter = LibraryFilter()
        filter.decisions = ["undecided", "reject"]
        let chosen = counter.reloads
        app.collections.filter = filter
        XCTAssertTrue(spin { counter.reloads > chosen }, "filter applied (when chosen: one reload)")
        let reloads = counter.reloads

        for _ in 3...20 {
            tether.capture()
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        XCTAssertTrue(spin(timeout: 60) { tether.strip.received == 20 && app.visibleCount == 20 },
                      "received \(tether.strip.received), visible \(app.visibleCount)")
        tether.disconnect()

        XCTAssertEqual(lib.items.count, 22, "every frame joined the open library")
        XCTAssertEqual(counter.reloads, reloads, "frames arrive without reloading the library")
        XCTAssertGreaterThan(counter.updates, 0)
        XCTAssertEqual(app.collections.filter, filter, "the filter bar is untouched")
        XCTAssertEqual(app.source, .album(album))
        XCTAssertTrue(app.canUndo, "the undo stack survives 20 frames")
        let names = app.visibleIDs.map { lib.items[$0].name }
        XCTAssertTrue(names.contains("studio_0001_SAMPLE_0001.jpg"), "{original} applied: \(names.prefix(3))")
        app.undo()
        XCTAssertEqual(app.cull[try XCTUnwrap(lib.itemOfImage[rejected])].decision, .undecided,
                       "undo reaches the frame decided before the others arrived")
    }
}
