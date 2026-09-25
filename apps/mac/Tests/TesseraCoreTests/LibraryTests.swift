import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Library bridge (M2-12): rule diagnostics mapped to Swift string ranges, the rule tree
/// round trip, album ordering and the filter bar, all on scratch folders.
final class LibraryTests: XCTestCase {
    func testSupportDirectoryResolution() {
        let env = ["TESSERA_APP_DIR": "/tmp/tessera-test-env"]
        XCTAssertEqual(EngineLibrary.supportDirectory(arguments: ["Tessera"], environment: env).path,
                       "/tmp/tessera-test-env")
        XCTAssertEqual(EngineLibrary.supportDirectory(arguments: ["Tessera", "--app-dir", "/tmp/tessera-test-arg"],
                                                     environment: env).path, "/tmp/tessera-test-arg")
        XCTAssertTrue(EngineLibrary.supportDirectory(arguments: ["Tessera"], environment: [:]).path
            .hasSuffix("/Library/Application Support/Tessera"))
    }

    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    /// Four distinct JPEGs in a scratch folder, indexed through the engine.
    private func openLibrary() throws -> (EngineLibrary, LibraryCatalog) {
        let temp = root.appendingPathComponent("build/library-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        for (n, name) in ["a", "b", "c", "d"].enumerated() {
            try writeJPEG(folder.appendingPathComponent("\(name).jpg"), shade: 40 + n * 50)
        }
        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        return (library, try LibraryCatalog(library: library))
    }

    func testRuleDiagnosticsMapToSwiftRangesThroughTheBridge() throws {
        let (_, catalog) = try openLibrary()
        // A bad field after non-ASCII text: the engine reports UTF-8 bytes; Swift gets the rule.
        let text = "café 海 wat:x"
        let check = try catalog.store.checkRule(text: text)
        let d = try XCTUnwrap(check.diagnostic)
        XCTAssertEqual(String(text[d.range(in: text)]), "wat:x")
        XCTAssertTrue(d.message.hasPrefix("Unsupported field"), d.message)
        XCTAssertEqual((text as NSString).substring(with: d.nsRange(in: text)), "wat:x")

        // "expected …" at the end is an empty range at the end of the text.
        let open = "(beach OR sand"
        let e = try XCTUnwrap(try catalog.store.checkRule(text: open).diagnostic)
        XCTAssertTrue(e.range(in: open).isEmpty)
        XCTAssertEqual(e.range(in: open).lowerBound, open.endIndex)

        // Unknown album names are compile errors over the whole text.
        let album = try XCTUnwrap(try catalog.store.checkRule(text: "album:Nope").diagnostic)
        XCTAssertTrue(album.message.contains("Nope"))

        // Valid text: a tree whose items re-render to the same rule.
        let good = try catalog.store.checkRule(text: "rating>=2 AND (keyword:beach OR NOT mark:Review)")
        XCTAssertNil(good.diagnostic)
        let tree = try XCTUnwrap(RuleNode.from(good.items))
        XCTAssertEqual(tree.kind, .all)
        XCTAssertEqual(tree.children.map(\.kind), [.rule, .any])
        XCTAssertEqual(tree.children[1].children.map(\.kind), [.rule, .not])
        XCTAssertEqual(try catalog.store.formatRule(items: tree.items).text, "rating>=2 AND (keyword:beach OR NOT mark:Review)")
        // An editor leaf error is located in the rendered text.
        var bad = tree
        bad.children[0].value = "9"
        let rendered = try catalog.store.formatRule(items: bad.items)
        let r = try XCTUnwrap(rendered.diagnostic)
        XCTAssertEqual(String(rendered.text[r.range(in: rendered.text)]), "rating>=9")
        // A single condition becomes "all of: it" so the editor always has a root group.
        let single = try XCTUnwrap(RuleNode.from(try catalog.store.checkRule(text: "beach").items))
        XCTAssertEqual(single.kind, .all)
        XCTAssertEqual(single.children.first?.field, "text")
    }

