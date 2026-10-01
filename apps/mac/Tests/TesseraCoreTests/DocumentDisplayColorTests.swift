import AppKit
import CoreGraphics
import IOSurface
import Metal
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-30: the canvas colour-manages non-sRGB documents by TAGGING (no pixel work). The backend keeps
/// writing the document's own encoded samples; the viewport layer, its surfaces and the detail pane carry
/// the document profile, so macOS converts them to the display. Untagged and sRGB documents keep the sRGB
/// path; a profile CoreGraphics cannot use falls back to sRGB with a diagnostic.
@MainActor
final class DocumentDisplayColorTests: XCTestCase {
    private var engine: Engine?
    private var window: NSWindow?

    override func tearDown() async throws {
        window?.close()
        window = nil
    }

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-display-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    /// A 32 × 16 document of `profile` whose layer is filled with the samples (1, 0, 0).
    private func redDocument(profile: String?) throws -> (DocumentController, any DocumentFiltersBackend) {
        let e = try engine ?? Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        engine = e
        let backend = try EngineDocumentEngine.for(e).newDocument(width: 32, height: 16, depth: .u8, profile: profile)
        let layer = try backend.layers()[0].id
        let tools = try XCTUnwrap(backend as? any DocumentToolsBackend)
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 1, g: 0, b: 0)), opacity: 1)
        let doc = try DocumentController(backend: backend)
        doc.selection = [layer]
        return (doc, try XCTUnwrap(backend as? any DocumentFiltersBackend))
    }

    /// The 1:1 detail pane's surface for the layer (neutral filter) and its first pixel.
    private func pane(_ doc: DocumentController, _ f: any DocumentFiltersBackend) throws -> (IOSurfaceRef, FilterDetailSurface, [UInt8]) {
        var neutral = CameraRawDraft()
        neutral.amountPercent = 0
        let d = try f.filterDetail(layer: try XCTUnwrap(doc.primary).id, smartIndex: nil, filterJson: neutral.filterJson,
                                   x: 0, y: 0, width: 16, height: 16)
        let s = try XCTUnwrap(IOSurfaceLookup(d.surfaceId))
        IOSurfaceLock(s, .readOnly, nil)
        defer { IOSurfaceUnlock(s, .readOnly, nil) }
        let p = IOSurfaceGetBaseAddress(s).assumingMemoryBound(to: UInt8.self)
        return (s, d, [p[0], p[1], p[2], p[3]])
    }

    /// A viewport in an off-screen window (never ordered front), attached to `doc`.
    private func viewport(_ doc: DocumentController) -> DocumentViewportView {
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 320, height: 200), styleMask: [.titled],
                         backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        window = w
        let v = DocumentViewportView(frame: w.contentView!.bounds)
        w.contentView!.addSubview(v)
        v.attach(doc)
        addTeardownBlock { @MainActor in v.attach(nil) }
        return v
    }

    private func icc(_ space: CGColorSpace?) -> Data? { space?.copyICCData() as Data? }

    /// The same colour space by what it means: saturated primaries and a grey land on the same extended
    /// sRGB values. (CGImage may swap an ICC space for the system's equivalent named one, so its bytes differ.)
    private func sameColors(_ a: CGColorSpace?, _ b: CGColorSpace) -> Bool {
        guard let a, let target = CGColorSpace(name: CGColorSpace.extendedSRGB) else { return false }
        for c in [[1, 0, 0, 1], [0, 1, 0, 1], [0, 0, 1, 1], [0.5, 0.5, 0.5, 1]] as [[CGFloat]] {
            guard let x = CGColor(colorSpace: a, components: c)?.converted(to: target, intent: .relativeColorimetric, options: nil)?.components,
                  let y = CGColor(colorSpace: b, components: c)?.converted(to: target, intent: .relativeColorimetric, options: nil)?.components,
                  zip(x, y).allSatisfy({ abs($0 - $1) < 2e-3 }) else { return false }
        }
        return true
    }

    func testSRGBDocumentKeepsTheSRGBCanvasAndItsBytes() throws {
        let (doc, f) = try redDocument(profile: nil)   // new documents carry the built-in sRGB profile
        defer { doc.close() }
        XCTAssertNil(try doc.backend.displayProfileICC(), "built-in sRGB: no document tag, the sRGB path")
        XCTAssertTrue(doc.displayColor.isSRGB)
        XCTAssertNil(doc.displayColor.diagnostic)
        let (s, d, px) = try pane(doc, f)
        XCTAssertEqual(px, [255, 0, 0, 255], "the surface holds the document's own samples")
        let image = try XCTUnwrap(FilterSheetModel.image(s, width: Int(d.width), height: Int(d.height), space: doc.displayColor.space))
        XCTAssertEqual(image.colorSpace?.name, CGColorSpace.sRGB)
        let v = viewport(doc)
        XCTAssertEqual(v.layerColorSpace?.name, CGColorSpace.extendedLinearSRGB, "EDR layer unchanged")
        XCTAssertTrue(v.layerWantsEDR)
        XCTAssertEqual(v.surfacePixelFormat, .rgba8Unorm_srgb)
    }

    func testP3DocumentIsTaggedWithItsOwnProfileAndItsBytesAreUnchanged() throws {
        let (srgb, sf) = try redDocument(profile: nil)
        defer { srgb.close() }
        let (doc, f) = try redDocument(profile: "Display P3")
        defer { doc.close() }
        let profile = try XCTUnwrap(try doc.backend.displayProfileICC(), "a P3 document hands over its ICC bytes")
        XCTAssertFalse(doc.displayColor.isSRGB)
        XCTAssertNil(doc.displayColor.diagnostic)
        XCTAssertEqual(doc.displayColor.iccData, profile, "the display space is the document profile")
        // Tagging, not converting: the samples written are the same as an sRGB document's.
        let (s, d, px) = try pane(doc, f)
        XCTAssertEqual(px, try pane(srgb, sf).2)
        XCTAssertEqual(px, [255, 0, 0, 255])
        let image = try XCTUnwrap(FilterSheetModel.image(s, width: Int(d.width), height: Int(d.height), space: doc.displayColor.space))
        XCTAssertEqual(icc(image.colorSpace), profile, "the pane carries the document profile")
        XCTAssertTrue(sameColors(image.colorSpace, doc.displayColor.space))
        XCTAssertFalse(sameColors(image.colorSpace, CGColorSpace(name: CGColorSpace.sRGB)!), "and not sRGB")
        // The canvas: the layer is tagged with the profile, surfaces are sampled without an sRGB decode.
        let v = viewport(doc)
        XCTAssertEqual(icc(v.layerColorSpace), profile)
        XCTAssertEqual(v.surfacePixelFormat, .rgba8Unorm)
        XCTAssertFalse(v.ringSurfaces.isEmpty)
        for surface in v.ringSurfaces {
            XCTAssertEqual(IOSurfaceCopyValue(surface, kIOSurfaceColorSpace) as? Data, profile, "surfaces carry the profile")
        }
        // Back to an sRGB document: the same view returns to the sRGB path.
        v.attach(srgb)
        XCTAssertEqual(v.layerColorSpace?.name, CGColorSpace.extendedLinearSRGB)
        XCTAssertEqual(v.surfacePixelFormat, .rgba8Unorm_srgb)
    }

    func testUnsupportedProfilesFallBackToSRGBWithADiagnostic() throws {
        let none = DocumentDisplayColor.resolve(icc: nil, name: nil)
        XCTAssertTrue(none.isSRGB)
        XCTAssertNil(none.diagnostic, "untagged is sRGB, silently")
        let broken = DocumentDisplayColor.resolve(icc: Data("not an ICC profile".utf8), name: "Broken")
        XCTAssertTrue(broken.isSRGB)
        XCTAssertEqual(broken.space.name, CGColorSpace.sRGB)
        XCTAssertTrue(broken.diagnostic?.contains("Broken") ?? false, "\(String(describing: broken.diagnostic))")
        let grey = try XCTUnwrap(CGColorSpace(name: CGColorSpace.genericGrayGamma2_2)?.copyICCData() as Data?)
        let mono = DocumentDisplayColor.resolve(icc: grey, name: "Gray Gamma 2.2")
        XCTAssertTrue(mono.isSRGB, "a canvas surface is RGB: a non-RGB profile cannot tag it")
        XCTAssertNotNil(mono.diagnostic)
        let p3 = try XCTUnwrap(CGColorSpace(name: CGColorSpace.displayP3)?.copyICCData() as Data?)
        let ok = DocumentDisplayColor.resolve(icc: p3, name: "Display P3")
        XCTAssertFalse(ok.isSRGB)
        XCTAssertEqual(ok.iccData, p3)
    }
}
