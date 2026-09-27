import AppKit
import SwiftUI
import TesseraCore

// WP M2-56: the shell's layout contracts. The window is the budget: a column's content can
// overflow only inside that column (clipped at its bottom / trailing edge, or scrolled by the
// column itself), never enlarge the split view past the window, centre it at negative origins or
// push it under the toolbar. Panels yield space in a fixed order instead.

/// Reports the size it is offered, whatever its content asks for, places the content top-leading
/// at that size, and clips. SwiftUI otherwise lets a child whose minimum exceeds the proposal
/// overflow its parent centred, and a split-view column passes that minimum on to the window:
/// the document inspector's stacked minimums (675+ pt) made the whole split view taller than a
/// 748 pt window and centred it 34 pt up under the toolbar (audit D01/D02).
struct ContainedColumn: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let content = subviews.first else { return .zero }
        let natural = content.sizeThatFits(proposal)
        // Finite proposals are the answer (a minimum query of 0 gets 0: the column never raises the
        // window minimum); unspecified or infinite ones fall back to the content's own answer.
        func pick(_ offered: CGFloat?, _ own: CGFloat) -> CGFloat {
            guard let offered, offered.isFinite else { return own }
            return offered
        }
        return CGSize(width: pick(proposal.width, natural.width), height: pick(proposal.height, natural.height))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, anchor: .topLeading, proposal: ProposedViewSize(bounds.size))
    }
}

extension View {
    /// Keeps this column's content inside the size the split view gives it (see `ContainedColumn`).
    func containedColumn() -> some View {
        ContainedColumn { self }.clipped()
    }
}

/// The shell's width and height budget (DESIGN.md §3.1): which panels give way, and in what
/// order, when the window is small. All values are content-area points (the window minus its
/// toolbar), and pure so the policy is unit tested without a window.
enum ShellBudget {
    /// Declared minimum window content size: every state must be usable at this size.
    static let minWindow = CGSize(width: 960, height: 600)

    /// The detail column's minimum: the canvas stays usable (document tool palette + a canvas, or
    /// a grid of at least two thumbnail columns).
    static let detailMinWidth: CGFloat = Theme.Width.labelWide * 4

    /// Width the split view needs for the detail, the sidebar (when shown) and the inspector at
    /// `inspectorWidth` (when shown). On macOS 26 the floating sidebar and the inspector overlay the
    /// detail and the split view counts the inspector column once more (see B5-10's note in
    /// ContentView), so the inspector is counted twice there.
    static func requiredWidth(sidebar: Bool, inspector: Bool, inspectorWidth: CGFloat = Theme.Width.inspectorMin) -> CGFloat {
        var w = detailMinWidth
        if sidebar { w += sidebarSpan }
        if inspector { w += inspectorWidth * inspectorFactor }
        return w
    }

    /// The sidebar's share of the width: its ideal column plus, on macOS 26, the floating
    /// sidebar's inset (measured: a 228 pt split item for the 220 pt column).
    static var sidebarSpan: CGFloat {
        if #available(macOS 26.0, *) { return Theme.Width.sidebarIdeal + Theme.Space.s }
        return Theme.Width.sidebarIdeal
    }

    /// Second step: the widest inspector that fits beside the detail minimum (and the sidebar when
    /// shown), between the inspector's minimum and maximum widths.
    static func inspectorFit(windowWidth: CGFloat, sidebar: Bool) -> CGFloat {
        let room = (windowWidth - detailMinWidth - (sidebar ? sidebarSpan : 0)) / inspectorFactor
        return min(max(room.rounded(.down), Theme.Width.inspectorMin), Theme.Width.inspectorMax)
    }

    /// How many times the inspector's width counts against the window (see `requiredWidth`).
    static var inspectorFactor: CGFloat {
        if #available(macOS 26.0, *) { return 2 }
        return 1
    }

    /// First step of the yield order: the library sidebar collapses when the window cannot hold it
    /// beside the detail minimum and the inspector at its minimum width (the second step, the
    /// inspector shrinking to its minimum, is the split view's own behaviour).
    static func sidebarFits(windowWidth: CGFloat, inspector: Bool) -> Bool {
        windowWidth + 0.5 >= requiredWidth(sidebar: true, inspector: inspector)
    }

    /// Below this window width the toolbar's text buttons show their icons only (their titles stay
    /// in help and accessibility), so the principal mode picker and the document tabs keep room.
    static let compactToolbarWidth: CGFloat = 1280

    /// Third step: the filmstrip hides when the detail column is too short to keep a useful grid or
    /// loupe above it (the canvas keeps at least `canvasMinHeight`).
    static let canvasMinHeight: CGFloat = 320
    static func filmstripFits(detailHeight: CGFloat, chrome: CGFloat) -> Bool {
        detailHeight - chrome - Theme.Height.filmstrip - Theme.Space.hairline >= canvasMinHeight
    }
}

/// Root-containment check for tests and debug builds: every split view, split item and hosting
/// view below `root` lies inside `root`'s bounds (after converting through every ancestor), so no
/// region is centred at negative origins or pushed under the toolbar. With `columnContent`, scroll
/// views inside the columns are checked too (a column whose own content is taller than the window,
/// clipped at its bottom edge, fails that stricter check but not the shell's). Returns one line per
/// violation; empty when contained.
@MainActor
enum ShellLayoutAudit {
    static func containmentViolations(in root: NSView, columnContent: Bool = false, tolerance: CGFloat = 1) -> [String] {
        var out: [String] = []
        let limit = root.bounds.insetBy(dx: -tolerance, dy: -tolerance)
        func walk(_ view: NSView) {
            let name = String(describing: type(of: view))
            let scroller = name.contains("HostingScrollView")
            let region = view is NSSplitView || name.contains("SplitView") || (name.contains("Hosting") && !scroller)
            if view !== root, !view.isHidden, region || (columnContent && scroller) {
                let frame = view.convert(view.bounds, to: root)
                if frame.width > 0, frame.height > 0, !limit.contains(frame) {
                    out.append("\(name) \(NSStringFromRect(frame)) outside \(NSStringFromRect(root.bounds))")
                }
            }
            // Scroll content is allowed to extend past its clip view.
            if view is NSClipView { return }
            for child in view.subviews { walk(child) }
        }
        walk(root)
        return out
    }
}

private struct ToolbarCompactKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// The window is narrow: toolbar buttons and toggles show icons only.
    var toolbarCompact: Bool {
        get { self[ToolbarCompactKey.self] }
        set { self[ToolbarCompactKey.self] = newValue }
    }
}
