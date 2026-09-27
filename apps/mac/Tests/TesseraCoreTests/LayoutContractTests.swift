import AppKit
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

/// Shared width and paint contracts (WP M2-56, layout audit D03–D10): exact rectangles, no windows.
@MainActor
final class LayoutContractTests: XCTestCase {
    private func fit<V: View>(_ view: V, width: CGFloat) -> CGSize {
        NSHostingController(rootView: view).sizeThatFits(in: CGSize(width: width, height: 2000))
    }

    // MARK: ValueSlider (D03, D08)

    /// "Highlights" and the widest signed readout at the three-way grading widths: at 80 pt (inspector
    /// minimum) the value keeps its full width, the title stops a gap short of it (and truncates);
    /// at 110 pt (inspector maximum) the whole title fits.
    func testValueSliderTitleAndValueNeverTouch() {
        let title = "Highlights" as NSString
        let titleWidth = ceil(title.size(withAttributes: [.font: Theme.NSFonts.caption]).width)
        for value in ["-100", "+100", "0", "+2.50"] {
            let valueWidth = ceil((value as NSString).size(withAttributes: [.font: Theme.NSFonts.captionNumericMedium]).width)
            for width in [80.0, 82.67, 110.67, 110] as [CGFloat] {
                let r = ValueSlider.textRects(width: width, valueWidth: valueWidth)
                XCTAssertEqual(r.value.width, valueWidth, "\(value) @\(width): the value is never cut")
                XCTAssertEqual(r.value.maxX, width, accuracy: 0.001)
                XCTAssertGreaterThanOrEqual(r.value.minX - r.title.maxX, ValueSlider.labelValueGap - 0.001, "\(value) @\(width)")
                XCTAssertGreaterThanOrEqual(r.title.minX, 0)
                if width >= 110 { XCTAssertGreaterThanOrEqual(r.title.width, titleWidth, "\(value) @\(width): title fits whole") }
            }
        }
        // The audit's measured case: 53.02 pt title + 27 pt value in 80 pt had a −0.02 pt gap.
        let r = ValueSlider.textRects(width: 80, valueWidth: 27)
        XCTAssertEqual(r.title.width, 80 - 27 - ValueSlider.labelValueGap, accuracy: 0.001)
        XCTAssertLessThan(r.title.width, titleWidth, "at 80 pt the title truncates rather than touching −100")
    }

    func testValueSliderRingPaintsInsideTheThumb() {
        let thumb = NSRect(x: 0, y: 10, width: Theme.Height.thumb, height: Theme.Height.thumb)   // minimum value
        for line in [Theme.Space.hairline, 2] {
            let painted = ValueSlider.ringPath(thumb: thumb, lineWidth: line).insetBy(dx: -line / 2, dy: -line / 2)
            XCTAssertGreaterThanOrEqual(painted.minX, 0, "stroke \(line)")
            XCTAssertLessThanOrEqual(painted.maxX, thumb.maxX + 0.0001)
        }
    }

    func testValueSliderDrawsWithoutOverflowAt80Points() throws {
        let slider = ValueSlider(frame: NSRect(x: 0, y: 0, width: 80, height: Theme.Height.slider))
        slider.title = "Highlights"
        slider.doubleValue = -100
        XCTAssertEqual(slider.valueText, "-100")
        let rep = try XCTUnwrap(slider.bitmapImageRepForCachingDisplay(in: slider.bounds))
        slider.cacheDisplay(in: slider.bounds, to: rep)   // draws through the real path (no exceptions)
    }

    // MARK: FlowRow (D06)

    func testFlowRowOffersOversizedItemsTheRowWidth() {
        let width: CGFloat = 264
        // First item oversized: no longer placed at 500 pt past the row's edge.
        var f = FlowRow.frames(sizes: [CGSize(width: 500, height: 20)], width: width, spacing: 4) { _, w in CGSize(width: w, height: 20) }
        XCTAssertEqual(f[0], CGRect(x: 0, y: 0, width: 264, height: 20))
        // Oversized after a small one: wraps to its own row, and is bounded.
        f = FlowRow.frames(sizes: [CGSize(width: 40, height: 20), CGSize(width: 500, height: 20), CGSize(width: 30, height: 20)],
                           width: width, spacing: 4) { _, w in CGSize(width: w, height: 20) }
        XCTAssertEqual(f[1], CGRect(x: 0, y: 24, width: 264, height: 20))
        XCTAssertEqual(f[2].minY, 48)
        XCTAssertTrue(f.allSatisfy { $0.maxX <= width })
        // Ordinary chips keep their ideal widths and wrap as before.
        f = FlowRow.frames(sizes: Array(repeating: CGSize(width: 100, height: 20), count: 3), width: width, spacing: 4)
        XCTAssertEqual(f.map(\.minX), [0, 104, 0])
    }

