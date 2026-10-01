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
    /// A synthesized 24 MP document; count actual sheet submissions, including release callbacks.
    @MainActor func testSliderSubmissionsAreLatestWins24MP() async throws {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        let backend = EngineDocumentBackend(session: try engine.newDocument(width: 6000, height: 4000, depth: .u8, profile: nil))
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        doc.selection = [try XCTUnwrap(doc.layers.first).id]
        let cr = DocumentCameraRaw()
        cr.open(doc)
        let sheet = try XCTUnwrap(cr.sheet)
        var submitted: [Double] = []
        sheet.previewSubmitter = { json, _ in
            submitted.append(try XCTUnwrap(CameraRawDraft(filterJson: json)).value(CameraRawControls.exposure))
            let first = submitted.count == 1
            try await Task.sleep(for: .milliseconds(first ? 250 : 20))
            return first ? 7 : 2
        }
        let start = Date()
        // A stream of release callbacks must obey the same bound as drag callbacks.
        for tick in 1...60 { sheet.set(CameraRawControls.exposure, Double(tick) / 20, final: true) }
        sheet.set(CameraRawControls.exposure, 3, final: true)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertNotEqual(sheet.previewLevel, 7, "a superseded result must not land")
        try await Task.sleep(for: .milliseconds(300))
        print("B5-41 24MP ticks=60 submits=\(submitted.count) elapsed=\(Date().timeIntervalSince(start)) final=\(submitted.last ?? -1)")
        XCTAssertLessThanOrEqual(submitted.count, 3)
        XCTAssertEqual(submitted.last, 3)
        XCTAssertEqual(sheet.draft.value(CameraRawControls.exposure), 3)
        XCTAssertEqual(sheet.previewLevel, 2)
        sheet.cancel()
    }

    @MainActor func testTimedEngineSliderSubmissions24MP() async throws {
        let engine = try Engine.open(appSupportDir: try temp().appendingPathComponent("support").path)
        let backend = EngineDocumentBackend(session: try engine.newDocument(width: 6000, height: 4000, depth: .u8, profile: nil))
        let layer = try XCTUnwrap(try backend.layers().first).id
        _ = try backend.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.4, g: 0.4, b: 0.4)), opacity: 1)
        try backend.setViewport(level: 2, x: 0, y: 0, width: 1500, height: 1000, zoom: 0.25)
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        doc.selection = [layer]
        let cr = DocumentCameraRaw()
        cr.open(doc)
        let sheet = try XCTUnwrap(cr.sheet)
        var submitted: [Double] = []
        sheet.previewSubmitter = { json, region in
            submitted.append(try XCTUnwrap(CameraRawDraft(filterJson: json)).value(CameraRawControls.exposure))
            try backend.previewFilter(layer: layer, filterJson: json, region: region)
            return try backend.filterPreviewLevel(layer: layer, smartIndex: nil, filterJson: json)
        }
        for release in [true, false] {
            submitted = []
            let start = Date()
            for tick in 1...60 {
                sheet.set(CameraRawControls.exposure, Double(tick) / 20, final: release)
            }
            sheet.set(CameraRawControls.exposure, 3, final: true)
            let inputSeconds = Date().timeIntervalSince(start)
            try await Task.sleep(for: .milliseconds(600))
            print("B5-41 engine24MP releaseCallbacks=\(release) ticks=60 submits=\(submitted.count) inputSeconds=\(inputSeconds) elapsed=\(Date().timeIntervalSince(start)) final=\(submitted.last ?? -1)")
            XCTAssertLessThanOrEqual(submitted.count, 3)
            XCTAssertEqual(submitted.last, 3)
        }
        sheet.cancel()
    }

    @MainActor func testCancelDropsPendingSliderSubmissionAndLateError() async throws {
        let (doc, _) = try greyDocument()
        defer { doc.close() }
        let cr = DocumentCameraRaw()
        cr.open(doc)
        let sheet = try XCTUnwrap(cr.sheet)
        var submits = 0
        sheet.previewSubmitter = { _, _ in
            submits += 1
            try await Task.sleep(for: .milliseconds(150))
            throw DocumentError.invalid("superseded preview")
        }
        sheet.setAmount(20, final: true)
        await Task.yield()
        sheet.setAmount(37, final: false)
        sheet.setAmount(37, final: true)
        sheet.cancel()
        try await Task.sleep(for: .milliseconds(400))
        XCTAssertLessThanOrEqual(submits, 1)
        XCTAssertNil(sheet.error)
        XCTAssertEqual(sheet.amountPercent, 37)
    }

    @MainActor func testDragEventsAcrossRunLoopKeepExactAmountAndDropStaleError() async throws {
        let (doc, _) = try greyDocument()
        defer { doc.close() }
        let cr = DocumentCameraRaw()
        cr.open(doc)
        let sheet = try XCTUnwrap(cr.sheet)
        var amounts: [Double] = []
        sheet.previewSubmitter = { json, _ in
            amounts.append(try XCTUnwrap(CameraRawDraft(filterJson: json)).amountPercent)
            let first = amounts.count == 1
            try await Task.sleep(for: .milliseconds(first ? 250 : 20))
            if first { throw DocumentError.invalid("stale drag error") }
            return 2
        }
        for tick in 1...60 {
            sheet.setAmount(Double(tick) + 0.25, final: false)
            try await Task.sleep(for: .milliseconds(1))
        }
        sheet.setAmount(60.25, final: true)
        try await Task.sleep(for: .milliseconds(600))
        XCTAssertLessThanOrEqual(amounts.count, 3, "separate run-loop events must also coalesce")
        XCTAssertEqual(amounts.last, 60.25)
        XCTAssertEqual(sheet.amountPercent, 60.25)
        XCTAssertEqual(sheet.previewLevel, 2)
        XCTAssertNil(sheet.error)
        sheet.cancel()
    }

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
        // B5-18b: no lens profile / CA analysis on rendered pixels (whole-image work the sheet cannot show).
        XCTAssertEqual(lookup(p, ["settings", "lens", "profile", "kind"]) as? String, "none")
        XCTAssertEqual(lookup(p, ["settings", "lens", "remove_chromatic_aberration"]) as? Bool, false)
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

    /// B5-18b review: the canvas preview omits Sharpening, Noise Reduction, Texture and Clarity when the engine
    /// renders it on a pyramid level > 0 (their pixel radii are full-resolution); the sheet says so only then, and
    /// only when one of them is active. Between 50 % and 100 % the viewport is still level 0: no note.
    func testDetailPreviewNoteFollowsTheSubmittedPreviewLevel() throws {
        let neutral = CameraRawDraft()
        for level in 0...3 { XCTAssertNil(neutral.detailPreviewNote(previewLevel: level), "neutral at level \(level)") }
        let active: [DevelopControl] = [DetailControls.amount, DetailControls.luminance, DetailControls.color,
                                        CameraRawControls.texture, CameraRawControls.clarity]
        for c in active {
            for v in [c.range.upperBound / 2, c.range.lowerBound < 0 ? c.range.lowerBound / 2 : c.range.upperBound] {
                var d = CameraRawDraft()
                d.set(c, v)
                XCTAssertNil(d.detailPreviewNote(previewLevel: 0), "\(c.id): level 0 includes the effects")
                for level in [1, 2, 5] {
                    let note = try XCTUnwrap(d.detailPreviewNote(previewLevel: level), "\(c.id) = \(v) at level \(level)")
                    XCTAssertTrue(note.contains("100"), note)
                }
            }
        }
        // The viewport's level for a zoom (what it pushes to the engine): 50-100 % is level 0, no note.
        var sharp = CameraRawDraft()
        sharp.set(DetailControls.amount, 60)
        for zoom in [0.99, 0.75, 2.0 / 3, 0.51, 1, 3] {
            XCTAssertEqual(DocumentViewportMath.level(forZoom: zoom), 0, "zoom \(zoom)")
            XCTAssertNil(sharp.detailPreviewNote(previewLevel: DocumentViewportMath.level(forZoom: zoom)), "zoom \(zoom)")
        }
        for zoom in [0.5, 1.0 / 3, 0.25, 0.1] {
            XCTAssertGreaterThan(DocumentViewportMath.level(forZoom: zoom), 0, "zoom \(zoom)")
            XCTAssertNotNil(sharp.detailPreviewNote(previewLevel: DocumentViewportMath.level(forZoom: zoom)), "zoom \(zoom)")
        }
        // Settings the zoomed-out preview shows as they are need no note.
        let shown: [DevelopControl] = [CameraRawControls.exposure, CameraRawControls.dehaze, CameraRawControls.saturation,
                                       DetailControls.radius, DetailControls.detail, DetailControls.colorSmoothness]
        for c in shown {
            var d = CameraRawDraft()
            d.set(c, c.clamp(c.defaultValue + (c.range.upperBound - c.range.lowerBound) * 0.2))
            XCTAssertNil(d.detailPreviewNote(previewLevel: 2), c.id)
        }
        // A recipe without detail values gets the engine's defaults (sharpening 40, colour NR 25): active.
        let recipe = try XCTUnwrap(CameraRawDraft(filterJson: #"{"id":"camera_raw","params":{"settings":{}}}"#))
        XCTAssertNotNil(recipe.detailPreviewNote(previewLevel: 2))
        XCTAssertNil(recipe.detailPreviewNote(previewLevel: 0))
    }

    /// The stub's preview level is its viewport level (the engine's is the level `filter_preview_level` reports).
    func testStubPreviewLevelIsTheViewportLevel() throws {
        let b = StubDocumentBackend()
        let layer = try XCTUnwrap(try b.layers().first).id
        let json = CameraRawDraft().filterJson
        XCTAssertEqual(try b.filterPreviewLevel(layer: layer, smartIndex: nil, filterJson: json), 0)
        try b.setViewport(level: 2, x: 0, y: 0, width: 64, height: 64, zoom: 0.25)
        XCTAssertEqual(try b.filterPreviewLevel(layer: layer, smartIndex: nil, filterJson: json), 2)
        try b.setViewport(level: 0, x: 0, y: 0, width: 64, height: 64, zoom: 0.75)
        XCTAssertEqual(try b.filterPreviewLevel(layer: layer, smartIndex: nil, filterJson: json), 0)
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

    /// Linear light of an sRGB-encoded sample given as 0…255 (untagged documents are sRGB-encoded, B5-28).
    private func decoded(_ v: Double) -> Double {
        let e = v / 255
        return e <= 0.04045 ? e / 12.92 : pow((e + 0.055) / 1.055, 2.4)
    }

    /// Mean red sample of the layer through `filterJson`, 0…255. B5-27: the detail pane holds the
    /// document's own samples, like the canvas surfaces.
    private func mean(_ f: any DocumentFiltersBackend, _ layer: DocLayerID, _ json: String,
                      smartIndex: UInt32? = nil) throws -> Double {
        let d = try f.filterDetail(layer: layer, smartIndex: smartIndex, filterJson: json, x: 0, y: 0, width: 16, height: 16)
        let s = try XCTUnwrap(IOSurfaceLookup(d.surfaceId))
        IOSurfaceLock(s, .readOnly, nil)
        defer { IOSurfaceUnlock(s, .readOnly, nil) }
        let base = IOSurfaceGetBaseAddress(s).assumingMemoryBound(to: UInt8.self)
        let stride = IOSurfaceGetBytesPerRow(s)
        var sum = 0.0
        for y in 0..<16 { for x in 0..<16 { sum += Double(base[y * stride + x * 4]) } }
        return sum / 256
    }

    @MainActor func testDetailNoteIncludesOtherCameraRawStages() async throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        _ = try f.convertForSmartFilters(layer: layer)
        var sharp = CameraRawDraft()
        sharp.set(DetailControls.amount, 60)
        _ = try f.applyFilter(layer: layer, filterJson: sharp.filterJson)
        _ = try f.applyFilter(layer: layer, filterJson: CameraRawDraft().filterJson)
        let cr = DocumentCameraRaw()
        for level: UInt8 in [2, 0, 1] {
            try doc.backend.setViewport(level: level, x: 0, y: 0, width: 12, height: 8, zoom: level == 0 ? 1 : 0.25)
            let rows = try f.smartFilters(layer: layer)
            cr.edit(doc, layer: layer, row: rows[1])
            let sheet = try XCTUnwrap(cr.sheet)
            sheet.start()
            try await Task.sleep(for: .milliseconds(200))
            XCTAssertFalse(sheet.draft.hasDetailEffects)
            XCTAssertEqual(sheet.previewLevel, Int(level))
            XCTAssertEqual(sheet.detailPreviewNote != nil, level > 0, "lower stage has sharpening")
            sheet.cancel()
        }
        // Replacing the only active detail stage with a neutral draft removes the note.
        cr.edit(doc, layer: layer, row: try f.smartFilters(layer: layer)[0])
        let resetSheet = try XCTUnwrap(cr.sheet)
        resetSheet.start()
        resetSheet.reset()
        XCTAssertNil(resetSheet.detailPreviewNote)
        resetSheet.cancel()
        // A disabled saved stage contributes no detail effects to the canvas.
        _ = try f.setSmartFilter(layer: layer, index: 0, change: .enabled(false))
        cr.edit(doc, layer: layer, row: try f.smartFilters(layer: layer)[1])
        let disabledSheet = try XCTUnwrap(cr.sheet)
        disabledSheet.start()
        XCTAssertNil(disabledSheet.detailPreviewNote)
        disabledSheet.cancel()
    }

    @MainActor func testFullResolutionStackKeepsDetailNoteOffAtL2() async throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        _ = try f.convertForSmartFilters(layer: layer)
        var sharp = CameraRawDraft()
        sharp.set(DetailControls.amount, 60)
        _ = try f.applyFilter(layer: layer, filterJson: sharp.filterJson)
        let mesh: [String: Any] = [
            "width": 48, "height": 32, "cell_size": 32, "version": 1,
            "displacement": Array(repeating: [0, 0], count: 6),
            "freeze": Array(repeating: 0, count: 6)
        ]
        let data = try JSONSerialization.data(withJSONObject: [
            "id": "liquify", "params": ["mesh": mesh, "interpolation": "bilinear"]
        ])
        _ = try f.applyFilter(layer: layer, filterJson: String(decoding: data, as: UTF8.self))
        try doc.backend.setViewport(level: 2, x: 0, y: 0, width: 12, height: 8, zoom: 0.25)
        let cr = DocumentCameraRaw()
        cr.edit(doc, layer: layer, row: try f.smartFilters(layer: layer)[0])
        let sheet = try XCTUnwrap(cr.sheet)
        sheet.start()
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertTrue(sheet.draft.hasDetailEffects)
        XCTAssertEqual(sheet.previewLevel, 0)
        XCTAssertNil(sheet.detailPreviewNote)
        sheet.cancel()
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
        // The pane holds the layer's own samples (0.4 grey → 102). Those samples
        // are sRGB-ENCODED in an untagged document (B5-28): +1 EV doubles their
        // DECODED light, 0.4 → 0.551.
        XCTAssertEqual(before, 102, accuracy: 1.5)
        XCTAssertEqual(decoded(after) / decoded(before), 2, accuracy: 0.05, "exposure +1 doubles linear light")
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

    /// B5-18b: the detail pane of a re-edit replaces the saved smart filter instead of stacking the edit on it.
    @MainActor func testReEditDetailPaneShowsTheFilterOnce() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        _ = try f.convertForSmartFilters(layer: layer)
        var plus = CameraRawDraft()
        plus.set(CameraRawControls.exposure, 1)
        _ = try f.applyFilter(layer: layer, filterJson: plus.filterJson)
        var zero = CameraRawDraft()
        zero.amountPercent = 0
        let grey = try mean(f, layer, zero.filterJson, smartIndex: 0)
        XCTAssertEqual(grey, 102, accuracy: 1.5, "the re-edited filter at amount 0 shows the unfiltered layer")
        // Samples are sRGB-encoded (B5-28): +1 EV doubles decoded light.
        let once = try mean(f, layer, plus.filterJson, smartIndex: 0)
        XCTAssertEqual(decoded(once) / decoded(grey), 2, accuracy: 0.05, "applied once")
        let twice = try mean(f, layer, plus.filterJson)
        XCTAssertEqual(decoded(twice) / decoded(grey), 4, accuracy: 0.1, "a new filter stacks (×4 linear, 0.4 → 0.755)")
        let cr = DocumentCameraRaw()
        doc.reloadModel()
        cr.edit(doc, layer: layer, row: try f.smartFilters(layer: layer)[0])
        XCTAssertEqual(try XCTUnwrap(cr.sheet).smartIndex, 0)
        cr.sheet?.cancel()
    }

    /// The engine names this filter "Camera Raw Filter" (B5-18b, `Spec::name`): history rows and smart filter
    /// rows show the menu title with no Swift-side mapping (A review of B5-18: they read "camera_raw").
    @MainActor func testHistoryAndSmartFilterRowsShowTheFilterTitle() throws {
        let (doc, f) = try greyDocument()
        defer { doc.close() }
        let layer = try XCTUnwrap(doc.primary).id
        var draft = CameraRawDraft()
        draft.set(CameraRawControls.exposure, 0.5)
        _ = try f.applyFilter(layer: layer, filterJson: draft.filterJson)
        XCTAssertEqual(try doc.backend.historyItems().last?.label, "Camera Raw Filter")

        _ = try f.convertForSmartFilters(layer: layer)
        _ = try f.applyFilter(layer: layer, filterJson: draft.filterJson)
        XCTAssertEqual(try doc.backend.historyItems().last?.label, "Camera Raw Filter")
        let rows = try f.smartFilters(layer: layer)
        XCTAssertEqual(rows.map(\.filterId), ["camera_raw"], "the id is unchanged; only the name is the title")
        XCTAssertEqual(rows.map(\.name), ["Camera Raw Filter"])
        doc.reloadHistory()
        XCTAssertTrue(doc.history.contains { $0.label == "Camera Raw Filter" }, "\(doc.history.map(\.label))")
        XCTAssertFalse(doc.history.contains { $0.label == "camera_raw" }, "\(doc.history.map(\.label))")
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
