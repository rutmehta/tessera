import Foundation

/// The tether naming template, mirrored from `crates/tether/src/naming.rs` so the panel can
/// show a live example and explain a problem while typing. The engine validates again when the
/// session starts; the two must agree (`TetherNamingTests`).
///
/// Tokens: `{sequence}` (four digits, growing past 9999), `{original}` (the camera's file name
/// without extension) and `{ext}` (its extension, unchanged).
public enum TetherNaming {
    public static let defaultTemplate = "{sequence}_{original}.{ext}"
    public static let tokens: [(token: String, title: String)] = [
        ("{sequence}", "Sequence (0001)"),
        ("{original}", "Original name"),
        ("{ext}", "Extension"),
    ]
    /// The file the panel's example uses.
    public static let exampleOriginal = "DSC01234.ARW"

    public enum Problem: Error, Equatable, CustomStringConvertible {
        case invalidOriginal
        case empty
        case hidden
        case pathOrUnknownToken
        case tooLong
        case extensionChanged

        public var description: String {
            switch self {
            case .invalidOriginal: "The original file name is not valid"
            case .empty: "Enter a file name"
            case .hidden: "Names cannot start with a dot"
            case .pathOrUnknownToken: "Use only {sequence}, {original} and {ext}; no folders or : in names"
            case .tooLong: "Names are limited to 240 bytes"
            case .extensionChanged: "End the name with .{ext} so the file keeps its format"
            }
        }
    }

    /// The file name for `original` as frame `sequence` of the session.
    public static func render(_ template: String, original: String, sequence: UInt64) throws(Problem) -> String {
        let url = URL(fileURLWithPath: original)
        let ext = url.pathExtension
        let stem = url.deletingPathExtension().lastPathComponent
        guard !stem.isEmpty, !original.hasPrefix("."), !ext.isEmpty else { throw .invalidOriginal }
        let seq = String(format: "%04llu", sequence)
        let name = template
            .replacingOccurrences(of: "{sequence}", with: seq)
            .replacingOccurrences(of: "{original}", with: stem)
            .replacingOccurrences(of: "{ext}", with: ext)
        if name.isEmpty { throw .empty }
        if name.hasPrefix(".") { throw .hidden }
        if name.contains(where: { "/\\:{}\0".contains($0) }) || name.unicodeScalars.contains(where: { $0.properties.generalCategory == .control }) {
            throw .pathOrUnknownToken
        }
        if name.utf8.count > 240 { throw .tooLong }
        if !name.hasSuffix(".\(ext)") { throw .extensionChanged }
        return name
    }

    /// The + menu: tokens go before the extension so the name keeps its format.
    public static func inserting(_ token: String, into template: String) -> String {
        if token != "{ext}", let r = template.range(of: ".{ext}", options: .backwards) {
            return template.replacingCharacters(in: r, with: "_\(token).{ext}")
        }
        return template + token
    }

    /// The live example under the field, or the problem.
    public static func example(_ template: String, sequence: UInt64 = 1) -> Result<String, Problem> {
        Result { () throws(Problem) -> String in try render(template, original: exampleOriginal, sequence: sequence) }
    }

    /// A folder-safe session name: slashes and colons become dashes, surrounding space trimmed.
    public static func sessionFolderName(_ name: String) -> String {
        let cleaned = name.trimmingCharacters(in: .whitespacesAndNewlines)
            .map { "/:\\".contains($0) ? "-" : $0 }
        let s = String(cleaned).trimmingCharacters(in: CharacterSet(charactersIn: "."))
        return s.isEmpty ? "Tether session" : s
    }

    /// "Tether 2026-09-25", or "Tether 2026-09-25 (2)" when that folder exists already.
    public static func defaultSessionName(date: Date = Date(), existing: (String) -> Bool = { _ in false }) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        let base = "Tether \(f.string(from: date))"
        var name = base
        var n = 2
        while existing(name) { name = "\(base) (\(n))"; n += 1 }
        return name
    }
}
