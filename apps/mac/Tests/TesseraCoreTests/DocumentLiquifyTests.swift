import CoreGraphics
import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-13: the Liquify workspace's tools, brush controls, output rules, view mapping, overlay geometry and preview
/// throttle, and the engine / stub adapters (one history node per apply; cancel never commits).
final class DocumentLiquifyTests: XCTestCase {
    func testTenToolsWithPhotoshopKeysAndOptionReversal() {
        XCTAssertEqual(LiquifyToolKind.allCases.count, 10)
        let keys = LiquifyToolKind.allCases.compactMap(\.key)
        XCTAssertEqual(Set(keys).count, keys.count, "unique keys")
        XCTAssertEqual(LiquifyToolKind.forKey("W"), .forwardWarp)
        XCTAssertEqual(LiquifyToolKind.forKey("d"), .thaw)
        XCTAssertNil(LiquifyToolKind.forKey("z"))
        XCTAssertEqual(LiquifyToolKind.twirlClockwise.withOption(true), .twirlCounterClockwise)
        XCTAssertEqual(LiquifyToolKind.pucker.withOption(true), .bloat)
        XCTAssertEqual(LiquifyToolKind.forwardWarp.withOption(true), .forwardWarp)
        XCTAssertEqual(LiquifyToolKind.twirlClockwise.withOption(false), .twirlClockwise)
        XCTAssertFalse(LiquifyToolKind.forwardWarp.actsInPlace)
        XCTAssertFalse(LiquifyToolKind.pushLeft.usesRate)
        XCTAssertTrue(LiquifyToolKind.bloat.actsInPlace)
        // Every tool maps to a distinct engine tool.
        XCTAssertEqual(Set(LiquifyToolKind.allCases.map { "\($0.ffi)" }).count, 10)
    }

    func testBrushSettingsClampAndMapToEngineUnits() {
        var b = LiquifyBrushSettings(size: 0, density: 150, pressure: 0, rate: -5)
        XCTAssertEqual(b.size, 1)
        XCTAssertEqual(b.density, 100)
        XCTAssertEqual(b.pressure, 1)
        XCTAssertEqual(b.rate, 0)
        b.setSize(.nan)
        XCTAssertEqual(b.size, 1, "non-finite input ignored")
        b.setSize(250); b.setDensity(50); b.setPressure(80); b.setRate(40)
        let e = b.engine
        XCTAssertEqual(e.size, 250)
        XCTAssertEqual(e.density, 0.5, accuracy: 1e-6)
        XCTAssertEqual(e.pressure, 0.8, accuracy: 1e-6)
        XCTAssertEqual(e.rate, 0.4, accuracy: 1e-6)
        b.step(larger: true)
        XCTAssertEqual(b.size, 300)
        b.step(larger: false)
        b.step(larger: false)
        XCTAssertEqual(b.size, 200)
        b.setSize(5); b.step(larger: false)
        XCTAssertEqual(b.size, 4)
    }

    func testOutputsFollowTheTarget() {
        let pixel = LiquifyOutput.availability(layerKind: .pixel, hasSelection: false, reEditing: false)
        XCTAssertTrue(pixel.allSatisfy { $0.1 == nil })
        let selected = LiquifyOutput.availability(layerKind: .pixel, hasSelection: true, reEditing: false)
        XCTAssertNotNil(selected.first { $0.0 == .smartFilter }?.1, "smart output refuses a selection")
        XCTAssertNil(selected.first { $0.0 == .currentLayer }?.1)
        let smart = LiquifyOutput.availability(layerKind: .smartObject, hasSelection: false, reEditing: true)
        XCTAssertNil(smart.first { $0.0 == .smartFilter }?.1)
        XCTAssertNotNil(smart.first { $0.0 == .newLayer }?.1)
        XCTAssertEqual(LiquifyOutput.preferred(layerKind: .smartObject), .smartFilter)
        XCTAssertEqual(LiquifyOutput.preferred(layerKind: .pixel), .currentLayer)
        XCTAssertTrue(LiquifyMenuState.enabled(layerKind: .pixel, busy: false))
        XCTAssertTrue(LiquifyMenuState.enabled(layerKind: .smartObject, busy: false))
        XCTAssertFalse(LiquifyMenuState.enabled(layerKind: .text, busy: false))
        XCTAssertFalse(LiquifyMenuState.enabled(layerKind: .pixel, busy: true))
        XCTAssertFalse(LiquifyMenuState.enabled(layerKind: nil, busy: false))
    }

