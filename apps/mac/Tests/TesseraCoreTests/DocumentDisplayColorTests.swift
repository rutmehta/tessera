import AppKit
import CoreGraphics
import IOSurface
import Metal
import QuartzCore
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
        // The canvas decodes sRGB transfer curves and tags the linearized document primaries.
        let v = viewport(doc)
        XCTAssertEqual(icc(v.layerColorSpace), icc(CGColorSpaceCreateLinearized(doc.displayColor.space)))
        XCTAssertFalse(v.layerWantsEDR)
        let metalLayer = try XCTUnwrap(v.layer as? CAMetalLayer)
        metalLayer.drawableSize = CGSize(width: 2, height: 2)
        XCTAssertNotNil(metalLayer.nextDrawable(), "Metal accepts the linearized P3 layer without EDR")
        XCTAssertEqual(v.surfacePixelFormat, .rgba8Unorm_srgb)
        XCTAssertFalse(v.ringSurfaces.isEmpty)
        for surface in v.ringSurfaces {
            XCTAssertEqual(IOSurfaceCopyValue(surface, kIOSurfaceColorSpace) as? Data, profile, "surfaces carry the profile")
        }
        // Back to an sRGB document: the same view returns to the sRGB path.
        v.attach(srgb)
        XCTAssertEqual(v.layerColorSpace?.name, CGColorSpace.extendedLinearSRGB)
        XCTAssertEqual(v.surfacePixelFormat, .rgba8Unorm_srgb)
    }

    func testP3ChannelOverlayAndLayerThumbnailCarryDocumentEncoding() throws {
        let (doc, f) = try redDocument(profile: "Display P3")
        defer { doc.close() }
        let (surface, _, _) = try pane(doc, f)
        let overlay = try XCTUnwrap(ChannelImages.components(surface,
            ComponentVisibility(red: true, green: true, blue: false), space: doc.displayColor.space))
        XCTAssertTrue(sameColors(overlay.colorSpace, doc.displayColor.space))
        XCTAssertEqual(Array((overlay.dataProvider!.data! as Data).prefix(4)), [255, 0, 0, 255])
        let thumb = try XCTUnwrap(ThumbnailCache.image(from: surface, space: doc.displayColor.space))
        let cg = try XCTUnwrap(thumb.cgImage(forProposedRect: nil, context: nil, hints: nil))
        XCTAssertTrue(sameColors(cg.colorSpace, doc.displayColor.space))
        XCTAssertEqual(Array((cg.dataProvider!.data! as Data).prefix(4)), [255, 0, 0, 255])
        let channels = DocumentChannels.shared
        defer { channels.reload(nil) }
        channels.reload(doc)
        let rows = channels.rows(doc)
        XCTAssertEqual(rows.count, 4)
        for row in rows {
            let image = try XCTUnwrap(channels.componentThumbnail(doc, row))
            XCTAssertTrue(sameColors(image.cgImage(forProposedRect: nil, context: nil, hints: nil)?.colorSpace,
                                     doc.displayColor.space))
        }
    }

    func testAsyncLayerThumbnailCarriesP3() async throws {
        let (doc, _) = try redDocument(profile: "Display P3")
        defer { doc.close() }
        let backend = doc.backend, layer = try XCTUnwrap(doc.primary).id
        let loader = LayerThumbnailLoader()
        let image: NSImage? = await withCheckedContinuation { continuation in
            loader.load(key: "p3", slot: "layer", space: doc.displayColor.space,
                        fetch: { try? backend.layerThumbnail(id: layer, maxPx: 16) }) {
                continuation.resume(returning: $0)
            }
        }
        let cg = try XCTUnwrap(image?.cgImage(forProposedRect: nil, context: nil, hints: nil))
        XCTAssertTrue(sameColors(cg.colorSpace, doc.displayColor.space))
    }

    func testOpenProfileFallbackReachesLateStatusCallback() async throws {
        let base = try StubDocumentEngine().newDocument(width: 32, height: 16, depth: .u8, profile: "Broken")
        let doc = try DocumentController(backend: DisplayProfileBackend(base))
        defer { doc.close() }
        let diagnostic = try XCTUnwrap(doc.displayColor.diagnostic)
        var messages: [String] = []
        let delivered = expectation(description: "deferred display diagnostic")
        doc.report = { messages.append($0); delivered.fulfill() }
        XCTAssertTrue(messages.isEmpty, "defer past the workspace Opened status")
        await fulfillment(of: [delivered], timeout: 2)
        XCTAssertEqual(messages, [diagnostic], "the status callback is installed after opening")
        doc.report = nil
        doc.report = { messages.append($0) }
        await Task.yield()
        XCTAssertEqual(messages, [diagnostic], "consume the pending warning only once")
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
    func testSameNameDifferentICCRefreshRetagsRetainedRingAndLayer() throws {
        let backend = ProfileSwitchBackend()
        backend.profile = icc(CGColorSpace(name: CGColorSpace.displayP3))
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let v = viewport(doc)
        let ids = v.ringSurfaces.map { IOSurfaceGetID($0) }.sorted()
        backend.profile = icc(CGColorSpace(name: CGColorSpace.adobeRGB1998))
        doc.reloadModel() // info.profileName deliberately unchanged
        XCTAssertEqual(doc.displayColor.iccData, backend.profile)
        XCTAssertEqual(icc(v.layerColorSpace), backend.profile)
        XCTAssertEqual(v.surfacePixelFormat, .rgba8Unorm)
        XCTAssertEqual(v.ringSurfaces.map { IOSurfaceGetID($0) }.sorted(), ids)
        for surface in v.ringSurfaces {
            XCTAssertEqual(IOSurfaceCopyValue(surface, kIOSurfaceColorSpace) as? Data, backend.profile)
        }
    }

    func testUnavailableLinearTwinKeepsEncodedProfileWithDiagnostic() throws {
        let profile = try XCTUnwrap(icc(CGColorSpace(name: CGColorSpace.displayP3)))
        let color = DocumentDisplayColor.resolve(icc: profile, name: "P3", linearize: { _ in nil })
        XCTAssertEqual(color.iccData, profile)
        XCTAssertFalse(color.isSRGB)
        XCTAssertNotNil(color.diagnostic, "unavailable linear twin must explain encoded fallback")
    }

    func testNonSRGBCurvesRetainDocumentEncodedPath() throws {
        for name in [CGColorSpace.adobeRGB1998, CGColorSpace.rommrgb, CGColorSpace.linearSRGB] {
            let backend = ProfileSwitchBackend()
            backend.profile = try XCTUnwrap(icc(CGColorSpace(name: name)))
            let doc = try DocumentController(backend: backend)
            defer { doc.close() }
            let v = viewport(doc)
            XCTAssertEqual(v.surfacePixelFormat, .rgba8Unorm, "\(name)")
            XCTAssertEqual(icc(v.layerColorSpace), backend.profile)
            XCTAssertFalse(v.layerWantsEDR)
            v.attach(nil)
            window?.close(); window = nil
        }
    }

    func testFailedThemeConversionUsesNeutralAndReportsDiagnostic() throws {
        let color = DocumentDisplayColor.resolve(icc: icc(CGColorSpace(name: CGColorSpace.displayP3)), name: "P3")
        var messages: [String] = []
        let result = DocumentViewportView.themeColor(.systemRed, displayColor: color,
                                                     convert: { _, _ in nil }, diagnostic: { messages.append($0) })
        XCTAssertEqual(result, SIMD4<Float>(0, 0, 0, 1), "safe document-space black, never source-space components")
        XCTAssertEqual(messages.count, 1)
    }

    /// Uses the production encoder, actual IOSurface texture format, Metal bilinear sampler and shader.
    /// 2x2 input -> 1x1 RGBA16F output is 50% zoom; the only output pixel straddles black and white.
    func testRenderedHalfStepAndAlphaAreLinearForSRGBRepresentationsAndP3() throws {
        let profiles: [String?] = [nil, "/System/Library/ColorSync/Profiles/sRGB Profile.icc", "Display P3"]
        for profile in profiles {
            let (doc, _) = try redDocument(profile: profile)
            defer { doc.close() }
            let v = viewport(doc)
            let renderer = try XCTUnwrap(DocumentRenderer(), "Metal required for pixel acceptance")
            let surface = try XCTUnwrap(DocumentSurfaces.make(width: 2, height: 2))
            let tex = try XCTUnwrap(renderer.texture(for: surface, format: v.surfacePixelFormat))
            let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba16Float, width: 1, height: 1, mipmapped: false)
            desc.usage = [.renderTarget]
            desc.storageMode = .shared
            let output = try XCTUnwrap(renderer.device.makeTexture(descriptor: desc))
            let queue = try XCTUnwrap(renderer.device.makeCommandQueue())
            let gray = DocumentViewportView.themeColor(NSColor(srgbRed: 167.0/255, green: 167.0/255, blue: 167.0/255, alpha: 1), displayColor: doc.displayColor)
            let u = DocumentRenderer.Uniforms(viewSize: SIMD2(1, 1), center: SIMD2(1, 1), canvas: SIMD2(2, 2),
                                              zoom: 0.5, checker: 8, frameRect: SIMD4(0, 0, 2, 2), uvScale: SIMD2(1, 1),
                                              nearest: 0, pad: 0, checkA: gray, checkB: gray, background: gray)
            for alpha in [false, true] {
                IOSurfaceLock(surface, [], nil)
                let bytes = IOSurfaceGetBaseAddress(surface).assumingMemoryBound(to: UInt8.self)
                for y in 0..<2 { for x in 0..<2 {
                    let i = y * IOSurfaceGetBytesPerRow(surface) + x * 4
                    let sample: UInt8 = alpha || x == 0 ? 0 : 255
                    bytes[i] = sample; bytes[i+1] = sample; bytes[i+2] = sample; bytes[i+3] = alpha ? 128 : 255
                } }
                IOSurfaceUnlock(surface, [], nil)
                let cmd = try XCTUnwrap(queue.makeCommandBuffer())
                XCTAssertTrue(renderer.encode(to: output, commandBuffer: cmd, texture: tex, uniforms: u))
                cmd.commit(); cmd.waitUntilCompleted()
                XCTAssertEqual(cmd.status, .completed, "\(String(describing: cmd.error))")
                var bits = [UInt16](repeating: 0, count: 4)
                output.getBytes(&bits, bytesPerRow: 8, from: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0)
                let components = bits.map { CGFloat(Float(Float16(bitPattern: $0))) }
                let encoded = try XCTUnwrap(CGColor(colorSpace: try XCTUnwrap(v.layerColorSpace), components: components)?
                    .converted(to: doc.displayColor.space, intent: .relativeColorimetric, options: nil)?.components)
                let measured = encoded[0] * 255
                print("B5-30c pixels \(profile ?? "built-in sRGB") \(alpha ? "alpha" : "half-step"): \(measured)")
                XCTAssertEqual(measured, alpha ? 122 : 187.5, accuracy: 2, "\(profile ?? "built-in sRGB") \(alpha ? "alpha" : "half-step")")
            }
            v.attach(nil)
            window?.close(); window = nil
        }
    }

}

