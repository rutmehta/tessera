import AppKit
import TesseraCore

/// Culling key map (docs/06 §2–3), installed as a local event monitor so it works regardless of
/// which pane has focus. Keys pass through when a text field is editing, a panel/sheet is up,
/// or ⌘/⌃ is held (menu shortcuts).
///
///   X reject · U undecided · P keep (auto-advance)       1 / 2 / 3 grade (implies Keep)
///   6 7 8 9 toggle mark · B basket · A auto-advance      G grid · E / Return loupe · Esc grid
///   Loupe: ← → previous/next group, ↑ ↓ previous/next frame within the group
///   Grid:  arrows move spatially (⇧ extends), ⌥ + arrows = group navigation as in the loupe
///   K keep the group's suggested best and reject the rest · C compare · ⌫ remove from album
///   Assist (toolbar): Y confirm every suggested decision in view · N dismiss the suggestion here
///   Compare: ← → pick side · Return choose this · Z fit/1:1 · Esc back
///   Masking (loupe): M on/off · O overlay (⇧ colour) · [ ] brush size (⇧ feather) · X invert · ⌫ delete
///   Develop (loupe): S soft proofing on/off · ⇧S gamut warning
///   Document mode (B5-02, `DocumentKeyMap`): V move · M marquee · Space-drag pan · Tab panels ·
///   F screen modes · ⌫ delete layer; no culling key fires. ⌘ shortcuts are Layer / Select / View menu items.
///   Document tools (B5-04, `ToolKeyMap`): V M L W B E S J G C T I H Z (⇧ cycles M / L / W), [ ] size,
///   ⇧[ ⇧] hardness, 0–9 opacity, X swap / D default colours, Return / Esc, ⌫ clears the selection.
/// First responders that own their keyboard input. The local monitor must leave their events
/// untouched even when they do not handle a particular key themselves.
@MainActor protocol KeyOwningControl: AnyObject {}

@MainActor
final class KeyRouter {
    private var monitor: Any?
    private let model: AppModel

    init(model: AppModel) { self.model = model }

    func install() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            // Local monitors run on the main thread.
            nonisolated(unsafe) let e = event
            let handled = MainActor.assumeIsolated {
                e.type == .keyUp ? (self?.handleKeyUp(e) ?? false) : (self?.handle(e) ?? false)
            }
            return handled ? nil : event
        }
    }

    private func shouldIgnore(_ event: NSEvent) -> Bool {
        guard let window = event.window else { return true }
        if window is NSPanel || window.attachedSheet != nil || window.sheetParent != nil || NSApp.modalWindow != nil { return true }
        if window.firstResponder is NSText || window.firstResponder is NSTextField ||
            window.firstResponder is KeyOwningControl { return true }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        return mods.contains(.command) || mods.contains(.control)
    }

    /// Space released: the document viewport stops panning.
    func handleKeyUp(_ event: NSEvent) -> Bool {
        guard event.keyCode == 49, model.documents.spaceHeld else { return false }
        model.documents.spaceHeld = false
        model.documents.current?.viewport?.cursorDidChange()
        return model.viewMode == .document
    }

    func handle(_ event: NSEvent) -> Bool {
        if shouldIgnore(event) { return false }
        if model.viewMode == .document { return handleDocument(event) }
        // Develop tools receive shortcuts only when a key-owning control is not focused.
        if MaskTools.shared.handleKey(event) { return true }
        if DevelopTools.shared.handleKey(event) { return true }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let shift = mods.contains(.shift)
        let option = mods.contains(.option)
        let loupe = model.viewMode == .loupe
        let comparing = model.viewMode == .compare

        if comparing {
            switch event.keyCode {
            case 36, 76: model.chooseInCompare(); return true                        // Return: choose this
            case 53: model.exitCompare(); return true                                // Esc
            default: break
            }
        }
        switch event.keyCode {
        case 51, 117: model.deletePressed(); return true                          // ⌫ / ⌦
        case 123: model.navigate(.left, groupwise: loupe || option, extend: shift); return true
        case 124: model.navigate(.right, groupwise: loupe || option, extend: shift); return true
        case 126: model.navigate(.up, groupwise: loupe || option, extend: shift); return true
        case 125: model.navigate(.down, groupwise: loupe || option, extend: shift); return true
        case 36, 76: model.viewMode = loupe ? .grid : .loupe; return true      // Return / Enter
        case 53: if loupe { model.viewMode = .grid; return true }; return false  // Esc
        default: break
        }

        guard let ch = event.charactersIgnoringModifiers?.lowercased(), ch.count == 1 else { return false }
        switch ch {
        case "x": model.perform(.reject)
        case "u": model.perform(.undecided)
        case "p": model.perform(.keep)
        case "1", "2", "3": model.perform(.grade(UInt8(ch)!))
        case "6", "7", "8", "9": model.perform(.mark(UInt8(ch)!))
        case "b": model.perform(.toggleBasket)
        case "a": model.autoAdvance.toggle(); model.statusMessage = "Auto-advance \(model.autoAdvance ? "on" : "off")"
        case "k": model.keepBestRejectRest()
        case "y": model.assist.confirmAll()
        case "n": model.assist.dismiss(model.targetIDs)
        case "c": comparing ? model.exitCompare() : model.enterCompare()
        case "z" where comparing: model.toggleCompareZoom()
        case "s" where loupe:
            if shift {
                SoftProof.shared.gamutWarning.toggle()
                model.statusMessage = "Gamut warning \(SoftProof.shared.gamutWarning ? "on" : "off")"
            } else {
                SoftProof.shared.toggle()
                model.statusMessage = "Soft proofing \(SoftProof.shared.enabled ? "on" : "off")"
            }
        case "g": model.viewMode = .grid
        case "e": model.viewMode = .loupe
        default: return false
        }
        return true
    }

    /// Document mode: only the document key map; culling, develop and mask keys never fire here.
    private func handleDocument(_ event: NSEvent) -> Bool {
        // WP B5-04: tool letters (⇧ cycles a group), [ ] / ⇧[ ⇧] brush size and hardness, 0–9 opacity,
        // X / D colours, Return / Esc (transform, polygon lasso), ⌫ clears a pixel selection.
        if DocumentTools.shared.handleKey(event) { return true }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var mods: DocumentKeyMap.Mods = []
        if flags.contains(.shift) { mods.insert(.shift) }
        if flags.contains(.option) { mods.insert(.option) }
        guard let action = DocumentKeyMap.action(keyCode: event.keyCode, characters: event.charactersIgnoringModifiers ?? "",
                                                 mods: mods) else { return false }
        let docs = model.documents
        switch action {
        case .tool(let t):
            docs.current?.tool = t
            docs.current?.viewport?.cursorDidChange()
        case .panHold:
            if !docs.spaceHeld {
                docs.spaceHeld = true
                docs.current?.viewport?.cursorDidChange()
            }
        case .togglePanels: docs.togglePanels()
        case .cycleScreenMode: docs.cycleScreenMode()
        case .deleteLayer: docs.current?.deleteSelection()
        default: return false   // ⌘ actions are menu items
        }
        return true
    }
}
