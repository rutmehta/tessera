import Foundation

// Run surgery and the editing session of the Type tool (WP B5-10), UI-free and unit tested
// (DocumentTextTests). All offsets are UTF-8 offsets into the concatenated run text; callers pass
// caret stops (`TextLayoutIndex.caretStops`), so runs are only ever split at grapheme and shaping
// cluster boundaries.

public enum TextRuns {
    /// Start offsets of each run and the total length.
    public static func starts(_ runs: [TextRunModel]) -> [Int] {
        var out: [Int] = []
        var o = 0
        for r in runs {
            out.append(o)
            o += r.text.utf8.count
        }
        out.append(o)
        return out
    }

    /// Splits `text` at UTF-8 offset `o` (must be a scalar boundary).
    static func split(_ text: String, at o: Int) -> (String, String) {
        let u = text.utf8
        let i = u.index(u.startIndex, offsetBy: max(0, min(o, u.count)))
        precondition(i.samePosition(in: text.unicodeScalars) != nil, "split inside a scalar")
        return (String(text[..<i]), String(text[i...]))
    }

    /// Splits runs so a run boundary falls at `o`; returns the index of the run starting there.
    public static func splitRuns(_ runs: inout [TextRunModel], at o: Int) -> Int {
        let s = starts(runs)
        if let i = s.firstIndex(of: o), i < runs.count { return i }
        if o >= s.last! { return runs.count }
        guard let i = (0..<runs.count).last(where: { s[$0] < o }) else { return 0 }
        let (a, b) = split(runs[i].text, at: o - s[i])
        var tail = runs[i]
        tail.text = b
        runs[i].text = a
        runs.insert(tail, at: i + 1)
        return i + 1
    }

    /// Removes empty runs, keeping one (with its style) when nothing is left.
    public static func dropEmpty(_ runs: inout [TextRunModel]) {
        let kept = runs.filter { !$0.text.isEmpty }
        if kept.isEmpty {
            if let first = runs.first { runs = [first] }
        } else {
            runs = kept
        }
    }

    /// Replaces UTF-8 range `r` with `text` in `style` (nil: the style at the edit). Text joins a
    /// neighbouring run of the same style instead of creating a new run.
    public static func replace(_ runs: [TextRunModel], range r: Range<Int>, with text: String,
                               style: TextRunModel? = nil) -> [TextRunModel] {
        var runs = runs
        if runs.isEmpty { runs = [style?.style ?? TextRunModel()] }
        let insertStyle = (style ?? Self.style(at: r.lowerBound, in: runs, selecting: !r.isEmpty)).style
        // Delete.
        if !r.isEmpty {
            let a = splitRuns(&runs, at: r.lowerBound)
            let b = splitRuns(&runs, at: r.upperBound)
            for i in a..<b { runs[i].text = "" }
        }
        guard !text.isEmpty else {
            dropEmpty(&runs)
            return runs
        }
        // Insert at r.lowerBound: prefer the run ending there, then the one starting there.
        let s = starts(runs)
        let o = r.lowerBound
        if let i = (0..<runs.count).last(where: { s[$0] < o && s[$0 + 1] >= o }), runs[i].sameStyle(insertStyle) {
            let (a, b) = split(runs[i].text, at: o - s[i])
            runs[i].text = a + text + b
        } else if let i = (0..<runs.count).first(where: { s[$0] == o && runs[$0].sameStyle(insertStyle) }) {
            runs[i].text = text + runs[i].text
        } else {
            let at = splitRuns(&runs, at: o)
            var new = insertStyle
            new.text = text
            runs.insert(new, at: at)
        }
        dropEmpty(&runs)
        return runs
    }

    /// The style typed text takes at `o`: the run before the caret (the first selected character's
    /// run when replacing a selection), or the first run at the start.
    public static func style(at o: Int, in runs: [TextRunModel], selecting: Bool = false) -> TextRunModel {
        guard !runs.isEmpty else { return TextRunModel() }
        let s = starts(runs)
        if selecting || o == 0 {
            if let i = (0..<runs.count).first(where: { s[$0] <= o && s[$0 + 1] > o }) { return runs[i] }
            return runs[0]
        }
        if let i = (0..<runs.count).last(where: { s[$0] < o && s[$0 + 1] >= o }) { return runs[i] }
        return runs[runs.count - 1]
    }

    /// Applies `change` to the runs covering `r`, splitting at its ends. Runs outside are untouched.
    public static func applyStyle(_ runs: [TextRunModel], range r: Range<Int>,
                                  _ change: (inout TextRunModel) -> Void) -> [TextRunModel] {
        guard !r.isEmpty else { return runs }
        var runs = runs
        let a = splitRuns(&runs, at: r.lowerBound)
        let b = splitRuns(&runs, at: r.upperBound)
        for i in a..<b { change(&runs[i]) }
        dropEmpty(&runs)
        return runs
    }

