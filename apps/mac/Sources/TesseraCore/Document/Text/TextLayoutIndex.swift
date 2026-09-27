import CoreGraphics
import Foundation

// The host side of the Type tool's caret (WP B5-10). The engine lays text out (`layout_text`, the
// same typography layout its renderers rasterize); this file turns that layout into caret stops,
// caret geometry, pointer hit tests and selection rectangles. Nothing here re-lays text out with
// CoreText: the engine's glyph origins, advances and UTF-8 clusters are the oracle.
//
// Three index domains, converted explicitly (`TextIndexMap`):
//  * UTF-8 byte offsets into the concatenated run text — the model, the layout clusters and every
//    caret / selection offset in this file;
//  * UTF-16 code units — `NSRange`s of `NSTextInputClient` (IME, services);
//  * run indexes (+ UTF-8 offset within the run) — `EditTextRuns` and style splits.

/// A positioned glyph (local level-0 pixels; y is the baseline position, down).
public struct TextGlyphInfo: Equatable, Sendable {
    public var run: Int
    /// UTF-8 offset of the glyph's shaping cluster.
    public var cluster: Int
    public var x: Double
    public var y: Double
    public var advance: Double
    public var angle: Double
    public var rtl: Bool
    public init(run: Int, cluster: Int, x: Double, y: Double, advance: Double, angle: Double = 0, rtl: Bool = false) {
        self.run = run; self.cluster = cluster; self.x = x; self.y = y; self.advance = advance; self.angle = angle
        self.rtl = rtl
    }
}

public struct TextLineInfo: Equatable, Sendable {
    /// UTF-8 source range (a trailing separator included).
    public var source: Range<Int>
    /// Indexes into `glyphs`, visual order.
    public var glyphs: Range<Int>
    public var x: Double
    public var baseline: Double
    public var width: Double
    /// nil for point text.
    public var availableWidth: Double?
    public var ascent: Double
    public var descent: Double
    public init(source: Range<Int>, glyphs: Range<Int>, x: Double, baseline: Double, width: Double,
                availableWidth: Double?, ascent: Double, descent: Double) {
        self.source = source; self.glyphs = glyphs; self.x = x; self.baseline = baseline; self.width = width
        self.availableWidth = availableWidth; self.ascent = ascent; self.descent = descent
    }
}

/// The engine's layout of one model.
public struct TextLayoutInfo: Equatable, Sendable {
    public var glyphs: [TextGlyphInfo]
    public var lines: [TextLineInfo]
    public var overflow: Bool
    public var textLength: Int
    public init(glyphs: [TextGlyphInfo], lines: [TextLineInfo], overflow: Bool, textLength: Int) {
        self.glyphs = glyphs; self.lines = lines; self.overflow = overflow; self.textLength = textLength
    }
    public static let empty = TextLayoutInfo(glyphs: [], lines: [], overflow: false, textLength: 0)

    /// Union of glyph boxes (origin to advance, ascent above the baseline) and empty lines.
    public var bounds: CGRect? {
        var r: CGRect?
        for l in lines {
            let lineGlyphs = glyphs[l.glyphs]
            let x0 = lineGlyphs.map(\.x).min() ?? l.x
            let x1 = lineGlyphs.map { $0.x + $0.advance }.max() ?? l.x
            let box = CGRect(x: x0, y: l.baseline - l.ascent, width: max(x1 - x0, 1), height: l.ascent + l.descent)
            r = r.map { $0.union(box) } ?? box
        }
        return r
    }
}

// MARK: - Index domains

/// UTF-8 ⇄ UTF-16 ⇄ run conversions over one text, and its grapheme boundaries.
public struct TextIndexMap: Equatable, Sendable {
    public let text: String
    /// UTF-16 offset of every UTF-8 offset that starts a scalar (and of the end).
    private let utf16AtScalar: [Int: Int]
    /// Scalar-start UTF-8 offsets in order, with the end.
    private let scalarStarts: [Int]
    private let utf16Starts: [Int]
    /// UTF-8 offsets of extended grapheme cluster boundaries (Swift `Character`s), with 0 and the end.
    public let graphemeBoundaries: [Int]

