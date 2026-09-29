import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Auto-Align / Auto-Blend Layers and Photomerge (WP B5-19): option mapping to the FFI records, menu
/// enablement from the Layers selection (two or more top-level, unlocked pixel layers), the sheets'
/// requests, the stub backend's validation, and the engine adapter's wiring.
final class DocumentStackTests: XCTestCase {
    private func row(_ id: DocLayerID, _ kind: LayerKindTag = .pixel, parent: DocLayerID? = nil,
                     clipped: Bool = false, locks: LayerLockFlags = LayerLockFlags()) -> LayerRecord {
        LayerRecord(id: id, parent: parent, index: 0, depth: parent == nil ? 0 : 1, kind: kind, name: "L\(id)",
                    clipped: clipped, locks: locks)
    }

    // MARK: Option mapping

    func testAlignSettingsMapToFFI() {
        XCTAssertEqual(StackAlignLayout.allCases.map(\.title),
                       ["Auto", "Perspective", "Cylindrical", "Spherical", "Collage", "Reposition"])
        let pairs: [(StackAlignLayout, StackAlignMode)] = [(.auto, .auto), (.perspective, .perspective),
            (.cylindrical, .cylindrical), (.spherical, .spherical), (.collage, .collage), (.reposition, .reposition)]
        for (layout, mode) in pairs {
            XCTAssertEqual(StackAlignSettings(layout: layout).ffi.mode, mode)
            XCTAssertEqual(StackAlignLayout(mode), layout)
        }
        let s = StackAlignSettings(layout: .collage, referenceIndex: 2, vignetteRemoval: true, geometricDistortion: true,
                                   seed: 9)
        let f = s.ffi
        XCTAssertEqual(f.referenceIndex, 2)
        XCTAssertTrue(f.vignetteRemoval)
        XCTAssertTrue(f.geometricDistortion)
        XCTAssertEqual(f.seed, 9)
        XCTAssertTrue(f.lensCorrections.isEmpty, "no calibration is sent from the sheets")
        XCTAssertEqual(StackAlignSettings(), StackAlignSettings(layout: .auto))
    }

    func testBlendSettingsMapToFFI() {
        XCTAssertEqual(StackBlendMethod.allCases.map(\.title), ["Panorama", "Stack Images"])
        let b = StackBlendSettings(method: .stackImages, seamlessTones: false, contentAwareFill: true, seed: 3).ffi
        XCTAssertEqual(b.mode, .stackImages)
        XCTAssertFalse(b.seamlessTones)
        XCTAssertTrue(b.contentAwareFill)
        XCTAssertEqual(b.seed, 3)
        let d = StackBlendSettings()
        XCTAssertEqual(d.method, .panorama)
        XCTAssertTrue(d.seamlessTones)
        XCTAssertFalse(d.contentAwareFill)
    }

    func testDefaultsMatchTheEngine() {
        let a = defaultStackAlignOptions(), b = defaultStackBlendOptions()
        XCTAssertEqual(StackAlignSettings().ffi.mode, a.mode)
        XCTAssertEqual(StackAlignSettings().ffi.vignetteRemoval, a.vignetteRemoval)
        XCTAssertEqual(StackBlendSettings().ffi.mode, b.mode)
        XCTAssertEqual(StackBlendSettings().ffi.seamlessTones, b.seamlessTones)
        XCTAssertEqual(StackBlendSettings().ffi.contentAwareFill, b.contentAwareFill)
    }

    // MARK: Menu enablement