    /// Runs overlapping `r` (the run before an empty caret range).
    public static func runs(_ runs: [TextRunModel], covering r: Range<Int>) -> [TextRunModel] {
        let s = starts(runs)
        if r.isEmpty { return [style(at: r.lowerBound, in: runs)] }
        return (0..<runs.count).filter { s[$0] < r.upperBound && s[$0 + 1] > r.lowerBound && !runs[$0].text.isEmpty }.map { runs[$0] }
    }

    /// The run-index splice turning `base` into `new` (common leading/trailing runs kept); nil when
    /// equal. Mirrors the engine's derivation so hosts can predict `EditTextRuns`.
    public static func splice(from base: [TextRunModel], to new: [TextRunModel]) -> (range: Range<Int>, runs: [TextRunModel])? {
        guard base != new else { return nil }
        var p = 0
        while p < base.count, p < new.count, base[p] == new[p] { p += 1 }
        var sfx = 0
        while sfx < min(base.count, new.count) - p, base[base.count - 1 - sfx] == new[new.count - 1 - sfx] { sfx += 1 }
        return (p..<(base.count - sfx), Array(new[p..<(new.count - sfx)]))
    }
}

/// A value shown by a Character control over a selection: one value, or mixed.
public enum TextStyleValue<T: Equatable & Sendable>: Equatable, Sendable {
    case one(T)
    case mixed
    public var value: T? { if case .one(let v) = self { v } else { nil } }

    public static func of(_ values: [T]) -> TextStyleValue<T>? {
        guard let first = values.first else { return nil }
        return values.allSatisfy { $0 == first } ? .one(first) : .mixed
    }
}

/// The Character panel's view of a selection.
public struct TextStyleSummary: Equatable, Sendable {
    public var family: TextStyleValue<String>?
    public var weight: TextStyleValue<UInt16>?
    public var italic: TextStyleValue<Bool>?
    public var size: TextStyleValue<Float>?
    public var tracking: TextStyleValue<Float>?
    public var leading: TextStyleValue<Float>?
    public var baselineShift: TextStyleValue<Float>?
    public var kerning: TextStyleValue<Bool>?
    public var color: TextStyleValue<[UInt8]>?

    public init(_ runs: [TextRunModel]) {
        family = .of(runs.map(\.family))
        weight = .of(runs.map(\.weight))
        italic = .of(runs.map(\.italic))
        size = .of(runs.map(\.size))
        tracking = .of(runs.map(\.tracking))
        leading = .of(runs.map(\.leading))
        baselineShift = .of(runs.map(\.baselineShift))
        kerning = .of(runs.map(\.kerning))
        color = .of(runs.map(\.color))
    }
}

/// The Type tool's host-side editing state: the draft model, the selection (a caret when empty) and
/// IME composition. Marked text lives in the draft (so the canvas preview shows it) but is never
/// committed: `cancelComposition` restores the model from before composition began, and history only
/// ever records a whole draft.
public struct TextEditSession: Equatable, Sendable {
    /// The committed model the session started from (Esc restores it).
    public private(set) var base: TextSourceModel
    public private(set) var model: TextSourceModel
    /// UTF-8 selection; empty = caret.
    public private(set) var selection: Range<Int>
    /// UTF-8 range of the marked (composing) text in `model`.
    public private(set) var marked: Range<Int>?
    /// The model and selection before composition began.
    private var beforeComposition: (model: TextSourceModel, selection: Range<Int>)?
    /// Style for the next typed text at an empty caret (Character edits with nothing selected).
    public var typingStyle: TextRunModel?
    /// Caret stops of the current model (from the engine layout); nil until a layout arrives, then
    /// every edit snaps to them.
    public var stops: [Int]?

    public static func == (a: TextEditSession, b: TextEditSession) -> Bool {
        a.base == b.base && a.model == b.model && a.selection == b.selection && a.marked == b.marked
            && a.typingStyle == b.typingStyle && a.beforeComposition?.model == b.beforeComposition?.model
            && a.beforeComposition?.selection == b.beforeComposition?.selection
    }

    public init(model: TextSourceModel, selection: Range<Int>? = nil) {
        base = model
        self.model = model
        let n = model.utf8Count
        self.selection = selection ?? n..<n
    }

    public var text: String { model.text }
    public var length: Int { model.utf8Count }
    public var caret: Int { selection.upperBound }
    public var isComposing: Bool { marked != nil }
    /// The draft differs from its base.
    public var isChanged: Bool { model != base }

    private func clamp(_ r: Range<Int>) -> Range<Int> {
        let n = length
        let a = max(0, min(r.lowerBound, n)), b = max(a, min(r.upperBound, n))
        let map = TextIndexMap(model.text)
        return map.scalarFloor(a)..<map.scalarFloor(b)
    }

    public mutating func select(_ r: Range<Int>) {
        selection = clamp(r)
    }