    func testKeywordChipRowStaysInsideTheInspector() {
        let keyword = "an-unbroken-keyword-that-is-much-wider-than-any-inspector-column-could-ever-be"
        for width in [Theme.Width.inspectorMin - 2 * Theme.Space.gutter, Theme.Width.inspectorIdeal - 2 * Theme.Space.gutter] {
            let size = fit(FlowRow(spacing: Theme.Space.xs) {
                KeywordChip(name: "short", mixed: false) {}
                KeywordChip(name: keyword, mixed: false) {}
            }.frame(width: width), width: width)
            XCTAssertLessThanOrEqual(size.width, width + 0.5)
            XCTAssertEqual(size.height, Theme.Height.small * 2 + Theme.Space.xs, accuracy: 0.5, "two rows, the long chip truncated")
            // The chip itself accepts the row's width.
            let chip = fit(KeywordChip(name: keyword, mixed: false) {}.frame(maxWidth: width), width: width)
            XCTAssertLessThanOrEqual(chip.width, width + 0.5)
        }
    }

    // MARK: Menus (D05)

    func testThemeMenuTruncatesInsteadOfOverflowing() {
        let profile = "Very Long Printer Profile Name — Glossy Baryta Photo Paper 300 gsm (PK) v2.icc"
        for width: CGFloat in [120, 160, 200] {
            let size = fit(MenuPicker(selection: .constant(1), options: [(1, profile), (2, "sRGB")]), width: width)
            XCTAssertLessThanOrEqual(size.width, width + 0.5, "menu at \(width)")
        }
        // With room, it still hugs its title (does not stretch to the row).
        let roomy = fit(HStack { MenuPicker(selection: .constant(2), options: [(1, profile), (2, "sRGB")]); Spacer() }, width: 600)
        XCTAssertEqual(roomy.width, 600, accuracy: 0.5)
        let alone = fit(MenuPicker(selection: .constant(2), options: [(1, profile), (2, "sRGB")]), width: 600)
        XCTAssertLessThan(alone.width, 120, "a short title hugs")
    }

    func testSoftProofRowFitsTheMinimumInspector() {
        let width = Theme.Width.inspectorMin - 2 * Theme.Space.gutter
        let size = fit(SoftProofPanel(proof: SoftProof.shared).frame(width: width), width: width)
        XCTAssertLessThanOrEqual(size.width, width + 0.5)
    }

    // MARK: Compare (D04)

    func testCompareCaptionAndBadgeShareOnePartition() {
        for width in stride(from: CGFloat(120), through: 900, by: 37) {
            for badge in [0, 20, 90, 180, 400] as [CGFloat] {
                let p = ComparePaneView.captionPartition(width: width, badgeWidth: badge)
                XCTAssertEqual(p.caption.lowerBound, Theme.Space.m)
                XCTAssertEqual(p.badge.upperBound, width - Theme.Space.m, accuracy: 0.001)
                XCTAssertLessThanOrEqual(p.caption.upperBound, p.badge.lowerBound, "\(width)/\(badge): no shared strip")
                if badge > 0 {
                    XCTAssertGreaterThanOrEqual(p.badge.lowerBound - p.caption.upperBound, Theme.Space.s - 0.001)
                    XCTAssertLessThanOrEqual(p.badge.upperBound - p.badge.lowerBound, max(0, width - 2 * Theme.Space.m - Theme.Space.s) * 0.4 + 0.001)
                }
            }
        }
        // The audit's example: 300 pt pane, badge text 108 pt wide.
        let p = ComparePaneView.captionPartition(width: 300, badgeWidth: 108)
        XCTAssertLessThan(p.caption.upperBound, p.badge.lowerBound)
    }

    // MARK: Handles (D07, D09)

    func testColorWheelPuckStaysInsideAtFullSaturation() {
        for size in [CGSize(width: 72, height: 72), CGSize(width: 80, height: 72), CGSize(width: 264, height: 144)] {
            let bounds = NSRect(origin: .zero, size: size)
            for hue in stride(from: 0.0, to: 360, by: 15) {
                let r = ColorWheelView.puckPaintRect(in: bounds, hue: hue, saturation: 100)
                XCTAssertTrue(bounds.insetBy(dx: -0.001, dy: -0.001).contains(r), "\(size) hue \(hue): \(r)")
            }
        }
    }

    func testCurveEndKnotsStayInsideTheView() {
        let inset = CurveEditorView.knotRadius
        for selected in [false, true] {
            let r = CurveEditorView.knotPaintRect(center: CGPoint(x: inset, y: inset), selected: selected)
            XCTAssertGreaterThanOrEqual(r.minX, -0.001, "selected \(selected)")
            XCTAssertGreaterThanOrEqual(r.minY, -0.001)
        }
    }

    // MARK: Sidebar (D10)

    func testSidebarHiddenBadgeGivesItsWidthBack() {
        func titleWidth(badge: String?) -> CGFloat {
            let cell = SidebarCell()
            cell.frame = NSRect(x: 0, y: 0, width: Theme.Width.sidebarMin, height: Theme.Height.row)
            let row = SidebarRow(.folder(URL(fileURLWithPath: "/tmp/x")), key: "recent:/a/very/long/path/shoot",
                                 title: "A very long repeated folder name for a shoot")
            row.detail = "parent-folder"
            row.badge = badge
            row.count = 1234
            cell.configure(row)
            cell.layoutSubtreeIfNeeded()
            let title = cell.textField!.frame.width
            let detail = cell.subviews.compactMap { $0 as? NSTextField }.first { $0.stringValue == "parent-folder" }?.frame.width ?? 0
            return title + detail
        }
        let with = titleWidth(badge: "⌂")
        let without = titleWidth(badge: nil)
        XCTAssertGreaterThanOrEqual(without - with, Theme.Height.chip + Theme.Space.xs - 0.5,
                                    "an absent badge's 16 pt slot and gap go to the title/detail")
    }

