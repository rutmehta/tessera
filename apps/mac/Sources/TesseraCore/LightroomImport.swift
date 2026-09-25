import Foundation
import TesseraFFI

/// UI-free models behind File ▸ Import Lightroom Catalog… (WP M2-13b): the mapping tables the
/// user edits, the fidelity grid's sort/filter, and the final report as Markdown. The engine
/// (`LrcatImport`) does the reading and writing; these types only turn user choices into
/// `LrcatOptions` and engine results into text.

// MARK: Marks

/// What one Lightroom colour label becomes.
public enum MarkChoice: Hashable, Sendable {
    /// Keep the label text as the mark name (lossless; round-trips to Lightroom).
    case keepLabel
    /// One of Tessera's named marks.
    case mark(String)
    /// No mark.
    case drop
}

public struct MarkMappingTable: Equatable, Sendable {
    public struct Row: Identifiable, Equatable, Sendable {
        public var label: String
        public var count: Int
        public var choice: MarkChoice
        public var id: String { label }
    }

    public private(set) var rows: [Row]
    /// Tessera's named marks, in key order (6–9).
    public let appMarks: [String]

    public init(rows: [LrcatMarkRow], appMarks: [String] = CullController.markNames.sorted { $0.key < $1.key }.map(\.value)) {
        self.appMarks = appMarks
        self.rows = rows.map { r in
            let choice: MarkChoice = r.mark.isEmpty ? .drop
                : r.mark == r.label ? .keepLabel
                : .mark(r.mark)
            return Row(label: r.label, count: Int(r.count), choice: choice)
        }
    }

    public mutating func set(_ label: String, to choice: MarkChoice) {
        guard let i = rows.firstIndex(where: { $0.label == label }) else { return }
        rows[i].choice = choice
    }

    /// Mark name a label becomes; nil when dropped.
    public func mark(for label: String) -> String? {
        switch rows.first(where: { $0.label == label })?.choice ?? .keepLabel {
        case .keepLabel: label
        case .mark(let name): name
        case .drop: nil
        }
    }

    /// Labels that keep text Tessera has no key for (still stored and searchable as `mark:`).
    public var unnamedMarks: [String] {
        rows.filter { $0.choice == .keepLabel && !appMarks.contains($0.label) }.map(\.label)
    }

    /// Target marks that several labels merge into.
    public var mergedTargets: [String] {
        let targets = rows.compactMap { mark(for: $0.label) }
        return Set(targets.filter { t in targets.filter { $0 == t }.count > 1 }).sorted()
    }

    public var mappings: [LrcatMarkMapping] {
        rows.map { LrcatMarkMapping(label: $0.label, mark: mark(for: $0.label) ?? "") }
    }

    public var markedPhotos: Int { rows.filter { mark(for: $0.label) != nil }.reduce(0) { $0 + $1.count } }
}

// MARK: Folders and relocation

public struct FolderMappingTable: Equatable, Sendable {
    public struct Root: Identifiable, Equatable, Sendable {
        /// As recorded in the catalog (e.g. `/Volumes/Old Drive/Photos/`).
        public var catalogPath: String
        /// Where it is now (edited by "Locate…").
        public var path: String
        public var images: Int
        public var id: String { catalogPath }
    }

    public private(set) var roots: [Root]
    public var libraryFolder: String

    public init(options: LrcatOptions, preview: LrcatPlanPreview? = nil) {
        libraryFolder = options.libraryFolder
        roots = options.relocations.map { r in
            Root(catalogPath: r.from, path: r.to,
                 images: Int(preview?.roots.first { $0.catalogPath == r.from }?.images ?? 0))
        }
    }

    /// Points a catalog root at its new location. When every root used to sit under the library
    /// folder's old location, the library folder follows the move.
    public mutating func relocate(_ catalogPath: String, to path: String) {
        guard let i = roots.firstIndex(where: { $0.catalogPath == catalogPath }) else { return }
        let old = roots[i].path
        roots[i].path = Self.trimmed(path)
        let library = Self.trimmed(libraryFolder)
        if library == Self.trimmed(old) || Self.contains(Self.trimmed(old), library) {
            // The library folder was the root (or inside it): move it along.
            libraryFolder = Self.trimmed(path) + String(library.dropFirst(Self.trimmed(old).count))
        }
    }

    public mutating func updateCounts(from preview: LrcatPlanPreview) {
        for i in roots.indices {
            roots[i].images = Int(preview.roots.first { $0.catalogPath == roots[i].catalogPath }?.images ?? 0)
        }
    }

    public mutating func reset(_ catalogPath: String) {
        relocate(catalogPath, to: catalogPath)
    }

    public var relocations: [LrcatRelocation] {
        roots.map { LrcatRelocation(from: $0.catalogPath, to: $0.path) }
    }

