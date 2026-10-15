import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// CS-1: library.json lives in `.tessera/` at the root of the opened folder. A legacy
/// `<folder>/library.json` is read once and moved; EngineLibrary and LibraryCatalog agree on
/// the location, and the drive state written by one app dir is seen by another.
final class LibraryDocumentLocationTests: XCTestCase {
    private func scratch() throws -> URL {
        let temp = FileManager.default.temporaryDirectory.appendingPathComponent("library-doc-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        // The engine reports canonical paths (`/private/var/...`): compare on that form.
        return URL(fileURLWithPath: temp.resolvingSymlinksInPath().path, isDirectory: true)
    }

    private func jpeg(_ url: URL, shade: UInt8) throws {
        let w = 32, h = 24
        var pixels = [UInt8](repeating: shade, count: w * h * 4)
        for i in stride(from: 0, to: pixels.count, by: 4) { pixels[i + 1] = UInt8(truncatingIfNeeded: i / 4) }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                          space: CGColorSpaceCreateDeviceRGB(),
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                          provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
    }

    private func shoot(in temp: URL) throws -> URL {
        let folder = temp.appendingPathComponent("shoot", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try jpeg(folder.appendingPathComponent("a.jpg"), shade: 40)
        try jpeg(folder.appendingPathComponent("b.jpg"), shade: 200)
        return folder
    }

    func testLegacyLibraryDocumentMovesIntoTesseraFolderOnOpen() throws {
        let temp = try scratch()
        let folder = try shoot(in: temp)
        // A document written by a pre-CS-1 build, with an album.
        let legacy = folder.appendingPathComponent("library.json")
        try Data("""
        {"schema_version":1,"albums":{"Picks":{"id":1,"name":"Picks","parent":null,"images":[]}}}
        """.utf8).write(to: legacy)
        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let catalog = try LibraryCatalog(library: library)
        // The engine reports the canonical folder (`/private/var/...` for a temp dir).
        let canonical = try XCTUnwrap(library.folder)
        let moved = LibraryCatalog.libraryDocument(in: canonical)
        XCTAssertEqual(try library.session.libraryPath(), moved.path)
        XCTAssertTrue(FileManager.default.fileExists(atPath: moved.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: legacy.path), "read once and moved")
        XCTAssertEqual(catalog.folder, canonical.path)
        XCTAssertEqual(try catalog.nodes().map(\.name), ["Picks"])
        XCTAssertEqual(LibraryCatalog.folder(ofLibraryDocument: moved).path, canonical.path)
        XCTAssertEqual(LibraryCatalog.folder(ofLibraryDocument: legacy).path, folder.path)
        // Edits land in the moved document and the cull session (basket) sees them.
        let picks = try XCTUnwrap(try catalog.nodes().first?.id)
        try catalog.addToAlbum(picks, items: [0])
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: moved)) as? [String: Any]
        let albums = json?["albums"] as? [String: Any]
        let album = albums?["Picks"] as? [String: Any]
        XCTAssertEqual((album?["images"] as? [String])?.count, 1)
        XCTAssertEqual(library.makeCullController().members(ofAlbum: "Picks"), [0])
    }

    func testDriveStateWrittenByOneAppDirOpensFromAnother() throws {
        let temp = try scratch()
        let folder = try shoot(in: temp)
        let first = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("one"))
        let catalog = try LibraryCatalog(library: first)
        let album = try catalog.store.createAlbum(name: "Wall", parent: nil)
        try catalog.addToAlbum(album, items: [1])
        let cull = first.makeCullController()
        try cull.apply(.keep, to: [1])
        try first.engine.setScore(imageId: first.imageIDs[1], signal: "quality", value: 0.75, model: "test")
        XCTAssertGreaterThan(try first.engine.flushDriveState(), 0)
        XCTAssertEqual(try first.engine.flushDriveState(), 0, "nothing changed since")
        let tessera = folder.appendingPathComponent(".tessera")
        XCTAssertTrue(FileManager.default.fileExists(atPath: tessera.appendingPathComponent("library.json").path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: tessera.appendingPathComponent("analysis").path))
        try first.session.shutdown()

        let second = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("two"))
        XCTAssertEqual(second.imageIDs, first.imageIDs, "ids follow the files, not the app dir")
        let reopened = try LibraryCatalog(library: second)
        XCTAssertEqual(try reopened.nodes().map(\.name), ["Wall"])
        XCTAssertEqual(try reopened.albumMembers(album), [1])
        XCTAssertEqual(second.makeCullController()[1].decision, .keep)
        let scores = try second.engine.changesSince(sequence: 0)
        XCTAssertFalse(scores.reset)
        XCTAssertEqual(try second.engine.flushDriveState(), 0, "imported records are not dirty")
        try second.session.shutdown()
    }
}
