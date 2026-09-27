import AppKit
import TesseraCore

/// The Type tool's keyboard owner (WP B5-10): an invisible first responder over the document canvas
/// that adopts `NSTextInputClient`. While a text session is active it holds focus, so the input
/// method sees every key (marked text, candidate windows, dead keys) and `KeyRouter` leaves its
/// events alone (`KeyOwningControl`): T / V / X / D / Q, digits, Space and ⌫ type text instead of
/// switching tools or deleting layers, and ⌘A / ⌘C / ⌘X / ⌘V / ⌘Z act on the text.
///
/// NSRanges here are UTF-16 offsets into the draft's concatenated text; `DocumentText` converts them
/// to the UTF-8 offsets of the model and layout (`TextIndexMap`).
@MainActor
final class TextInputView: NSView, @preconcurrency NSTextInputClient, KeyOwningControl {
    weak var viewport: DocumentViewportView?
    private var text: DocumentText { DocumentText.shared }

    override var acceptsFirstResponder: Bool { text.isEditing }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override var isFlipped: Bool { true }

    override func becomeFirstResponder() -> Bool {
        setAccessibilityElement(true)
        return true
    }

    override func resignFirstResponder() -> Bool {
        // Focus moving to an inspector field keeps the session; the IME's composition is committed.
        if text.isComposing { text.commitComposition() }
        inputContext?.discardMarkedText()
        return true
    }

    // MARK: Keys

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        // Keypad Enter or ⌘Return applies; Esc cancels (after the input method had its chance).
        if !text.isComposing, event.keyCode == 76 || (event.keyCode == 36 && flags.contains(.command)) {
            text.apply()
            return
        }
        if inputContext?.handleEvent(event) == true { return }
        interpretKeyEvents([event])
    }

    /// ⌘ shortcuts reach the view hierarchy before the menu: text clipboard, select all, undo.
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self, text.isEditing else { return false }
        let flags = event.modifierFlags.intersection([.command, .shift, .option, .control])
        let key = event.charactersIgnoringModifiers?.lowercased() ?? ""
        switch (flags, key) {
        case (.command, "a"): selectAll(nil)
        case (.command, "c"): copy(nil)
        case (.command, "x"): cut(nil)
        case (.command, "v"): paste(nil)
        case (.command, "z"): text.undo()
        case ([.command, .shift], "z"): text.redo()
        case (.command, "\r"): text.apply()
        default: return false
        }
        return true
    }

    override func doCommand(by selector: Selector) {
        text.command(selector)
    }

    override func selectAll(_ sender: Any?) { text.selectAll() }
    @objc func copy(_ sender: Any?) {
        let s = text.selectedText
        guard !s.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
    }
    @objc func cut(_ sender: Any?) {
        copy(sender)
        text.insert("")
    }
    @objc func paste(_ sender: Any?) {
        guard let s = NSPasteboard.general.string(forType: .string) else { return }
        text.insert(s)
    }
    @objc func undo(_ sender: Any?) { text.undo() }
    @objc func redo(_ sender: Any?) { text.redo() }

    // MARK: NSTextInputClient

    func insertText(_ string: Any, replacementRange: NSRange) {
        text.insert(Self.plain(string), replacing: replacementRange.location == NSNotFound ? nil : replacementRange)
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        text.setMarked(Self.plain(string), selected: selectedRange,
                       replacing: replacementRange.location == NSNotFound ? nil : replacementRange)
    }

    func unmarkText() { text.commitComposition() }

    func selectedRange() -> NSRange { text.selectedRange16 }

    func markedRange() -> NSRange { text.markedRange16 ?? NSRange(location: NSNotFound, length: 0) }

    func hasMarkedText() -> Bool { text.isComposing }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        guard let (s, actual) = text.substring16(range) else { return nil }
        actualRange?.pointee = actual
        return NSAttributedString(string: s)
    }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [.underlineStyle] }

    /// Screen rectangle of a character range (candidate windows sit below it).
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        actualRange?.pointee = range
        guard let v = viewport, let window = v.window, let canvas = text.canvasRect(for16: range) else { return .zero }
        let a = v.viewPoint(canvas: canvas.origin), b = v.viewPoint(canvas: CGPoint(x: canvas.maxX, y: canvas.maxY))
        let viewRect = CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
        return window.convertToScreen(v.convert(viewRect, to: nil))
    }

    func characterIndex(for point: NSPoint) -> Int {
        guard let v = viewport, let window = v.window else { return NSNotFound }
        let local = v.convert(window.convertPoint(fromScreen: point), from: nil)
        return text.characterIndex16(atView: local, in: v) ?? NSNotFound
    }

    private static func plain(_ s: Any) -> String {
        (s as? NSAttributedString)?.string ?? (s as? String) ?? ""
    }
}
