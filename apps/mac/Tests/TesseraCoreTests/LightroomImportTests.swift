import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Lightroom import (M2-13b): the mapping tables behind the sheet, the fidelity grid's sort and
/// "looks different" filter, and the Markdown report written next to library.json.
final class LightroomImportTests: XCTestCase {
    private func options(library: String = "/Volumes/Old Drive/Photos") -> LrcatOptions {
        LrcatOptions(libraryFolder: library,
                     relocations: [LrcatRelocation(from: "/Volumes/Old Drive/Photos/", to: "/Volumes/Old Drive/Photos"),
                                   LrcatRelocation(from: "/Users/me/Pictures/", to: "/Users/me/Pictures")],
                     marks: [], overwriteExistingEdits: false)
    }

    func testMarkTableMapsKeepsAndDropsLabels() {
        var table = MarkMappingTable(rows: [
            LrcatMarkRow(label: "Red", mark: "Red", count: 2),
            LrcatMarkRow(label: "Client", mark: "Client", count: 1),
            LrcatMarkRow(label: "Blue", mark: "", count: 4),
        ])
        XCTAssertEqual(table.appMarks, ["Needs Retouch", "Client Favourite", "Print", "Review"])
        XCTAssertEqual(table.rows.map(\.choice), [.keepLabel, .keepLabel, .drop])
        XCTAssertEqual(table.unnamedMarks, ["Red", "Client"])
        XCTAssertEqual(table.markedPhotos, 3)

        table.set("Red", to: .mark("Needs Retouch"))
        table.set("Client", to: .mark("Needs Retouch"))
        table.set("Blue", to: .keepLabel)
        table.set("Nope", to: .drop)   // unknown labels are ignored
        XCTAssertEqual(table.mark(for: "Red"), "Needs Retouch")
        XCTAssertEqual(table.mark(for: "Blue"), "Blue")
        XCTAssertEqual(table.mergedTargets, ["Needs Retouch"])
        XCTAssertEqual(table.unnamedMarks, ["Blue"])
        XCTAssertEqual(table.mappings, [
            LrcatMarkMapping(label: "Red", mark: "Needs Retouch"),
            LrcatMarkMapping(label: "Client", mark: "Needs Retouch"),
            LrcatMarkMapping(label: "Blue", mark: "Blue"),
        ])
        table.set("Client", to: .drop)
        XCTAssertEqual(table.mappings[1].mark, "", "an empty mark tells the engine to drop the label")
        XCTAssertEqual(table.markedPhotos, 6)
    }

    func testFolderTableRelocatesRootsAndMovesTheLibraryWithThem() {
        var table = FolderMappingTable(options: options())
        XCTAssertEqual(table.roots.map(\.catalogPath), ["/Volumes/Old Drive/Photos/", "/Users/me/Pictures/"])
        XCTAssertFalse(table.roots.contains { table.isRelocated($0) })

        // The library folder was the moved root: it follows.
        table.relocate("/Volumes/Old Drive/Photos/", to: "/Volumes/New Drive/Archive/Photos/")
        XCTAssertEqual(table.roots[0].path, "/Volumes/New Drive/Archive/Photos")
        XCTAssertTrue(table.isRelocated(table.roots[0]))
        XCTAssertEqual(table.libraryFolder, "/Volumes/New Drive/Archive/Photos")
        // Relocating another root leaves an unrelated library folder alone.
        table.relocate("/Users/me/Pictures/", to: "/Users/me/Moved")
        XCTAssertEqual(table.libraryFolder, "/Volumes/New Drive/Archive/Photos")
        XCTAssertEqual(table.relocations.map(\.to), ["/Volumes/New Drive/Archive/Photos", "/Users/me/Moved"])

        // A library folder inside the root keeps its relative place; a sibling prefix does not match.
        var nested = FolderMappingTable(options: options(library: "/Volumes/Old Drive/Photos/2026"))
        nested.relocate("/Volumes/Old Drive/Photos/", to: "/Photos")
        XCTAssertEqual(nested.libraryFolder, "/Photos/2026")
        var sibling = FolderMappingTable(options: options(library: "/Volumes/Old Drive/Photos2"))
        sibling.relocate("/Volumes/Old Drive/Photos/", to: "/Photos")
        XCTAssertEqual(sibling.libraryFolder, "/Volumes/Old Drive/Photos2")

        XCTAssertEqual(table.displayName("/Volumes/New Drive/Archive/Photos/2026/wedding"), "Photos/2026/wedding")
        XCTAssertEqual(table.displayName("/Volumes/New Drive/Archive/Photos"), "Photos")
        XCTAssertEqual(table.displayName("/Elsewhere/x"), "/Elsewhere/x")
        table.reset("/Volumes/Old Drive/Photos/")
        XCTAssertFalse(table.isRelocated(table.roots[0]))
        XCTAssertEqual(table.libraryFolder, "/Volumes/Old Drive/Photos")
        let built = table.options(marks: [LrcatMarkMapping(label: "Red", mark: "")], overwrite: true)
        XCTAssertEqual(built.libraryFolder, table.libraryFolder)
        XCTAssertEqual(built.marks.count, 1)
        XCTAssertTrue(built.overwriteExistingEdits)
    }