    /// A folder path shown relative to its (relocated) root: "Photos/2026/wedding".
    public func displayName(_ path: String) -> String {
        let p = Self.trimmed(path)
        guard let root = roots.map({ Self.trimmed($0.path) }).filter({ Self.contains($0, p) }).max(by: { $0.count < $1.count })
        else { return p }
        let name = (root as NSString).lastPathComponent
        return p == root ? name : name + String(p.dropFirst(root == "/" ? 0 : root.count))
    }

    public func isRelocated(_ root: Root) -> Bool { Self.trimmed(root.path) != Self.trimmed(root.catalogPath) }

    public func options(marks: [LrcatMarkMapping], overwrite: Bool) -> LrcatOptions {
        LrcatOptions(libraryFolder: libraryFolder, relocations: relocations, marks: marks,
                     overwriteExistingEdits: overwrite)
    }

    static func trimmed(_ p: String) -> String {
        var s = p
        while s.count > 1, s.hasSuffix("/") { s.removeLast() }
        return s
    }

    /// `child` is `parent` or inside it (path components, not string prefixes).
    static func contains(_ parent: String, _ child: String) -> Bool {
        child == parent || child.hasPrefix(parent == "/" ? "/" : parent + "/")
    }
}

// MARK: Selection mapping

public enum SelectionText {
    /// "Reject", "Undecided", "Keep", "Keep · Grade 2 (Good)".
    public static func tessera(_ decision: TesseraFFI.Decision, grade: UInt8?) -> String {
        switch decision {
        case .reject: return "Reject"
        case .undecided: return "Undecided"
        case .keep:
            guard let g = grade, (1...3).contains(Int(g)) else { return "Keep" }
            return "Keep · Grade \(g) (\(CullState.gradeNames[Int(g)]))"
        }
    }
}

// MARK: Fidelity

public struct FidelityGrid: Equatable, Sendable {
    public enum Sort: String, CaseIterable, Identifiable, Sendable {
        case largestDifference = "Largest difference"
        case smallestDifference = "Smallest difference"
        case name = "Name"
        public var id: String { rawValue }
    }

    /// Mean ΔE2000 above which a pair counts as "looks different" (≈ clearly visible side by side),
    /// or a 95th percentile above `p95Threshold` (a region is clearly off).
    public static let meanThreshold: Float = 3
    public static let p95Threshold: Float = 10

    public var samples: [LrcatFidelitySample]
    public var sort: Sort = .largestDifference
    public var onlyDifferent = false

    public init(samples: [LrcatFidelitySample]) { self.samples = samples }

    public static func looksDifferent(_ s: LrcatFidelitySample) -> Bool {
        s.status == .compared && (s.deltaEMean >= meanThreshold || s.deltaEP95 >= p95Threshold)
    }

    public var differentCount: Int { samples.filter(Self.looksDifferent).count }

    public var visible: [LrcatFidelitySample] {
        let shown = onlyDifferent ? samples.filter(Self.looksDifferent) : samples
        // Uncompared samples (no preview, failed) sort last in the ΔE orders.
        let key: (LrcatFidelitySample) -> Float = { $0.status == .compared ? $0.deltaEMean : -1 }
        switch sort {
        case .largestDifference:
            return shown.sorted { (key($0), $1.name) > (key($1), $0.name) }
        case .smallestDifference:
            return shown.sorted { a, b in
                let (x, y) = (key(a) < 0 ? .infinity : key(a), key(b) < 0 ? .infinity : key(b))
                return x != y ? x < y : a.name < b.name
            }
        case .name:
            return shown.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        }
    }

    public var meanOfMeans: Float? {
        let compared = samples.filter { $0.status == .compared }
        guard !compared.isEmpty else { return nil }
        return compared.map(\.deltaEMean).reduce(0, +) / Float(compared.count)
    }
}

// MARK: Report

public enum LightroomImportReport {
    public static let fileName = "import-report.md"

