import Foundation
import IOSurface
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// The engine adapter of document mode (WP M5-13b): record conversion both ways for every enum value,
/// history ids, listener coalescing, a real `DocumentSession` behind `DocumentBackend`, and the
/// workspace's engine-vs-stub choice.
final class EngineDocumentBackendTests: XCTestCase {
    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("engine-doc-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func engine(_ dir: URL) throws -> Engine { try Engine.open(appSupportDir: dir.appendingPathComponent("support").path) }

    // MARK: Record conversion

    func testEnumsRoundTripEveryValue() {
        for d in DocBitDepth.allCases { XCTAssertEqual(DocBitDepth(d.ffi), d) }
        XCTAssertEqual([DocDepth.u8, .u16, .f32].map { DocBitDepth($0).ffi }, [.u8, .u16, .f32])
        for k in LayerKindTag.allCases { XCTAssertEqual(LayerKindTag(k.ffi), k) }
        let kinds: [DocLayerKind] = [.pixel, .adjustment, .fill, .group, .smartObject, .text]
        XCTAssertEqual(kinds.map { LayerKindTag($0).ffi }, kinds)
        XCTAssertEqual(Set(kinds.map { LayerKindTag($0) }).count, LayerKindTag.allCases.count)
        for m in LayerGroupMode.allCases { XCTAssertEqual(LayerGroupMode(m.ffi), m) }
        XCTAssertEqual([DocGroupMode.passThrough, .isolated].map { LayerGroupMode($0).ffi }, [.passThrough, .isolated])
        for m in LayerMaskInit.allCases { XCTAssertEqual(LayerMaskInit(m.ffi), m) }
        XCTAssertEqual([MaskInit.revealAll, .hideAll, .fromSelection].map { LayerMaskInit($0).ffi }, [.revealAll, .hideAll, .fromSelection])
        for f in DocExportFormat.allCases { XCTAssertEqual(DocExportFormat(f.ffi), f) }
        XCTAssertEqual([ExportFormat.png, .jpeg, .tiff].map { DocExportFormat($0).ffi }, [.png, .jpeg, .tiff])
        for c in DocExportColor.allCases { XCTAssertEqual(DocExportColor(c.ffi), c) }
        let colors: [ExportColor] = [.document, .srgb, .displayP3, .adobeRgb, .proPhoto, .rec2020]
        XCTAssertEqual(colors.map { DocExportColor($0).ffi }, colors)
        XCTAssertEqual(Set(colors.map { DocExportColor($0) }).count, DocExportColor.allCases.count)
        let news: [NewLayerKind] = [.pixel, .group(mode: .passThrough), .group(mode: .isolated),
                                    .adjustment(json: #"{"kind":"invert"}"#), .fill(json: #"{"kind":"solid","color":[1,0,0]}"#)]
        for n in news { XCTAssertEqual(NewLayerKind(n.ffi), n) }
    }

    func testRecordsRoundTripFieldByField() {
        let rect = CanvasRect(x: -3, y: 4, width: 500, height: 7)
        XCTAssertEqual(CanvasRect(rect.ffi), rect)
        let locks = LayerLockFlags(transparency: true, pixels: false, position: true, all: false)
        XCTAssertEqual(LayerLockFlags(locks.ffi), locks)
        let layer = LayerRecord(id: 42, parent: 7, index: 3, depth: 2, kind: .fill, name: "Gradient Fill 1", visible: false,
                                opacity: 0.25, fillOpacity: 0.5, blendMode: "linear_dodge", groupMode: .isolated, clipped: true,
                                locks: locks, knockout: "shallow", background: true, hasMask: true, maskEnabled: false,
                                maskLinked: false, maskDensity: 0.75, adjustmentJson: "{}", fillJson: #"{"kind":"solid"}"#,
                                bounds: rect, revision: 99)
        XCTAssertEqual(LayerRecord(layer.ffi), layer)
        var root = layer
        root.parent = nil; root.groupMode = nil; root.bounds = nil; root.adjustmentJson = nil; root.fillJson = nil
        XCTAssertEqual(LayerRecord(root.ffi), root)
        XCTAssertNil(root.ffi.parent)

        let props = LayerProperties(name: "A", visible: false, opacity: 0.3, fillOpacity: 0.6, blendMode: "hard_mix",
                                    clipped: true, locks: locks, knockout: "deep", colorTag: "red")
        XCTAssertEqual(LayerProperties(props.ffi), props)
        let summary = DocumentSummary(id: "doc#3", path: "/tmp/a.psd", title: "a.psd", width: 10, height: 20, depth: .f32,
                                      profileName: "Display P3", dirty: true, historyHead: 12, canUndo: true, canRedo: false,
                                      selectedLayerIds: [4, 5], selectionBounds: rect, sourceImageId: "img-1", layerCount: 6,
                                      epoch: 77, backend: "Metal (test)")
        XCTAssertEqual(DocumentSummary(summary.ffi()), summary)
        let change = DocumentChange(layersChanged: [1, 2], created: [3], historyHead: 9, dirtyRect: rect, epoch: 5, dirty: true)
        XCTAssertEqual(DocumentChange(change.ffi()), change)
        let frame = DocFrame(surfaceId: 11, level: 2, x: 5, y: 6, width: 300, height: 200, canvasRect: rect, levelWidth: 1300,
                             levelHeight: 867, zoom: 0.23, epoch: 8, renderMs: 3.7, fullRecomposite: true, blocks: 4902)
        XCTAssertEqual(DocFrame(frame.ffi), frame)
        let plan = DocViewportPlan(level: 3, width: 651, height: 434)
        XCTAssertEqual(DocViewportPlan(plan.ffi), plan)
    }

    func testBlendModeStringsMatchTheEngine() throws {
        XCTAssertEqual(DocBlendMode.allCases.map(\.backendName), EngineDocumentEngine.blendModeNames,
                       "menu order and names are the compositor's")
        let dir = try temp()
        let docs = EngineDocumentEngine.for(try engine(dir))
        let doc = try docs.newDocument(width: 32, height: 32, depth: .u8, profile: nil)
        defer { doc.close() }
        let id = try XCTUnwrap(doc.layers().first?.id)
        for mode in DocBlendMode.allCases {
            _ = try doc.setBlendMode(id: id, mode: mode.backendName)
            XCTAssertEqual(try doc.layer(id: id).blendMode, mode.backendName)
        }
        let g = try XCTUnwrap(doc.addLayer(kind: .group(mode: .isolated), name: "", parent: nil, index: nil).created.first)
        _ = try doc.setBlendMode(id: g, mode: "pass_through")
        XCTAssertEqual(try doc.layer(id: g).blendMode, "pass_through")
        XCTAssertEqual(try doc.layer(id: g).groupMode, .passThrough)
        XCTAssertEqual(DocBlendMode.title(backendName: "pass_through", groupMode: .passThrough), "Pass Through")
        XCTAssertThrowsError(try doc.setBlendMode(id: id, mode: "bogus")) { e in XCTAssertTrue(e is DocumentError, "\(e)") }
    }

    // MARK: History ids

    func testHistoryIDMapShowsTheBaseAsOpened() {
        func item(_ id: UInt64, _ parent: UInt64?, current: Bool = false) -> DocHistoryItem {
            DocHistoryItem(id: id, label: "s\(id)", parent: parent, isCurrent: current, author: "user")
        }
        var map = DocumentHistoryIDMap()
        let rows = map.rows([item(0, nil), item(1, 0), item(2, 1, current: true), item(3, 0)])
        XCTAssertEqual(rows.map(\.id), [1, 2, 3])
        XCTAssertEqual(rows.map(\.parent), [nil, 1, nil], "children of the base list no parent (the Opened row)")
        XCTAssertEqual(rows.map(\.isCurrent), [false, true, false])
        XCTAssertEqual(map.ui(0), 0)
        XCTAssertEqual(map.ui(2), 2)
        XCTAssertEqual(map.engine(0), 0)
        // Pruning removed node 0: the oldest retained parentless node is the base.
        let pruned = map.rows([item(4, nil), item(5, 4, current: true)])
        XCTAssertEqual(pruned.map(\.id), [5])
        XCTAssertEqual(map.base, 4)
        XCTAssertEqual(map.ui(4), 0)
        XCTAssertEqual(map.engine(0), 4)
        XCTAssertEqual(DocumentSummary(DocumentInfo(id: "d", path: nil, title: "t", width: 1, height: 1, depth: .u8,
                                                    profileName: nil, dirty: false, historyHead: 4, canUndo: false,
                                                    canRedo: true, selectedLayerIds: [], selectionBounds: nil,
                                                    sourceImageId: nil, layerCount: 1, epoch: 0, backend: "CPU"),
                                       history: map).historyHead, 0)
    }

    // MARK: Listener coalescing

    private final class Recorder: DocumentBackendListener, @unchecked Sendable {
        var frames: [DocFrame] = []
        var layers: [[DocLayerID]] = []
        var heads: [DocHistoryID] = []
        var failures: [String] = []
        var onMain: [Bool] = []
        func onFrame(frame: DocFrame) { frames.append(frame); onMain.append(Thread.isMainThread) }
        func onLayersChanged(layerIds: [DocLayerID]) { layers.append(layerIds) }
        func onHistoryChanged(head: DocHistoryID) { heads.append(head) }
        func onRenderFailed(message: String) { failures.append(message) }
    }

    private final class ManualQueue: @unchecked Sendable {
        var jobs: [@Sendable () -> Void] = []
        func run() { let j = jobs; jobs = []; j.forEach { $0() } }
    }

    private func frameInfo(epoch: UInt64, surface: UInt32 = 1) -> DocFrameInfo {
        DocFrameInfo(surfaceId: surface, level: 1, x: 0, y: 0, width: 10, height: 10,
                     canvasRect: DocRect(x: 0, y: 0, width: 20, height: 20), levelWidth: 10, levelHeight: 10, zoom: 0.5,
                     epoch: epoch, renderMs: 1, fullRecomposite: false, blocks: 1)
    }

    func testListenerCoalescesUntilTheMainQueueDrains() {
        let target = Recorder()
        let queue = ManualQueue()
        let adapter = EngineDocumentListenerAdapter(target: target, schedule: { job in queue.jobs.append(job) },
                                                    mapHead: { $0 == 7 ? 0 : $0 })
        for e in 1...10 { adapter.onFrame(frame: frameInfo(epoch: UInt64(e))) }
        adapter.onLayersChanged(layerIds: [3, 1])
        adapter.onLayersChanged(layerIds: [1, 2])
        adapter.onHistoryChanged(head: 5)
        adapter.onHistoryChanged(head: 6)
        adapter.onRenderFailed(message: "gpu lost")
        XCTAssertEqual(queue.jobs.count, 1, "one drain scheduled for everything")
        XCTAssertTrue(target.frames.isEmpty)
        queue.run()
        XCTAssertEqual(target.frames.map(\.epoch), [10], "the newest frame only")
        XCTAssertEqual(target.layers, [[3, 1, 2]], "union in first-seen order")
        XCTAssertEqual(target.heads, [6])
        XCTAssertEqual(target.failures, ["gpu lost"])
        XCTAssertEqual(adapter.received, 15)
        XCTAssertEqual(adapter.deliveries, 1)
        // The next callback schedules again; heads are mapped.
        adapter.onHistoryChanged(head: 7)
        XCTAssertEqual(queue.jobs.count, 1)
        queue.run()
        XCTAssertEqual(target.heads, [6, 0])
        XCTAssertTrue(target.frames.count == 1 && target.layers.count == 1, "nothing else re-delivered")
        // Cancelled: pending and later callbacks are dropped.
        adapter.onFrame(frame: frameInfo(epoch: 11))
        adapter.cancel()
        queue.run()
        adapter.onFrame(frame: frameInfo(epoch: 12))
        XCTAssertTrue(queue.jobs.isEmpty)
        XCTAssertEqual(target.frames.map(\.epoch), [10])
    }

    func testListenerDeliversOnTheMainThread() {
        let target = Recorder()
        let adapter = EngineDocumentListenerAdapter(target: target)
        let done = expectation(description: "delivered")
        DispatchQueue.global().async {
            for e in 1...20 { adapter.onFrame(frame: self.frameInfo(epoch: UInt64(e))) }
            DispatchQueue.main.async { done.fulfill() }
        }
        wait(for: [done], timeout: 5)
        XCTAssertFalse(target.frames.isEmpty)
        XCTAssertEqual(target.frames.last?.epoch, 20)
        XCTAssertTrue(target.onMain.allSatisfy { $0 })
        XCTAssertLessThan(adapter.deliveries, 20)
    }

    // MARK: A real session behind DocumentBackend

    func testEngineSessionThroughTheAdapter() throws {
        let dir = try temp()
        let e = try engine(dir)
        let docs = EngineDocumentEngine.for(e)
        XCTAssertTrue(EngineDocumentEngine.for(e) === docs, "one adapter per engine")
        let doc = try docs.newDocument(width: 64, height: 48, depth: .u16, profile: "sRGB IEC61966-2.1")
        XCTAssertTrue(doc is EngineDocumentBackend)
        let info = try doc.info()
        XCTAssertEqual([info.width, info.height], [64, 48])
        XCTAssertEqual(info.depth, .u16)
        XCTAssertEqual(info.profileName, "sRGB IEC61966-2.1")
        XCTAssertEqual(info.historyHead, 0, "as opened")
        XCTAssertTrue(info.backend.hasPrefix("Metal") || info.backend == "CPU", info.backend)
        let layers = try doc.layers()
        XCTAssertEqual(layers.map(\.name), ["Layer 1"], "a blank canvas: one transparent pixel layer")
        XCTAssertEqual(layers.first?.kind, .pixel)
        XCTAssertEqual(try doc.historyItems(), [], "the base node is the Opened row, not a history row")
        let base = try XCTUnwrap(layers.first?.id)

        // Every adjustment and fill the Properties panel writes is accepted and reads back.
        for kind in AdjustmentModel.Kind.allCases {
            let c = try doc.addLayer(kind: .adjustment(json: kind.neutral.json), name: "", parent: nil, index: nil)
            let row = try doc.layer(id: try XCTUnwrap(c.created.first))
            XCTAssertEqual(AdjustmentModel(json: row.adjustmentJson)?.kind, kind, row.name)
            XCTAssertEqual(row.name, "\(kind.title) 1")
        }
        for kind in FillModel.Kind.allCases {
            let c = try doc.addLayer(kind: .fill(json: FillModel.neutral(kind, width: 64, height: 48).json), name: "",
                                     parent: nil, index: nil)
            let row = try doc.layer(id: try XCTUnwrap(c.created.first))
            XCTAssertEqual(FillModel(json: row.fillJson)?.kind, kind, row.name)
        }
        let exposure = try XCTUnwrap(try doc.layers().first { $0.name == "Exposure 1" })
        _ = try doc.setAdjustmentJson(id: exposure.id, json: AdjustmentModel.exposure(exposure: 1, offset: 0, gamma: 1).json,
                                      interactive: false)
        XCTAssertEqual(AdjustmentModel(json: try doc.layer(id: exposure.id).adjustmentJson),
                       .exposure(exposure: 1, offset: 0, gamma: 1))

        // Interactive + commit: no history node until the release, then one.
        let rows = try doc.historyItems().count
        let rev = try doc.layer(id: base).revision
        for v: Float in [0.9, 0.8, 0.7, 0.6, 0.5, 0.4] {
            let c = try doc.setOpacity(id: base, value: v, interactive: true)
            XCTAssertEqual(c.historyHead, try doc.historyItems().last?.id ?? 0)
        }
        XCTAssertEqual(try doc.historyItems().count, rows)
        XCTAssertEqual(try doc.layer(id: base).opacity, 0.4, accuracy: 1e-4, "live")
        XCTAssertEqual(try doc.layer(id: base).revision, rev, "properties do not invalidate the thumbnail")
        let committed = try doc.commit(label: "Opacity 40 %")
        XCTAssertEqual(try doc.historyItems().count, rows + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Opacity 40 %")
        XCTAssertEqual(committed.historyHead, try doc.historyItems().last?.id)

        // Selection edits are history nodes.
        _ = try doc.setSelectionRect(x: 4, y: 4, width: 20, height: 10, feather: 0)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 4, y: 4, width: 20, height: 10))
        XCTAssertEqual(try doc.historyItems().count, rows + 2)
        _ = try doc.clearSelection()
        XCTAssertNil(try doc.info().selectionBounds)
        XCTAssertEqual(try doc.historyItems().count, rows + 3)

        // Undo and "as opened".
        _ = try doc.undo()
        XCTAssertNotNil(try doc.info().selectionBounds)
        let opened = try doc.checkoutHistory(id: 0)
        XCTAssertEqual(opened.historyHead, 0)
        XCTAssertEqual(try doc.info().historyHead, 0)
        XCTAssertEqual(try doc.layers().map(\.name), ["Layer 1"])
        XCTAssertEqual(try doc.undo().historyHead, 0, "nothing before the base")
        XCTAssertNotEqual(try doc.redo().historyHead, 0)

        // Errors arrive as DocumentError.
        XCTAssertThrowsError(try doc.layer(id: 999_999)) { e in XCTAssertTrue(e is DocumentError, "\(e)") }

        // Same path twice = the same backend; closing forgets it.
        let count = try doc.layers().count
        let path = dir.appendingPathComponent("a.tessera-doc").path
        try doc.saveAs(path: path)
        XCTAssertFalse(try doc.info().dirty)
        XCTAssertTrue(try docs.openDocument(path: path) === doc)
        doc.close()
        let reopened = try docs.openDocument(path: path)
        XCTAssertFalse(reopened === doc)
        XCTAssertEqual(try reopened.layers().count, count)
        reopened.close()
    }

    func testFramesReachTheListenerOnMain() throws {
        let dir = try temp()
        let doc = try EngineDocumentEngine.for(try engine(dir)).newDocument(width: 256, height: 128, depth: .u8, profile: nil)
        defer { doc.close() }
        let target = Recorder()
        doc.setListener(listener: target)
        let surfaces = (0..<3).compactMap { _ in DocumentSurfaces.make(width: 128, height: 64) }
        for s in surfaces { try doc.attachSurface(iosurfaceId: IOSurfaceGetID(s), width: 128, height: 64) }
        try doc.setViewport(level: 1, x: 0, y: 0, width: 128, height: 64, zoom: 0.5)
        _ = try doc.addLayer(kind: .fill(json: FillModel.solid(color: [1, 0, 0]).json), name: "", parent: nil, index: nil)
        let end = Date().addingTimeInterval(10)
        while target.frames.last.map({ $0.epoch < (try? doc.info().epoch) ?? 0 }) ?? true, Date() < end {
            RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        }
        let f = try XCTUnwrap(target.frames.last)
        XCTAssertTrue(target.onMain.allSatisfy { $0 })
        XCTAssertEqual(f.level, 1)
        XCTAssertEqual([f.width, f.height], [128, 64])
        XCTAssertEqual(f.canvasRect, CanvasRect(x: 0, y: 0, width: 256, height: 128))
        XCTAssertTrue(surfaces.map(IOSurfaceGetID).contains(f.surfaceId))
        XCTAssertFalse(target.layers.isEmpty, "layers_changed arrives with the frame")
        doc.detachSurfaces()
    }

    // MARK: Workspace: engine vs stub

    @MainActor func testWorkspaceChoosesTheEngineOrTheStub() throws {
        let dir = try temp()
        let e = try engine(dir)
        XCTAssertTrue(DocumentWorkspace.selectEngine(library: nil, policy: .stub) { e } is StubDocumentEngine)
        XCTAssertTrue(DocumentWorkspace.selectEngine(library: StubLibrary.empty, policy: .stub) { e } is StubDocumentEngine)
        XCTAssertTrue(DocumentWorkspace.selectEngine(library: nil, policy: .engine) { nil } is StubDocumentEngine,
                      "the stub when the engine cannot open")
        let standalone = try XCTUnwrap(DocumentWorkspace.selectEngine(library: nil, policy: .engine) { e } as? EngineDocumentEngine)
        XCTAssertTrue(standalone.engine === e)

        // An engine-backed library always gives its own engine.
        let folder = dir.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let lib = try EngineLibrary.scan(folder: folder, appSupport: dir.appendingPathComponent("support2"))
        for policy in [DocumentWorkspace.BackendPolicy.stub, .engine] {
            let chosen = try XCTUnwrap(DocumentWorkspace.selectEngine(library: lib, policy: policy) { e } as? EngineDocumentEngine)
            XCTAssertTrue(chosen.engine === lib.engine)
        }

        // The workspace: stub by default (tests), an explicit engine wins, documents install from it.
        let model = AppModel()
        XCTAssertTrue(model.documents.engine is StubDocumentEngine)
        model.documents.engine = standalone
        model.documents.newDocument(NewDocumentSettings(width: 300, height: 200))
        let doc = try XCTUnwrap(model.documents.current)
        XCTAssertTrue(doc.backend is EngineDocumentBackend)
        XCTAssertEqual(doc.layers.map(\.name), ["Layer 1"])
        XCTAssertEqual(doc.selection, [doc.layers[0].id], "the blank layer is selected")
        XCTAssertEqual(doc.history, [])
        doc.setOpacity(40, final: false)
        doc.setOpacity(35, final: true)
        XCTAssertEqual(doc.history.map(\.label), ["Opacity 35 %"])
        model.undo()
        XCTAssertEqual(doc.info.historyHead, 0)
        XCTAssertEqual(doc.node(doc.layers[0].id)?.opacity ?? 0, 1, accuracy: 1e-5)
        doc.close()
    }
}
