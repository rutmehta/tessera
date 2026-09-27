import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Layer styles in document mode (WP B5-07): the engine schema, `LayerStyles` JSON ↔ model for every
/// effect kind, repeatable effects and their order, Global Light propagation in the UI model, and the
/// engine and stub backends behind `DocumentStylesBackend`.
final class DocumentStylesTests: XCTestCase {
    private var schema: StyleSchema { StyleSchema(json: TesseraFFI.styleEffectsSchemaJson())! }

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-styles-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    // MARK: Schema

    func testSchemaDescribesEveryKindTopFirst() throws {
        let s = schema
        XCTAssertEqual(s.effects.map(\.kind), StyleEffectKind.allCases, "engine order, top first")
        XCTAssertEqual(s.effects.map(\.kind.rank), s.effects.map(\.kind.rank).sorted(by: >))
        for e in s.effects {
            XCTAssertEqual(e.repeatable, e.kind.isRepeatable, "\(e.kind)")
            XCTAssertEqual(e.globalLight, e.kind.usesGlobalLight, "\(e.kind)")
            XCTAssertFalse(e.fields.isEmpty, "\(e.kind)")
            for f in e.fields where f.type == .number { XCTAssertLessThan(f.min, f.max, "\(e.kind).\(f.key)") }
            for f in e.fields { XCTAssertNotNil(e.defaults[f.key], "\(e.kind).\(f.key) has a default") }
        }
        // Contour / jitter (and bevel texture) are metadata, never controls.
        XCTAssertEqual(s.effect(.bevel)?.metadata.map(\.key), ["shape", "texture"])
        XCTAssertEqual(s.effect(.dropShadow)?.metadata.map(\.key), ["shape"])
        XCTAssertFalse(s.effects.flatMap(\.fields).contains { $0.key == "shape" || $0.key == "contour" || $0.key == "jitter" })
        XCTAssertEqual(s.scale.displayScale, 100)
        XCTAssertEqual(s.effect(.dropShadow)?.fields.first { $0.key == "opacity" }?.unit, "%")
        XCTAssertEqual(s.effect(.stroke)?.fields.first { $0.key == "position" }?.options.map(\.title), ["Outside", "Inside", "Center"])
        let g = s.defaults(.gradientOverlay, width: 640)
        XCTAssertEqual(g["fill"]?["end"]?.doubles, [640, 0], "gradient overlays span the canvas")
    }

    // MARK: JSON ↔ model

