import Foundation
import TesseraFFI

// MARK: - Sidebar tree

/// One album, album group or smart album from library.json, with its children.
public struct CollectionNode: Identifiable, Equatable, Sendable {
    public enum Kind: Sendable, Equatable { case group, album, smartAlbum }
    public var id: Int64
    public var kind: Kind
    public var name: String
    public var parent: Int64?
    public var depth: Int
    /// Album handle (library.json key and basket target name).
    public var handle: String?
    public var imageCount: Int
    public var rule: String?
    public var scoped: Bool
    public var children: [CollectionNode] = []

    init(_ n: LibraryNode) {
        id = n.id
        kind = switch n.kind { case .group: .group; case .album: .album; case .smartAlbum: .smartAlbum }
        name = n.name
        parent = n.parent
        depth = Int(n.depth)
        handle = n.handle
        imageCount = Int(n.imageCount)
        rule = n.rule
        scoped = n.scoped
    }

    /// Builds the tree from the engine's depth-first listing (sibling order preserved).
    public static func tree(_ flat: [LibraryNode]) -> [CollectionNode] {
        var byParent: [Int64?: [CollectionNode]] = [:]
        var order: [Int64?] = []
        for n in flat {
            let key: Int64? = n.depth == 0 ? nil : n.parent
            if byParent[key] == nil { order.append(key) }
            byParent[key, default: []].append(CollectionNode(n))
        }
        func build(_ parent: Int64?) -> [CollectionNode] {
            (byParent[parent] ?? []).map { node in
                var node = node
                if node.kind == .group { node.children = build(node.id) }
                return node
            }
        }
        return build(nil)
    }

    /// Depth-first flattening of `nodes`.
    public static func flatten(_ nodes: [CollectionNode]) -> [CollectionNode] {
        nodes.flatMap { [$0] + flatten($0.children) }
    }
}

// MARK: - Filter bar state

