import Foundation
import XCTest

/// DESIGN.md §6: every colour, spacing, radius and font used by the app's views comes from
/// `Theme` (Sources/Tessera/App/Theme.swift). This scans the view sources for raw literals.
///
/// Exempt: Theme.swift itself; the Metal presenter (MetalLoupeView / LoupeRenderer, owned by
/// the loupe pipeline); PrintComposer (print output, not interface). A line may opt out with
/// `// lint:allow (<reason>)` when the colour is data (a user-chosen overlay, an OkLab swatch).
final class ThemeLintTests: XCTestCase {
    private static let exempt: Set<String> = [
        "App/Theme.swift", "Loupe/MetalLoupeView.swift", "Loupe/LoupeRenderer.swift", "Print/PrintComposer.swift",
    ]

    private static let rules: [(String, String)] = [
        (#"Color\((red|white|hue)\s*:"#, "raw SwiftUI Color"),
        (#"Color\.(white|black|gray|red|green|blue|orange|yellow|pink|purple|primary|secondary)\b"#, "system Color constant"),
        (#"[(\s:,?]\.(white|black|gray|purple)\.opacity"#, "system Color constant"),
        (#"NSColor\((srgbRed|calibratedWhite|calibratedRed|deviceRed|deviceWhite|red:|white:)"#, "raw NSColor"),
        (#"NSColor\.(white|black|red|green|blue|gray|purple|magenta|labelColor|secondaryLabelColor|tertiaryLabelColor|windowBackgroundColor)\b"#, "system NSColor"),
        (#"CGColor\((gray|red|srgbRed)"#, "raw CGColor"),
        (#"\.padding\(\s*[0-9]"#, "numeric padding"),
        (#"\.padding\(\.[a-zA-Z]+,\s*[0-9]"#, "numeric padding"),
        (#"spacing:\s*[1-9]"#, "numeric spacing"),
        (#"cornerRadius:\s*[0-9]"#, "numeric corner radius"),
        (#"\.font\(\.system\("#, "ad-hoc font"),
        (#"(?i)systemFont\(ofSize:"#, "ad-hoc NSFont"),
        (#"foregroundStyle\(\.(primary|secondary|tertiary|black|white)\)"#, "hierarchical style instead of a Theme ink"),
        (#"pickerStyle\(\.segmented\)"#, "native segmented control (use SegmentedPicker)"),
        (#"buttonStyle\(\.link\)"#, "link button (use .theme(.borderless))"),
    ]

    func testViewsUseThemeTokensOnly() throws {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Sources/Tessera")
        let files = try XCTUnwrap(FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil))
            .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
        XCTAssertGreaterThan(files.count, 20, "scanned \(root.path)")
        let compiled = try Self.rules.map { (try NSRegularExpression(pattern: $0.0), $0.1) }
        var violations: [String] = []
        for file in files {
            let rel = String(file.path.dropFirst(root.path.count + 1))
            if Self.exempt.contains(rel) { continue }
            let lines = try String(contentsOf: file, encoding: .utf8).components(separatedBy: "\n")
            for (i, line) in lines.enumerated() {
                let code = line.trimmingCharacters(in: .whitespaces)
                if code.hasPrefix("//") || line.contains("lint:allow") { continue }
                let range = NSRange(line.startIndex..., in: line)
                for (re, what) in compiled where re.firstMatch(in: line, range: range) != nil {
                    violations.append("\(rel):\(i + 1): \(what): \(code)")
                }
            }
        }
        XCTAssert(violations.isEmpty, "Use Theme tokens (DESIGN.md):\n" + violations.joined(separator: "\n"))
    }
}
