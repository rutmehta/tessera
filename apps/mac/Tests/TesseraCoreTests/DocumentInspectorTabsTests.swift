import XCTest
@testable import TesseraCore

/// WP B5-16: the document inspector's pure layout policy (tabs, height budget, the Layers row rule,
/// the document tab strip cap).
final class DocumentInspectorTabsTests: XCTestCase {
    private let budget = DocumentInspectorBudget(tabBar: 32, separators: 2, tabMinimum: 290, historyHeader: 32, historyMinimum: 80)

    func testTabsAndShortcuts() {
        XCTAssertEqual(DocumentInspectorTab.allCases.map(\.title), ["Stack", "Properties", "Channels"])
        XCTAssertEqual(DocumentInspectorTab.allCases.map(\.shortcutDigit), ["1", "2", "3"])
        XCTAssertTrue(DocumentInspectorTab.properties.help.hasSuffix("(⌃2)"))
    }

    func testBudgetFitsTheSmallestColumn() {
        XCTAssertEqual(budget.minimumColumn(historyExpanded: false), 356)
        XCTAssertEqual(budget.minimumColumn(historyExpanded: true), 436)
        XCTAssertLessThanOrEqual(budget.minimumColumn(historyExpanded: true), 548)
    }

    func testHistoryHeightIsClampedToItsMinimumAndTheTabMinimum() {
        // Plenty of room: the request stands.
        XCTAssertEqual(budget.historyHeight(requested: 200, column: 848), 200)
        // Below the minimum: the minimum.
        XCTAssertEqual(budget.historyHeight(requested: 10, column: 848), 80)
        // Too tall for the column: the tab content keeps its minimum.
        XCTAssertEqual(budget.historyHeight(requested: 900, column: 548), 548 - 32 - 2 - 290 - 32)
        // A column too short for both: the tab content wins.
        XCTAssertEqual(budget.historyHeight(requested: 200, column: 400), 44)
        XCTAssertEqual(budget.historyHeight(requested: 200, column: 300), 0)
    }

    func testOpacityAndFillShareARowFrom300Points() {
        XCTAssertFalse(DocumentLayersRow.slidersSideBySide(interiorWidth: 264))   // 288 pt inspector
        XCTAssertFalse(DocumentLayersRow.slidersSideBySide(interiorWidth: 299.5))
        XCTAssertTrue(DocumentLayersRow.slidersSideBySide(interiorWidth: 300))
        XCTAssertTrue(DocumentLayersRow.slidersSideBySide(interiorWidth: 356))    // 380 pt inspector
    }

    func testTabStripShowsAtMostThreeIncludingTheCurrentOne() {
        XCTAssertEqual(DocumentTabStrip.visible(count: 0, current: nil), 0..<0)
        XCTAssertEqual(DocumentTabStrip.visible(count: 2, current: 1), 0..<2)
        XCTAssertEqual(DocumentTabStrip.visible(count: 3, current: 2), 0..<3)
        XCTAssertEqual(DocumentTabStrip.visible(count: 8, current: 0), 0..<3)
        XCTAssertEqual(DocumentTabStrip.visible(count: 8, current: 2), 0..<3)
        XCTAssertEqual(DocumentTabStrip.visible(count: 8, current: 3), 1..<4)
        XCTAssertEqual(DocumentTabStrip.visible(count: 8, current: 7), 5..<8)
        XCTAssertEqual(DocumentTabStrip.visible(count: 8, current: nil), 0..<3)
        XCTAssertEqual(DocumentTabStrip.overflow(count: 5, current: 4), [0, 1])
        XCTAssertEqual(DocumentTabStrip.overflow(count: 3, current: 0), [])
        XCTAssertEqual(DocumentTabStrip.visible(count: 8, current: 5, cap: DocumentTabStrip.compactCap), 5..<6)
        XCTAssertEqual(DocumentTabStrip.overflow(count: 3, current: 1, cap: DocumentTabStrip.compactCap), [0, 2])
        for count in 1...10 {
            for current in 0..<count {
                let shown = DocumentTabStrip.visible(count: count, current: current)
                XCTAssertTrue(shown.contains(current))
                XCTAssertEqual(shown.count, min(count, DocumentTabStrip.cap))
                XCTAssertEqual(shown.count + DocumentTabStrip.overflow(count: count, current: current).count, count)
            }
        }
    }
}