    func testFolderTableTakesImageCountsFromThePlan() {
        var table = FolderMappingTable(options: options())
        let preview = plan(roots: [LrcatRootRow(catalogPath: "/Users/me/Pictures/", path: "/Users/me/Pictures", exists: true, images: 7, missing: 1)])
        table.updateCounts(from: preview)
        XCTAssertEqual(table.roots.map(\.images), [0, 7])
    }

    func testSelectionText() {
        XCTAssertEqual(SelectionText.tessera(.reject, grade: nil), "Reject")
        XCTAssertEqual(SelectionText.tessera(.undecided, grade: nil), "Undecided")
        XCTAssertEqual(SelectionText.tessera(.keep, grade: nil), "Keep")
        XCTAssertEqual(SelectionText.tessera(.keep, grade: 3), "Keep · Grade 3 (Best)")
    }

    private func sample(_ name: String, _ mean: Float, _ p95: Float, _ status: LrcatFidelityStatus = .compared) -> LrcatFidelitySample {
        LrcatFidelitySample(catalogId: Int64(name.hashValue & 0xffff), name: name, path: "/p/\(name)", status: status,
                            message: status == .compared ? "" : "Lightroom has no cached preview for this photo",
                            deltaEMean: mean, deltaEP95: p95, lightroomJpeg: Data(), tesseraJpeg: Data())
    }

    func testFidelityGridSortsAndFiltersLooksDifferent() {
        var grid = FidelityGrid(samples: [
            sample("b.jpg", 2.1, 4.6), sample("a.jpg", 5.6, 12.7), sample("c.jpg", 1.7, 11.0),
            sample("d.jpg", 0, 0, .noPreview),
        ])
        XCTAssertEqual(grid.visible.map(\.name), ["a.jpg", "b.jpg", "c.jpg", "d.jpg"])
        grid.sort = .smallestDifference
        XCTAssertEqual(grid.visible.map(\.name), ["c.jpg", "b.jpg", "a.jpg", "d.jpg"], "uncompared samples last")
        grid.sort = .name
        XCTAssertEqual(grid.visible.map(\.name), ["a.jpg", "b.jpg", "c.jpg", "d.jpg"])
        // Mean ≥ 3 or p95 ≥ 10: a (mean) and c (a bad region), never an uncompared sample.
        XCTAssertEqual(grid.differentCount, 2)
        grid.onlyDifferent = true
        grid.sort = .largestDifference
        XCTAssertEqual(grid.visible.map(\.name), ["a.jpg", "c.jpg"])
        XCTAssertEqual(grid.meanOfMeans ?? 0, (2.1 + 5.6 + 1.7) / 3, accuracy: 1e-4)
        XCTAssertNil(FidelityGrid(samples: [sample("x", 0, 0, .failed)]).meanOfMeans)
    }

    private func plan(roots: [LrcatRootRow] = []) -> LrcatPlanPreview {
        LrcatPlanPreview(roots: roots, folders: [], selectionRows: [], selection: LrcatSelectionCounts(
            rejects: 0, keeps: 0, undecided: 0, grade1: 0, grade2: 0, grade3: 0, marked: 0),
                         marks: [], keywords: [], toImport: 0, missing: 0, virtualCopies: 0, conflicts: 0, skipped: [],
                         outsideLibrary: 0, libraryPath: "", libraryExists: false, unsupported: [], estimatedBytes: 0)
    }

    private func report(folder: String, cancelled: Bool = false) -> LrcatReport {
        LrcatReport(catalogPath: "/Lr/Fixture.lrcat", cancelled: cancelled, imported: 4, resumed: 1, virtualCopies: 1,
                    skipped: [LrcatSkip(name: "lost-01.jpg", path: "/Photos/2026/portraits/lost-01.jpg",
                                        reason: "original not found (relocate its folder if the drive moved)"),
                              LrcatSkip(name: "a|b*.jpg", path: "/Photos/a|b*.jpg", reason: "already has Tessera edits; kept them")],
                    unsupported: [LrcatIssue(category: "Develop settings", reason: "crs:FutureKnob: unknown key; source preserved",
                                             count: 1, examples: ["ceremony-01.jpg"]),
                                  LrcatIssue(category: "Smart collections", reason: "rule is kept | cannot run", count: 1, examples: ["Blue label"])],
                    albums: 2, albumGroups: 1, smartAlbums: 2, keywords: 6,
                    selection: LrcatSelectionCounts(rejects: 1, keeps: 3, undecided: 1, grade1: 1, grade2: 1, grade3: 1, marked: 2),
                    libraryPath: folder + "/library.json", bundlePath: folder + "/.tessera-import/Fixture-1234abcd",
                    indexed: 5, seconds: 0.42)
    }