    public init(_ text: String) {
        self.text = text
        var map: [Int: Int] = [:]
        var starts: [Int] = []
        var u16: [Int] = []
        var o8 = 0, o16 = 0
        for s in text.unicodeScalars {
            map[o8] = o16
            starts.append(o8)
            u16.append(o16)
            o8 += String(s).utf8.count
            o16 += s.utf16.count
        }
        map[o8] = o16
        starts.append(o8)
        u16.append(o16)
        utf16AtScalar = map
        scalarStarts = starts
        utf16Starts = u16
        var g: [Int] = [0]
        var o = 0
        for ch in text {
            o += ch.utf8.count
            g.append(o)
        }
        graphemeBoundaries = g
    }

    public var utf8Count: Int { scalarStarts.last ?? 0 }
    public var utf16Count: Int { utf16Starts.last ?? 0 }

    /// The UTF-8 offset rounded down to a scalar start.
    public func scalarFloor(_ utf8: Int) -> Int {
        let o = max(0, min(utf8, utf8Count))
        var lo = 0, hi = scalarStarts.count - 1
        while lo < hi {
            let mid = (lo + hi + 1) / 2
            if scalarStarts[mid] <= o { lo = mid } else { hi = mid - 1 }
        }
        return scalarStarts[lo]
    }

    public func utf16(fromUTF8 o: Int) -> Int { utf16AtScalar[scalarFloor(o)] ?? 0 }

    /// A UTF-16 offset (rounded down out of a surrogate pair) as UTF-8.
    public func utf8(fromUTF16 o: Int) -> Int {
        let o = max(0, min(o, utf16Count))
        var lo = 0, hi = utf16Starts.count - 1
        while lo < hi {
            let mid = (lo + hi + 1) / 2
            if utf16Starts[mid] <= o { lo = mid } else { hi = mid - 1 }
        }
        return scalarStarts[lo]
    }

    public func utf16Range(_ r: Range<Int>) -> NSRange {
        let a = utf16(fromUTF8: r.lowerBound), b = utf16(fromUTF8: r.upperBound)
        return NSRange(location: a, length: max(0, b - a))
    }

    public func utf8Range(_ r: NSRange) -> Range<Int> {
        let a = utf8(fromUTF16: r.location), b = utf8(fromUTF16: r.location + max(0, r.length))
        return a..<max(a, b)
    }

    public func isGraphemeBoundary(_ o: Int) -> Bool { graphemeBoundaries.binarySearchContains(o) }

    /// Index of the first grapheme boundary greater than `o`.
    public func firstBoundaryIndex(after o: Int) -> Int {
        var lo = 0, hi = graphemeBoundaries.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if graphemeBoundaries[mid] <= o { lo = mid + 1 } else { hi = mid }
        }
        return lo
    }

    /// The grapheme boundary before `o` (0 at the start).
    public func boundary(before o: Int) -> Int {
        let i = firstBoundaryIndex(after: o - 1) - 1
        return i >= 0 ? graphemeBoundaries[i] : 0
    }

    /// The substring of UTF-8 range `r` (clamped to scalars).
    public func substring(_ r: Range<Int>) -> String {
        let u = text.utf8
        let a = u.index(u.startIndex, offsetBy: scalarFloor(r.lowerBound))
        let b = u.index(u.startIndex, offsetBy: scalarFloor(r.upperBound))
        return String(text[a..<b])
    }

    /// Run index and UTF-8 offset in that run for `o`. At a boundary between runs, `preferPrevious`
    /// picks the run ending there (typing continues the style before the caret).
    public static func run(at o: Int, in runs: [TextRunModel], preferPrevious: Bool = true) -> (run: Int, offset: Int) {
        guard !runs.isEmpty else { return (0, 0) }
        var start = 0
        for (i, r) in runs.enumerated() {
            let end = start + r.text.utf8.count
            if o < end || (o == end && preferPrevious && end > start) || i == runs.count - 1 {
                if o == start, preferPrevious, i > 0, runs[i - 1].text.utf8.count > 0 {
                    return (i - 1, runs[i - 1].text.utf8.count)
                }
                return (i, min(max(o - start, 0), r.text.utf8.count))
            }
            start = end
        }
        return (runs.count - 1, runs[runs.count - 1].text.utf8.count)
    }
}

