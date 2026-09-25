import CoreGraphics
import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Export and print (M2-20): naming templates (parity with the engine), settings JSON, preset
/// persistence through the engine, and the page-layout maths behind contact sheets.
final class ExportPrintTests: XCTestCase {
    private var scratch: URL!

    override func setUpWithError() throws {
        scratch = FileManager.default.temporaryDirectory.appendingPathComponent("tessera-export-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: scratch)
    }

    // MARK: Naming templates

    func testNamingTokensAndProblems() {
        XCTAssertEqual(try ExportNaming.fileName(template: "{date}_{name}-{seq}", name: "DSC_0042", sequence: 7,
                                                 date: "2026-09-25", extension: "jpg").get(),
                       "2026-09-25_DSC_0042-7.jpg")
        XCTAssertEqual(try ExportNaming.fileName(template: "Wedding {seq}", name: "x", sequence: 12, date: "",
                                                 extension: "tif").get(), "Wedding 12.tif")
        let problems: [(String, ExportNaming.Problem)] = [
            ("{name", .unclosedToken), ("{nme}", .unknownToken("{nme}")), ("", .unsafeName),
            ("..", .unsafeName), ("a/b", .unsafeName), ("c:d", .unsafeName), ("tab\t", .unsafeName),
        ]
        for (template, expected) in problems {
            guard case .failure(let problem) = ExportNaming.fileName(template: template, name: "n", sequence: 1,
                                                                     date: "", extension: "jpg") else {
                return XCTFail("\(template) should fail")
            }
            XCTAssertEqual(problem, expected, template)
        }
        XCTAssertEqual(ExportNaming.dateToken(Date(timeIntervalSince1970: 1_790_000_000),
                                              calendar: { var c = Calendar(identifier: .gregorian); c.timeZone = TimeZone(identifier: "UTC")!; return c }()),
                       "2026-09-21")
    }

    /// The sheet's live example must agree with what the engine will write.
    func testNamingMatchesTheEngine() {
        let templates = ["{name}", "{name}-{seq}", "{date} {name}", "Print_{seq}_{name}", "{name}{", "{bad}",
                         "../{name}", "x\\y", "{seq}", ".", "..", "ok.{name}", "é {name} ✓"]
        for template in templates {
            for (name, seq, date) in [("IMG_0001", 1, "2026-01-02"), ("DSC 7", 12, "")] {
                let swift = try? ExportNaming.fileName(template: template, name: name, sequence: seq, date: date,
                                                       extension: "jpg").get()
                let engine = try? exportFilename(template: template, name: name, sequence: UInt32(seq), date: date,
                                                 extension: "jpg")
                XCTAssertEqual(swift, engine, "template \(template)")
            }
        }
        XCTAssertEqual(ExportNaming.example(template: "{name}", firstName: "A", date: Date(), format: .tiff, count: 3), "A.tif, …")
        XCTAssertTrue(ExportNaming.example(template: "same", firstName: "A", date: Date(), format: .jpeg, count: 3)
            .contains("add {seq}"))
    }

    // MARK: Settings JSON

    func testSettingsRoundTripThroughTheEngine() throws {
        var s = ExportSettings()
        s.format = .tiff
        s.bitDepth = 16
        s.colorSpace = .prophoto
        s.resize.mode = .longEdge
        s.resize.unit = .in
        s.resize.longEdge = 12
        s.dpi = 300
        s.sharpening = .glossy
        s.naming = "{name}-{seq}"
        s.destination = "/tmp/out"
        s.onConflict = .skip
        // The engine accepts exactly what Swift writes (it rejects unknown keys) …
        let normalized = try normalizeExportSettings(json: s.json)
        // … and Swift reads back exactly what the engine writes.
        XCTAssertEqual(try ExportSettings(json: normalized), s)
        // Missing keys take defaults, like the engine's serde defaults.
        let partial = try ExportSettings(json: #"{"format":"png","resize":{"mode":"percent"}}"#)
        XCTAssertEqual(partial.format, .png)
        XCTAssertEqual(partial.resize.percent, 100)
        XCTAssertEqual(partial.quality, 90)
        XCTAssertEqual(partial.naming, "{name}")
        // Invalid combinations are the engine's to reject.
        var bad = ExportSettings()
        bad.bitDepth = 16
        XCTAssertThrowsError(try normalizeExportSettings(json: bad.json))
        bad.normalizeForFormat()
        XCTAssertNoThrow(try normalizeExportSettings(json: bad.json))
    }

    func testOutputSizes() {
        var s = ExportSettings()
        XCTAssertTrue(s.outputSize(width: 6000, height: 4000)! == (6000, 4000))
        s.resize.mode = .longEdge
        s.resize.longEdge = 2048
        XCTAssertTrue(s.outputSize(width: 6000, height: 4000)! == (2048, 1365))
        XCTAssertTrue(s.outputSize(width: 4000, height: 6000)! == (1365, 2048))
        s.resize.unit = .in
        s.resize.longEdge = 12
        s.dpi = 300
        XCTAssertTrue(s.outputSize(width: 6000, height: 4000)! == (3600, 2400))
        s.resize.unit = .cm
        s.resize.longEdge = 2.54
        s.dpi = 100
        XCTAssertTrue(s.outputSize(width: 300, height: 200)! == (100, 67))
        s.resize = .init()
        s.resize.mode = .fit
        s.resize.width = 1000
        s.resize.height = 1000
        XCTAssertTrue(s.outputSize(width: 3000, height: 2000)! == (1000, 667))
        s.resize.mode = .percent
        s.resize.percent = 50
        s.upscale = 2
        XCTAssertTrue(s.outputSize(width: 3000, height: 2000)! == (3000, 2000))
        XCTAssertNil(s.outputSize(width: 0, height: 10))
        XCTAssertEqual(ExportSettings().summary, "Full size · JPEG 90 · sRGB · 72 dpi")
    }

    // MARK: Presets

    func testPresetsPersistUnderTheAppDirectory() throws {
        let support = scratch.appendingPathComponent("support").path
        var store = ExportPresetStore(engine: try Engine.open(appSupportDir: support))
        let shipped = try store.list()
        XCTAssertEqual(shipped.map(\.name), ["Web 2048 sRGB", "Full-size JPEG", "16-bit TIFF ProPhoto", "Print 300 dpi"])
        XCTAssertEqual(shipped[0].settings.resize.longEdge, 2048)
        XCTAssertEqual(shipped[0].settings.quality, 85)
        XCTAssertEqual(shipped[2].settings.bitDepth, 16)
        XCTAssertEqual(shipped[2].settings.colorSpace, .prophoto)
        XCTAssertEqual(shipped[3].settings.dpi, 300)
        XCTAssertEqual(shipped[3].settings.resize.unit, .in)

        var mine = shipped[0].settings
        mine.quality = 70
        mine.destination = "/Users/someone/Desktop"
        try store.save("Client proofs", mine)
        try store.delete("Full-size JPEG")
        try store.rename("Print 300 dpi", to: "Lab 8x10")
        XCTAssertThrowsError(try store.rename("Lab 8x10", to: "Client proofs"))
        XCTAssertThrowsError(try store.save("", mine))

        // A new engine on the same directory (a relaunch) sees the same presets.
        store = ExportPresetStore(engine: try Engine.open(appSupportDir: support))
        let reloaded = try store.list()
        XCTAssertEqual(reloaded.map(\.name), ["Web 2048 sRGB", "16-bit TIFF ProPhoto", "Client proofs", "Lab 8x10"])
        let client = try XCTUnwrap(reloaded.first { $0.name == "Client proofs" })
        XCTAssertEqual(client.settings.quality, 70)
        XCTAssertEqual(client.settings.destination, "", "presets describe files, not folders")
        let files = try FileManager.default.contentsOfDirectory(atPath: support + "/ExportPresets").filter { $0.hasSuffix(".json") }
        XCTAssertEqual(files.count, 4)
        try store.restoreDefaults()
        XCTAssertEqual(try store.list().count, 6)
    }

    // MARK: Print layout

    private let letter = CGSize(width: 612, height: 792)

    func testContactSheetGrid() {
        var layout = PrintLayout()
        layout.style = .contactSheet
        layout.rows = 5
        layout.columns = 4
        layout.spacing = 9
        let cells = layout.cells(page: letter)
        XCTAssertEqual(cells.count, 20)
        // (612 - 72 - 3·9) / 4 = 128.25 wide; (792 - 72 - 4·9) / 5 = 136.8 high.
        XCTAssertEqual(cells[0], CGRect(x: 36, y: 36, width: 128.25, height: 136.8))
        XCTAssertEqual(cells[1].minX, 36 + 128.25 + 9, accuracy: 1e-9)
        XCTAssertEqual(cells[4].minY, 36 + 136.8 + 9, accuracy: 1e-9)
        XCTAssertEqual(cells[19].maxX, 612 - 36, accuracy: 1e-9)
        XCTAssertEqual(cells[19].maxY, 792 - 36, accuracy: 1e-9)
        XCTAssertEqual(layout.pageCount(images: 45, page: letter), 3)
        XCTAssertEqual(layout.pages(images: 45, page: letter).map(\.count), [20, 20, 5])
        XCTAssertEqual(layout.pageCount(images: 0, page: letter), 0)
        // Captions take a band under each picture.
        let area = layout.imageArea(in: cells[0])
        XCTAssertEqual(area.height, 136.8 - PrintLayout.captionHeight, accuracy: 1e-9)
        XCTAssertEqual(layout.captionArea(in: cells[0])?.minY ?? 0, area.maxY, accuracy: 1e-9)
        layout.captions = false
        XCTAssertEqual(layout.imageArea(in: cells[0]), cells[0])
        XCTAssertNil(layout.captionArea(in: cells[0]))
        // Margins that leave no room give no cells (and no pages).
        layout.marginLeft = 400
        layout.marginRight = 400
        XCTAssertTrue(layout.cells(page: letter).isEmpty)
        XCTAssertEqual(layout.pageCount(images: 3, page: letter), 0)
    }

    func testSingleAndCustomCells() {
        var layout = PrintLayout()
        XCTAssertEqual(layout.cells(page: letter), [CGRect(x: 36, y: 36, width: 540, height: 720)])
        XCTAssertEqual(layout.pageCount(images: 3, page: letter), 3)
        layout.style = .custom
        layout.cellWidth = 4 * 72
        layout.cellHeight = 6 * 72
        layout.spacing = 0
        layout.marginTop = 18; layout.marginBottom = 18; layout.marginLeft = 18; layout.marginRight = 18
        // Landscape letter (11 × 8.5 in) fits two 4 × 6 portrait cells side by side, centred.
        let landscape = CGSize(width: 792, height: 612)
        let cells = layout.cells(page: landscape)
        XCTAssertEqual(cells.count, 2)
        XCTAssertEqual(cells[0].width, 288)
        XCTAssertEqual(cells[0].minX, (792 - 576) / 2, accuracy: 1e-9)
        XCTAssertEqual(cells[0].minY, (612 - 432) / 2, accuracy: 1e-9)
        // A cell larger than the page does not fit at all.
        layout.cellWidth = 20 * 72
        XCTAssertTrue(layout.cells(page: landscape).isEmpty)
    }

    func testPlacementRotatesToFitAndSizesRenders() {
        var layout = PrintLayout()
        let portraitCell = CGRect(x: 0, y: 0, width: 400, height: 600)
        // A 3:2 landscape picture in a 2:3 portrait cell: turned, it fills the cell.
        let turned = layout.place(aspect: 1.5, in: portraitCell)
        XCTAssertTrue(turned.rotated)
        XCTAssertEqual(turned.frame, portraitCell)
        XCTAssertEqual(turned.pictureSize, CGSize(width: 600, height: 400))
        layout.rotateToFit = false
        let upright = layout.place(aspect: 1.5, in: portraitCell)
        XCTAssertFalse(upright.rotated)
        XCTAssertEqual(upright.frame.width, 400, accuracy: 1e-9)
        XCTAssertEqual(upright.frame.height, 400 / 1.5, accuracy: 1e-9)
        XCTAssertEqual(upright.frame.midY, 300, accuracy: 1e-9)
        // Same orientation never turns.
        layout.rotateToFit = true
        XCTAssertFalse(layout.place(aspect: 0.8, in: portraitCell).rotated)
        // 600 × 400 pt at 300 dpi = 2500 × 1667 px (the picture's own axes).
        XCTAssertTrue(PrintLayout.pixels(for: turned, dpi: 300) == (2500, 1667))
        var settings = PrintSettings()
        settings.dpi = 360
        XCTAssertTrue(settings.renderBox(for: portraitCell) == (3000, 3000))
    }

    func testPrintSettingsPersist() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "tessera-print-\(UUID().uuidString)"))
        XCTAssertEqual(PrintSettings.load(defaults), PrintSettings())
        var s = PrintSettings()
        s.layout.style = .contactSheet
        s.layout.rows = 7
        s.colorHandling = .application
        s.profilePath = "/Library/ColorSync/Profiles/Paper.icc"
        s.save(defaults)
        XCTAssertEqual(PrintSettings.load(defaults), s)
    }
}