    func testViewTransformFitsAndZoomsAroundThePointer() {
        var t = LiquifyViewTransform.fit(source: CGSize(width: 4000, height: 2000), in: CGSize(width: 1000, height: 800), margin: 20)
        XCTAssertEqual(t.scale, 960.0 / 4000, accuracy: 1e-9)
        let r = t.viewRect(source: CGSize(width: 4000, height: 2000))
        XCTAssertEqual(Double(r.midX), 500, accuracy: 1e-9)
        XCTAssertEqual(Double(r.midY), 400, accuracy: 1e-9)
        let anchor = CGPoint(x: 300, y: 250)
        let under = t.sourcePoint(view: anchor)
        t.zoom(by: 3.7, around: anchor)
        let after = t.viewPoint(source: under)
        XCTAssertEqual(Double(after.x), 300, accuracy: 1e-9)
        XCTAssertEqual(Double(after.y), 250, accuracy: 1e-9)
        t.pan(dx: 12, dy: -7)
        let p = CGPoint(x: 1234.5, y: 77.25)
        let back = t.sourcePoint(view: t.viewPoint(source: p))
        XCTAssertEqual(Double(back.x), 1234.5, accuracy: 1e-9)
        XCTAssertEqual(Double(back.y), 77.25, accuracy: 1e-9)
        t.zoom(by: 1e9, around: anchor)
        XCTAssertEqual(t.scale, LiquifyViewTransform.scaleRange.upperBound)
        XCTAssertEqual(t.viewLength(10), 320)
    }

    func testMeshOverlayIsTheWarpedGridAndFreezePlane() {
        // 3 × 2 nodes, 16 px apart; node (1, 0) samples 4 px to its left → its content appears 4 px right.
        var d = [Float](repeating: 0, count: 12)
        d[2] = -4
        let m = LiquifyMeshData(columns: 3, rows: 2, cellSize: 16, displacement: d, freeze: [0, 0.5, 1, 0, 0, 0], maxDisplacement: 4)
        XCTAssertEqual(m.warpedNode(1, 0), CGPoint(x: 20, y: 0))
        XCTAssertEqual(m.warpedNode(2, 1), CGPoint(x: 32, y: 16))
        let lines = m.gridLines(step: 1)
        XCTAssertEqual(lines.count, 2 + 3)
        XCTAssertEqual(lines[0].count, 3)
        XCTAssertEqual(m.gridLines(step: 2).count, 1 + 2)
        XCTAssertEqual(m.freezeBytes, [0, 128, 255, 0, 0, 0])
        XCTAssertTrue(m.isEdited && m.hasFreeze)
        XCTAssertEqual(m.lineStep(scale: 0.25, minSpacing: 20), 5, "16 px at 25 % is 4 pt: every 5th line")
        XCTAssertEqual(m.lineStep(scale: 4, minSpacing: 20), 1)
    }

    func testPreviewThrottleRunsOneAtATimeWithTheNewestState() {
        var t = PreviewThrottle()
        XCTAssertTrue(t.request(), "idle: start now")
        XCTAssertFalse(t.request(), "in flight: coalesce")
        XCTAssertFalse(t.request())
        XCTAssertTrue(t.finished(), "one more run for the coalesced requests")
        XCTAssertFalse(t.finished(), "then idle")
        XCTAssertFalse(t.inFlight)
        XCTAssertTrue(t.request())
        t.reset()
        XCTAssertFalse(t.inFlight || t.dirty)
    }

