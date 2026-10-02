import Foundation

/// Percent-encode user/model keys so whitespace and separators cannot change an AX path.
enum AccessibilityKey {
    static func component(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_:"))) ?? value
    }
}
