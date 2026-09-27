import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-06: `AdjustmentModel` mirrors `compositor::Adjustment` serde exactly. The fixture
/// crates/tessera-ffi/tests/fixtures/adjustments.json is checked on the Rust side (adjustment_json.rs:
/// serde round trip, every variant, `layers()` shape and names); here each variant's objects decode and
/// re-encode to the same JSON, and the exact JSON the engine emits through `layers()` decodes to the same
/// model and survives a round trip through Swift back into the engine unchanged.
final class DocumentAdjustmentJSONTests: XCTestCase {
    static let fixtureURL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("crates/tessera-ffi/tests/fixtures/adjustments.json")

    static func fixture() throws -> [[String: Any]] {
        let data = try Data(contentsOf: fixtureURL)
        return try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [[String: Any]])
    }

    /// JSON equality with numbers compared as doubles (`0` == `0.0`) and `null` == `NSNull`.
    static func same(_ a: Any?, _ b: Any?, _ path: String = "$") -> String? {
        switch (a, b) {
        case (let x as [String: Any], let y as [String: Any]):
            if Set(x.keys) != Set(y.keys) { return "\(path): keys \(x.keys.sorted()) vs \(y.keys.sorted())" }
            for k in x.keys { if let d = same(x[k], y[k], path + "." + k) { return d } }
            return nil
        case (let x as [Any], let y as [Any]):
            if x.count != y.count { return "\(path): count \(x.count) vs \(y.count)" }
            for i in x.indices { if let d = same(x[i], y[i], "\(path)[\(i)]") { return d } }
            return nil
        case (let x as NSNumber, let y as NSNumber):
            let xb = CFGetTypeID(x) == CFBooleanGetTypeID(), yb = CFGetTypeID(y) == CFBooleanGetTypeID()
            if xb || yb { return xb == yb && x.boolValue == y.boolValue ? nil : "\(path): \(x) vs \(y)" }
            return Float(x.doubleValue) == Float(y.doubleValue) ? nil : "\(path): \(x) vs \(y)"
        case (let x as String, let y as String): return x == y ? nil : "\(path): \(x) vs \(y)"
        case (is NSNull, is NSNull), (nil, is NSNull), (is NSNull, nil): return nil
        default: return "\(path): \(String(describing: a)) vs \(String(describing: b))"
        }
    }

    func testFixtureCoversEveryKindAndRoundTrips() throws {
        let objects = try Self.fixture()
        XCTAssertEqual(Set(objects.compactMap { $0["kind"] as? String }), Set(AdjustmentModel.Kind.allCases.map(\.rawValue)))
        for o in objects {
            let m = try XCTUnwrap(AdjustmentModel(object: o), "\(o)")
            XCTAssertEqual(m.kind.rawValue, o["kind"] as? String)
            XCTAssertNil(Self.same(m.jsonObject, o), "\(m.kind.title)")
            XCTAssertEqual(AdjustmentModel(json: m.json), m, m.kind.title)
        }
    }

    private var engine: Engine?
    private var doc: (any DocumentBackend)?

    private func document() throws -> any DocumentBackend {
        if let doc { return doc }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("adj-json-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let e = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        engine = e
        let d = try EngineDocumentEngine.for(e).newDocument(width: 32, height: 16, depth: .u8, profile: "sRGB IEC61966-2.1")
        doc = d
        return d
    }

    /// One variant: add each fixture object as a layer; the engine's JSON decodes to the fixture's model; the
    /// model's JSON sent back (`set_adjustment_json`) reads back as the identical string; the name is the title.
    private func checkVariant(_ kind: String) throws {
        let doc = try document()
        let objects = try Self.fixture().filter { $0["kind"] as? String == kind }
        XCTAssertFalse(objects.isEmpty, kind)
        for o in objects {
            let expected = try XCTUnwrap(AdjustmentModel(object: o))
            let json = String(decoding: try JSONSerialization.data(withJSONObject: o), as: UTF8.self)
            let c = try doc.addLayer(kind: .adjustment(json: json), name: "", parent: nil, index: nil)
            let id = try XCTUnwrap(c.created.first)
            let row = try doc.layer(id: id)
            XCTAssertTrue(row.name.hasPrefix(expected.kind.title + " "), row.name)
            let emitted = try XCTUnwrap(row.adjustmentJson)
            let decoded = try XCTUnwrap(AdjustmentModel(json: emitted), emitted)
            XCTAssertEqual(decoded, expected, kind)
            let emittedObject = try JSONSerialization.jsonObject(with: Data(emitted.utf8))
            XCTAssertNil(Self.same(decoded.jsonObject, emittedObject), kind)
            _ = try doc.setAdjustmentJson(id: id, json: decoded.json, interactive: false)
            XCTAssertEqual(try doc.layer(id: id).adjustmentJson, emitted, "Swift → engine → Swift is exact for \(kind)")
        }
    }

    func testBrightnessContrast() throws { try checkVariant("brightness_contrast") }
    func testLevels() throws { try checkVariant("levels") }
    func testCurves() throws { try checkVariant("curves") }
    func testExposure() throws { try checkVariant("exposure") }
    func testVibrance() throws { try checkVariant("vibrance") }
    func testHueSaturation() throws { try checkVariant("hue_saturation") }
    func testColorBalance() throws { try checkVariant("color_balance") }
    func testBlackWhite() throws { try checkVariant("black_white") }
    func testPhotoFilter() throws { try checkVariant("photo_filter") }
    func testChannelMixer() throws { try checkVariant("channel_mixer") }
    func testColorLookup() throws { try checkVariant("color_lookup") }
    func testInvert() throws { try checkVariant("invert") }
    func testPosterize() throws { try checkVariant("posterize") }
    func testThreshold() throws { try checkVariant("threshold") }
    func testGradientMap() throws { try checkVariant("gradient_map") }
    func testSelectiveColor() throws { try checkVariant("selective_color") }
    func testShadowsHighlights() throws { try checkVariant("shadows_highlights") }
    func testHdrToning() throws { try checkVariant("hdr_toning") }
    func testDesaturate() throws { try checkVariant("desaturate") }
    func testMatchColor() throws { try checkVariant("match_color") }
    func testReplaceColor() throws { try checkVariant("replace_color") }
    func testEqualize() throws { try checkVariant("equalize") }
    func testAuto() throws { try checkVariant("auto") }

    /// Every kind's neutral value (what a new layer starts with) is accepted by the engine unchanged.
    func testNeutralValuesRoundTripThroughTheEngine() throws {
        let doc = try document()
        for kind in AdjustmentModel.Kind.allCases {
            let c = try doc.addLayer(kind: .adjustment(json: kind.neutral.json), name: "", parent: nil, index: nil)
            let row = try doc.layer(id: try XCTUnwrap(c.created.first))
            XCTAssertEqual(AdjustmentModel(json: row.adjustmentJson), kind.neutral, kind.title)
        }
    }
}
