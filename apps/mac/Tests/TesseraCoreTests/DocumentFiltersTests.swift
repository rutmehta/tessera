import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Filters in document mode (WP M5-12): schema → control mapping, filter JSON, Last Filter memory,
/// the engine adapter (preview without history, apply as one node, smart filters) and the dialogs'
/// controller.
final class DocumentFiltersTests: XCTestCase {
    // MARK: Schema → controls

    func testSchemaMapsEveryControlKind() {
        let json = """
        {"params":[
         {"key":"radius","label":"Radius","kind":"slider","min":0.1,"max":250,"default":2,"step":0.1,"unit":"px","spatial":true,"integer":false},
         {"key":"angle","label":"Angle","kind":"angle","min":-90,"max":90,"default":0},
         {"key":"method","label":"Blur method","kind":"choice","options":[{"value":"spin","label":"Spin"},{"value":"zoom","label":"Zoom"}],"default":"spin"},
         {"key":"center","label":"Center","kind":"point","default":[0.5,0.25]},
         {"key":"mono","label":"Monochromatic","kind":"toggle","default":true},
         {"key":"x","label":"Unknown","kind":"wheel"}]}
        """
        let e = FilterCatalogEntry(id: "t", group: "Blur", name: "Test", schemaJson: json)
        XCTAssertEqual(e.params.map(\.key), ["radius", "angle", "method", "center", "mono"], "unknown kinds are skipped")
        XCTAssertEqual(e.params[0].control, .slider(min: 0.1, max: 250, defaultValue: 2, step: 0.1, unit: "px", spatial: true, integer: false))
        XCTAssertEqual(e.params[0].control.format, "%.1f px")
        XCTAssertEqual(e.params[1].control, .angle(min: -90, max: 90, defaultValue: 0))
        XCTAssertEqual(e.params[1].control.format, "%.0f°")
        XCTAssertEqual(e.params[2].control, .choice(options: [.init(value: "spin", label: "Spin"), .init(value: "zoom", label: "Zoom")],
                                                     defaultValue: "spin"))
        XCTAssertEqual(e.params[3].control, .point(defaultX: 0.5, defaultY: 0.25))
        XCTAssertEqual(e.params[4].control, .toggle(defaultValue: true))
        XCTAssertEqual(e.defaults, ["radius": .number(2), "angle": .number(0), "method": .text("spin"),
                                    "center": .point(0.5, 0.25), "mono": .bool(true)])
        XCTAssertEqual(e.menuTitle, "Test…")
        XCTAssertEqual(FilterCatalogEntry(id: "f", group: "Stylize", name: "Find Edges", params: []).menuTitle, "Find Edges")
        // Clamping to the control.
        XCTAssertEqual(e.params[0].control.clamped(.number(900)), .number(250))
        XCTAssertEqual(e.params[2].control.clamped(.text("warp")), .text("spin"))
        XCTAssertEqual(e.params[3].control.clamped(.point(2, -1)), .point(1, 0))
        XCTAssertEqual(e.params[4].control.clamped(.number(1)), .bool(true), "wrong type falls back to the default")
        let percent = FilterControl.slider(min: 1, max: 500, defaultValue: 100, step: 1, unit: "%", spatial: false, integer: true)
        XCTAssertEqual(percent.format, "%.0f %%")
        XCTAssertEqual(percent.clamped(.number(12.6)), .number(13))
    }

    func testTheEngineCatalogueDecodesAndGroups() {
        let all = FilterCatalogEntry.engineCatalogue
        XCTAssertGreaterThanOrEqual(all.count, 20)
        let groups = FilterCatalogEntry.grouped(all).map(\.group)
        XCTAssertEqual(groups, FilterCatalogEntry.groupOrder)
        let g = all.first { $0.id == "gaussian_blur" }
        XCTAssertEqual(g?.name, "Gaussian Blur")
        XCTAssertEqual(g?.params.first?.key, "radius")
        if case .slider(_, _, _, _, let unit, let spatial, _) = g?.params.first?.control {
            XCTAssertEqual(unit, "px")
            XCTAssertTrue(spatial)
        } else { XCTFail("radius is a slider") }
        let motion = all.first { $0.id == "motion_blur" }
        XCTAssertEqual(motion?.params.first?.control, .angle(min: -90, max: 90, defaultValue: 0))
        XCTAssertTrue(all.contains { $0.params.contains { if case .point = $0.control { true } else { false } } })
        XCTAssertTrue(all.contains { $0.params.contains { if case .choice = $0.control { true } else { false } } })
        XCTAssertTrue(all.contains { $0.params.contains { if case .toggle = $0.control { true } else { false } } })
    }