extension Array where Element == Int {
    func binarySearchContains(_ v: Int) -> Bool {
        var lo = 0, hi = count
        while lo < hi {
            let mid = (lo + hi) / 2
            if self[mid] < v { lo = mid + 1 } else { hi = mid }
        }
        return lo < count && self[lo] == v
    }
}

// MARK: - Caret geometry

/// Caret stops, caret rectangles, hit testing and selection rectangles over the engine layout of a
/// model (local level-0 pixels, y down).
public struct TextLayoutIndex: Sendable {
    public let model: TextSourceModel
    public let layout: TextLayoutInfo
    public let map: TextIndexMap
    /// Valid caret offsets: grapheme boundaries that are not inside a shaping cluster (ligatures,
    /// clusters spanning several graphemes), sorted.
    public let caretStops: [Int]
    /// Per line: its clusters `(start, end)` in logical order.
    private let lineClusters: [[(start: Int, end: Int)]]

    private static let separators: Set<Character> = ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}", "\u{0B}", "\u{0C}", "\u{85}"]

    public init(model: TextSourceModel, layout: TextLayoutInfo) {
        self.model = model
        self.layout = layout
        let map = TextIndexMap(model.text)
        self.map = map
        var interior = Set<Int>()
        var clustersByLine: [[(start: Int, end: Int)]] = []
        for line in layout.lines {
            let starts = Array(Set(layout.glyphs[line.glyphs].map(\.cluster))).sorted()
            let visibleEnd = Self.visibleEnd(line, starts: starts, map: map)
            var clusters: [(Int, Int)] = []
            for (i, s) in starts.enumerated() {
                let e = i + 1 < starts.count ? starts[i + 1] : max(visibleEnd, s)
                // The cluster ends at the first grapheme boundary >= its last scalar when the next start is
                // far (trailing spaces trimmed from a wrapped line are not glyphs).
                clusters.append((s, e))
                var k = map.firstBoundaryIndex(after: s)
                while k < map.graphemeBoundaries.count, map.graphemeBoundaries[k] < e {
                    // A grapheme boundary inside a multi-grapheme cluster (a ligature) is not a caret stop,
                    // unless the gap is unshaped source (trimmed spaces, separators).
                    let b = map.graphemeBoundaries[k]
                    if Self.isInsideShaped(b, clusterStart: s, next: e, map: map) { interior.insert(b) }
                    k += 1
                }
            }
            clustersByLine.append(clusters)
        }
        lineClusters = clustersByLine
        caretStops = map.graphemeBoundaries.filter { !interior.contains($0) }
    }

    /// End of the shaped part of a line: separators (and, when not shaped, trailing spaces) trimmed.
    private static func visibleEnd(_ line: TextLineInfo, starts: [Int], map: TextIndexMap) -> Int {
        var end = line.source.upperBound
        let lastStart = starts.last ?? line.source.lowerBound
        while end > line.source.lowerBound, end > lastStart {
            let prev = max(map.boundary(before: end), line.source.lowerBound)
            let ch = map.substring(prev..<end)
            if let c = ch.first, separators.contains(c) || ((c == " " || c == "\t") && !starts.contains(prev)) {
                end = prev
            } else { break }
        }
        return end
    }

    /// `b` lies inside the shaped cluster `[clusterStart, next)` (not in trailing unshaped text).
    private static func isInsideShaped(_ b: Int, clusterStart: Int, next: Int, map: TextIndexMap) -> Bool {
        let tail = map.substring(b..<next)
        return !tail.allSatisfy { $0 == " " || $0 == "\t" || separators.contains($0) }
    }

    // MARK: Stops

    public var textLength: Int { map.utf8Count }

    /// The nearest caret stop at or before `o` (at or after with `forward`).
    public func snap(_ o: Int, forward: Bool = false) -> Int {
        let o = max(0, min(o, textLength))
        if forward { return caretStops.first { $0 >= o } ?? textLength }
        return caretStops.last { $0 <= o } ?? 0
    }

    public func previousStop(_ o: Int) -> Int { caretStops.last { $0 < o } ?? 0 }
    public func nextStop(_ o: Int) -> Int { caretStops.first { $0 > o } ?? textLength }

    /// Word boundaries (⌥-arrows, double-click): stops at transitions between word and non-word characters.
    public func wordRange(at o: Int) -> Range<Int> {
        let isWord = { (c: Character) in c.isLetter || c.isNumber || c == "_" }
        var a = snap(o), b = snap(o, forward: true)
        func char(_ r: Range<Int>) -> Character? { map.substring(r).first }
        while a > 0, let c = char(previousStop(a)..<a), isWord(c) { a = previousStop(a) }
        while b < textLength, let c = char(b..<nextStop(b)), isWord(c) { b = nextStop(b) }
        if a == b, b < textLength { b = nextStop(b) }
        return a..<b
    }

    // MARK: Lines

    /// The line showing offset `o` (the next line at a line break).
    public func lineIndex(for o: Int) -> Int? {
        guard !layout.lines.isEmpty else { return nil }
        for (i, l) in layout.lines.enumerated() where o >= l.source.lowerBound && o < l.source.upperBound { return i }
        if let i = layout.lines.lastIndex(where: { $0.source.lowerBound <= o }) { return i }
        return 0
    }

    /// After a final separator the caret sits on a new empty line below the last one.
    private func trailingLine() -> TextLineInfo? {
        guard let last = layout.lines.last, textLength > 0, last.source.upperBound == textLength,
              let lastChar = model.text.last, Self.separators.contains(lastChar) else { return nil }
        let step = last.ascent + last.descent > 0 ? (last.ascent / 0.9) * 1.2 : 24
        let x = model.paragraph.leftIndent
        return TextLineInfo(source: textLength..<textLength, glyphs: layout.glyphs.count..<layout.glyphs.count,
                            x: Double(x), baseline: last.baseline + step, width: 0, availableWidth: last.availableWidth,
                            ascent: last.ascent, descent: last.descent)
    }

    /// A caret at `o`: its top and bottom points (local pixels) and line.
    public func caret(at o: Int) -> (top: CGPoint, bottom: CGPoint, line: Int) {
        let o = max(0, min(o, textLength))
        if o == textLength, let t = trailingLine() {
            return (CGPoint(x: t.x, y: t.baseline - t.ascent), CGPoint(x: t.x, y: t.baseline + t.descent), layout.lines.count)
        }
        guard let li = lineIndex(for: o) else {
            let size = Double(model.runs.first?.size ?? 24)
            let x = model.textBox.isParagraph ? Double(model.paragraph.leftIndent + model.paragraph.firstLineIndent) : 0
            return (CGPoint(x: x, y: 0), CGPoint(x: x, y: size * 1.15), 0)
        }
        let line = layout.lines[li]
        let x = caretX(o, line: line)
        return (CGPoint(x: x, y: line.baseline - line.ascent), CGPoint(x: x, y: line.baseline + line.descent), li)
    }

    private func caretX(_ o: Int, line: TextLineInfo) -> Double {
        let gs = layout.glyphs[line.glyphs]
        guard !gs.isEmpty else { return line.x }
        let at = gs.filter { $0.cluster == o }
        if let g = at.first {
            return g.rtl ? at.map { $0.x + $0.advance }.max()! : at.map(\.x).min()!
        }
        // Trailing edge of the cluster ending at `o`.
        if let c = gs.map(\.cluster).filter({ $0 < o }).max() {
            let cg = gs.filter { $0.cluster == c }
            return cg[0].rtl ? cg.map(\.x).min()! : cg.map { $0.x + $0.advance }.max()!
        }
        // Before every cluster of the line: the leading edge of the first logical cluster.
        let first = gs.map(\.cluster).min()!
        let fg = gs.filter { $0.cluster == first }
        return fg[0].rtl ? fg.map { $0.x + $0.advance }.max()! : fg.map(\.x).min()!
    }

    /// The caret offset nearest a local point (visual: RTL clusters swap their edges).
    public func hitTest(_ p: CGPoint) -> Int {
        guard !layout.lines.isEmpty else { return textLength }
        var best = 0
        var bestDistance = Double.infinity
        for (i, l) in layout.lines.enumerated() {
            let top = l.baseline - l.ascent, bottom = l.baseline + l.descent
            let d = p.y < top ? top - p.y : p.y > bottom ? p.y - bottom : 0
            if d < bestDistance { bestDistance = d; best = i }
        }
        if let t = trailingLine(), p.y > t.baseline - t.ascent, bestDistance > 0, p.y > layout.lines[best].baseline {
            return textLength
        }
        let line = layout.lines[best]
        let boxes = clusterBoxes(line: best)
        guard !boxes.isEmpty else { return snap(line.source.lowerBound) }
        let sorted = boxes.sorted { $0.x0 < $1.x0 }
        let x = p.x
        if x <= sorted[0].x0 {
            let b = sorted[0]
            return snap(b.rtl ? b.end : b.start)
        }
        if x >= sorted[sorted.count - 1].x1 {
            let b = sorted[sorted.count - 1]
            return snap(b.rtl ? b.start : b.end)
        }
        let b = sorted.first { x >= $0.x0 && x < $0.x1 } ?? sorted.min { abs(($0.x0 + $0.x1) / 2 - x) < abs(($1.x0 + $1.x1) / 2 - x) }!
        let leftHalf = x < (b.x0 + b.x1) / 2
        let o = leftHalf != b.rtl ? b.start : b.end
        return snap(o)
    }

    struct ClusterBox { var x0: Double; var x1: Double; var start: Int; var end: Int; var rtl: Bool }

    private func clusterBoxes(line li: Int) -> [ClusterBox] {
        let line = layout.lines[li]
        let gs = layout.glyphs[line.glyphs]
        return lineClusters[li].map { c in
            let cg = gs.filter { $0.cluster == c.start }
            let x0 = cg.map(\.x).min() ?? line.x
            let x1 = cg.map { $0.x + $0.advance }.max() ?? line.x
            return ClusterBox(x0: x0, x1: x1, start: c.start, end: c.end, rtl: cg.first?.rtl ?? false)
        }
    }

    /// Selection rectangles for `r` (local pixels), one or more per line, visual.
    public func selectionRects(_ r: Range<Int>) -> [CGRect] {
        guard !r.isEmpty else { return [] }
        var out: [CGRect] = []
        for (li, line) in layout.lines.enumerated() {
            let top = line.baseline - line.ascent, h = line.ascent + line.descent
            var spans: [(Double, Double)] = []
            for b in clusterBoxes(line: li) where b.start < r.upperBound && b.end > r.lowerBound {
                spans.append((b.x0, b.x1))
            }
            // A selected line break shows as a sliver at the line end.
            if line.source.upperBound > line.source.lowerBound, r.contains(line.source.upperBound - 1),
               let last = model.text.utf8.count >= line.source.upperBound ? map.substring((line.source.upperBound - 1)..<line.source.upperBound).first : nil,
               Self.separators.contains(last) {
                let end = (clusterBoxes(line: li).map(\.x1).max() ?? line.x)
                spans.append((end, end + max(line.ascent * 0.3, 2)))
            }
            spans.sort { $0.0 < $1.0 }
            var merged: [(Double, Double)] = []
            for s in spans {
                if let l = merged.last, s.0 <= l.1 + 0.01 { merged[merged.count - 1].1 = max(l.1, s.1) } else { merged.append(s) }
            }
            out += merged.map { CGRect(x: $0.0, y: top, width: $0.1 - $0.0, height: h) }
        }
        return out
    }

    /// Up / down arrows: the offset on the neighbouring line nearest the caret's x.
    public func verticalMove(from o: Int, down: Bool, preferredX: Double? = nil) -> Int {
        let c = caret(at: o)
        let x = preferredX ?? c.top.x
        let target = c.line + (down ? 1 : -1)
        if target < 0 { return 0 }
        if target >= layout.lines.count {
            return down ? textLength : o
        }
        let l = layout.lines[target]
        return hitTest(CGPoint(x: x, y: l.baseline))
    }

    /// Start / end of the line showing `o` (⌘← / ⌘→).
    public func lineBounds(of o: Int) -> Range<Int> {
        guard let li = lineIndex(for: o) else { return 0..<textLength }
        let l = layout.lines[li]
        let clusters = lineClusters[li]
        let end = clusters.map(\.end).max() ?? l.source.lowerBound
        return l.source.lowerBound..<max(l.source.lowerBound, end)
    }
}
