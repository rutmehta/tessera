import XCTest
@testable import Tessera
@testable import TesseraCore

/// Deliver (redesign SPEC §5, WP UX-15): the How rows' settings, the size estimate, the agent
/// field's phrases, and the mode on a stub library.
final class DeliverTests: XCTestCase {
    func testHowRowsPreferTheShippedPresetsAndFallBackToTheSameValues() {
        var web = ExportSettings()
        web.quality = 70
        web.resize.mode = .longEdge
        web.resize.longEdge = 1600
        let presets = [ExportPresetEntry(name: "Web 2048 sRGB", settings: web)]
        XCTAssertEqual(DeliverHow.web.settings(presets: presets).resize.longEdge, 1600, "the shipped preset wins when present")
        XCTAssertEqual(DeliverHow.web.settings(presets: []).resize.longEdge, 2048)
        XCTAssertEqual(DeliverHow.web.settings(presets: []).format, .jpeg)

        let instagram = DeliverHow.instagram.settings(presets: presets)
        XCTAssertNil(DeliverHow.instagram.presetName, "Instagram is built in")
        XCTAssertEqual(instagram.resize.mode, .fit)
        XCTAssertEqual(instagram.outputSize(width: 6000, height: 4000).map { [$0.width, $0.height] }, [1080, 720])
        XCTAssertEqual(instagram.outputSize(width: 4000, height: 6000).map { [$0.width, $0.height] }, [900, 1350])

        let full = DeliverHow.fullSize.settings(presets: [])
        XCTAssertEqual(full.format, .tiff)
        XCTAssertEqual(full.bitDepth, 16)
        XCTAssertEqual(full.outputSize(width: 6000, height: 4000).map { [$0.width, $0.height] }, [6000, 4000])
        XCTAssertEqual(DeliverHow.print.settings(presets: []).resize.unit, .in)
        for how in DeliverHow.allCases { XCTAssertTrue(how.settings(presets: []).destination.isEmpty, "\(how): the folder is the person's") }
    }

    func testSizeEstimateFollowsFormatQualityAndLimit() {
        let web = DeliverHow.web.settings(presets: [])
        let webBytes = web.estimatedBytes(width: 6000, height: 4000)
        XCTAssertGreaterThan(webBytes, 500_000)
        XCTAssertLessThan(webBytes, 1_500_000, "a 2048 px JPEG is under 1.5 MB")
        let tiff = DeliverHow.fullSize.settings(presets: [])
        XCTAssertGreaterThan(tiff.estimatedBytes(width: 6000, height: 4000), 100_000_000, "16-bit TIFF at 24 MP is over 100 MB")
        var limited = web
        limited.maxFileBytes = 300_000
        XCTAssertEqual(limited.estimatedBytes(width: 6000, height: 4000), 300_000, "the JPEG size limit caps the estimate")
        XCTAssertEqual(web.estimatedBytes(width: 0, height: 0), 0, "unknown size: nothing to estimate")
    }

    func testAgentPhrasesMapToTheRows() {
        XCTAssertEqual(DeliverAgentPhrase.parse("export the keeps for Instagram"), DeliverAgentPhrase(how: .instagram, what: .keeps))
        XCTAssertEqual(DeliverAgentPhrase.parse("Export the keeps as web JPEGs"), DeliverAgentPhrase(how: .web, what: .keeps))
        XCTAssertEqual(DeliverAgentPhrase.parse("full size tiffs of everything"), DeliverAgentPhrase(how: .fullSize, what: .shoot))
        XCTAssertEqual(DeliverAgentPhrase.parse("print the selected ones"), DeliverAgentPhrase(how: .print, what: .selection))
        XCTAssertEqual(DeliverAgentPhrase.parse("whole shoot"), DeliverAgentPhrase(how: nil, what: .shoot), "only a What: the How stays")
        XCTAssertEqual(DeliverAgentPhrase.parse("instagram"), DeliverAgentPhrase(how: .instagram, what: nil), "only a How: the What stays")
        XCTAssertNil(DeliverAgentPhrase.parse("make them look warmer"))
    }

    @MainActor
    func testDeliverModeOnAStubLibrary() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.selectShellMode(.deliver)
        XCTAssertEqual(model.shellMode, .deliver)
        XCTAssertTrue(model.deliverMode)
        XCTAssertFalse(model.isCullWorkspace, "Deliver covers Cull")
        // Stub items have no engine pixels: every What row is empty and says so in one sentence.
        for what in DeliverWhat.allCases { XCTAssertTrue(model.deliverItems(what).isEmpty, "\(what)") }
        model.setDeliverWhat(.keeps)
        XCTAssertEqual(model.deliverEmptyMessage, "No keeps yet. Cull first, or pick Selection or Whole shoot.")
        XCTAssertNil(model.deliverEstimate)
        XCTAssertEqual(model.deliverHeadline, "0 photos · \(model.deliverHow.title) · \(model.deliverHow.subtitle)")

        // The agent field picks the rows, then reports why nothing ran (no engine).
        model.submitAgentInstruction("export the keeps for instagram")
        XCTAssertEqual(model.deliverHow, .instagram)
        XCTAssertEqual(model.deliverWhat, .keeps)
        XCTAssertEqual(model.statusMessage, "Export needs a folder opened on the engine")
        model.submitAgentInstruction("make them look warmer")
        XCTAssertEqual(model.statusMessage, "Agent: make them look warmer", "unknown text is reported")

        // A finished result is cleared by changing a row and by leaving Deliver.
        model.deliverResult = DeliverResult(files: [], folder: URL(fileURLWithPath: "/tmp"), failed: 0)
        model.setDeliverHow(.web)
        XCTAssertNil(model.deliverResult)
        model.deliverResult = DeliverResult(files: [], folder: URL(fileURLWithPath: "/tmp"), failed: 0)
        model.selectShellMode(.cull)
        XCTAssertEqual(model.shellMode, .cull)
        XCTAssertNil(model.deliverResult)
    }
}