    func testFilterSettingsJSONRoundTrips() {
        let entry = FilterCatalogEntry.engineCatalogue.first { $0.id == "twirl" }!
        let s = FilterSettings(entry, values: ["angle": .number(120), "center": .point(0.25, 0.75)])
        XCTAssertEqual(s.json, #"{"id":"twirl","params":{"angle":120,"center":[0.25,0.75]}}"#)
        XCTAssertEqual(FilterSettings(json: s.json), s)
        let noise = FilterSettings(id: "add_noise", values: ["monochromatic": .bool(true), "distribution": .text("gaussian")])
        XCTAssertEqual(FilterSettings(json: noise.json), noise)
        XCTAssertNil(FilterSettings(json: "{}"))
    }

    // MARK: Last Filter memory

    func testLastFilterMemoryRemembersPerFilterAndPersists() throws {
        let suite = "tessera.filters.test.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        addTeardownBlock { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        let catalogue = FilterCatalogEntry.engineCatalogue
        let gaussian = catalogue.first { $0.id == "gaussian_blur" }!, median = catalogue.first { $0.id == "median" }!
        let memory = FilterMemory(defaults: defaults)
        XCTAssertNil(memory.last)
        XCTAssertEqual(memory.settings(for: gaussian), FilterSettings(gaussian), "defaults before any use")
        memory.record(FilterSettings(gaussian, values: ["radius": .number(7.5)]))
        memory.record(FilterSettings(median, values: ["radius": .number(3)]))
        XCTAssertEqual(memory.last, FilterSettings(id: "median", values: ["radius": .number(3)]))
        XCTAssertEqual(memory.settings(for: gaussian).number("radius"), 7.5, "each dialog reopens with its last values")
        let again = FilterMemory(defaults: defaults)
        XCTAssertEqual(again.last?.id, "median")
        XCTAssertEqual(again.settings(for: gaussian).number("radius"), 7.5)
    }

    // MARK: Engine adapter

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-filters-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testEngineFiltersThroughTheAdapter() throws {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: 96, height: 64, depth: .u8, profile: nil)
        defer { doc.close() }
        let filters = try XCTUnwrap(doc as? any DocumentFiltersBackend)
        let layer = try doc.layers()[0].id
        let gaussian = #"{"id":"gaussian_blur","params":{"radius":3}}"#
        let before = try doc.historyItems()
        try filters.previewFilter(layer: layer, filterJson: gaussian, region: nil)
        try filters.previewAdjustment(layer: layer, adjustmentJson: #"{"kind":"invert"}"#)
        try filters.clearPreview()
        XCTAssertEqual(try doc.historyItems(), before, "previews record nothing")
        XCTAssertThrowsError(try filters.previewFilter(layer: layer, filterJson: #"{"id":"nope"}"#, region: nil))

        let c = try filters.applyFilter(layer: layer, filterJson: gaussian)
        XCTAssertTrue(c.dirty)
        XCTAssertEqual(try doc.historyItems().count, before.count + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Gaussian Blur")
        _ = try filters.applyAdjustment(layer: layer, adjustmentJson: #"{"kind":"invert"}"#)
        XCTAssertEqual(try doc.historyItems().last?.label, "Invert")
        let d = try filters.filterDetail(layer: layer, filterJson: gaussian, x: 0, y: 0, width: 32, height: 16)
        XCTAssertEqual([d.width, d.height], [32, 16])
        XCTAssertNotEqual(d.surfaceId, 0)

        _ = try filters.convertForSmartFilters(layer: layer)
        XCTAssertEqual(try doc.layer(id: layer).kind, .smartObject)
        _ = try filters.applyFilter(layer: layer, filterJson: gaussian)
        var rows = try filters.smartFilters(layer: layer)
        XCTAssertEqual(rows.map(\.filterId), ["gaussian_blur"])
        XCTAssertEqual(rows[0].name, "Gaussian Blur")
        XCTAssertTrue(rows[0].enabled)
        _ = try filters.setSmartFilter(layer: layer, index: 0, change: .enabled(false))
        _ = try filters.setSmartFilter(layer: layer, index: 0, change: .blending(mode: "multiply", opacity: 0.5))
        rows = try filters.smartFilters(layer: layer)
        XCTAssertEqual([rows[0].enabled ? 1 : 0], [0])
        XCTAssertEqual(rows[0].blendMode, "multiply")
        XCTAssertEqual(rows[0].opacity, 0.5, accuracy: 1e-6)
        XCTAssertNotEqual(try filters.smartFilterMaskThumbnail(layer: layer, index: 0, maxPx: 32), 0)
        XCTAssertEqual(try doc.historyItems().suffix(2).map(\.label), ["Disable Smart Filter", "Smart Filter Blending Options"])
        _ = try filters.removeSmartFilter(layer: layer, index: 0)
        XCTAssertTrue(try filters.smartFilters(layer: layer).isEmpty)
    }

    func testTheStubListsFiltersButNeedsTheEngineToApply() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 64, height: 64, depth: .u8, profile: nil)
        defer { doc.close() }
        let filters = try XCTUnwrap(doc as? any DocumentFiltersBackend)
        XCTAssertEqual(filters.listFilters().map(\.id), FilterCatalogEntry.engineCatalogue.map(\.id))
        let layer = try doc.layers()[0].id
        XCTAssertNoThrow(try filters.previewFilter(layer: layer, filterJson: "{}", region: nil))
        XCTAssertThrowsError(try filters.applyFilter(layer: layer, filterJson: "{}"))
        XCTAssertEqual(try filters.smartFilters(layer: layer), [])
    }

    // MARK: Controller

    @MainActor func testMenusOpenDialogsAndLastFilterReapplies() throws {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let backend = try EngineDocumentEngine.for(engine).newDocument(width: 64, height: 64, depth: .u8, profile: nil)
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        var messages: [String] = []
        doc.report = { messages.append($0) }
        let suite = "tessera.filters.ctl.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        addTeardownBlock { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        let filters = DocumentFilters(memory: FilterMemory(defaults: defaults))
        let catalogue = filters.catalogue(doc)
        XCTAssertFalse(catalogue.isEmpty)
        let gaussian = try XCTUnwrap(catalogue.first { $0.id == "gaussian_blur" })
        XCTAssertEqual(DocumentFilters.target(doc)?.kind, .pixel)

        filters.open(gaussian, doc)
        let sheet = try XCTUnwrap(filters.filterSheet)
        XCTAssertEqual(sheet.title, "Gaussian Blur")
        sheet.set(gaussian.params[0], .number(4))
        XCTAssertEqual(sheet.settings.number("radius"), 4)
        sheet.reset()
        XCTAssertEqual(sheet.settings.number("radius"), 2)
        sheet.set(gaussian.params[0], .number(5))
        let nodes = doc.history.count
        sheet.ok()
        XCTAssertNil(filters.filterSheet)
        let applied = expectation(description: "applied")
        Task { @MainActor in
            while filters.busy != nil { try? await Task.sleep(for: .milliseconds(10)) }
            applied.fulfill()
        }
        wait(for: [applied], timeout: 20)
        XCTAssertEqual(doc.history.count, nodes + 1)
        XCTAssertEqual(doc.history.last?.label, "Gaussian Blur")
        XCTAssertEqual(filters.memory.last?.number("radius"), 5)
        XCTAssertEqual(filters.lastFilterTitle, "Last Filter: Gaussian Blur")

        // ⌃F: again, no dialog.
        filters.reapplyLast(doc)
        XCTAssertNil(filters.filterSheet)
        let again = expectation(description: "again")
        Task { @MainActor in
            while filters.busy != nil { try? await Task.sleep(for: .milliseconds(10)) }
            again.fulfill()
        }
        wait(for: [again], timeout: 20)
        XCTAssertEqual(doc.history.count, nodes + 2)

        // Image ▸ Adjustments ▸ Levels… opens a sheet; an adjustment layer is not a target.
        filters.openAdjustment(.levels, doc)
        XCTAssertEqual(filters.adjustmentSheet?.model.kind, .levels)
        filters.adjustmentSheet?.cancel(filters)
        XCTAssertNil(filters.adjustmentSheet)
        doc.addAdjustment(.exposure)
        XCTAssertNil(DocumentFilters.target(doc))
        filters.open(gaussian, doc)
        XCTAssertNil(filters.filterSheet)
        XCTAssertTrue(messages.last?.contains("select a pixel layer") == true)
    }
}