    /// The draft is committed: it becomes the new base (the session continues).
    public mutating func rebase() {
        base = model
    }

    /// `m` (a model the draft passed through) was committed: it is the new base.
    public mutating func rebase(to m: TextSourceModel) {
        base = m
    }

    /// Replaces the model wholesale (inspector paragraph/box edits, a reloaded layer).
    public mutating func setModel(_ m: TextSourceModel, keepSelection: Bool = true) {
        model = m
        selection = keepSelection ? clamp(selection) : m.utf8Count..<m.utf8Count
        marked = nil
        beforeComposition = nil
    }

    // MARK: Typing

    /// Types `s` over the selection (or the marked text, committing the composition).
    public mutating func insert(_ s: String, replacing r: Range<Int>? = nil) {
        let target = r.map(clamp) ?? marked ?? selection
        let style = typingStyle.flatMap { target.isEmpty ? $0 : nil }
        model.runs = TextRuns.replace(model.runs, range: target, with: s, style: style)
        let c = target.lowerBound + s.utf8.count
        selection = c..<c
        marked = nil
        beforeComposition = nil
        typingStyle = nil
    }

    /// ⌫: the selection, or the grapheme/cluster before the caret.
    public mutating func deleteBackward() {
        if !selection.isEmpty { insert(""); return }
        guard caret > 0 else { return }
        let from = previousStop(caret)
        insert("", replacing: from..<caret)
    }

    /// ⌦.
    public mutating func deleteForward() {
        if !selection.isEmpty { insert(""); return }
        guard caret < length else { return }
        insert("", replacing: caret..<nextStop(caret))
    }

    public func previousStop(_ o: Int) -> Int {
        if let stops { return stops.last { $0 < o } ?? 0 }
        let g = TextIndexMap(model.text).graphemeBoundaries
        return g.last { $0 < o } ?? 0
    }

    public func nextStop(_ o: Int) -> Int {
        if let stops { return stops.first { $0 > o } ?? length }
        let g = TextIndexMap(model.text).graphemeBoundaries
        return g.first { $0 > o } ?? length
    }

    // MARK: IME

    /// `setMarkedText`: composing text replaces the marked range (or the selection / `replacing`
    /// when composition starts). `selected` is UTF-8 within `s`.
    public mutating func setMarked(_ s: String, selected: Range<Int>, replacing r: Range<Int>? = nil) {
        if beforeComposition == nil { beforeComposition = (model, selection) }
        let target = marked ?? r.map(clamp) ?? selection
        if s.isEmpty {
            model.runs = TextRuns.replace(model.runs, range: target, with: "")
            marked = nil
            selection = target.lowerBound..<target.lowerBound
            return
        }
        model.runs = TextRuns.replace(model.runs, range: target, with: s, style: typingStyle)
        let m = target.lowerBound..<(target.lowerBound + s.utf8.count)
        marked = m
        let a = min(m.lowerBound + max(0, selected.lowerBound), m.upperBound)
        let b = min(m.lowerBound + max(0, selected.upperBound), m.upperBound)
        selection = a..<max(a, b)
    }

    /// `unmarkText`: the marked text becomes ordinary draft text (the composition is committed).
    public mutating func commitComposition() {
        guard let m = marked else { return }
        marked = nil
        beforeComposition = nil
        selection = m.upperBound..<m.upperBound
        typingStyle = nil
    }

    /// Esc / tool switch during composition: the model returns to before the composition.
    public mutating func cancelComposition() {
        guard let b = beforeComposition else { marked = nil; return }
        model = b.model
        selection = b.selection
        marked = nil
        beforeComposition = nil
    }

    // MARK: Styles

    /// A Character edit: applies to the selected runs, or (caret) to the next typed text.
    public mutating func applyStyle(_ change: (inout TextRunModel) -> Void) {
        if selection.isEmpty {
            var s = typingStyle ?? TextRuns.style(at: caret, in: model.runs)
            change(&s)
            s.text = ""
            typingStyle = s
            // An empty model shows the new style on its (empty) run.
            if length == 0, !model.runs.isEmpty {
                let text = model.runs[0].text
                model.runs[0] = s
                model.runs[0].text = text
            }
            return
        }
        model.runs = TextRuns.applyStyle(model.runs, range: selection, change)
    }

    /// Summary for the Character panel.
    public var styleSummary: TextStyleSummary {
        if selection.isEmpty, let t = typingStyle { return TextStyleSummary([t]) }
        return TextStyleSummary(TextRuns.runs(model.runs, covering: selection))
    }

    /// Paragraph edits (whole model).
    public mutating func editParagraph(_ change: (inout TextParagraphModel) -> Void) {
        change(&model.paragraph)
    }

    public mutating func setBox(_ box: TextBoxModel) { model.textBox = box }

    /// The selected text (clipboard).
    public var selectedText: String { TextIndexMap(model.text).substring(selection) }
}
