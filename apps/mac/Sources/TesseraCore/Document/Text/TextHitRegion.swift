import CoreGraphics
import Foundation

/// Where a Type-tool click on existing text resumes editing instead of starting new text (WP B5-10c).
///
/// Point text is hit-tested per line: the line box runs from the first glyph's origin to the last
/// glyph's advance (the whole line width, trailing spaces included), from the ascent above to the
/// descent below the baseline. Each line box is widened horizontally by the trailing margin on BOTH
/// ends (the trailing end of right-aligned or RTL lines is the left one) and vertically by
/// `edgeMargin`. Area text keeps its paragraph box plus `edgeMargin`. When in doubt the click
/// resumes the existing layer: new text is only created clearly away from existing text.
public enum TextHitRegion {
    /// Trailing margin as a fraction of the line height (ascent + descent): half a line, about the
    /// advance of two lowercase letters. 48 px Helvetica → ≈ 27 px.
    public static let trailingMarginLineHeights = 0.5
    /// The trailing margin is at least this many view points (small text at low zoom).
    public static let minimumTrailingMarginPoints = 12.0
    /// Margin above / below line boxes and around area boxes (local pixels), as before B5-10c.
    public static let edgeMargin = 4.0

    /// Line boxes of a point-text layout (local pixels), widened by the margins. `pixelsPerPoint`
    /// converts `minimumTrailingMarginPoints` to local pixels (1 / the viewport's points per pixel).
    public static func lineBoxes(_ layout: TextLayoutInfo, pixelsPerPoint: Double) -> [CGRect] {
        layout.lines.map { l in
            let gs = layout.glyphs[l.glyphs]
            let x0 = min(gs.map(\.x).min() ?? l.x, l.x)
            let x1 = max(gs.map { $0.x + $0.advance }.max() ?? l.x, l.x + l.width)
            let height = l.ascent + l.descent
            let m = max(trailingMarginLineHeights * height, minimumTrailingMarginPoints * max(pixelsPerPoint, 0))
            return CGRect(x: x0 - m, y: l.baseline - l.ascent - edgeMargin, width: x1 - x0 + 2 * m, height: height + 2 * edgeMargin)
        }
    }

    /// Whether local point `p` resumes a text layer laid out as `layout` (`box`: the area-text box).
    public static func contains(_ p: CGPoint, layout: TextLayoutInfo, box: CGSize?, pixelsPerPoint: Double) -> Bool {
        if let box {
            return CGRect(origin: .zero, size: box).insetBy(dx: -edgeMargin, dy: -edgeMargin).contains(p)
        }
        return lineBoxes(layout, pixelsPerPoint: pixelsPerPoint).contains { $0.contains(p) }
    }
}
