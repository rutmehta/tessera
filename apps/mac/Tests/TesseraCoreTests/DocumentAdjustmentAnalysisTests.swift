import Foundation
import ImageIO
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-06: the frozen-parameter analysis (Swift ports of adjust/statistics.rs, hdr.rs and lookup.rs), the
/// controller's composite reads, menus and the Save As sheet's name logic.
@MainActor
final class DocumentAdjustmentAnalysisTests: XCTestCase {
    private func hist(_ counts: [Int: UInt64], bins: Int = 256) -> [UInt64] {
        (0..<bins).map { counts[$0] ?? 0 }
    }

    func testAutoToneStretchesEachChannelAndColorSetsGamma() {
        let h = [hist([64: 10, 192: 10]), hist([32: 10, 224: 10]), hist([0: 10, 255: 10])]
        let tone = AdjustmentAnalysis.auto(.tone, histograms: h, clip: 0)
        XCTAssertEqual(tone.black.map { ($0 * 255).rounded() }, [64, 32, 0])
        XCTAssertEqual(tone.white.map { ($0 * 255).rounded() }, [192, 224, 255])
        XCTAssertEqual(tone.gamma, [1, 1, 1])
        let contrast = AdjustmentAnalysis.auto(.contrast, histograms: h, clip: 0)
        XCTAssertEqual(contrast.black, [0, 0, 0], "pooled endpoints")
        XCTAssertEqual(contrast.white, [1, 1, 1])
        let skewed = [hist([0: 30, 64: 10, 255: 1]), hist([0: 1, 255: 1]), hist([0: 1, 255: 1])]
        let color = AdjustmentAnalysis.auto(.color, histograms: skewed, clip: 0)
        XCTAssertGreaterThan(color.gamma[0], 1, "a dark channel: its mean is lifted to 0.5 (t^(1/γ), γ > 1)")
        XCTAssertEqual(color.gamma[1], 1)
        let clipped = AdjustmentAnalysis.auto(.tone, histograms: [hist([0: 1, 100: 499, 150: 499, 200: 1]), hist([0: 1, 255: 1]), hist([0: 1, 255: 1])],
                                              clip: 0.005)
        XCTAssertEqual((clipped.black[0] * 255).rounded(), 100, "0.5 % tails are clipped")
        XCTAssertEqual((clipped.white[0] * 255).rounded(), 150)
        XCTAssertEqual(AdjustmentAnalysis.auto(.tone, histograms: [hist([:]), hist([:]), hist([:])], clip: 0).white, [1, 1, 1],
                       "an empty histogram is the identity")
        // M5-32: separate shadow / highlight clips (percent), stored with the result and round-tripped.
        let tails = [hist([0: 1, 100: 499, 150: 499, 200: 1]), hist([0: 1, 255: 1]), hist([0: 1, 255: 1])]
        let shadowOnly = AdjustmentAnalysis.auto(.tone, histograms: tails, shadowClip: 0.5, highlightClip: 0)
        XCTAssertEqual((shadowOnly.black[0] * 255).rounded(), 100)
        XCTAssertEqual((shadowOnly.white[0] * 255).rounded(), 200, "the highlight tail is kept")
        XCTAssertEqual(shadowOnly.shadowClip, 0.5)
        XCTAssertEqual(shadowOnly.highlightClip, 0)
        XCTAssertEqual(AdjustmentModel(json: AdjustmentModel.auto(shadowOnly).json), .auto(shadowOnly))
        XCTAssertEqual(AutoAdjustmentModel.fresh(.color).shadowClip, AutoAdjustmentModel.photoshopClip)
        XCTAssertEqual(AdjustmentModel(json: #"{"kind":"auto","mode":"tone","black":[0,0,0],"white":[1,1,1],"gamma":[1,1,1]}"#),
                       .auto(AutoAdjustmentModel()), "a pre-M5-32 Auto decodes with the engine's 0.5 % default clips")
        XCTAssertEqual(AutoAdjustmentModel().highlightClip, 0.5)
    }

    func testEqualizeMapsAreCDFMinNormalized() {
        let maps = AdjustmentAnalysis.equalizeMaps([hist([10: 1, 20: 1, 30: 2], bins: 32), hist([5: 4], bins: 32), hist([:], bins: 32)])
        XCTAssertEqual(maps[0][10], 0)
        XCTAssertEqual(maps[0][20], Double(Float(1.0 / 3)), accuracy: 1e-7)
        XCTAssertEqual(maps[0][31], 1)
        XCTAssertEqual(maps[1][31], 1, "a constant population maps linearly")
        XCTAssertEqual(maps[1][16], Double(Float(16.0 / 31)), accuracy: 1e-7)
        XCTAssertEqual(maps[2].count, 32)
    }

    func testLabStatisticsAndMatchColor() throws {
        let gray = Array(repeating: SIMD3<Float>(0.5, 0.5, 0.5), count: 4)
        let s = try XCTUnwrap(AdjustmentAnalysis.labStats(gray))
        XCTAssertEqual(s.mean[0], 53.39, accuracy: 0.05, "sRGB 0.5 is L* 53.4")
        XCTAssertEqual(s.mean[1], 0, accuracy: 0.01)
        XCTAssertEqual(s.std, [0, 0, 0])
        let warm = [SIMD3<Float>(0.9, 0.5, 0.3), SIMD3<Float>(0.7, 0.4, 0.2)]
        let m = try XCTUnwrap(AdjustmentAnalysis.matchColor(sourceLayer: 7, source: warm, target: gray, neutralize: false))
        XCTAssertEqual(m.sourceLayer, 7)
        XCTAssertGreaterThan(m.sourceMean[1], 5, "the warm source has positive a*")
        XCTAssertFalse(m.neutralize)
        let n = try XCTUnwrap(AdjustmentAnalysis.matchColor(sourceLayer: 7, source: warm, target: gray, neutralize: true, keeping: m))
        XCTAssertTrue(n.neutralize, "M5-32: Neutralize is the engine's persisted field")
        XCTAssertEqual(n.sourceMean, m.sourceMean, "the frozen source statistics are unchanged; the engine removes the chroma")
        XCTAssertEqual(AdjustmentModel(json: AdjustmentModel.matchColor(n).json), .matchColor(n), "Neutralize round-trips")
        XCTAssertNil(AdjustmentAnalysis.matchColor(sourceLayer: 0, source: warm, target: gray, neutralize: false), "the root is not a source")
        XCTAssertNil(AdjustmentAnalysis.matchColor(sourceLayer: 7, source: [], target: gray, neutralize: false))
    }

    func testBlackWhiteAutoMatchesLumaAndKeepsDefaultsForMissingHues() {
        XCTAssertEqual(AdjustmentAnalysis.blackWhiteAuto([SIMD3(0.5, 0.5, 0.5)]), BlackWhitePresets.default, "no colour: defaults")
        // Pure red: gray = slider/100 must equal its Rec. 601 luma, 0.299 → 30 %.
        let red = Array(repeating: SIMD3<Float>(1, 0, 0), count: 50)
        let s = AdjustmentAnalysis.blackWhiteAuto(red)
        XCTAssertEqual(s[0], 30)
        XCTAssertEqual(Array(s[2...4]), Array(BlackWhitePresets.default[2...4]), "absent hues keep the default")
    }

    func testHDREqualizeFreezesALuminanceMap() {
        let m = AdjustmentAnalysis.hdrEqualize([SIMD3(0.25, 0.25, 0.25), SIMD3(0.5, 0.5, 0.5), SIMD3(1, 1, 1)], bins: 8)
        XCTAssertEqual(m.method, .equalizeHistogram)
        XCTAssertEqual(m.equalizeMap.count, 8)
        XCTAssertEqual(m.equalizeMap.last, 1)
        XCTAssertEqual(m.equalizeMax, 1)
        XCTAssertEqual(m.equalizeMap, m.equalizeMap.sorted(), "monotone, as the engine validates")
    }

    func testCubeAnd3DLLoaders() throws {
        let cube = """
        TITLE "swap"
        # red and blue swapped
        LUT_3D_SIZE 2
        DOMAIN_MIN 0 0 0
        DOMAIN_MAX 1 1 1
        0 0 0
        0 0 1
        0 1 0
        0 1 1
        1 0 0
        1 0 1
        1 1 0
        1 1 1
        """
        let c = try ColorLookupFile.cube(cube)
        XCTAssertEqual(c.size, 2)
        XCTAssertEqual(Array(c.data[3..<6]), [0, 0, 1], "entry r=1 (red-fastest) holds blue")
        XCTAssertThrowsError(try ColorLookupFile.cube("LUT_1D_SIZE 4\n0 0 0"))
        XCTAssertThrowsError(try ColorLookupFile.cube("LUT_3D_SIZE 2\n0 0 0"), "too few entries")
        XCTAssertThrowsError(try ColorLookupFile.cube("LUT_3D_SIZE 2\nDOMAIN_MAX 2 2 2\n"))
        // 3DL: blue fastest in the file, 10-bit output.
        var lines = ["0 1023"]
        for r in 0..<2 { for g in 0..<2 { for b in 0..<2 { lines.append("\(r * 1023) \(g * 1023) \(b * 1023)") } } }
        let d = try ColorLookupFile.threeDL(lines.joined(separator: "\n"))
        XCTAssertEqual(d.size, 2)
        XCTAssertEqual(d.data, ColorLookupFile.identity(size: 2), "reordered to red-fastest and scaled by 1023")
        XCTAssertEqual(AdjustmentModel(json: AdjustmentModel.colorLookup(size: 2, data: d.data).json), .colorLookup(size: 2, data: d.data))
    }

    func testPhotoFilterPresetsMatchTheEngineSwatches() {
        XCTAssertEqual(PhotoFilterPreset.allCases.count, 20)
        XCTAssertEqual(PhotoFilterPreset.matching(PhotoFilterPreset.sepia.color), .sepia)
        XCTAssertNil(PhotoFilterPreset.matching([0.1, 0.2, 0.3]))
        guard case .photoFilter(let color, _, _) = AdjustmentModel.Kind.photoFilter.neutral else { return XCTFail() }
        XCTAssertEqual(PhotoFilterPreset.matching(color), .warming85, "Photoshop's default filter")
    }

    func testMenusListEveryKindOnceInPhotoshopOrder() {
        let layer = AdjustmentModel.Kind.layerMenuSections.flatMap { $0 }
        XCTAssertEqual(layer, AdjustmentModel.Kind.allCases)
        XCTAssertEqual(Set(layer).count, layer.count)
        XCTAssertEqual(Array(layer.prefix(4)), [.brightnessContrast, .levels, .curves, .exposure])
        let image = AdjustmentModel.Kind.imageMenuSections.flatMap { $0 }
        XCTAssertEqual(Set(image), Set(AdjustmentModel.Kind.allCases).subtracting([.auto]), "Auto is Image ▸ Auto Tone / Contrast / Color")
    }

    func testStubRendersTheNewPointwiseKinds() {
        let c = SIMD3<Float>(0.8, 0.4, 0.2)
        func run(_ m: AdjustmentModel) -> SIMD3<Float> { StubCompositor.Adjust(m).apply(c) }
        XCTAssertEqual(run(.desaturate), SIMD3(repeating: 0.5))
        XCTAssertEqual(run(.brightnessContrast(brightness: 0, contrast: 0, legacy: false)).x, 0.8, accuracy: 1e-5)
        let bw = run(.blackWhite(sliders: BlackWhitePresets.default, tint: nil))
        XCTAssertEqual(bw.x, bw.y)
        XCTAssertEqual(run(.colorLookup(size: 2, data: ColorLookupFile.identity(size: 2))).x, 0.8, accuracy: 1e-5)
        XCTAssertEqual(run(.gradientMap(.init())).x, StubCompositor.Adjust.luma(c), accuracy: 1e-5, "black → white maps luma")
        XCTAssertEqual(run(.equalize(maps: [[0, 1], [0, 1], [0, 1]])), c)
    }

    // MARK: Controller over the engine

    private func engineController() throws -> DocumentController {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("adj-analysis-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let e = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(e).newDocument(width: 64, height: 32, depth: .u8, profile: "sRGB IEC61966-2.1")
        return try DocumentController(backend: doc)
    }

    func testCompositeAnalysisNeutralizesTheLayerAndLeavesNoTrace() throws {
        let doc = try engineController()
        doc.addLayer(.fill(json: FillModel.solid(color: [0.25, 0.5, 0.75]).json))
        let fill = try XCTUnwrap(doc.primary?.id)
        doc.addAdjustment(.invert)
        let invert = try XCTUnwrap(doc.primary?.id)
        XCTAssertNotEqual(fill, invert)
        let historyBefore = try doc.backend.historyItems().count
        let inverted = doc.compositeSamples()
        XCTAssertFalse(inverted.isEmpty)
        XCTAssertEqual(inverted[0].x, 0.75, accuracy: 0.01)
        let below = doc.compositeSamples(neutralizing: invert)
        XCTAssertEqual(below[0].x, 0.25, accuracy: 0.01, "the image below the adjustment")
        XCTAssertEqual(doc.compositeSamples()[0].x, 0.75, accuracy: 0.01, "restored")
        doc.reloadModel()
        XCTAssertEqual(AdjustmentModel(json: doc.node(invert)?.adjustmentJson), .invert)
        XCTAssertEqual(try doc.backend.historyItems().count, historyBefore, "no history row")
    }

    func testNewImageDependentLayersAreAnalysed() throws {
        let doc = try engineController()
        doc.addLayer(.fill(json: FillModel.solid(color: [0.25, 0.5, 0.75]).json))
        doc.addAdjustment(.auto)
        guard case .auto(let a) = try XCTUnwrap(doc.adjustment(of: try XCTUnwrap(doc.primary?.id))) else { return XCTFail("auto") }
        XCTAssertEqual(a.black, [0, 0, 0], "a constant image has no range to stretch: identity")
        doc.addAdjustment(.equalize)
        guard case .equalize(let maps) = try XCTUnwrap(doc.adjustment(of: try XCTUnwrap(doc.primary?.id))) else { return XCTFail("eq") }
        XCTAssertEqual(maps.map(\.count), [256, 256, 256], "measured from the composite")
        XCTAssertEqual(doc.primary?.name, "Equalize 1")
    }

    func testLegacyMatchColorNeutralizeReopenDisableUndoAndSave() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("legacy-match-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let png = dir.appendingPathComponent("warm.png")
        let bytes: [UInt8] = (0..<16).flatMap { _ in [230, 128, 77, 255] }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(bytes) as CFData))
        let colorSpace = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        let image = try XCTUnwrap(CGImage(width: 4, height: 4, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: 16, space: colorSpace, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(png as CFURL, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let documents = EngineDocumentEngine.for(engine)
        let original = try DocumentController(backend: documents.openDocument(path: png.path))
        let source = try XCTUnwrap(original.layers.first(where: { $0.kind == .pixel })?.id)
        let sourcePixels = original.layerSamples(source)
        XCTAssertFalse(sourcePixels.isEmpty)
        var legacy = try XCTUnwrap(AdjustmentAnalysis.matchColor(sourceLayer: source, source: sourcePixels,
            target: sourcePixels, neutralize: false))
        legacy.sourceMean[1] = 0
        legacy.sourceMean[2] = 0
        var object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(AdjustmentModel.matchColor(legacy).json.utf8)) as? [String: Any])
        object.removeValue(forKey: "neutralize")
        let legacyJSON = String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
        original.addLayer(.adjustment(json: legacyJSON))
        let adjustment = try XCTUnwrap(original.primary?.id)
        let path = dir.appendingPathComponent("legacy.tessera-doc").path
        try original.backend.saveAs(path: path)
        original.close()
        let reopened = try DocumentController(backend: documents.openDocument(path: path))
        defer { reopened.close() }
        func match(_ doc: DocumentController) throws -> MatchColorModel {
            guard case .matchColor(let model) = try XCTUnwrap(doc.adjustment(of: adjustment)) else {
                throw NSError(domain: "ExpectedMatchColor", code: 1)
            }
            return model
        }
        let before = try match(reopened)
        XCTAssertTrue(before.neutralized, "legacy checkbox must reopen enabled")
        XCTAssertFalse(before.neutralize, "loading must not rewrite frozen parameters")
        XCTAssertEqual(before.sourceMean, legacy.sourceMean)
        let off = try XCTUnwrap(reopened.matchColorSettingNeutralize(before, to: false,
            target: reopened.compositeSamples(neutralizing: adjustment)))
        XCTAssertFalse(off.neutralized)
        XCTAssertGreaterThan(abs(off.sourceMean[1]) + abs(off.sourceMean[2]), 1, "disabling recovers warm source chroma")
        reopened.setAdjustment(adjustment, .matchColor(off), final: true)
        reopened.undo()
        XCTAssertEqual(try match(reopened), before, "undo restores the frozen legacy model")
        reopened.setAdjustment(adjustment, .matchColor(off), final: true)
        try reopened.backend.save()
        reopened.close()
        let saved = try DocumentController(backend: documents.openDocument(path: path))
        defer { saved.close() }
        XCTAssertEqual(try match(saved), off)
        XCTAssertFalse(try match(saved).neutralized)
    }

    func testLegacyMatchColorMissingSourceDoesNotPretendToDisable() throws {
        let doc = try engineController()
        var legacy = MatchColorModel()
        legacy.sourceLayer = 9999
        legacy.sourceMean = [45, 0, 0]
        XCTAssertNil(doc.matchColorSettingNeutralize(legacy, to: false, target: [SIMD3(repeating: 0.5)]))
        XCTAssertTrue(legacy.neutralized)
    }

    func testModernMatchColorToggleKeepsFrozenStatisticsWithoutReadingPixels() throws {
        let doc = try engineController()
        var modern = MatchColorModel()
        modern.sourceLayer = 9999
        modern.sourceMean = [45, 12, 18]
        modern.neutralize = true
        func unexpectedRead() -> [SIMD3<Float>] { XCTFail("modern toggle must not reanalyze"); return [] }
        let off = try XCTUnwrap(doc.matchColorSettingNeutralize(modern, to: false, target: unexpectedRead()))
        XCTAssertFalse(off.neutralized)
        XCTAssertEqual(off.sourceMean, modern.sourceMean)
        XCTAssertEqual(off.targetMean, modern.targetMean)
        let on = try XCTUnwrap(doc.matchColorSettingNeutralize(off, to: true, target: unexpectedRead()))
        XCTAssertEqual(on, modern)
    }

    // MARK: Save As sheet

    func testSaveAsNames() {
        XCTAssertEqual(SaveAsRequest.defaultName("Untitled-1", path: nil), "Untitled-1.tessera-doc")
        XCTAssertEqual(SaveAsRequest.defaultName("Poster.psd", path: "/x/Poster.psd"), "Poster.psd")
        XCTAssertEqual(SaveAsRequest.fileName("Poster"), "Poster.tessera-doc")
        XCTAssertEqual(SaveAsRequest.fileName("Poster.PSB"), "Poster.PSB")
        XCTAssertEqual(SaveAsRequest.format(of: "Poster.psd"), .psd)
        XCTAssertEqual(SaveAsRequest.renamed("Poster.tessera-doc", to: .psd), "Poster.psd")
        XCTAssertEqual(SaveAsRequest.renamed("Poster.v2", to: .psb), "Poster.v2.psb")
    }
}