private final class DisplayProfileBackend: DocumentBackend, @unchecked Sendable {
    let base: any DocumentBackend
    init(_ base: any DocumentBackend) { self.base = base }
    func id() -> String { base.id() }
    func info() throws -> DocumentSummary { try base.info() }
    func layers() throws -> [LayerRecord] { try base.layers() }
    func layer(id: DocLayerID) throws -> LayerRecord { try base.layer(id: id) }
    func setSelectedLayers(ids: [DocLayerID]) throws { try base.setSelectedLayers(ids: ids) }
    func layerThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 { try base.layerThumbnail(id: id, maxPx: maxPx) }
    func maskThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32 { try base.maskThumbnail(id: id, maxPx: maxPx) }
    func compositeThumbnail(maxPx: UInt32) throws -> UInt32 { try base.compositeThumbnail(maxPx: maxPx) }
    func addLayer(kind: NewLayerKind, name: String, parent: DocLayerID?, index: UInt32?) throws -> DocumentChange { try base.addLayer(kind: kind, name: name, parent: parent, index: index) }
    func duplicateLayer(id: DocLayerID) throws -> DocumentChange { try base.duplicateLayer(id: id) }
    func removeLayer(id: DocLayerID) throws -> DocumentChange { try base.removeLayer(id: id) }
    func moveLayer(id: DocLayerID, parent: DocLayerID?, index: UInt32) throws -> DocumentChange { try base.moveLayer(id: id, parent: parent, index: index) }
    func setProps(id: DocLayerID, props: LayerProperties) throws -> DocumentChange { try base.setProps(id: id, props: props) }
    func renameLayer(id: DocLayerID, name: String) throws -> DocumentChange { try base.renameLayer(id: id, name: name) }
    func setVisible(id: DocLayerID, visible: Bool) throws -> DocumentChange { try base.setVisible(id: id, visible: visible) }
    func setOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange { try base.setOpacity(id: id, value: value, interactive: interactive) }
    func setFillOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange { try base.setFillOpacity(id: id, value: value, interactive: interactive) }
    func setBlendMode(id: DocLayerID, mode: String) throws -> DocumentChange { try base.setBlendMode(id: id, mode: mode) }
    func setGroupMode(id: DocLayerID, mode: LayerGroupMode) throws -> DocumentChange { try base.setGroupMode(id: id, mode: mode) }
    func setLocks(id: DocLayerID, locks: LayerLockFlags) throws -> DocumentChange { try base.setLocks(id: id, locks: locks) }
    func setAdjustmentJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange { try base.setAdjustmentJson(id: id, json: json, interactive: interactive) }
    func setFillJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange { try base.setFillJson(id: id, json: json, interactive: interactive) }
    func addMask(id: DocLayerID, mask: LayerMaskInit) throws -> DocumentChange { try base.addMask(id: id, mask: mask) }
    func removeMask(id: DocLayerID) throws -> DocumentChange { try base.removeMask(id: id) }
    func setMaskEnabled(id: DocLayerID, enabled: Bool) throws -> DocumentChange { try base.setMaskEnabled(id: id, enabled: enabled) }
    func setMaskDensity(id: DocLayerID, density: Float) throws -> DocumentChange { try base.setMaskDensity(id: id, density: density) }
    func setMaskLinked(id: DocLayerID, linked: Bool) throws { try base.setMaskLinked(id: id, linked: linked) }
    func setClipped(id: DocLayerID, clipped: Bool) throws -> DocumentChange { try base.setClipped(id: id, clipped: clipped) }
    func mergeDown(id: DocLayerID) throws -> DocumentChange { try base.mergeDown(id: id) }
    func flatten() throws -> DocumentChange { try base.flatten() }
    func groupLayers(ids: [DocLayerID], name: String) throws -> DocumentChange { try base.groupLayers(ids: ids, name: name) }
    func ungroupLayer(id: DocLayerID) throws -> DocumentChange { try base.ungroupLayer(id: id) }
    func setSelectionRect(x: Int64, y: Int64, width: Int64, height: Int64, feather: Float) throws -> DocumentChange { try base.setSelectionRect(x: x, y: y, width: width, height: height, feather: feather) }
    func clearSelection() throws -> DocumentChange { try base.clearSelection() }
    func commit(label: String) throws -> DocumentChange { try base.commit(label: label) }
    func undo() throws -> DocumentChange { try base.undo() }
    func redo() throws -> DocumentChange { try base.redo() }
    func historyItems() throws -> [DocHistoryEntry] { try base.historyItems() }
    func checkoutHistory(id: DocHistoryID) throws -> DocumentChange { try base.checkoutHistory(id: id) }
    func snapshot(name: String) throws { try base.snapshot(name: name) }
    func snapshots() throws -> [String] { try base.snapshots() }
    func restoreSnapshot(name: String) throws -> DocumentChange { try base.restoreSnapshot(name: name) }
    func setMaxStates(maxStates: UInt32) throws { try base.setMaxStates(maxStates: maxStates) }
    func historyMemoryBytes() throws -> UInt64 { try base.historyMemoryBytes() }
    func setListener(listener: (any DocumentBackendListener)?) { base.setListener(listener: listener) }
    func planSurface(width: UInt32, height: UInt32) throws -> DocViewportPlan { try base.planSurface(width: width, height: height) }
    func attachSurface(iosurfaceId: UInt32, width: UInt32, height: UInt32) throws { try base.attachSurface(iosurfaceId: iosurfaceId, width: width, height: height) }
    func setViewport(level: UInt8, x: UInt32, y: UInt32, width: UInt32, height: UInt32, zoom: Double) throws { try base.setViewport(level: level, x: x, y: y, width: width, height: height, zoom: zoom) }
    func setDisplayHeadroom(headroom: Float) throws { try base.setDisplayHeadroom(headroom: headroom) }
    func displayProfileICC() throws -> Data? { Data("not an ICC profile".utf8) }
    func refresh() throws { try base.refresh() }
    func detachSurfaces() { base.detachSurfaces() }
    func save() throws { try base.save() }
    func saveAs(path: String) throws { try base.saveAs(path: path) }
    func saveAs(path: String, intent: DocSaveDestinationIntent) throws -> DocSaveAsResult { try base.saveAs(path: path, intent: intent) }
    func exportFlat(path: String, format: DocExportFormat, quality: UInt8, color: DocExportColor) throws { try base.exportFlat(path: path, format: format, quality: quality, color: color) }
    func close() { base.close() }
}
