import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-09: Remove tool options, distraction review, the Neural Filters sheet's outputs, error
/// presentation, the selection-outline generation counter, and the engine / stub adapters.
final class DocumentRetouchTests: XCTestCase {
    // MARK: Tool options

    func testRemoveOptionsMapToEngineParams() {
        var o = RemoveToolOptions()
        XCTAssertEqual(o.engine, .auto)
        XCTAssertEqual(o.paramsJson, #"{"dilation":2}"#)
        o.dilation = 100
        XCTAssertEqual(o.paramsJson, #"{"dilation":64}"#, "clamped to the engine's 0…64")
        o.dilation = -3
        XCTAssertEqual(o.paramsJson, #"{"dilation":0}"#)
        o.setSize(0)
        XCTAssertEqual(o.size, 1)
        o.setSize(.nan)
        XCTAssertEqual(o.size, 60)
        o.setSize(90)
        o.bracket(larger: true)
        XCTAssertEqual(o.size, 100)
        o.bracket(larger: false)
        XCTAssertEqual(o.size, 90)
        XCTAssertEqual(RemoveEngine.allCases.map(\.title), ["Auto", "PatchMatch", "LaMa"])
        XCTAssertEqual(RemoveEngine.lama.ffi, .lama)
        XCTAssertEqual(RemoveEngine.patchMatch.ffi, .patchMatch)
        XCTAssertEqual(RemoveEngine.auto.ffi, .auto)
    }

    // MARK: Suggestion review

    private func scan() -> DistractionScanResult {
        DistractionScanResult(candidates: [
            DistractionCandidate(id: 0, kind: .wire, bounds: CanvasRect(x: 10, y: 20, width: 40, height: 2), pixels: 80),
            DistractionCandidate(id: 1, kind: .faceBox, bounds: CanvasRect(x: 0, y: 0, width: 100, height: 100), pixels: 10_000),
        ], faces: "supplied", limitation: "Geometric proxies")
    }

    func testSuggestionReviewStartsAcceptedAndAppliesOnlyAccepted() {
        var r = DistractionReview(layer: 7, scan: scan())
        XCTAssertEqual(r.acceptedIds, [0, 1])
        XCTAssertEqual(r.summary, "2 of 2 suggestions selected")
        r.toggle(1)
        XCTAssertEqual(r.acceptedIds, [0])
        XCTAssertFalse(r.isAccepted(1))
        r.toggle(99)
        XCTAssertEqual(r.acceptedIds, [0], "unknown ids are ignored")
        // A click inside both picks the smaller (the wire inside the face box).
        XCTAssertEqual(r.hit(CanvasPoint(x: 20, y: 21))?.id, 0)
        XCTAssertEqual(r.hit(CanvasPoint(x: 90, y: 90))?.id, 1)
        XCTAssertNil(r.hit(CanvasPoint(x: 200, y: 200)))
        r.setAll(false)
        XCTAssertFalse(r.canApply)
        r.setAll(true)
        XCTAssertTrue(r.canApply)
        let empty = DistractionReview(layer: 7, scan: DistractionScanResult(candidates: [], faces: "none", limitation: ""))
        XCTAssertEqual(empty.summary, "Nothing found")
        XCTAssertFalse(empty.canApply)
    }

    // MARK: Neural sheet

    private var specs: [NeuralFilterSpec] { NeuralFilterSpec.engineCatalogue }

    func testNeuralCatalogueComesFromTheEngine() {
        XCTAssertEqual(specs.map(\.kind), [.skinSmoothing, .colorize, .jpegArtifactRemoval])
        XCTAssertEqual(specs.map(\.requiresWeights), [false, true, true])
        XCTAssertEqual(specs[0].controls.map(\.key), ["blur", "smoothness"])
        XCTAssertEqual(specs[2].controls.first?.key, "strength")
        XCTAssertEqual(NeuralKind(filterId: "neural/colorize"), .colorize)
        XCTAssertNil(NeuralKind(filterId: "gaussian_blur"))
    }

    func testNeuralSheetDestinationsFollowTheLayer() {
        var pixel = NeuralSheetState(specs: specs, layerKind: .pixel, hasSelection: false)
        XCTAssertEqual(pixel.output, .currentLayer)
        XCTAssertEqual(NeuralOutput.allCases.filter(pixel.allowed), [.currentLayer, .newLayer, .smartFilter])
        let selected = NeuralSheetState(specs: specs, layerKind: .pixel, hasSelection: true)
        XCTAssertFalse(selected.allowed(.smartFilter), "smart retouch filters are not masked by a selection")
        XCTAssertNotNil(selected.reason(.smartFilter))
        let smart = NeuralSheetState(specs: specs, layerKind: .smartObject, hasSelection: false)
        XCTAssertEqual(smart.output, .smartFilter)
        XCTAssertFalse(smart.allowed(.newLayer))
        XCTAssertEqual(smart.reason(.newLayer), "New layer output needs a pixel layer")
        let group = NeuralSheetState(specs: specs, layerKind: .group, hasSelection: false)
        XCTAssertTrue(NeuralOutput.allCases.filter(group.allowed).isEmpty)

        // Values: clamped, per filter, into the params JSON; reset restores defaults.
        pixel.kind = .jpegArtifactRemoval
        let strength = pixel.spec!.controls[0]
        pixel.set(strength, 3)
        XCTAssertEqual(pixel.value(strength), 1)
        XCTAssertEqual(pixel.paramsJson, #"{"strength":1}"#)
        pixel.reset()
        XCTAssertEqual(pixel.paramsJson, #"{"strength":0.5}"#)
        pixel.kind = .skinSmoothing
        XCTAssertEqual(pixel.paramsJson, #"{"blur":4,"smoothness":0.5}"#, "no faces: the engine finds them")
    }

    func testReEditingANeuralSmartFilterKeepsItsFacesAndOutput() {
        let json = #"{"id":"neural/skin_smoothing","params":{"blur":2,"faces":[[10,12,30,40]],"smoothness":0.25}}"#
        let s = NeuralSheetState(specs: specs, layerKind: .smartObject, hasSelection: true, kind: .skinSmoothing,
                                 smartIndex: 0, filterJson: json)
        XCTAssertEqual(s.output, .smartFilter)
        XCTAssertEqual(NeuralOutput.allCases.filter(s.allowed), [.smartFilter])
        XCTAssertEqual(s.faces, [[10, 12, 30, 40]])
        XCTAssertEqual(s.value(s.spec!.controls[0]), 2)
        XCTAssertEqual(s.filterJson,
                       #"{"id":"neural/skin_smoothing","params":{"blur":2,"faces":[[10,12,30,40]],"smoothness":0.25}}"#)
    }

    // MARK: Errors

    func testMissingWeightsAreNamedWithTheirSource() {
        let msg = "Colorize needs the filters/ddcolor model weights, which are not installed. Tessera never downloads "
            + "weights on its own: the file comes from https://huggingface.co/x/ddcolor.onnx and is expected, "
            + "hash-verified, at /tmp/cache/abc.onnx"
        let p = RetouchErrorPresentation(operation: "Colorize", message: msg)
        XCTAssertTrue(p.isMissingWeights)
        XCTAssertEqual(p.modelId, "filters/ddcolor")
        XCTAssertEqual(p.sourceURL, "https://huggingface.co/x/ddcolor.onnx")
        XCTAssertEqual(p.title, "Colorize needs a model that is not installed")
        XCTAssertEqual(p.detail, msg)
        XCTAssertTrue(p.statusLine.contains("filters/ddcolor"))

        let c = RetouchErrorPresentation(operation: "Remove", message: "cancelled")
        XCTAssertTrue(c.isCancel)
        XCTAssertEqual(c.statusLine, "Remove cancelled")
        let s = RetouchErrorPresentation(operation: "Content-Aware Fill", message: "make a selection first")
        XCTAssertEqual(s.title, "Content-Aware Fill needs a selection")
        let o = RetouchErrorPresentation(operation: "Remove", message: "the layer's pixels are locked")
        XCTAssertEqual(o.title, "Remove failed")
        XCTAssertFalse(o.isMissingWeights)
    }

    // MARK: Outline generations

    func testOutlineGenerationDropsStaleResults() {
        var g = OutlineRequestGate()
        let first = g.begin()
        XCTAssertTrue(g.accepts(first))
        // The selection is cleared while `first` is in flight: the clear starts a generation too.
        _ = g.begin()
        XCTAssertFalse(g.accepts(first), "a result from before the clear is stale")
        let third = g.begin()
        XCTAssertTrue(g.accepts(third))
        XCTAssertFalse(g.accepts(third - 1))
    }

    // MARK: Adapters

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-retouch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testEngineRetouchThroughTheAdapter() throws {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(engine).newDocument(width: 64, height: 48, depth: .u8, profile: nil)
        defer { doc.close() }
        let r = try XCTUnwrap(doc as? any DocumentRetouchBackend)
        let tools = try XCTUnwrap(doc as? any DocumentToolsBackend)
        let layer = try doc.layers()[0].id
        _ = try tools.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.8, g: 0.4, b: 0.2)), opacity: 1)

        // Models are listed from the local cache; nothing is installed in a fresh support dir.
        let models = try r.retouchModels()
        XCTAssertEqual(Set(models.map(\.modelId)), ["remove/lama", "filters/ddcolor", "enhance/drunet-color", "opencv/yunet", "opencv/sface"])
        XCTAssertTrue(models.allSatisfy { !$0.installed && $0.sourceURL.hasPrefix("https://") })

        XCTAssertThrowsError(try r.contentAwareFill(layer: layer, paramsJson: "{}"), "needs a selection")
        _ = try tools.selectMarquee(.rect, rect: CGRect(x: 20, y: 10, width: 10, height: 10), feather: 0, antialias: false, op: .replace)
        let before = try doc.historyItems().count
        let o = try r.removeSelection(layer: layer, engine: .auto, paramsJson: #"{"dilation":0}"#)
        XCTAssertEqual(o.backend, "PatchMatch")
        XCTAssertTrue(o.note?.contains("remove/lama") ?? false)
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Remove")
        XCTAssertThrowsError(try r.removeSelection(layer: layer, engine: .lama, paramsJson: "{}")) { e in
            XCTAssertTrue(e.localizedDescription.contains("remove/lama"))
        }

        // A stroke: one node.
        _ = try doc.clearSelection()
        try r.beginRemoveStroke(layer: layer, size: 8, engine: .patchMatch)
        let dirty = try r.removeStrokePoints([CanvasPoint(x: 10, y: 10), CanvasPoint(x: 30, y: 12)])
        XCTAssertNotNil(dirty)
        let n = try doc.historyItems().count
        _ = try r.endRemoveStroke(paramsJson: "{}")
        XCTAssertEqual(try doc.historyItems().count, n + 1)

        // Neural: the missing model is named; skin smoothing to a new layer is one node.
        XCTAssertThrowsError(try r.neuralFilter(layer: layer, kind: .colorize, paramsJson: "{}", output: .currentLayer)) { e in
            XCTAssertTrue(RetouchErrorPresentation(operation: "Colorize", message: e.localizedDescription).isMissingWeights)
        }
        let m = try doc.historyItems().count
        _ = try r.neuralFilter(layer: layer, kind: .skinSmoothing, paramsJson: #"{"faces":[[5,5,20,20]]}"#, output: .newLayer)
        XCTAssertEqual(try doc.historyItems().count, m + 1)
        XCTAssertEqual(try doc.layers().count, 2)
    }

    func testTheStubNeedsTheEngine() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 32, height: 32, depth: .u8, profile: nil)
        defer { doc.close() }
        let r = try XCTUnwrap(doc as? any DocumentRetouchBackend)
        XCTAssertEqual(r.neuralFilterSpecs().count, 3)
        XCTAssertThrowsError(try r.contentAwareFill(layer: 1, paramsJson: "{}"))
        XCTAssertThrowsError(try r.neuralFilter(layer: 1, kind: .skinSmoothing, paramsJson: "{}", output: .newLayer))
        XCTAssertEqual(try r.retouchModels(), [])
    }
}