    // MARK: Status bar (L3)

    func testStatusBarFitsANarrowDetailWithLongStrings() {
        let model = AppModel()
        model.install(StubLibrary.synthetic(count: 40))
        model.statusMessage = String(repeating: "A long status message that must truncate. ", count: 6)
        for width: CGFloat in [ShellBudget.detailMinWidth, 480, 640] {
            let size = fit(StatusBar(model: model).frame(width: width), width: width)
            XCTAssertLessThanOrEqual(size.width, width + 0.5, "status bar at \(width)")
            XCTAssertEqual(size.height, Theme.Height.statusBar + Theme.Space.hairline, accuracy: 0.5)
        }
    }

    // MARK: Budget

    func testShellBudgetYieldOrder() {
        let twice = ShellBudget.inspectorFactor
        let needed = ShellBudget.detailMinWidth + ShellBudget.sidebarSpan + twice * Theme.Width.inspectorMin
        XCTAssertTrue(ShellBudget.sidebarFits(windowWidth: needed, inspector: true))
        XCTAssertFalse(ShellBudget.sidebarFits(windowWidth: needed - 1, inspector: true))
        XCTAssertTrue(ShellBudget.sidebarFits(windowWidth: ShellBudget.minWindow.width, inspector: false))
        // At the declared minimum, collapsing the sidebar leaves room for the detail and the inspector.
        XCTAssertLessThanOrEqual(ShellBudget.requiredWidth(sidebar: false, inspector: true), ShellBudget.minWindow.width)
        // Step 2: the inspector narrows to what fits, never below its minimum or above its maximum.
        XCTAssertEqual(ShellBudget.inspectorFit(windowWidth: ShellBudget.minWindow.width, sidebar: false),
                       max(Theme.Width.inspectorMin, ((ShellBudget.minWindow.width - ShellBudget.detailMinWidth) / twice).rounded(.down)))
        XCTAssertEqual(ShellBudget.inspectorFit(windowWidth: 3000, sidebar: true), Theme.Width.inspectorMax)
        XCTAssertEqual(ShellBudget.inspectorFit(windowWidth: 500, sidebar: true), Theme.Width.inspectorMin)
        for w: CGFloat in [960, 1100, 1188, 1280, 1440] {
            let sidebar = ShellBudget.sidebarFits(windowWidth: w, inspector: true)
            let used = ShellBudget.requiredWidth(sidebar: sidebar, inspector: true,
                                                 inspectorWidth: ShellBudget.inspectorFit(windowWidth: w, sidebar: sidebar))
            XCTAssertLessThanOrEqual(used, w, "\(w): the columns fit the window")
        }
        XCTAssertTrue(ShellBudget.filmstripFits(detailHeight: ShellBudget.canvasMinHeight + Theme.Height.filmstrip + 1, chrome: 0))
        XCTAssertFalse(ShellBudget.filmstripFits(detailHeight: ShellBudget.canvasMinHeight + Theme.Height.filmstrip - 1, chrome: 0))
    }

    // MARK: --app-dir (Machine B)

    func testAppDirDefaultsAreIsolatedFromTheSharedDomain() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("m256-defaults-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        XCTAssertNil(AppDefaultsIsolation.explicitAppDirectory(arguments: ["Tessera"], environment: [:]))
        XCTAssertEqual(AppDefaultsIsolation.explicitAppDirectory(arguments: ["Tessera", "--app-dir", dir.path], environment: [:])?.path, dir.path)
        XCTAssertEqual(AppDefaultsIsolation.explicitAppDirectory(arguments: ["Tessera"], environment: ["TESSERA_APP_DIR": dir.path])?.path, dir.path)

        let key = "M256RecentFolderPaths-\(UUID().uuidString)"
        UserDefaults.standard.set(["/shared/shoot"], forKey: key)
        addTeardownBlock { UserDefaults.standard.removeObject(forKey: key) }
        let store = try XCTUnwrap(AppDefaultsIsolation.defaults(in: dir))
        XCTAssertNil(store.stringArray(forKey: key), "a fresh app dir does not see the shared folder registry")
        store.set(["/scratch/shoot"], forKey: key)
        _ = store.synchronize()
        XCTAssertEqual(UserDefaults.standard.stringArray(forKey: key), ["/shared/shoot"], "and does not change it")
        XCTAssertEqual(AppDefaultsIsolation.defaults(in: dir)?.stringArray(forKey: key), ["/scratch/shoot"])
        XCTAssertTrue(FileManager.default.fileExists(atPath: dir.appendingPathComponent(AppDefaultsIsolation.fileName).path))
    }
}
