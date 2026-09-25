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
///   Compare: ← → pick side · Return choose this · Z fit/1:1 · Esc back
///   Masking (loupe): M on/off · O overlay (⇧ colour) · [ ] brush size (⇧ feather) · X invert · ⌫ delete
/// A develop control that handles keys itself while focused (the curve editor's point nudge).
@MainActor protocol DevelopKeyHandling: AnyObject {
    func handleDevelopKey(_ event: NSEvent) -> Bool
}

@MainActor
final class KeyRouter {
    private var monitor: Any?
    private let model: AppModel

    init(model: AppModel) { self.model = model }

    func install() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            // Local monitors run on the main thread.
            nonisolated(unsafe) let e = event
            let handled = MainActor.assumeIsolated { self?.handle(e) ?? false }
            return handled ? nil : event
        }
    }

    private func shouldIgnore(_ event: NSEvent) -> Bool {
        guard let window = event.window else { return true }
        if window is NSPanel || window.attachedSheet != nil || NSApp.modalWindow != nil { return true }
        if window.firstResponder is NSText { return true }   // field editor / text view is editing
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        return mods.contains(.command) || mods.contains(.control)
    }

    func handle(_ event: NSEvent) -> Bool {
        if shouldIgnore(event) { return false }
        // Develop tools first: a focused curve editor takes arrows; the crop tool takes its keys.
        if let target = event.window?.firstResponder as? DevelopKeyHandling, target.handleDevelopKey(event) { return true }
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
        case "c": comparing ? model.exitCompare() : model.enterCompare()
        case "z" where comparing: model.toggleCompareZoom()
        case "g": model.viewMode = .grid
        case "e": model.viewMode = .loupe
        default: return false
        }
        return true
    }
}
