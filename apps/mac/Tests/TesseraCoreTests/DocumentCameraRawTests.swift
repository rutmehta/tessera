import Foundation
import IOSurface
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Filter ▸ Camera Raw Filter… (WP B5-18): the draft behind the sheet (Develop control paths, clamping,
/// white balance, amount, re-edit parse, refusals) and the sheet model against the engine (preview without
/// history, OK as one node, cancel, smart filter append and re-edit, the selection rule).
final class DocumentCameraRawTests: XCTestCase {
    // MARK: Draft

    private func params(_ draft: CameraRawDraft) throws -> [String: Any] {
        let root = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(draft.filterJson.utf8)) as? [String: Any])
        XCTAssertEqual(root["id"] as? String, "camera_raw")
        return try XCTUnwrap(root["params"] as? [String: Any])
    }

    private func lookup(_ obj: [String: Any], _ path: [String]) -> Any? {
        var cur: Any? = obj
        for k in path { cur = (cur as? [String: Any])?[k] }
        return cur
    }

    func testEveryControlPatchesItsDevelopControlPath() throws {
        let all = CameraRawPanel.allCases.flatMap { $0.sections.flatMap(\.controls) }
        XCTAssertGreaterThan(all.count, 50, "Basic, Curve, HSL, Color Grading, Detail and Effects")
        XCTAssertEqual(Set(all.map(\.id)).count, all.count, "no control twice")
        // The sheet reuses the Develop panels' definitions, not a second table.
        var shared: [DevelopControl] = HueBand.allCases.flatMap { b in HSLProperty.allCases.map { $0.control(b) } }
        shared += DetailControls.sharpening
        shared += DetailControls.luminanceNoise
        shared += DetailControls.colorNoise
        shared += EffectsControls.vignette
        shared += EffectsControls.grain
        shared += ParametricRegion.allCases.map(\.control)
        shared += GradeRange.allCases.flatMap { g -> [DevelopControl] in [g.hue, g.saturation, g.luminance] }
        shared += [GradeRange.blending, GradeRange.balance]
        for c in shared { XCTAssertTrue(all.contains(c), "\(c.id) is in the sheet") }
        XCTAssertEqual(CameraRawControls.exposure.path, DevelopParameter.exposure.path)
        XCTAssertEqual(CameraRawControls.temperature.path, DevelopParameter.temperature.path)

        for c in all {
            var d = CameraRawDraft()
            let v = c.clamp(c.defaultValue + (c.range.upperBound - c.range.lowerBound) * 0.1)
            d.set(c, v)
            XCTAssertEqual(d.value(c), v, accuracy: 1e-9, c.id)
            let p = try params(d)
            let stored = try XCTUnwrap(lookup(p, ["settings"] + c.path) as? NSNumber, c.id).doubleValue
            XCTAssertEqual(stored, v, accuracy: 1e-6, c.id)
        }
    }

    func testValuesAreClampedToTheControlRange() throws {
        var d = CameraRawDraft()
        d.set(CameraRawControls.exposure, 9)
        XCTAssertEqual(d.value(CameraRawControls.exposure), 5)
        d.set(GradeRange.shadows.hue, -20)
        XCTAssertEqual(d.value(GradeRange.shadows.hue), 0)
        d.set(DetailControls.radius, 7)
        XCTAssertEqual(d.value(DetailControls.radius), 3)
        let p = try params(d)
        XCTAssertEqual((lookup(p, ["settings", "tone", "exposure"]) as? NSNumber)?.doubleValue, 5)
    }

    func testNeutralDraftTurnsOffRawOnlyDefaultsAndResets() throws {
        var d = CameraRawDraft()
        XCTAssertTrue(d.isNeutral)
        // DevelopSettings::default sharpens (40) and colour-denoises (25) raw files; on rendered pixels the
        // neutral filter must be identity, as Camera Raw's own defaults for non-raw images.
        XCTAssertEqual(d.value(DetailControls.amount), 0)
        XCTAssertEqual(d.value(DetailControls.color), 0)
        XCTAssertEqual(d.value(DetailControls.radius), 1, "untouched controls show the engine default")
        let p = try params(d)
        XCTAssertEqual((p["amount"] as? NSNumber)?.doubleValue, 1)
        XCTAssertEqual((lookup(p, ["settings", "detail", "sharpening", "amount"]) as? NSNumber)?.doubleValue, 0)
        XCTAssertEqual((lookup(p, ["settings", "detail", "noise_reduction", "color"]) as? NSNumber)?.doubleValue, 0)
        XCTAssertNil(lookup(p, ["settings", "tone"]))
        XCTAssertNil(lookup(p, ["settings", "white_balance"]), "as shot unless Temp or Tint moves")

        d.set(CameraRawControls.contrast, 30)
        d.amountPercent = 40
        XCTAssertFalse(d.isNeutral)
        d.reset()
        XCTAssertTrue(d.isNeutral)
        XCTAssertEqual(d, CameraRawDraft())
    }

    func testWhiteBalanceSwitchesToCustomWithBothValues() throws {
        var d = CameraRawDraft()
        XCTAssertEqual(d.value(CameraRawControls.temperature), 6500, "as shot on rendered pixels ≈ D65")
        XCTAssertFalse(d.customWhiteBalance)
        d.set(CameraRawControls.tint, 12)
        XCTAssertTrue(d.customWhiteBalance)
        let p = try params(d)
        XCTAssertEqual(lookup(p, ["settings", "white_balance", "mode"]) as? String, "custom")
        XCTAssertEqual((lookup(p, ["settings", "white_balance", "temperature"]) as? NSNumber)?.doubleValue, 6500)
        XCTAssertEqual((lookup(p, ["settings", "white_balance", "tint"]) as? NSNumber)?.doubleValue, 12)
    }

    func testAmountIsEncodedAsAFraction() throws {
        var d = CameraRawDraft()
        d.amountPercent = 50
        XCTAssertEqual((try params(d)["amount"] as? NSNumber)?.doubleValue, 0.5)
        d.amountPercent = 150
        XCTAssertEqual(d.amountPercent, 100)
        d.amountPercent = -3
        XCTAssertEqual((try params(d)["amount"] as? NSNumber)?.doubleValue, 0)
    }

    func testSplitPointsKeepTheirOrder() {
        var d = CameraRawDraft()
        let shadow = CameraRawControls.split(.shadow), mid = CameraRawControls.split(.midtone)
        d.set(shadow, 80)
        XCTAssertLessThan(d.value(shadow), d.value(mid))
        d.set(mid, 10)
        XCTAssertGreaterThan(d.value(mid), d.value(shadow))
    }

    func testReEditParsesAndRoundTripsIncludingSettingsTheSheetDoesNotShow() throws {
        var d = CameraRawDraft()
        d.set(CameraRawControls.exposure, 0.75)
        d.set(HSLProperty.hue.control(.orange), -10)
        d.set(CameraRawControls.temperature, 4800)
        d.amountPercent = 60
        let again = try XCTUnwrap(CameraRawDraft(filterJson: d.filterJson))
        XCTAssertEqual(again, d)
        XCTAssertEqual(again.filterJson, d.filterJson)
        XCTAssertEqual(again.value(HSLProperty.hue.control(.orange)), -10)
        XCTAssertEqual(again.amountPercent, 60, accuracy: 1e-9)

        // A recipe from elsewhere (MCP, a preset) keeps what the sheet has no control for.
        let foreign = #"{"id":"camera_raw","params":{"settings":{"tone":{"curves":{"rgb":[[0,0],[0.5,0.6],[1,1]]},"exposure":0.5}}}}"#
        let f = try XCTUnwrap(CameraRawDraft(filterJson: foreign))
        XCTAssertEqual(f.amountPercent, 100, "amount defaults to 1 as in the engine")
        XCTAssertEqual(f.value(CameraRawControls.exposure), 0.5)
        XCTAssertEqual(f.value(DetailControls.amount), 40, "no neutral base injected into a saved recipe")
        let p = try params(f)
        XCTAssertNotNil(lookup(p, ["settings", "tone", "curves", "rgb"]))

        XCTAssertNil(CameraRawDraft(filterJson: #"{"id":"gaussian_blur","params":{"radius":2}}"#))
        XCTAssertNil(CameraRawDraft(filterJson: "not json"))
    }

    func testRefusalsForTargetsSelectionAndAIMasks() throws {
        XCTAssertNil(CameraRawFilter.refusal(kind: .pixel, hasSelection: false))
        XCTAssertNil(CameraRawFilter.refusal(kind: .pixel, hasSelection: true), "pixel layers filter inside the selection")
        XCTAssertNil(CameraRawFilter.refusal(kind: .smartObject, hasSelection: false))
        let so = try XCTUnwrap(CameraRawFilter.refusal(kind: .smartObject, hasSelection: true))
        XCTAssertTrue(so.contains("Deselect"), so)
        XCTAssertTrue(so.contains("smart"), so)
        XCTAssertNotNil(CameraRawFilter.refusal(kind: .adjustment, hasSelection: false))
        XCTAssertNotNil(CameraRawFilter.refusal(kind: nil, hasSelection: false))

        let ai = #"{"id":"camera_raw","params":{"settings":{"locals":{"adjustments":[{"components":[{"kind":"sky"}]}]}}}}"#
        let d = try XCTUnwrap(CameraRawDraft(filterJson: ai))
        XCTAssertEqual(d.aiMaskKinds, ["sky"])
        let msg = try XCTUnwrap(d.aiMaskRefusal)
        XCTAssertTrue(msg.contains("AI mask"), msg)
        XCTAssertNil(CameraRawDraft().aiMaskRefusal)
    }

    func testDetailRequestsAreLatestWins() {
        var gate = LatestRequestBuffer<String>()
        let a = gate.submit("a")
        XCTAssertNotNil(a)
        XCTAssertNil(gate.submit("b"))
        XCTAssertNil(gate.submit("c"))
        let (accept, next) = gate.finish(a!.generation)
        XCTAssertFalse(accept, "a newer draft replaced it")
        XCTAssertEqual(next?.value, "c", "only the newest pending draft runs")
    }

    // MARK: Engine

    private var documents: EngineDocumentEngine?

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-cr-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    /// A grey pixel document (the layer filled with 40 % grey).
    @MainActor private func greyDocument() throws -> (DocumentController, any DocumentFiltersBackend) {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        documents = EngineDocumentEngine.for(engine)
        let backend = try documents!.newDocument(width: 48, height: 32, depth: .u8, profile: nil)
        let filters = try XCTUnwrap(backend as? any DocumentFiltersBackend)
        let layer = try backend.layers()[0].id
        let tools = try XCTUnwrap(backend as? any DocumentToolsBackend)
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.4, g: 0.4, b: 0.4)), opacity: 1)
        let doc = try DocumentController(backend: backend)
        doc.selection = [layer]
        return (doc, filters)
    }

    /// Mean red of the layer through `filterJson` (the detail pane's linear samples, 0…255).
    private func mean(_ f: any DocumentFiltersBackend, _ layer: DocLayerID, _ json: String) throws -> Double {
        let d = try f.filterDetail(layer: layer, filterJson: json, x: 0, y: 0, width: 16, height: 16)
        let s = try XCTUnwrap(IOSurfaceLookup(d.surfaceId))
        IOSurfaceLock(s, .readOnly, nil)
        defer { IOSurfaceUnlock(s, .readOnly, nil) }
        let base = IOSurfaceGetBaseAddress(s).assumingMemoryBound(to: UInt8.self)
        let stride = IOSurfaceGetBytesPerRow(s)
        var sum = 0.0
        for y in 0..<16 { for x in 0..<16 { sum += Double(base[y * stride + x * 4]) } }
        return sum / 256
    }

    @MainActor func testNeutralIsIdentityAndExposureDoublesLinear() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        var zero = CameraRawDraft()
        zero.amountPercent = 0
        let before = try mean(f, layer, zero.filterJson)
        XCTAssertEqual(try mean(f, layer, CameraRawDraft().filterJson), before, accuracy: 1.5, "neutral draft is identity")
        var plus = CameraRawDraft()
        plus.set(CameraRawControls.exposure, 1)
        let after = try mean(f, layer, plus.filterJson)
        // `filter_detail` writes the document's linear samples (0.4 grey → 102), so +1 EV is ×2 in the surface.
        XCTAssertEqual(after / before, 2, accuracy: 0.05, "exposure +1 doubles linear light")
    }

    @MainActor func testEngineRejectsOutOfDomainValuesWithoutHistory() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        let before = try doc.backend.historyItems()
        XCTAssertThrowsError(try f.applyFilter(layer: layer, filterJson: #"{"id":"camera_raw","params":{"settings":{},"exposure":1}}"#))
        XCTAssertThrowsError(try f.applyFilter(layer: layer, filterJson: #"{"id":"camera_raw","params":{"settings":{"tone":{"exposure":11}}}}"#))
        XCTAssertEqual(try doc.backend.historyItems(), before)
    }

    @MainActor private func settle(_ cr: DocumentCameraRaw) {
        let done = expectation(description: "apply settles")
        Task { @MainActor in
            while cr.busy != nil { try? await Task.sleep(for: .milliseconds(10)) }
            done.fulfill()
        }
        wait(for: [done], timeout: 60)
    }

    @MainActor func testSheetPreviewsThenAppliesOneUndoableNode() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        let cr = DocumentCameraRaw()
        cr.open(doc)
        let sheet = try XCTUnwrap(cr.sheet)
        XCTAssertNil(sheet.smartIndex)
        XCTAssertEqual(sheet.panel, .basic)
        let history = doc.history.count
        sheet.start()
        sheet.set(CameraRawControls.exposure, 1, final: true)
        XCTAssertEqual(sheet.draft.value(CameraRawControls.exposure), 1)
        XCTAssertNil(sheet.error)
        XCTAssertEqual(doc.history.count, history, "previews record nothing")
        sheet.showBefore = true
        XCTAssertTrue(sheet.showBefore)
        sheet.showBefore = false

        var zero = CameraRawDraft()
        zero.amountPercent = 0
        let grey = try mean(f, layer, zero.filterJson)
        sheet.ok()
        XCTAssertNotNil(cr.busy, "the sheet stays up with progress and Cancel while applying")
        settle(cr)
        XCTAssertNil(cr.sheet)
        XCTAssertEqual(doc.history.count, history + 1, "one history node")
        XCTAssertGreaterThan(try mean(f, layer, zero.filterJson), grey + 20, "the pixels got brighter")
        doc.undo()
        XCTAssertEqual(try mean(f, layer, zero.filterJson), grey, accuracy: 1)
    }

    @MainActor func testCancelLeavesHistoryAndPixelsUnchanged() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        var zero = CameraRawDraft()
        zero.amountPercent = 0
        let grey = try mean(f, layer, zero.filterJson)
        let before = try doc.backend.historyItems()
        let cr = DocumentCameraRaw()
        cr.open(doc)
        let sheet = try XCTUnwrap(cr.sheet)
        sheet.start()
        sheet.set(CameraRawControls.exposure, 2, final: true)
        sheet.cancel()
        XCTAssertNil(cr.sheet)
        XCTAssertNil(cr.busy)
        XCTAssertEqual(try doc.backend.historyItems(), before)
        XCTAssertEqual(try mean(f, layer, zero.filterJson), grey, accuracy: 0.5)
    }

    @MainActor func testSmartObjectAppendsOneSmartFilterAndReEditReplacesIt() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        _ = try f.convertForSmartFilters(layer: layer)
        doc.reloadModel()
        XCTAssertEqual(doc.primary?.kind, .smartObject)
        let cr = DocumentCameraRaw()
        cr.open(doc)
        var sheet = try XCTUnwrap(cr.sheet)
        XCTAssertTrue(sheet.subtitle.contains("Smart filter"), sheet.subtitle)
        sheet.set(CameraRawControls.contrast, 25, final: true)
        sheet.ok()
        settle(cr)
        var rows = try f.smartFilters(layer: layer)
        XCTAssertEqual(rows.map(\.filterId), ["camera_raw"])

        // Double-click on the row: the sheet re-opens with the saved values, previewing the re-edit.
        cr.edit(doc, layer: layer, row: rows[0])
        sheet = try XCTUnwrap(cr.sheet)
        XCTAssertEqual(sheet.smartIndex, 0)
        XCTAssertEqual(sheet.draft.value(CameraRawControls.contrast), 25)
        sheet.set(CameraRawControls.contrast, -40, final: true)
        XCTAssertNil(sheet.error)
        let nodes = doc.history.count
        sheet.ok()
        settle(cr)
        rows = try f.smartFilters(layer: layer)
        XCTAssertEqual(rows.count, 1, "replaced, not duplicated")
        XCTAssertEqual(CameraRawDraft(filterJson: rows[0].filterJson)?.value(CameraRawControls.contrast), -40)
        XCTAssertEqual(doc.history.count, nodes + 1)
        XCTAssertEqual(doc.history.last?.label, "Edit Smart Filter")

        // Native save / reopen keeps the smart filter and its settings.
        let path = try temp().appendingPathComponent("CameraRaw.tessera-doc").path
        try doc.backend.saveAs(path: path)
        let saved = rows[0].filterJson
        doc.close()
        let reopened = try XCTUnwrap(try XCTUnwrap(documents).openDocument(path: path) as? any DocumentFiltersBackend)
        let back = try reopened.smartFilters(layer: layer)
        XCTAssertEqual(back.map(\.filterId), ["camera_raw"])
        XCTAssertEqual(CameraRawDraft(filterJson: back[0].filterJson), CameraRawDraft(filterJson: saved))
        (reopened as? any DocumentBackend)?.close()
    }

    @MainActor func testSmartObjectWithSelectionAndAIMasksAreRefusedWithAReason() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        var messages: [String] = []
        doc.report = { messages.append($0) }
        let layer = try XCTUnwrap(doc.primary).id
        _ = try f.convertForSmartFilters(layer: layer)
        _ = try doc.backend.setSelectionRect(x: 4, y: 4, width: 10, height: 10, feather: 0)
        doc.reloadModel()
        XCTAssertNotNil(doc.marquee)
        let cr = DocumentCameraRaw()
        cr.open(doc)
        XCTAssertNil(cr.sheet)
        XCTAssertTrue(messages.last?.contains("Deselect") == true, messages.last ?? "")

        let ai = SmartFilterRow(index: 0, filterId: "camera_raw", name: "camera_raw", enabled: true,
                                filterJson: #"{"id":"camera_raw","params":{"settings":{"locals":{"adjustments":[{"components":[{"kind":"subject"}]}]}}}}"#,
                                opacity: 1, blendMode: "normal", hasMask: false)
        cr.edit(doc, layer: layer, row: ai)
        XCTAssertNil(cr.sheet)
        XCTAssertTrue(messages.last?.contains("AI mask") == true, messages.last ?? "")
    }

    @MainActor func testTheFilterMenuHookRoutesCameraRawRows() {
        XCTAssertTrue(DocumentCameraRaw.handles(filterId: "camera_raw"))
        XCTAssertFalse(DocumentCameraRaw.handles(filterId: "gaussian_blur"))
        XCTAssertEqual(DocumentCameraRaw.menuTitle, "Camera Raw Filter…")
    }
}
