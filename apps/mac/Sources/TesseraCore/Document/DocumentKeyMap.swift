import Foundation

/// Document tools (spec 02 §3–6). M5-13 had Move and Rectangular Marquee; WP M5-11 adds the
/// selection, painting, retouching and navigation tools (titles, keys and groups in
/// `Document/Tools/EditorTools.swift`).
public enum DocumentTool: String, CaseIterable, Sendable {
    case move, marquee
    // WP M5-11
    case ellipseMarquee, lasso, polygonLasso, magneticLasso, quickSelect, wand, objectSelect
    case brush, eraser, cloneStamp, heal, gradient, crop, type, eyedropper, hand, zoom
}

/// What a key does in document mode. ⌘ shortcuts are menu items; they are listed here too so the
/// menu bar and the tests share one table.
public enum DocumentKeyAction: Equatable, Sendable {
    case tool(DocumentTool)
    /// Space held: temporary hand tool (released on key-up).
    case panHold
    case togglePanels
    case cycleScreenMode
    case selectAll, deselect
    case undo, redo
    case duplicate, group, ungroup, mergeDown
    case zoomIn, zoomOut, zoomFit, zoomActual
    case deleteLayer
}

public enum DocumentKeyMap {
    /// Modifier subset that matters for the map.
    public struct Mods: OptionSet, Sendable {
        public let rawValue: Int
        public init(rawValue: Int) { self.rawValue = rawValue }
        public static let command = Mods(rawValue: 1)
        public static let shift = Mods(rawValue: 2)
        public static let option = Mods(rawValue: 4)
        public static let control = Mods(rawValue: 8)
    }

    /// `keyCode` is the hardware key (49 space, 48 tab, 51 delete); `characters` ignores modifiers.
    public static func action(keyCode: UInt16, characters: String, mods: Mods) -> DocumentKeyAction? {
        let ch = characters.lowercased()
        if mods.contains(.control) { return nil }
        if mods.contains(.command) {
            let shift = mods.contains(.shift)
            switch ch {
            case "a" where !shift: return .selectAll
            case "d" where !shift: return .deselect
            case "z": return shift ? .redo : .undo
            case "j" where !shift: return .duplicate
            case "g": return shift ? .ungroup : .group
            case "e" where !shift: return .mergeDown
            case "=", "+": return .zoomIn
            case "-", "_": return .zoomOut
            case "0": return .zoomFit
            case "1": return .zoomActual
            default: return nil
            }
        }
        switch keyCode {
        case 49: return .panHold
        case 48: return mods.isEmpty ? .togglePanels : nil
        case 51, 117: return mods.isEmpty ? .deleteLayer : nil
        default: break
        }
        guard mods.subtracting(.shift).isEmpty else { return nil }
        switch ch {
        case "v": return .tool(.move)
        case "m": return .tool(.marquee)
        case "f": return .cycleScreenMode
        default: return nil
        }
    }
}