    func testAlbumOrderingAndSidebarNestingPersist() throws {
        let (library, catalog) = try openLibrary()
        let store = catalog.store
        let ids = library.items.map(\.id)
        let trip = try store.createGroup(name: "Trip", parent: nil)
        let album = try store.createAlbum(name: "Sequence", parent: nil)
        try catalog.addToAlbum(album, items: [ids[3], ids[1], ids[0]])
        XCTAssertEqual(try catalog.albumMembers(album), [ids[3], ids[1], ids[0]])
        // Drag ids[0] before ids[3] (grid reorder helper), then persist.
        let order = LibraryCatalog.reordered(try catalog.albumMembers(album), moving: [ids[0]], before: ids[3])
        XCTAssertEqual(order, [ids[0], ids[3], ids[1]])
        try catalog.reorderAlbum(album, items: order)
        XCTAssertEqual(LibraryCatalog.reordered(order, moving: [ids[0]], before: nil), [ids[3], ids[1], ids[0]])
        // Nest into the group, reopen: order and nesting survive, and album search keeps album order.
        try store.moveNode(id: album, parent: trip, index: 0)
        let reopened = try LibraryCatalog(library: library)
        let tree = try reopened.nodes()
        XCTAssertEqual(tree.map(\.name), ["Trip"])
        XCTAssertEqual(tree[0].children.map(\.name), ["Sequence"])
        XCTAssertEqual(tree[0].children[0].imageCount, 3)
        XCTAssertEqual(try reopened.albumMembers(album), [ids[0], ids[3], ids[1]])
        XCTAssertEqual(try reopened.search(LibraryFilter(), scope: .album(id: album)).ids, [ids[0], ids[3], ids[1]])
        XCTAssertThrowsError(try store.reorderAlbum(id: album, imageIds: [library.imageIDs[0]]))
        // The cull controller sees the same album through library.json.
        let cull = library.makeCullController()
        XCTAssertEqual(cull.members(ofAlbum: "Sequence"), [ids[0], ids[3], ids[1]])
        // Safe delete of the group keeps the album and its members.
        try store.deleteNode(id: trip)
        XCTAssertEqual(try reopened.nodes().map(\.name), ["Sequence"])
        XCTAssertEqual(try reopened.albumMembers(album).count, 3)
    }

    func testFilterBarFacetsAndNotInAnyAlbum() throws {
        let (library, catalog) = try openLibrary()
        let cull = library.makeCullController()
        let ids = library.items.map(\.id)
        try cull.apply(.keep, to: [ids[0], ids[1]])
        try cull.apply(.reject, to: [ids[2]])
        let album = try catalog.store.createAlbum(name: "Picks", parent: nil)
        try catalog.addToAlbum(album, items: [ids[0]])

        var filter = LibraryFilter()
        XCTAssertTrue(filter.isEmpty)
        filter.decisions = ["keep"]
        filter.albumStatus = "none"
        XCTAssertEqual(filter.facetFilters.map(\.field), [.decision, .album])
        let r = try catalog.search(filter, scope: .all)
        XCTAssertEqual(r.ids, [ids[1]])
        XCTAssertEqual(r.rule, "decision:keep AND album:none")
        // The decision facet ignores its own filter; album counts ignore theirs.
        XCTAssertEqual(r.facets.decisions.first { $0.value == "reject" }?.count, 1)
        XCTAssertEqual(r.facets.inNoAlbum, 1)
        XCTAssertEqual(r.facets.inAnyAlbum, 1)

        filter = LibraryFilter()
        filter.dateFrom = "2024"
        XCTAssertEqual(filter.dateValue, "2024..9998")
        filter.dateTo = "2024"
        XCTAssertEqual(filter.dateValue, "2024")
        filter.text = "rating>="
        let bad = try catalog.search(filter, scope: .all)
        XCTAssertNotNil(bad.diagnostic)
    }

    private func writeJPEG(_ url: URL, shade: Int) throws {
        let w = 64, h = 48
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        for y in 0..<h {
            for x in 0..<w {
                let i = (y * w + x) * 4
                pixels[i] = UInt8(clamping: shade + x)
                pixels[i + 1] = UInt8(clamping: shade + y)
                pixels[i + 2] = UInt8(clamping: 255 - shade)
                pixels[i + 3] = 255
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