    func testAutoAlignNeedsTwoTopLevelUnlockedPixelLayers() {
        let layers = [row(1), row(2), row(3, .smartObject), row(4, .group), row(5, parent: 4),
                      row(6, locks: LayerLockFlags(position: true)), row(7, clipped: true), row(8, .fill),
                      row(9, locks: LayerLockFlags(transparency: true))]
        XCTAssertTrue(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 2]))
        XCTAssertTrue(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 9]), "transparency lock is fine")
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1]))
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: []))
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 3]), "align needs pixel layers")
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 4]))
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 5]), "nested")
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 6]), "locked")
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 7]), "clipped")
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 8]))
        XCTAssertFalse(StackCommandRules.canAutoAlign(layers: layers, selected: [1, 99]), "unknown")
        XCTAssertEqual(StackCommandRules.alignProblem(layers: layers, selected: [1]),
                       "Select two or more layers to align")
        XCTAssertEqual(StackCommandRules.alignProblem(layers: layers, selected: [1, 5]),
                       "Auto-Align works on top-level layers; “L5” is inside a group")
        XCTAssertEqual(StackCommandRules.alignProblem(layers: layers, selected: [1, 6]), "“L6” is locked")
        XCTAssertEqual(StackCommandRules.alignProblem(layers: layers, selected: [1, 3]),
                       "Auto-Align needs pixel layers; “L3” is not a pixel layer")
        XCTAssertNil(StackCommandRules.alignProblem(layers: layers, selected: [2, 1]))
    }

    func testAutoBlendAcceptsAlignedLayers() {
        let layers = [row(1, .smartObject), row(2, .smartObject), row(3), row(4, .text), row(5, parent: 9)]
        XCTAssertTrue(StackCommandRules.canAutoBlend(layers: layers, selected: [1, 2]), "aligned layers are smart objects")
        XCTAssertTrue(StackCommandRules.canAutoBlend(layers: layers, selected: [1, 3]))
        XCTAssertFalse(StackCommandRules.canAutoBlend(layers: layers, selected: [1, 4]))
        XCTAssertFalse(StackCommandRules.canAutoBlend(layers: layers, selected: [1, 5]))
        XCTAssertFalse(StackCommandRules.canAutoBlend(layers: layers, selected: [3]))
    }

    func testSelectionOrderFollowsTheLayersPanelBottomFirst() {
        // Layers list top first; the stack sends ids bottom first (the reference is the bottom layer).
        let layers = [row(3), row(2), row(1)]
        XCTAssertEqual(StackCommandRules.orderedIDs(layers: layers, selected: [2, 3, 1]), [1, 2, 3])
    }

    // MARK: Photomerge sheet

    func testPhotomergeRequest() {
        var form = PhotomergeForm(sources: [.image(id: "a", name: "A.CR2")])
        XCTAssertNil(form.request, "one source is not enough")
        XCTAssertEqual(form.problem, "Choose two or more photos to merge")
        form.sources.append(.file(URL(fileURLWithPath: "/tmp/b.png")))
        form.layout = .cylindrical
        form.contentAwareFill = true
        let r = form.request
        XCTAssertEqual(r?.sources, ["a", "/tmp/b.png"])
        XCTAssertEqual(r?.align.layout, .cylindrical)
        XCTAssertEqual(r?.blend.method, .panorama)
        XCTAssertEqual(r?.blend.contentAwareFill, true)
        XCTAssertEqual(form.sources.map(\.title), ["A.CR2", "b.png"])
        form.sources.append(.file(URL(fileURLWithPath: "/tmp/b.png")))
        XCTAssertEqual(form.sources.count, 3)
        form.removeDuplicates()
        XCTAssertEqual(form.sources.count, 2)
        form.sources = (0..<129).map { .image(id: "\($0)", name: "\($0)") }
        XCTAssertEqual(form.problem, "Photomerge takes at most 128 photos")
        XCTAssertNil(form.request)
    }

    func testLensTogglesNeedCalibration() {
        XCTAssertFalse(StackCommandRules.lensCorrectionAvailable)
        XCTAssertTrue(StackCommandRules.lensCorrectionNote.contains("calibration"))
    }

    // MARK: Stub backend

    func testStubValidatesThenNeedsTheEngine() throws {
        let b = try StubDocumentEngine().newDocument(width: 400, height: 300, depth: .u8, profile: nil) as! StubDocumentBackend
        let head = try b.info().historyHead
        let layers = try b.layers()
        let paper = layers.first { $0.name == "Paper" }!.id, landscape = layers.first { $0.name == "Landscape" }!.id
        XCTAssertThrowsError(try b.autoAlignLayers(ids: [landscape], options: StackAlignSettings())) { e in
            XCTAssertEqual(e as? DocumentError, .invalid("Select two or more layers to align"))
        }
        XCTAssertThrowsError(try b.autoAlignLayers(ids: [paper, landscape], options: StackAlignSettings())) { e in
            XCTAssertEqual(e as? DocumentError, .invalid("“Paper” is locked"))
        }
        let extra = try b.addLayer(kind: .pixel, name: "", parent: nil, index: nil).created[0]
        XCTAssertThrowsError(try b.autoAlignLayers(ids: [landscape, extra], options: StackAlignSettings())) { e in
            guard case .unsupported(let m) = e as? DocumentError else { return XCTFail("\(e)") }
            XCTAssertTrue(m.contains("needs the engine"))
        }
        XCTAssertThrowsError(try b.autoBlendLayers(ids: [landscape, extra], options: StackBlendSettings()))
        XCTAssertThrowsError(try b.photomergeIntoLayers(sources: ["a", "b"], align: StackAlignSettings(),
                                                        blend: StackBlendSettings()))
        XCTAssertEqual(try b.layers().count, layers.count + 1)
        XCTAssertNotEqual(try b.info().historyHead, head, "only the added layer is a history node")
    }

    // MARK: Engine adapter

    func testEngineAdapterRejectsASingleLayerWithoutAHistoryNode() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("stack-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let engine = try Engine.open(appSupportDir: dir.path)
        let b = try EngineDocumentEngine.for(engine).newDocument(width: 64, height: 48, depth: .u8, profile: nil)
            as! EngineDocumentBackend
        defer { b.close() }
        let id = try b.layers()[0].id
        let head = try b.info().historyHead
        XCTAssertThrowsError(try b.autoAlignLayers(ids: [id], options: StackAlignSettings()))
        XCTAssertThrowsError(try b.autoBlendLayers(ids: [id], options: StackBlendSettings()))
        XCTAssertEqual(try b.info().historyHead, head)
        let el = try b.stackEligibility(ids: [id])
        XCTAssertFalse(el.canAlign)
        XCTAssertNotNil(el.reason)
    }
}