    func testReportMarkdownListsImportedSkippedAndUnsupported() throws {
        var opts = options()
        opts.relocations[0].to = "/Photos"
        opts.marks = [LrcatMarkMapping(label: "Red", mark: "Needs Retouch"), LrcatMarkMapping(label: "Client", mark: ""),
                      LrcatMarkMapping(label: "Blue", mark: "Blue")]
        let fidelity = LrcatFidelity(renderer: "native", previewsAvailable: true,
                                     samples: [sample("portrait-01.jpg", 5.52, 12.7), sample("ceremony-02.jpg", 1.9, 4.4),
                                               sample("x.jpg", 0, 0, .noPreview)])
        let date = ISO8601DateFormatter().date(from: "2026-09-25T10:00:00Z")!
        let md = LightroomImportReport.markdown(report: report(folder: "/Photos"), options: opts, fidelity: fidelity, date: date)
        let lines = md.components(separatedBy: "\n")
        XCTAssertEqual(lines.first, "# Lightroom import report")
        XCTAssertTrue(md.contains("- Catalog: `/Lr/Fixture.lrcat`"))
        XCTAssertTrue(md.contains("- Status: complete in 0.4 s"))
        XCTAssertTrue(lines.contains("| Photos with edits and selections written | 4 |"))
        XCTAssertTrue(lines.contains("| Photos already imported by an earlier run | 1 |"))
        XCTAssertTrue(lines.contains("| Albums | 2 |") && lines.contains("| Keywords added | 6 |"))
        XCTAssertTrue(md.contains("Selection: 3 Keep (grade 1: 1, grade 2: 1, grade 3: 1), 1 Reject, 1 Undecided; 2 marked."))
        XCTAssertTrue(md.contains("Colour labels: Red → Needs Retouch, Client → (dropped), Blue → Blue."))
        XCTAssertTrue(md.contains("Relocated folders: `/Volumes/Old Drive/Photos/` → `/Photos`."), "unmoved roots are not listed")
        XCTAssertFalse(md.contains("`/Users/me/Pictures/` →"))
        // Skipped: every photo with its reason (sorted by reason), plus the virtual copies line.
        XCTAssertTrue(md.contains("## Skipped (3)"))
        let skipped = lines.filter { $0.hasPrefix("- **") }
        XCTAssertEqual(skipped, [
            "- **a|b\\*.jpg** (`/Photos/a|b*.jpg`): already has Tessera edits; kept them",
            "- **lost-01.jpg** (`/Photos/2026/portraits/lost-01.jpg`): original not found (relocate its folder if the drive moved)",
        ])
        XCTAssertTrue(md.contains("- 1 virtual copy: preserved in the import bundle"))
        // Unsupported as a table with escaped pipes.
        XCTAssertTrue(lines.contains("| Smart collections | rule is kept \\| cannot run | 1 | Blue label |"))
        XCTAssertTrue(lines.contains("| Develop settings | crs:FutureKnob: unknown key; source preserved | 1 | ceremony-01.jpg |"))
        // Fidelity, largest difference first, uncompared last.
        XCTAssertTrue(md.contains("## Fidelity preview (native renderer)"))
        let fidelityRows = lines.filter { $0.hasPrefix("| portrait") || $0.hasPrefix("| ceremony-02") || $0.hasPrefix("| x.jpg") }
        XCTAssertEqual(fidelityRows, [
            "| portrait-01.jpg | 5.5 | 12.7 | looks different |",
            "| ceremony-02.jpg | 1.9 | 4.4 |  |",
            "| x.jpg | – | – | Lightroom has no cached preview for this photo |",
        ])
        XCTAssertTrue(md.contains("only read, never written"))
        XCTAssertTrue(md.hasSuffix("\n"))
    }

    func testCancelledReportSaysHowToResumeAndIsWrittenBesideLibraryJSON() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("lr-report-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: folder) }
        let r = report(folder: folder.path, cancelled: true)
        let md = LightroomImportReport.markdown(report: r)
        XCTAssertTrue(md.contains("**Status: cancelled.** Run the import again with the same settings to resume"))
        XCTAssertFalse(md.contains("Fidelity preview"))
        let url = try LightroomImportReport.write(md, report: r)
        XCTAssertEqual(url.lastPathComponent, "import-report.md")
        XCTAssertEqual(url.deletingLastPathComponent().standardizedFileURL, folder.standardizedFileURL)
        XCTAssertEqual(try String(contentsOf: url, encoding: .utf8), md)
    }

    func testBridgeRejectsANonCatalog() throws {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("not-a-catalog-\(UUID().uuidString).lrcat")
        try Data("hello".utf8).write(to: file)
        addTeardownBlock { try? FileManager.default.removeItem(at: file) }
        XCTAssertThrowsError(try inspectLrcat(path: file.path)) { error in
            XCTAssertTrue("\(error)".lowercased().contains("lrcat") || "\(error)".lowercased().contains("database"), "\(error)")
        }
        XCTAssertEqual(try Data(contentsOf: file), Data("hello".utf8), "the file is only read")
    }
}