    func testEveryEffectKindRoundTripsThroughTheModel() throws {
        for e in schema.effects {
            var settings = e.defaults
            if settings["shape"] != nil {
                settings["shape"] = .object(["contour": .array([StyleJSON([0, 0]), StyleJSON([0.5, 0.8]), StyleJSON([1, 1])]),
                                            "jitter": .number(0.25)])
            }
            settings["opacity"] = settings["opacity"] == nil ? nil : .number(0.4)
            let model = LayerStyleModel(effects: [StyleEffect(kind: e.kind, settings: settings)], scale: 1.5)
            let back = try XCTUnwrap(LayerStyleModel(json: model.json), "\(e.kind)")
            XCTAssertEqual(back, model, "\(e.kind)")
            XCTAssertEqual(back.effects[0].settings, settings, "every field kept, metadata included")
            XCTAssertEqual(back.json, model.json)
        }
        // Unknown top-level fields survive; unknown kinds are refused rather than dropped.
        let extra = try XCTUnwrap(LayerStyleModel(json: #"{"effects":[],"scale":2,"future":{"x":1}}"#))
        XCTAssertTrue(extra.json.contains(#""future":{"x":1}"#))
        XCTAssertNil(LayerStyleModel(json: #"{"effects":[{"kind":"lens_flare","settings":{}}]}"#))
        XCTAssertNil(LayerStyleModel(json: "nope"))
        // Whole numbers are written as integers (serde reads pattern sizes as u32).
        let pattern = try XCTUnwrap(schema.effect(.patternOverlay)?.defaults)
        XCTAssertTrue(LayerStyleModel(effects: [StyleEffect(kind: .patternOverlay, settings: pattern)]).json.contains(#""width":2"#))
        // Booleans and numbers keep their JSON types.
        XCTAssertEqual(StyleJSON.parse(#"{"a":true,"b":1,"c":0}"#), .object(["a": .bool(true), "b": .number(1), "c": .number(0)]))
        // Typed accessors.
        var shadow = StyleEffect(kind: .dropShadow, settings: schema.effect(.dropShadow)!.defaults)
        XCTAssertTrue(shadow.enabled)
        XCTAssertEqual(shadow.color("color"), [0, 0, 0, 1])
        shadow.enabled = false
        shadow.setColor("color", [1, 0, 0, 1])
        XCTAssertEqual(shadow.settings["enabled"], .bool(false))
        XCTAssertEqual(shadow.color("color"), [1, 0, 0, 1])
        var stroke = StyleEffect(kind: .stroke, settings: schema.effect(.stroke)!.defaults)
        XCTAssertEqual(stroke.fill, .solid(color: [0, 0, 0]))
        stroke.fill = .solid(color: [0, 0.5, 1])
        XCTAssertEqual(stroke.fill, .solid(color: [0, 0.5, 1]))
    }

    // MARK: Repeatable effects

    func testRepeatableEffectsOrderAndLimits() throws {
        let s = schema
        var m = LayerStyleModel()
        let shadow = try XCTUnwrap(m.add(.dropShadow, settings: s.effect(.dropShadow)!.defaults))
        let stroke = try XCTUnwrap(m.add(.stroke, settings: s.effect(.stroke)!.defaults))
        XCTAssertEqual(m.add(.bevel, settings: s.effect(.bevel)!.defaults), 2)
        XCTAssertNil(m.add(.bevel, settings: .object([:])), "bevel is not repeatable")
        XCTAssertFalse(m.canAdd(.bevel))
        // "+" on the first stroke adds the new one directly above it (later in the vector).
        let stroke2 = try XCTUnwrap(m.add(.stroke, settings: .object(["size": .number(9)]), above: stroke))
        XCTAssertEqual(stroke2, stroke + 1)
        XCTAssertEqual(m.effects.map(\.kind), [.dropShadow, .stroke, .stroke, .bevel])
        // Display order is the engine's: top first by rank, later repeats above earlier ones.
        XCTAssertEqual(m.displayOrder.map { m.effects[$0].kind }, [.bevel, .stroke, .stroke, .dropShadow])
        XCTAssertEqual(m.indices(of: .stroke), [stroke2, stroke])
        XCTAssertEqual(m.effects[m.indices(of: .stroke)[0]].number("size"), 9)
        // A second shadow without an anchor goes on top of the shadows.
        let shadow2 = try XCTUnwrap(m.add(.dropShadow, settings: .object(["distance": .number(20)])))
        XCTAssertEqual(shadow2, shadow + 1)
        XCTAssertEqual(m.indices(of: .dropShadow).first, shadow2)
        // Rows match the engine's summaries.
        let row = LayerStyleRow(layer: 7, model: m)
        XCTAssertEqual(row.effects.map(\.kind), [.bevel, .stroke, .stroke, .dropShadow, .dropShadow])
        // Ten of a kind at most.
        for _ in 0..<20 { m.add(.colorOverlay, settings: .object([:])) }
        XCTAssertEqual(m.count(of: .colorOverlay), LayerStyleModel.maxPerKind)
        m.remove(at: m.indices(of: .stroke)[0])
        XCTAssertEqual(m.count(of: .stroke), 1)
        m.remove(at: 999)
    }

    // MARK: Global Light

    func testGlobalLightPropagatesAcrossLayersInTheUIModel() throws {
        let s = schema
        let global = DocGlobalLight(angle: 120, altitude: 30)
        var a = LayerStyleModel(), b = LayerStyleModel(), local = LayerStyleModel()
        a.add(.dropShadow, settings: s.effect(.dropShadow)!.defaults)
        b.add(.bevel, settings: s.effect(.bevel)!.defaults)
        b.add(.innerShadow, settings: s.effect(.innerShadow)!.defaults)
        var lm = s.effect(.dropShadow)!.defaults
        lm["use_global_light"] = .bool(false)
        lm["angle"] = .number(45)
        local.add(.dropShadow, settings: lm)
        var ui = StyleLightModel(global: global, layers: [1: a, 2: b, 3: local])
        XCTAssertEqual(ui.followers, [1, 2])
        XCTAssertEqual(ui.angle(1, 0), 120)
        // Dragging layer 1's shadow angle moves the document light: layer 2 follows, layer 3 does not.
        XCTAssertTrue(ui.setAngle(60, layer: 1, index: 0))
        XCTAssertEqual(ui.global.angle, 60)
        XCTAssertEqual(ui.angle(2, 0), 60)
        XCTAssertEqual(ui.angle(2, 1), 60)
        XCTAssertEqual(ui.angle(3, 0), 45)
        XCTAssertEqual(ui.layers[1], a, "the layer itself is unchanged: its effect follows the document light")
        // A local angle changes only that effect.
        XCTAssertFalse(ui.setAngle(10, layer: 3, index: 0))
        XCTAssertEqual(ui.angle(3, 0), 10)
        XCTAssertEqual(ui.global.angle, 60)
        // Bevel altitude follows too.
        var bm = b
        XCTAssertEqual(bm.altitude(of: 0, global: ui.global), 30)
        let g2 = try XCTUnwrap(bm.setLight(of: 0, angle: 90, altitude: 50, global: ui.global))
        XCTAssertEqual(g2, DocGlobalLight(angle: 90, altitude: 50))
        // Turning Use Global Light off keeps the light where it is.
        bm.setUsesGlobalLight(false, of: 0, global: g2)
        XCTAssertFalse(bm.effects[0].usesGlobalLight)
        XCTAssertEqual(bm.effects[0].number("angle"), 90)
        XCTAssertEqual(bm.effects[0].number("elevation"), 50)
        XCTAssertEqual(bm.angle(of: 0, global: .default), 90)
        // Satin and overlays never follow the light.
        var satin = StyleEffect(kind: .satin, settings: s.effect(.satin)!.defaults)
        XCTAssertFalse(satin.usesGlobalLight)
        satin.usesGlobalLight = true
        XCTAssertNil(satin.settings["use_global_light"])
    }

    // MARK: Backends

    func testEngineBackendRoundTripsAndRecordsOneNodePerGesture() throws {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: 96, height: 64, depth: .u8, profile: nil)
        defer { doc.close() }
        let styles = try XCTUnwrap(doc as? any DocumentStylesBackend)
        let s = try XCTUnwrap(StyleSchema(json: styles.styleSchemaJson()))
        let layer = try doc.layers()[0].id
        XCTAssertEqual(try LayerStyleModel(json: styles.layerStylesJson(layer: layer)), LayerStyleModel())
        // Every kind through the engine and back.
        var all = LayerStyleModel()
        for e in s.effects { all.add(e.kind, settings: s.defaults(e.kind, width: 96)) }
        _ = try styles.setLayerStylesJson(layer: layer, json: all.json, interactive: false)
        XCTAssertEqual(LayerStyleModel(json: try styles.layerStylesJson(layer: layer)), all)
        let rows = try styles.layerStyleRows()
        XCTAssertEqual(rows.map(\.layer), [layer])
        XCTAssertEqual(rows[0].effects.map(\.kind), LayerStyleRow(layer: layer, model: all).effects.map(\.kind))
        // A drag is live, then one node.
        let before = try doc.historyItems().count
        var m = all
        for d in [2.0, 4, 8] {
            m.effects[m.indices(of: .dropShadow)[0]].setNumber("distance", d)
            _ = try styles.setLayerStylesJson(layer: layer, json: m.json, interactive: true)
        }
        XCTAssertEqual(try doc.historyItems().count, before)
        _ = try doc.commit(label: "Drop Shadow")
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Drop Shadow")
        // Global Light.
        _ = try styles.setGlobalLight(DocGlobalLight(angle: 45, altitude: 60), interactive: false)
        XCTAssertEqual(try styles.globalLight(), DocGlobalLight(angle: 45, altitude: 60))
        XCTAssertThrowsError(try styles.setGlobalLight(DocGlobalLight(angle: 0, altitude: 120), interactive: false))
        // Copy, paste, clear.
        try styles.copyLayerStyles(from: layer)
        XCTAssertTrue(styles.canPasteLayerStyles())
        _ = try styles.clearLayerStyles(layer: layer)
        XCTAssertTrue(try styles.layerStyleRows().isEmpty)
        _ = try styles.pasteLayerStyles(to: [layer])
        XCTAssertEqual(try styles.layerStyleRows().first?.effects.count, all.effects.count)
        XCTAssertEqual(try doc.historyItems().suffix(2).map(\.label), ["Clear Layer Style", "Paste Layer Style"])
        // Locked layers refuse.
        _ = try doc.setLocks(id: layer, locks: LayerLockFlags(all: true))
        XCTAssertThrowsError(try styles.setLayerStylesJson(layer: layer, json: LayerStyleModel().json, interactive: false))
    }

    func testStubBackendKeepsStylesPerDocument() throws {
        let a = StubDocumentBackend(), b = StubDocumentBackend()
        let layer = try XCTUnwrap(a.layers().first { $0.kind == .pixel }?.id)
        var m = LayerStyleModel()
        m.add(.stroke, settings: schema.effect(.stroke)!.defaults)
        _ = try a.setLayerStylesJson(layer: layer, json: m.json, interactive: false)
        XCTAssertEqual(LayerStyleModel(json: try a.layerStylesJson(layer: layer)), m)
        XCTAssertEqual(try a.layerStyleRows().map(\.layer), [layer])
        XCTAssertTrue(try b.layerStyleRows().isEmpty, "styles are per document")
        _ = try a.setGlobalLight(DocGlobalLight(angle: 10, altitude: 20), interactive: false)
        XCTAssertEqual(try a.globalLight().angle, 10)
        XCTAssertEqual(try b.globalLight(), .default)
        if let adj = try a.layers().first(where: { $0.kind == .adjustment }) {
            XCTAssertThrowsError(try a.setLayerStylesJson(layer: adj.id, json: m.json, interactive: false))
        }
        _ = try a.clearLayerStyles(layer: layer)
        XCTAssertTrue(try a.layerStyleRows().isEmpty)
    }

    // MARK: Controller

    @MainActor
    func testControllerAddsEditsAndFollowsTheGlobalLight() throws {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let backend = try EngineDocumentEngine.for(engine).newDocument(width: 64, height: 48, depth: .u8, profile: nil)
        let doc = try DocumentController(backend: backend)
        defer { doc.close() }
        let styles = DocumentStyles.shared
        let layer = try XCTUnwrap(doc.primary?.id)
        let before = doc.history.count
        styles.addEffect(doc, layer, .dropShadow)
        styles.addEffect(doc, layer, .stroke)
        XCTAssertEqual(doc.history.count, before + 2, "each add is one node")
        var m = try XCTUnwrap(styles.model(doc, layer))
        XCTAssertEqual(m.effects.map(\.kind), [.dropShadow, .stroke])
        // A slider drag: live steps, one node on release.
        let i = m.indices(of: .stroke)[0]
        for size in [4.0, 6, 8] {
            m.effects[i].setNumber("size", size)
            styles.apply(doc, layer, m, label: "Stroke", final: size == 8)
        }
        XCTAssertEqual(doc.history.count, before + 3)
        XCTAssertEqual(doc.history.last?.label, "Stroke")
        styles.setEnabled(doc, layer, i, false)
        XCTAssertEqual(styles.model(doc, layer)?.effects[i].enabled, false)
        // Global Light from the controller.
        styles.setGlobalLight(doc, DocGlobalLight(angle: 30, altitude: 40), final: false)
        styles.setGlobalLight(doc, DocGlobalLight(angle: 20, altitude: 40), final: true)
        XCTAssertEqual(styles.globalLight(doc), DocGlobalLight(angle: 20, altitude: 40))
        XCTAssertEqual(doc.history.last?.label, "Global Light")
        XCTAssertEqual(styles.model(doc, layer)?.angle(of: 0, global: styles.globalLight(doc)), 20)
        // Rows for the Layers panel.
        XCTAssertEqual(styles.rows(doc)[layer]?.effects.map(\.kind), [.stroke, .dropShadow])
        styles.removeEffect(doc, layer, 0)
        XCTAssertEqual(styles.model(doc, layer)?.effects.map(\.kind), [.stroke])
        doc.undo()
        XCTAssertEqual(styles.model(doc, layer)?.effects.map(\.kind), [.dropShadow, .stroke])
    }
}