    // MARK: Adapters

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-liquify-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func engineDoc(_ w: UInt32 = 96, _ h: UInt32 = 64) throws -> (any DocumentBackend, DocLayerID) {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: w, height: h, depth: .u8, profile: nil)
        let tools = try XCTUnwrap(doc as? any DocumentToolsBackend)
        let layer = try doc.layers()[0].id
        _ = try tools.selectMarquee(.rect, rect: CGRect(x: 30, y: 0, width: 8, height: Double(h)), feather: 0, antialias: false, op: .replace)
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.9, g: 0.1, b: 0.1)), opacity: 1)
        _ = try doc.clearSelection()
        return (doc, layer)
    }

    func testEngineLiquifyIsOneNodeThroughTheAdapter() throws {
        let (doc, layer) = try engineDoc()
        defer { doc.close() }
        let l = try XCTUnwrap(doc as? any DocumentLiquifyBackend)
        let info = try l.beginLiquify(layer: layer, stageIndex: nil)
        XCTAssertEqual(info.layer, layer)
        XCTAssertEqual(info.width, 96)
        XCTAssertEqual(info.previewFactor, 1)
        XCTAssertFalse(info.edited || info.smartObject)
        let before = try doc.historyItems().count
        let r = try l.liquifyBrush(token: info.token, tool: .forwardWarp, brush: LiquifyBrushSettings(size: 30),
                                   points: (0...10).map { LiquifyInputPoint(x: 28 + Float($0), y: 32) })
        XCTAssertGreaterThan(r.dabs, 0)
        XCTAssertNotNil(r.dirty)
        try l.liquifyEndStroke(token: info.token)
        let mesh = try l.liquifyMesh(token: info.token)
        XCTAssertTrue(mesh.isEdited)
        XCTAssertEqual(mesh.displacement.count, mesh.columns * mesh.rows * 2)
        let f = try l.previewLiquify(token: info.token, original: false)
        XCTAssertEqual(f.width, 96)
        XCTAssertNotNil(IOSurfaceLookup(f.surfaceId), "a live IOSurface")
        XCTAssertNotNil(LiquifyWorkspaceModel.image(f, .srgb))
        let o = try l.previewLiquify(token: info.token, original: true)
        XCTAssertTrue(o.original)
        XCTAssertEqual(try doc.historyItems().count, before, "workspace edits add no history")
        let c = try l.commitLiquify(token: info.token, output: .currentLayer)
        XCTAssertTrue(c.layersChanged.contains(layer))
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Liquify")
        XCTAssertThrowsError(try l.liquifyMesh(token: info.token), "closed after apply")
    }

    func testEngineWholeMeshCommandsAndSmartFilterReEdit() throws {
        let (doc, layer) = try engineDoc()
        defer { doc.close() }
        let l = try XCTUnwrap(doc as? any DocumentLiquifyBackend)
        let filters = try XCTUnwrap(doc as? any DocumentFiltersBackend)
        _ = try filters.convertForSmartFilters(layer: layer)
        let info = try l.beginLiquify(layer: layer, stageIndex: nil)
        XCTAssertTrue(info.smartObject)
        _ = try l.liquifyBrush(token: info.token, tool: .bloat, brush: LiquifyBrushSettings(size: 40),
                               points: Array(repeating: LiquifyInputPoint(x: 48, y: 32), count: 4))
        try l.liquifyReconstructAll(token: info.token, amount: 0.5)
        XCTAssertTrue(try l.liquifyMesh(token: info.token).isEdited)
        try l.liquifyFreezeAll(token: info.token, frozen: true)
        XCTAssertTrue(try l.liquifyMesh(token: info.token).freeze.allSatisfy { $0 == 1 })
        try l.liquifyFreezeAll(token: info.token, frozen: false)
        XCTAssertThrowsError(try l.commitLiquify(token: info.token, output: .newLayer), "smart objects take a smart filter")
        _ = try l.commitLiquify(token: info.token, output: .smartFilter)
        XCTAssertEqual(try filters.smartFilters(layer: layer).map(\.filterId), ["liquify"])
        let again = try l.beginLiquify(layer: layer, stageIndex: 0)
        XCTAssertTrue(again.edited)
        try l.liquifyReset(token: again.token, keepFreeze: false)
        XCTAssertFalse(try l.liquifyMesh(token: again.token).isEdited)
        _ = try l.liquifyBrush(token: again.token, tool: .twirlClockwise, brush: LiquifyBrushSettings(size: 40),
                               points: [LiquifyInputPoint(x: 48, y: 32)])
        _ = try l.commitLiquify(token: again.token, output: .smartFilter)
        XCTAssertEqual(try filters.smartFilters(layer: layer).count, 1, "re-edit replaces, never appends")
    }

    func testEngineCancelAndStaleNeverCommit() throws {
        let (doc, layer) = try engineDoc()
        defer { doc.close() }
        let l = try XCTUnwrap(doc as? any DocumentLiquifyBackend)
        let tools = try XCTUnwrap(doc as? any DocumentToolsBackend)
        let n = try doc.historyItems().count
        let a = try l.beginLiquify(layer: layer, stageIndex: nil)
        _ = try l.liquifyBrush(token: a.token, tool: .pucker, brush: LiquifyBrushSettings(size: 30), points: [LiquifyInputPoint(x: 30, y: 30)])
        l.cancelLiquify(token: a.token)
        XCTAssertThrowsError(try l.commitLiquify(token: a.token, output: .currentLayer))
        XCTAssertEqual(try doc.historyItems().count, n)
        let b = try l.beginLiquify(layer: layer, stageIndex: nil)
        _ = try l.liquifyBrush(token: b.token, tool: .pucker, brush: LiquifyBrushSettings(size: 30), points: [LiquifyInputPoint(x: 30, y: 30)])
        _ = try tools.selectAll()
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0, g: 0, b: 1)), opacity: 1)
        let m = try doc.historyItems().count
        XCTAssertThrowsError(try l.commitLiquify(token: b.token, output: .currentLayer)) { e in
            XCTAssertTrue(e.localizedDescription.contains("changed"), e.localizedDescription)
        }
        XCTAssertEqual(try doc.historyItems().count, m)
        _ = try doc.setLocks(id: layer, locks: LayerLockFlags(pixels: true))
        XCTAssertThrowsError(try l.beginLiquify(layer: layer, stageIndex: nil))
    }

    /// The engine's Liquify, except that Apply waits on a gate before it reaches the engine and Cancel is only
    /// recorded: the cancel lands after the engine's last check, so the engine commits the late apply.
    private final class LateCommitLiquify: DocumentLiquifyBackend, @unchecked Sendable {
        let real: any DocumentLiquifyBackend
        private let gate = DispatchSemaphore(value: 0)
        private let lock = NSLock()
        private var _entered = false
        private var _cancels = 0
        var entered: Bool { lock.withLock { _entered } }
        var cancels: Int { lock.withLock { _cancels } }
        init(_ real: any DocumentLiquifyBackend) { self.real = real }
        func release() { gate.signal() }

        func beginLiquify(layer: DocLayerID, stageIndex: UInt32?) throws -> LiquifyWorkspaceInfo {
            try real.beginLiquify(layer: layer, stageIndex: stageIndex)
        }
        func liquifyBrush(token: UInt64, tool: LiquifyToolKind, brush: LiquifyBrushSettings,
                          points: [LiquifyInputPoint]) throws -> LiquifyStrokeInfo {
            try real.liquifyBrush(token: token, tool: tool, brush: brush, points: points)
        }
        func liquifyEndStroke(token: UInt64) throws { try real.liquifyEndStroke(token: token) }
        func liquifyMesh(token: UInt64) throws -> LiquifyMeshData { try real.liquifyMesh(token: token) }
        func liquifyReconstructAll(token: UInt64, amount: Double) throws { try real.liquifyReconstructAll(token: token, amount: amount) }
        func liquifyReset(token: UInt64, keepFreeze: Bool) throws { try real.liquifyReset(token: token, keepFreeze: keepFreeze) }
        func liquifyFreezeAll(token: UInt64, frozen: Bool) throws { try real.liquifyFreezeAll(token: token, frozen: frozen) }
        func previewLiquify(token: UInt64, original: Bool) throws -> LiquifyPreviewFrame {
            try real.previewLiquify(token: token, original: original)
        }
        func commitLiquify(token: UInt64, output: LiquifyOutput) throws -> DocumentChange {
            lock.withLock { _entered = true }
            gate.wait()
            return try real.commitLiquify(token: token, output: output)
        }
        func cancelLiquify(token: UInt64) { lock.withLock { _cancels += 1 } }
    }

    @MainActor private func waitFor(_ what: String, timeout: TimeInterval = 10, _ condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > deadline { XCTFail("timed out waiting for \(what)"); return }
            try? await Task.sleep(for: .milliseconds(5))
        }
    }

    /// B5-09 parity: an Apply cancelled from the workspace whose engine job committed anyway (the cancel arrived
    /// after the engine's last check) is undone when it returns, so engine history and the panels agree.
    @MainActor func testACancelledApplyTheEngineCommittedIsUndone() async throws {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let backend = EngineDocumentBackend(session: try engine.newDocument(width: 64, height: 48, depth: .u8, profile: nil))
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let real = try XCTUnwrap(backend as (any DocumentBackend) as? any DocumentLiquifyBackend)
        let late = LateCommitLiquify(real)
        let layer = try XCTUnwrap(doc.layers.first).id
        let info = try real.beginLiquify(layer: layer, stageIndex: nil)
        _ = try real.liquifyBrush(token: info.token, tool: .bloat, brush: LiquifyBrushSettings(size: 30),
                                  points: Array(repeating: LiquifyInputPoint(x: 32, y: 24), count: 4))
        let head = doc.info.historyHead
        let engineHead = try backend.historyItems().first { $0.isCurrent }?.id
        let m = LiquifyWorkspaceModel(doc: doc, backend: late, info: info, layerName: "Layer", layerKind: .pixel,
                                      hasSelection: false)
        var ended: Result<DocumentChange, Error>?
        m.onApplied = { ended = $0 }
        m.apply()
        await waitFor("the apply reached the engine") { late.entered }
        m.cancel()
        await waitFor("the cancel was sent") { late.cancels == 1 }
        late.release()
        await waitFor("the late apply returned") { ended != nil }
        if case .success = ended { XCTFail("a cancelled apply never reports success") }
        XCTAssertEqual(try backend.historyItems().first { $0.isCurrent }?.id, engineHead,
                       "the step the engine committed after the cancel is undone")
        XCTAssertEqual(doc.info.historyHead, head, "the panels show the same history as the engine")
        XCTAssertNil(m.jobs.abandoned)
    }

    func testTheStubNeedsTheEngine() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 32, height: 32, depth: .u8, profile: nil)
        defer { doc.close() }
        let l = try XCTUnwrap(doc as? any DocumentLiquifyBackend)
        XCTAssertThrowsError(try l.beginLiquify(layer: 1, stageIndex: nil)) { e in
            XCTAssertTrue(e.localizedDescription.contains("engine"))
        }
        XCTAssertThrowsError(try l.previewLiquify(token: 1, original: false))
        l.cancelLiquify(token: 1)
    }
}
