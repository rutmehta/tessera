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
///   Channels (B5-08): Q toggles Quick Mask.
///   Vector tools (B5-11): U shapes (⇧ cycles rectangle / ellipse / polygon / line) · P Pen · A Path / Direct
///   Selection (A again or ⇧A cycles, B5-11b); Return finishes a Pen path, Esc cancels a drag or path, ⌫ deletes
///   selected anchors. B5-11b: tool letters also work while a slider or the Layers list has the keyboard.
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
        if isBusyWindow(event) { return true }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        return mods.contains(.command) || mods.contains(.control)
    }

    /// No window, a panel / sheet / modal is up, or a text field or key-owning control has focus.
    private func isBusyWindow(_ event: NSEvent) -> Bool {
        guard let window = event.window else { return true }
        if window is NSPanel || window.attachedSheet != nil || window.sheetParent != nil || NSApp.modalWindow != nil { return true }
        return window.firstResponder is NSText || window.firstResponder is NSTextField || window.firstResponder is KeyOwningControl
    }

    /// ⌘E outside document mode is Library ▸ Edit in Layers (B5-v step 144). It is routed here rather than
    /// left to the menu: the menu's ⌘E is also Layer ▸ Merge Down, and SwiftUI swaps the key equivalent
    /// between the two items when the mode changes; a stale menu item left ⌘E doing nothing in the grid.
    func handleEditInLayers(_ event: NSEvent) -> Bool {
        guard model.viewMode != .document, !model.isReviewing, model.source != .people, !isBusyWindow(event),
              event.modifierFlags.intersection([.command, .shift, .option, .control]) == .command,
              event.charactersIgnoringModifiers?.lowercased() == "e" else { return false }
        guard model.documents.opening == nil else { return true }
        model.requestLayeredCopy()
        return true
    }

    /// Space released: the document viewport stops panning.
    func handleKeyUp(_ event: NSEvent) -> Bool {
        guard event.keyCode == 49, model.documents.spaceHeld else { return false }
        model.documents.spaceHeld = false
        model.documents.current?.viewport?.cursorDidChange()
        return model.viewMode == .document
    }

    func handle(_ event: NSEvent) -> Bool {
        // A SwiftUI popover does not always take keyboard focus, so the app-level monitor must
        // dismiss it directly on Escape. All other events remain native popover input; none reach
        // the workspace key map while a disclosure is presented.
        if model.loupeDisclosurePresented {
            guard event.keyCode == 53, let dismiss = model.dismissLoupeDisclosure else { return false }
            dismiss()
            model.loupeDisclosurePresented = false
            model.dismissLoupeDisclosure = nil
            return true
        }
        // B5-10c begin: ⌘Return / keypad Enter / Esc reach an active text session whichever view of the
        // document window has the keyboard (e.g. the viewport after a box handle drag).
        if model.viewMode == .document, DocumentText.shared.routeSessionKey(event) { return true }
        // B5-10c end
        if handleEditInLayers(event) { return true }
        // B5-11b: a focused slider, curve or the Layers list does not swallow single-key tool shortcuts.
        if model.viewMode == .document, handleToolLetterOverKeyOwner(event) { return true }
        if shouldIgnore(event) { return false }
        if model.viewMode == .document { return handleDocument(event) }
        if model.isReviewing { return handleReview(event) }
        // The People view owns its keys (name fields, Esc); culling keys would act on a hidden photo.
        if model.source == .people && !model.isPhotoEditing { return false }
        // Develop tools receive shortcuts only when a key-owning control is not focused.
        if model.isPhotoEditing {
            if MaskTools.shared.model === model, MaskTools.shared.handleKey(event) { return true }
            if DevelopTools.shared.model === model, DevelopTools.shared.handleKey(event) { return true }
            return handlePhotoEdit(event)
        }
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
        case 36, 76: model.requestViewMode(loupe ? .grid : .loupe); return true      // Return / Enter
        case 53: if loupe { model.requestViewMode(.grid); return true }; return false  // Esc
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
        case "d": model.enterPhotoEdit()
        case "g": model.requestViewMode(.grid)
        case "e": model.requestViewMode(.loupe)
        default: return false
        }
        return true
    }

    private func handleReview(_ event: NSEvent) -> Bool {
        switch event.keyCode {
        case 53: model.backFromReview(); return true
        case 123, 126: model.moveReviewSelection(-1); return true
        case 124, 125: model.moveReviewSelection(1); return true
        default: break
        }
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "d", "e": model.editReviewedPhoto()
        case "g": model.returnToLibrary(grid: true)
        // Review has no cull/accept-all shortcuts. Each mutation is an explicit row action.
        case "x", "u", "p", "1", "2", "3", "6", "7", "8", "9", "b", "a", "k", "y", "n", "c": break
        default: return false
        }
        return true
    }

    /// Photo Edit has one photo target. Library decision keys never leak into it.
    private func handlePhotoEdit(_ event: NSEvent) -> Bool {
        switch event.keyCode {
        case 53: model.returnFromPhotoEdit(); return true
        case 123, 126: model.navigate(.left, groupwise: false, extend: false); return true
        case 124, 125: model.navigate(.right, groupwise: false, extend: false); return true
        default: break
        }
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "d": model.photoInspectorTab = .develop
        case "m": model.photoInspectorTab = .masks
        case "g": model.returnToLibrary(grid: true)
        case "s":
            if event.modifierFlags.contains(.shift) { SoftProof.shared.gamutWarning.toggle() }
            else { SoftProof.shared.toggle() }
        case "x", "u", "p", "1", "2", "3", "6", "7", "8", "9", "b", "a", "k", "y", "n", "c": break
        default: return false
        }
        return true
    }

    /// B5-11b: in document mode a tool letter (U, ⇧U, Z, A …) chooses its tool while a key-owning control
    /// that is not text input has the keyboard (a Properties slider, the curve editor, the Layers list:
    /// their own keys are arrows, Return / Esc and ⌫). Text fields and the Type tool's input keep letters.
    private func handleToolLetterOverKeyOwner(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, let window = event.window, !(window is NSPanel), window.attachedSheet == nil,
              window.sheetParent == nil, NSApp.modalWindow == nil,
              let owner = window.firstResponder, owner is KeyOwningControl, !(owner is TextInputView),
              let doc = model.documents.current else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.intersection([.command, .control, .option]).isEmpty else { return false }
        guard case .tool(let t)? = ToolKeyMap.action(keyCode: event.keyCode, characters: event.charactersIgnoringModifiers ?? "",
                                                     mods: flags.contains(.shift) ? .shift : [], current: doc.tool) else { return false }
        if DocumentTools.shared.document === doc {
            DocumentTools.shared.select(t)
        } else {
            doc.tool = t
            doc.viewport?.cursorDidChange()
        }
        return true
    }

    /// Document mode: only the document key map; culling, develop and mask keys never fire here.
    private func handleDocument(_ event: NSEvent) -> Bool {
        // WP B5-04: tool letters (⇧ cycles a group), [ ] / ⇧[ ⇧] brush size and hardness, 0–9 opacity,
        // X / D colours, Return / Esc (transform, polygon lasso), ⌫ clears a pixel selection.
        if DocumentTools.shared.handleKey(event) { return true }
        // B5-08 begin: Q toggles Quick Mask (a temporary channel that becomes the selection on exit).
        if event.charactersIgnoringModifiers?.lowercased() == "q",
           event.modifierFlags.intersection([.shift, .option, .command, .control]).isEmpty,
           model.documents.current != nil {
            DocumentChannels.shared.toggleQuickMask()
            return true
        }
        // B5-08 end
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
            DocumentTools.shared.publishHint()   // B5-11b
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
