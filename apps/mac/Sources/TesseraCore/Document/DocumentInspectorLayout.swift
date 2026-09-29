import CoreGraphics
import Foundation

/// WP B5-16: the document inspector's sub-tabs (Stack · Properties · Channels). History is a
/// collapsible pane under whichever tab is showing, not a tab.
public enum DocumentInspectorTab: String, CaseIterable, Identifiable, Sendable {
    /// The Layers panel: blend mode, Opacity / Fill, locks, the outline and its footer.
    case stack
    /// The selected layer's properties, then the tool sections (Color, Brushes).
    case properties
    /// The Channels panel.
    case channels

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .stack: "Stack"
        case .properties: "Properties"
        case .channels: "Channels"
        }
    }

    /// ⌃1 / ⌃2 / ⌃3.
    public var shortcutDigit: Character {
        switch self {
        case .stack: "1"
        case .properties: "2"
        case .channels: "3"
        }
    }

    public var help: String {
        let what: String = switch self {
        case .stack: "Layers: blend mode, opacity, locks and the layer list"
        case .properties: "The selected layer's properties, colour and brushes"
        case .channels: "Colour, alpha and spot channels"
        }
        return "\(what) (⌃\(shortcutDigit))"
    }
}

/// WP B5-16: the inspector column's height budget (M2-56 handoff H1–H3, H6). Pure, so the arithmetic
/// is unit tested without a window. The column is: the tab bar, a hairline, the tab's content (which
/// scrolls itself), then the History pane (header, and when expanded a resizable body that scrolls
/// the states and snapshots). The sum of the minimums must fit the smallest column (960 × 600 window
/// content minus the toolbar: 548 pt).
public struct DocumentInspectorBudget: Equatable, Sendable {
    /// Tab bar row (segmented header).
    public var tabBar: CGFloat
    /// Separators between the regions (hairlines), counted together.
    public var separators: CGFloat
    /// Smallest useful tab content (the Stack tab's: layer controls, four outline rows, footer).
    public var tabMinimum: CGFloat
    /// History pane header.
    public var historyHeader: CGFloat
    /// Smallest expanded History body (two rows plus the New Snapshot row).
    public var historyMinimum: CGFloat

    public init(tabBar: CGFloat, separators: CGFloat, tabMinimum: CGFloat, historyHeader: CGFloat, historyMinimum: CGFloat) {
        self.tabBar = tabBar
        self.separators = separators
        self.tabMinimum = tabMinimum
        self.historyHeader = historyHeader
        self.historyMinimum = historyMinimum
    }

    /// The smallest column height every part fits in, History expanded or collapsed.
    public func minimumColumn(historyExpanded: Bool) -> CGFloat {
        tabBar + separators + tabMinimum + historyHeader + (historyExpanded ? historyMinimum : 0)
    }

    /// The History body's height for a column of `column` points when the person asked for
    /// `requested`: at least `historyMinimum`, and never so tall that the tab content drops below its
    /// minimum (the tab content keeps priority when both cannot fit).
    public func historyHeight(requested: CGFloat, column: CGFloat) -> CGFloat {
        let room = column - tabBar - separators - tabMinimum - historyHeader
        return max(0, min(max(requested, historyMinimum), room))
    }
}

/// WP B5-16 (H4): the Layers panel's Opacity and Fill sliders sit side by side only when the
/// panel's interior is at least this wide; below it each gets its own row.
public enum DocumentLayersRow {
    public static let sideBySideMinimum: CGFloat = 300

    public static func slidersSideBySide(interiorWidth: CGFloat) -> Bool {
        interiorWidth >= sideBySideMinimum
    }
}

/// WP B5-16 (H8): the toolbar's document tab strip shows at most `cap` tabs (`compactCap` in a compact toolbar); the others are in an
/// overflow menu. The visible window always contains the current document and keeps the documents'
/// order; it starts as early as possible (the first `cap` tabs while the current one is among them).
public enum DocumentTabStrip {
    public static let cap = 3
    /// In a compact toolbar (window narrower than 1280 pt) only the current document is a tab, so the
    /// strip still fits beside the view picker instead of dropping into the toolbar's overflow.
    public static let compactCap = 1

    public static func visible(count: Int, current: Int?, cap: Int = DocumentTabStrip.cap) -> Range<Int> {
        guard count > 0, cap > 0 else { return 0..<0 }
        guard count > cap else { return 0..<count }
        let c = min(max(current ?? 0, 0), count - 1)
        let start = max(0, min(c - cap + 1, count - cap))
        return start..<(start + cap)
    }

    /// Documents not shown as tabs.
    public static func overflow(count: Int, current: Int?, cap: Int = DocumentTabStrip.cap) -> [Int] {
        let shown = visible(count: count, current: current, cap: cap)
        return (0..<count).filter { !shown.contains($0) }
    }
}
