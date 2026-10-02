import Foundation

/// Percent-encode non-private model keys so separators cannot change an AX path.
/// User names and paths must use an index or opaque model ID instead.
enum AccessibilityKey {
    static func component(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_:"))) ?? value
    }
}
