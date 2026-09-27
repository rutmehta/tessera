import Foundation
import TesseraFFI

/// Recoverable export warnings (M2-49, M2-51): the engine writes an exported file's omissions
/// ("Lens Blur skipped: depth model is not cached") beside it as `<file>.tessera-warnings.txt`,
/// one warning per line (`export::BatchReport::warnings`). The FFI report does not carry them, so
/// the app reads those files after a run and lists them in the completion toast.
public struct ExportWarnings: Equatable, Sendable {
    public struct Item: Equatable, Sendable {
        public let name: String
        public let warnings: [String]
        public init(name: String, warnings: [String]) { self.name = name; self.warnings = warnings }
    }

    public let items: [Item]
    /// Warning files that exist but could not be read (never reported as "no warnings").
    public let unreadable: [String]

    public init(items: [Item] = [], unreadable: [String] = []) { self.items = items; self.unreadable = unreadable }

    public static let suffix = ".tessera-warnings.txt"

    public static func path(for output: String) -> String { output + suffix }

    /// Reads every exported item's warning file (blocking file I/O; call off the main actor).
    public static func read(_ report: ExportReport, fileManager: FileManager = .default) -> ExportWarnings {
        var items: [Item] = [], unreadable: [String] = []
        for item in report.items {
            guard let output = item.outputPath else { continue }
            let path = path(for: output)
            guard fileManager.fileExists(atPath: path) else { continue }
            guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { unreadable.append(item.name); continue }
            let lines = text.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            if !lines.isEmpty { items.append(Item(name: item.name, warnings: lines)) }
        }
        return ExportWarnings(items: items, unreadable: unreadable)
    }

    public var isEmpty: Bool { items.isEmpty && unreadable.isEmpty }
    public var photoCount: Int { items.count + unreadable.count }

    /// Toast detail lines: "IMG_1.ARW: Lens Blur skipped: depth model is not cached".
    public var lines: [String] {
        items.flatMap { item in item.warnings.map { "\(item.name): \($0)" } }
            + unreadable.map { "\($0): export warnings could not be read" }
    }

    /// Toast headline and details for a finished run: failures first, then warnings.
    public static func toastLines(_ report: ExportReport, warnings: ExportWarnings) -> (headline: String, details: [String]) {
        let folder = URL(fileURLWithPath: report.destination).lastPathComponent
        let photos = { (n: Int) in "\(n) photo\(n == 1 ? "" : "s")" }
        var headline: String
        if report.cancelled {
            headline = "Export cancelled: \(photos(Int(report.exported))) written to \(folder)"
        } else if report.failed == 0 {
            headline = "Exported \(photos(Int(report.exported))) to \(folder) in \(String(format: "%.1f", report.seconds)) s"
        } else {
            headline = "Exported \(photos(Int(report.exported))) to \(folder); \(report.failed) failed"
        }
        if report.exported == 0, report.failed == 0, !report.cancelled { headline = "Nothing was exported" }
        if !warnings.isEmpty { headline += "; \(warnings.photoCount) with warnings" }
        let failures = report.items.compactMap { item in item.error.map { "\(item.name): \($0)" } }
        return (headline, failures + warnings.lines)
    }
}
