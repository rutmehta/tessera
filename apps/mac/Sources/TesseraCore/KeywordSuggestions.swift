import Foundation
import TesseraFFI

// MARK: - Suggested keywords (Keywords panel ▸ Suggested)

/// One suggested keyword, merged over the selection by the engine.
public struct SuggestedKeyword: Identifiable, Equatable, Sendable {
    public var keyword: String
    /// 0...1: independent per-keyword scores, not shares of 100 %.
    public var confidence: Double
    /// Photos of the selection it is suggested for (and not yet applied to).
    public var images: Int
    /// Where accepting puts it in the keyword tree.
    public var path: [String]
    /// Already in the keyword tree (by name or synonym); false: new under "Suggested".
    public var existing: Bool
    /// Several tree keywords match; accepting is refused until the tree is disambiguated.
    public var ambiguous: Bool

    public var id: String { keyword.lowercased() }

    public init(keyword: String, confidence: Double, images: Int = 1, path: [String]? = nil,
                existing: Bool = false, ambiguous: Bool = false) {
        self.keyword = keyword
        self.confidence = min(max(confidence, 0), 1)
        self.images = images
        self.path = path ?? ["Suggested", keyword]
        self.existing = existing
        self.ambiguous = ambiguous
    }

    public init(_ info: KeywordSuggestionInfo) {
        self.init(keyword: info.keyword, confidence: Double(info.confidence), images: Int(info.images),
                  path: info.path, existing: info.existing, ambiguous: info.ambiguous)
    }

    /// "Places › Beach" when it maps under an existing parent; nil for a root or a new keyword.
    public var mappedPath: String? {
        existing && path.count > 1 ? path.joined(separator: " › ") : nil
    }

    /// Whole percent for the chip's tooltip and accessibility value.
    public var percent: String { "\(Int((confidence * 100).rounded())) %" }
}

/// The Suggested section's chips: click accepts one, ⇧-click accepts every chip at or above the
/// threshold, ✕ rejects. Accepted and rejected chips leave the list at once (the engine is
/// updated by the caller with the returned keywords).
public struct SuggestionChips: Equatable, Sendable {
    public static let defaultThreshold = 0.5

    /// Highest confidence first (ties by name), never duplicated.
    public private(set) var items: [SuggestedKeyword]
    /// 0...1, for "accept all above".
    public var threshold: Double {
        didSet { threshold = min(max(threshold, 0), 1) }
    }

    public init(_ items: [SuggestedKeyword] = [], threshold: Double = SuggestionChips.defaultThreshold) {
        var seen = Set<String>()
        self.items = items
            .filter { seen.insert($0.id).inserted }
            .sorted { $0.confidence != $1.confidence ? $0.confidence > $1.confidence : $0.keyword < $1.keyword }
        self.threshold = min(max(threshold, 0), 1)
    }

    public var isEmpty: Bool { items.isEmpty }

    /// Chips a ⇧-click would accept (ambiguous ones are left for the photographer).
    public var aboveThreshold: [SuggestedKeyword] {
        items.filter { $0.confidence >= threshold && !$0.ambiguous }
    }

    /// Whether a chip is at or above the threshold (drawn with a stronger bar).
    public func isAboveThreshold(_ s: SuggestedKeyword) -> Bool { s.confidence >= threshold }

    /// Click: accept one. Returns the keywords to send to the engine (empty when unknown).
    public mutating func accept(_ keyword: String) -> [String] {
        guard let i = index(of: keyword), !items[i].ambiguous else { return [] }
        return [items.remove(at: i).keyword]
    }

    /// ⇧-click: accept every chip at or above the threshold.
    public mutating func acceptAllAboveThreshold() -> [String] {
        let chosen = aboveThreshold
        let ids = Set(chosen.map(\.id))
        items.removeAll { ids.contains($0.id) }
        return chosen.map(\.keyword)
    }

    /// ✕: reject one. Returns the keywords to send to the engine.
    public mutating func reject(_ keyword: String) -> [String] {
        guard let i = index(of: keyword) else { return [] }
        return [items.remove(at: i).keyword]
    }

    /// Fresh engine results replace the list; the threshold stays.
    public mutating func replace(with fresh: [SuggestedKeyword]) {
        self = SuggestionChips(fresh, threshold: threshold)
    }

    private func index(of keyword: String) -> Int? {
        let id = keyword.lowercased()
        return items.firstIndex { $0.id == id }
    }
}

// MARK: - Search terms (filter bar)

/// Builds saved-search grammar terms from values (library crate: quoted values use JSON string
/// escaping; `text:` matches names, keywords, captions, generated captions and text in images).
public enum SearchTerm {
    /// `text:"exit only"`; whitespace is collapsed, empty input gives "".
    public static func text(_ value: String) -> String {
        let words = value.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        guard !words.isEmpty else { return "" }
        return "text:" + quoted(words)
    }

    /// A JSON string literal (the grammar's quoting), without escaped slashes.
    public static func quoted(_ value: String) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        guard let data = try? encoder.encode(value), let s = String(data: data, encoding: .utf8) else { return "\"\"" }
        return s
    }

    /// ANDs `term` onto existing filter text (adjacency is AND). Text containing a top-level OR
    /// is parenthesised first so the new term applies to the whole of it.
    public static func appending(_ term: String, to text: String) -> String {
        let existing = text.trimmingCharacters(in: .whitespaces)
        guard !term.isEmpty else { return existing }
        guard !existing.isEmpty else { return term }
        if existing.contains(term) { return existing }
        return (hasTopLevelOr(existing) ? "(\(existing))" : existing) + " " + term
    }

    static func hasTopLevelOr(_ s: String) -> Bool {
        var depth = 0, quoted = false, escaped = false
        var word = ""
        func flush() -> Bool { defer { word = "" }; return depth == 0 && word.uppercased() == "OR" }
        for c in s {
            if quoted {
                if escaped { escaped = false } else if c == "\\" { escaped = true } else if c == "\"" { quoted = false }
                continue
            }
            switch c {
            case "\"": if flush() { return true }; quoted = true
            case "(": if flush() { return true }; depth += 1
            case ")": if flush() { return true }; depth = max(0, depth - 1)
            case _ where c.isWhitespace: if flush() { return true }
            default: word.append(c)
            }
        }
        return flush()
    }
}