/// Filter bar (docs/01 §1.9, adapted): facets are ANDed, values within a facet ORed, and the
/// free text uses the saved-search grammar. Encodes to the engine's `FacetFilter`s.
public struct LibraryFilter: Equatable, Sendable {
    public var text = ""
    public var decisions: Set<String> = []
    public var grades: Set<String> = []
    public var marks: Set<String> = []
    public var cameras: Set<String> = []
    public var lenses: Set<String> = []
    public var keywords: Set<String> = []
    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`; either side may be empty.
    public var dateFrom = ""
    public var dateTo = ""
    /// "none" (not in any album) or "any".
    public var albumStatus: String?

    public init() {}

    public var isEmpty: Bool { activeFacetCount == 0 && text.trimmingCharacters(in: .whitespaces).isEmpty }

    public var activeFacetCount: Int {
        [decisions, grades, marks, cameras, lenses, keywords].filter { !$0.isEmpty }.count
            + (dateValue == nil ? 0 : 1) + (albumStatus == nil ? 0 : 1)
    }

    /// Open-ended ranges use the grammar's calendar bounds.
    public var dateValue: String? {
        let from = dateFrom.trimmingCharacters(in: .whitespaces)
        let to = dateTo.trimmingCharacters(in: .whitespaces)
        switch (from.isEmpty, to.isEmpty) {
        case (true, true): return nil
        case (false, true): return "\(from)..9998"
        case (true, false): return "0001..\(to)"
        case (false, false): return from == to ? from : "\(from)..\(to)"
        }
    }

    public var facetFilters: [FacetFilter] {
        var out: [FacetFilter] = []
        func add(_ field: FacetField, _ values: Set<String>) {
            if !values.isEmpty { out.append(FacetFilter(field: field, values: values.sorted())) }
        }
        add(.decision, decisions)
        add(.grade, grades)
        add(.mark, marks)
        add(.camera, cameras)
        add(.lens, lenses)
        add(.keyword, keywords)
        if let d = dateValue { out.append(FacetFilter(field: .date, values: [d])) }
        if let a = albumStatus { out.append(FacetFilter(field: .album, values: [a])) }
        return out
    }
}

// MARK: - Diagnostics

public extension RuleDiagnostic {
    /// The diagnostic's UTF-8 byte range as a `String` range (clamped to `text`).
    func range(in text: String) -> Range<String.Index> {
        let utf8 = text.utf8
        func index(_ offset: UInt32) -> String.Index {
            var i = utf8.index(utf8.startIndex, offsetBy: min(Int(offset), utf8.count))
            // Round down to a scalar boundary (the engine reports boundaries already).
            while i > text.startIndex, i.samePosition(in: text.unicodeScalars) == nil { i = utf8.index(before: i) }
            return i
        }
        let lower = index(start)
        let upper = max(lower, index(end))
        return lower..<upper
    }

    /// UTF-16 range for AppKit text attributes.
    func nsRange(in text: String) -> NSRange { NSRange(range(in: text), in: text) }
}

// MARK: - Editable rule tree

/// Smart album rule editor model: AND / OR / NOT groups with nested conditions. Converts to and
/// from the engine's preorder `RuleItem`s; the engine renders and validates the text.
public struct RuleNode: Identifiable, Equatable, Sendable {
    public enum Kind: String, Sendable, CaseIterable { case all, any, not, rule }
    public var id = UUID()
    public var kind: Kind
    public var field = "keyword"
    public var op = ":"
    public var value = ""
    public var children: [RuleNode] = []

    public init(kind: Kind, field: String = "keyword", op: String = ":", value: String = "", children: [RuleNode] = []) {
        self.kind = kind; self.field = field; self.op = op; self.value = value; self.children = children
    }

    public static func condition(_ field: String = "keyword", _ op: String = ":", _ value: String = "") -> RuleNode {
        RuleNode(kind: .rule, field: field, op: op, value: value)
    }

    public var isGroup: Bool { kind != .rule }

    public var items: [RuleItem] {
        if kind == .rule { return [RuleItem(kind: .rule, children: 0, field: field, op: op, value: value)] }
        let k: RuleItemKind = switch kind { case .all: .all; case .any: .any; case .not: .not; case .rule: .rule }
        return [RuleItem(kind: k, children: UInt32(children.count), field: "", op: "", value: "")] + children.flatMap(\.items)
    }

    /// Rebuilds a tree; the root is always a group (a single condition becomes "all of: it").
    public static func from(_ items: [RuleItem]) -> RuleNode? {
        var i = 0
        func next() -> RuleNode? {
            guard i < items.count else { return nil }
            let item = items[i]
            i += 1
            switch item.kind {
            case .rule:
                return .condition(item.field, item.op, item.value)
            case .all, .any, .not:
                var node = RuleNode(kind: item.kind == .all ? .all : item.kind == .any ? .any : .not)
                for _ in 0..<item.children {
                    guard let child = next() else { return nil }
                    node.children.append(child)
                }
                return node
            }
        }
        guard let root = next(), i == items.count else { return nil }
        return root.isGroup ? root : RuleNode(kind: .all, children: [root])
    }
}

/// Fields of the grammar offered by the rule editor, with their operators.
public struct RuleField: Sendable, Identifiable {
    public let key: String
    public let title: String
    public let ops: [(op: String, title: String)]
    public let placeholder: String
    public var id: String { key }

    static let equality: [(op: String, title: String)] = [(":", "is"), ("!=", "is not")]
    static let numeric: [(op: String, title: String)] = [(">=", "≥"), ("<=", "≤"), ("=", "="), (">", ">"), ("<", "<"), ("!=", "≠")]

    public static let all: [RuleField] = [
        RuleField(key: "keyword", title: "Keyword", ops: equality, placeholder: "beach (includes child keywords)"),
        RuleField(key: "text", title: "Any text", ops: [(":", "contains")], placeholder: "words in name, caption, keywords, text in image"),
        RuleField(key: "rating", title: "Grade", ops: numeric, placeholder: "0–3"),
        RuleField(key: "decision", title: "Decision", ops: equality, placeholder: "keep / reject / undecided"),
        RuleField(key: "mark", title: "Mark", ops: equality, placeholder: "Needs Retouch"),
        RuleField(key: "camera", title: "Camera", ops: equality, placeholder: "exact model"),
        RuleField(key: "lens", title: "Lens", ops: equality, placeholder: "exact lens"),
        RuleField(key: "date", title: "Capture date", ops: [(":", "in")], placeholder: "2024 · 2024-06 · 2024-01..2024-06"),
        RuleField(key: "album", title: "Album", ops: equality, placeholder: "album name, none or any"),
        RuleField(key: "focus", title: "Focus score", ops: numeric, placeholder: "0–1"),
        RuleField(key: "person", title: "Person", ops: equality, placeholder: "name keyword"),
    ]

    public static func named(_ key: String) -> RuleField {
        all.first { $0.key == key } ?? RuleField(key: key, title: key, ops: equality, placeholder: "")
    }
}

// MARK: - Catalog facade

/// Search outcome mapped to the open folder's item ids.
public struct CatalogMatch: Sendable {
    /// Matching item ids: album order for an album scope, else capture time.
    public var ids: [Int]
    public var facets: SearchFacets
    public var diagnostic: RuleDiagnostic?
    /// Grammar text for "Save as Smart Album" (empty: nothing narrows the view).
    public var rule: String
    public var group: Int64?
}

/// The library.json + catalog operations the shell uses for albums, smart albums, the filter
/// bar, keywords and metadata. Blocking engine calls; cheap enough for the main actor at folder
/// scale, and `LibraryStore` is thread-safe for background use.
public final class LibraryCatalog: @unchecked Sendable {
    public let store: LibraryStore
    /// Canonical folder of the open library (search scope).
    public let folder: String
    /// Item ↔ image id maps of the library's current layout (see `libraryDidUpdate`).
    private let lock = NSLock()
    private var itemOfImage: [String: Int]
    private var imageIDs: [String]

    public init(library: EngineLibrary) throws {
        let path = try library.session.libraryPath()
            ?? URL(fileURLWithPath: library.folder?.path ?? "/").appendingPathComponent("library.json").path
        store = try library.engine.openLibrary(path: path)
        folder = URL(fileURLWithPath: path).deletingLastPathComponent().path
        itemOfImage = library.itemOfImage
        imageIDs = library.imageIDs
    }

    /// The library was updated in place: item ids now follow its new layout.
    public func libraryDidUpdate(_ library: EngineLibrary) {
        let (map, ids) = (library.itemOfImage, library.imageIDs)
        lock.withLock { itemOfImage = map; imageIDs = ids }
    }

    public func imageIDs(for items: [Int]) -> [String] {
        lock.withLock { items.compactMap { imageIDs.indices.contains($0) ? imageIDs[$0] : nil } }
    }
    public func items(for ids: [String]) -> [Int] { lock.withLock { ids.compactMap { itemOfImage[$0] } } }
    public func imageID(of item: Int) -> String? { lock.withLock { imageIDs.indices.contains(item) ? imageIDs[item] : nil } }

    public func nodes() throws -> [CollectionNode] { CollectionNode.tree(try store.nodes()) }

    public func search(_ filter: LibraryFilter, scope: SearchScope) throws -> CatalogMatch {
        let r = try store.search(request: SearchRequest(text: filter.text, filters: filter.facetFilters,
                                                        scope: scope, folder: folder))
        return CatalogMatch(ids: items(for: r.imageIds), facets: r.facets, diagnostic: r.diagnostic,
                            rule: r.rule, group: r.group)
    }

    public func albumMembers(_ id: Int64) throws -> [Int] { items(for: try store.albumImages(id: id)) }
    public func addToAlbum(_ id: Int64, items: [Int]) throws { try store.addToAlbum(id: id, imageIds: imageIDs(for: items)) }
    public func reorderAlbum(_ id: Int64, items: [Int]) throws { try store.reorderAlbum(id: id, imageIds: imageIDs(for: items)) }

    /// Moves `moving` (album members, in their current order) to sit before `before` (nil: end).
    public static func reordered(_ members: [Int], moving: [Int], before: Int?) -> [Int] {
        let set = Set(moving)
        let kept = members.filter { !set.contains($0) }
        let moved = members.filter { set.contains($0) }
        let at = before.flatMap { b in kept.firstIndex(of: b) } ?? kept.count
        return Array(kept[..<at]) + moved + Array(kept[at...])
    }

    public func keywords() throws -> [KeywordInfo] { try store.keywords(folder: folder) }
    public func applyKeywords(_ names: [String], to items: [Int], add: Bool) throws {
        try store.applyKeywords(imageIds: imageIDs(for: items), names: names, add: add)
    }
    public func metadata(of item: Int) throws -> ImageMetadata? {
        guard let id = imageID(of: item) else { return nil }
        return try store.metadata(imageId: id)
    }
    public func setIPTC(_ edit: IptcEdit, items: [Int]) throws { try store.setIptc(imageIds: imageIDs(for: items), edit: edit) }
}