    /// Markdown written next to library.json: what was imported, what was skipped and why.
    public static func markdown(report r: LrcatReport, summary: LrcatSummary? = nil, options: LrcatOptions? = nil,
                                fidelity: LrcatFidelity? = nil, date: Date = Date()) -> String {
        var out: [String] = []
        let when = ISO8601DateFormatter.string(from: date, timeZone: .current,
                                               formatOptions: [.withInternetDateTime])
        out.append("# Lightroom import report")
        out.append("")
        out.append("- Catalog: `\(r.catalogPath)`")
        out.append("- Library: `\(r.libraryPath)`")
        out.append("- Date: \(when)")
        if r.cancelled {
            out.append("- **Status: cancelled.** Run the import again with the same settings to resume; finished photos are skipped.")
        } else {
            out.append("- Status: complete in \(String(format: "%.1f", r.seconds)) s")
        }
        if let s = summary {
            out.append("- Catalog contents: \(s.images) photos (\(s.virtualCopies) virtual copies), \(s.folders) folders, "
                       + "\(s.keywords) keywords, \(s.collections) collections, \(s.collectionSets) collection sets, "
                       + "\(s.smartCollections) smart collections")
        }
        out.append("")
        out.append("## Imported")
        out.append("")
        out.append("| What | Count |")
        out.append("| --- | ---: |")
        out.append("| Photos with edits and selections written | \(r.imported) |")
        if r.resumed > 0 { out.append("| Photos already imported by an earlier run | \(r.resumed) |") }
        out.append("| Albums | \(r.albums) |")
        out.append("| Album groups | \(r.albumGroups) |")
        out.append("| Smart albums | \(r.smartAlbums) |")
        out.append("| Keywords added | \(r.keywords) |")
        out.append("| Photos indexed | \(r.indexed) |")
        out.append("")
        let c = r.selection
        out.append("Selection: \(c.keeps) Keep (grade 1: \(c.grade1), grade 2: \(c.grade2), grade 3: \(c.grade3)), "
                   + "\(c.rejects) Reject, \(c.undecided) Undecided; \(c.marked) marked.")
        if let marks = options?.marks, !marks.isEmpty {
            out.append("")
            out.append("Colour labels: " + marks.map { m in
                m.mark.isEmpty ? "\(m.label) → (dropped)" : m.mark == m.label ? "\(m.label) → \(m.label)" : "\(m.label) → \(m.mark)"
            }.joined(separator: ", ") + ".")
        }
        if let moved = options?.relocations.filter({ FolderMappingTable.trimmed($0.from) != FolderMappingTable.trimmed($0.to) }),
           !moved.isEmpty {
            out.append("")
            out.append("Relocated folders: " + moved.map { "`\($0.from)` → `\($0.to)`" }.joined(separator: ", ") + ".")
        }
        out.append("")
        out.append("## Skipped (\(r.skipped.count + Int(r.virtualCopies)))")
        out.append("")
        if r.skipped.isEmpty && r.virtualCopies == 0 {
            out.append("Nothing was skipped.")
        } else {
            for s in r.skipped.sorted(by: { ($0.reason, $0.path) < ($1.reason, $1.path) }) {
                out.append("- **\(escape(s.name))** (`\(s.path)`): \(escape(s.reason))")
            }
            if r.virtualCopies > 0 {
                out.append("- \(r.virtualCopies) virtual cop\(r.virtualCopies == 1 ? "y" : "ies"): preserved in the import bundle "
                           + "(`import-plan.json`), not as separate photos; album memberships point at the master.")
            }
        }
        out.append("")
        out.append("## Not fully supported (\(r.unsupported.count))")
        out.append("")
        if r.unsupported.isEmpty {
            out.append("Everything in the catalog has a Tessera equivalent.")
        } else {
            out.append("| Area | Reason | Count | Examples |")
            out.append("| --- | --- | ---: | --- |")
            for i in r.unsupported {
                out.append("| \(cell(i.category)) | \(cell(i.reason)) | \(i.count) | \(cell(i.examples.joined(separator: ", "))) |")
            }
        }
        if let f = fidelity {
            out.append("")
            out.append("## Fidelity preview (\(f.renderer) renderer)")
            out.append("")
            let compared = f.samples.filter { $0.status == .compared }
            if compared.isEmpty {
                out.append("No photo could be compared with a Lightroom preview.")
            } else {
                out.append("ΔE2000 against Lightroom's cached previews; \"looks different\" means mean ≥ "
                           + "\(fmt(FidelityGrid.meanThreshold)) or 95th percentile ≥ \(fmt(FidelityGrid.p95Threshold)).")
                out.append("")
                out.append("| Photo | Mean ΔE | 95th pct ΔE | |")
                out.append("| --- | ---: | ---: | --- |")
                for s in FidelityGrid(samples: f.samples).visible {
                    let flag = s.status != .compared ? cell(s.message) : FidelityGrid.looksDifferent(s) ? "looks different" : ""
                    let values = s.status == .compared ? "\(fmt(s.deltaEMean)) | \(fmt(s.deltaEP95))" : "– | –"
                    out.append("| \(cell(s.name)) | \(values) | \(flag) |")
                }
            }
        }
        out.append("")
        out.append("The Lightroom catalog, its previews and Lightroom's own XMP files were only read, never written. "
                   + "Full source data (virtual copies, history, snapshots, faces, stacks) is kept in `\(r.bundlePath)`.")
        out.append("")
        return out.joined(separator: "\n")
    }

    /// Writes the report next to library.json and returns its URL.
    @discardableResult
    public static func write(_ markdown: String, report: LrcatReport) throws -> URL {
        let url = URL(fileURLWithPath: report.libraryPath).deletingLastPathComponent().appendingPathComponent(fileName)
        try Data(markdown.utf8).write(to: url, options: .atomic)
        return url
    }

    static func fmt(_ v: Float) -> String { String(format: "%.1f", v) }
    static func escape(_ s: String) -> String { s.replacingOccurrences(of: "*", with: "\\*") }
    static func cell(_ s: String) -> String {
        escape(s).replacingOccurrences(of: "|", with: "\\|").replacingOccurrences(of: "\n", with: " ")
    }
}

public extension ByteCountFormatter {
    static func fileSize(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }
}
