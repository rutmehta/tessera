import Foundation

/// Export file-name templates, evaluated exactly like the engine's `export::filename`: tokens
/// `{name}` (source file name without extension), `{seq}` (1-based position in the export) and
/// `{date}` (capture date `YYYY-MM-DD`); anything else is literal. The result must be a single safe
/// file name. Swift evaluates it for the sheet's live example without a bridge call per keystroke;
/// `ExportNamingTests` checks parity with the engine.
public enum ExportNaming {
    public enum Problem: Error, Equatable, CustomStringConvertible {
        case unclosedToken
        case unknownToken(String)
        case unsafeName

        public var description: String {
            switch self {
            case .unclosedToken: "A “{” has no matching “}”"
            case .unknownToken(let t): "Unknown token \(t) (use {name}, {seq} or {date})"
            case .unsafeName: "Not a usable file name (empty, “.”, “..”, or contains / \\ : { })"
            }
        }
    }

    public static let tokens: [(token: String, title: String)] = [
        ("{name}", "File name"), ("{seq}", "Sequence"), ("{date}", "Capture date"),
    ]

    /// `IMG_0412-3.jpg` for `{name}-{seq}`.
    public static func fileName(template: String, name: String, sequence: Int, date: String,
                                extension ext: String) -> Result<String, Problem> {
        var result = ""
        var rest = Substring(template)
        while let open = rest.firstIndex(of: "{") {
            result += rest[..<open]
            guard let close = rest[open...].firstIndex(of: "}") else { return .failure(.unclosedToken) }
            let token = String(rest[open...close])
            switch token {
            case "{name}": result += name
            case "{seq}": result += String(sequence)
            case "{date}": result += date
            default: return .failure(.unknownToken(token))
            }
            rest = rest[rest.index(after: close)...]
        }
        result += rest
        let unsafe = result.isEmpty || result == "." || result == ".."
            || result.unicodeScalars.contains { $0.properties.generalCategory == .control || "/\\:{}".unicodeScalars.contains($0) }
            || !ext.unicodeScalars.allSatisfy { $0.isASCII && CharacterSet.alphanumerics.contains($0) }
        return unsafe ? .failure(.unsafeName) : .success("\(result).\(ext)")
    }

    /// Capture date as the `{date}` token spells it.
    public static func dateToken(_ date: Date, calendar: Calendar = .current) -> String {
        let c = calendar.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", c.year ?? 1970, c.month ?? 1, c.day ?? 1)
    }

    /// The sheet's live example: the first photo's output name, or the problem.
    public static func example(template: String, firstName: String, date: Date, format: ExportSettings.FileFormat,
                               count: Int) -> String {
        switch fileName(template: template, name: firstName, sequence: 1, date: dateToken(date), extension: format.fileExtension) {
        case .success(let name):
            if count > 1, !template.contains("{name}"), !template.contains("{seq}") {
                return "\(name) — every photo gets this name; add {seq} or {name}"
            }
            return count > 1 ? "\(name), …" : name
        case .failure(let problem):
            return problem.description
        }
    }
}
