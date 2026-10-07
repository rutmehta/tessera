# B5-25 handoff: keyboard data safety over focused panel / toolbar buttons

Branch `wp/B5-25` (Machine B), stacked on `wp/B5-21` d0e6e2bd. Swift only (no Rust, board.json or Cargo.lock changes).

## Problem (Machine A, FKA on)

With the keyboard on a native panel / toolbar button (Layers row eye `NSButton`, toolbar tool buttons), ⌫ / Delete
deleted the selected layer and Space panned the canvas instead of pressing the button. Two paths deleted: the
document key map (`DocumentKeyMap` ⌫ → `deleteLayer`, and `DocumentTools` ⌫ → clear selection / vector anchors),
and, for an eye button, the responder chain: the event climbs from the button through `LayerRowCell` to
`LayersOutlineView.keyDown`, which deletes the selection.

## Policy

New `KeyRouter.panelViewKey(_:)`, called in `handleDocument` right after the B5-21 Tab guard, before
`DocumentTools.handleKey`. It applies only while `panelViewHasKeyboard(in:)` holds (a visible native view other
than the canvas, not a stray responder: the same predicate as B5-21's Tab rule).

- ⌫ / ⌦ (key codes 51 / 117, any modifiers that reach the router): consumed, nothing happens. Consuming (not
  passing through) is required: passed through, the eye button's responder chain reaches `LayersOutlineView`.
- Space (49, no ⌘ ⌃ ⌥): a focused `NSButton` gets `performClick` once; key repeat is ignored (same rule as the
  B5-16 Dither checkbox and History −/+/↺). Never sets `spaceHeld`, so no pan. Over another focused view (a SwiftUI
  control's `KeyViewProxy`), the first Space goes to that view natively and repeats are dropped.
- Everything else is unchanged:
  - Layers list, sliders, curve, Dither / History / action buttons are `KeyOwningControl` and never reach
    `handleDocument` (`isBusyWindow`), so list ⌫ keeps its exact semantics (`LayersOutlineView.keyDown` →
    `deleteSelection`).
  - Canvas focused, or nothing focused: ⌫ still clears a pixel selection / deletes vector anchors / deletes the
    selected layer as on main; Space still holds the pan.
  - Text fields and `TextInputView` keep ⌫ / Space. Tool letters over a focused button still pick tools.
- A button that becomes disabled loses first responder (AppKit), so Space then behaves as "nothing focused" (pan)
  and never fires the button.

## Commits

- `4b29eb7d` RED: `apps/mac/Tests/TesseraCoreTests/DocumentPanelButtonKeySafetyTests.swift` (10 tests, 5 red on
  d0e6e2bd, 13 failing assertions), e.g.
  - `:159 testDeleteOverFocusedLayersEyeButtonDeletesNothing: ("3") is not equal to ("6") - ⌫ over a focused eye button must not delete the selected layer`
  - `:178 testDeleteOverFocusedPlainToolbarButtonDeletesNothing: ("2") is not equal to ("6")`
  - `:226 testSpaceOverFocusedPlainButtonPressesOnceAndDoesNotPan: ("0") is not equal to ("1") - Space presses the focused button`; `:227 ... must not start a canvas pan`
  - `testSpaceOverFocusedLayersEyeButtonTogglesVisibilityOnce: Optional(true) is not equal to Optional(false)`, no-pan assert
  - disabled-button Space test (later rewritten, see below).
- `2da4c714` fix: `apps/mac/Sources/Tessera/App/KeyRouter.swift` (`panelViewKey`, one call in `handleDocument`,
  header comment). Test adjustments in the same commit: the disabled-button test now pins AppKit's behaviour
  (disabling drops focus → Space pans as with nothing focused, never fires the button); added a focus-proxy test
  (first Space native, repeat dropped, ⌫ swallowed, no pan). 11 tests.
- this handoff.

## Gates (worktree, 2da4c714)

- `apps/mac/build-ffi.sh`: OK.
- Focused (`DocumentPanelTabTraversalTests|DocumentKeyRoutingTests|KeyFocusTests|InspectorFocusTraceTests|DocumentHistoryKeyboardTraversalTests|DocumentDitherCheckboxTests|DocumentPanelButtonKeySafetyTests|DocumentHistoryHeightControlTests|DocumentInspectorActionButtonTests|InspectorFocusAXBridgeTests`):
  81 tests, 0 failures (before the focus-proxy test was added; the class alone 11/11 after).
- Strict release build (`--scratch-path ~/.cache/tessera-strict-B5-25`, strict concurrency, warnings as errors):
  "Build of product 'Tessera' complete!".
- `tools/orchestrate/swift-gate.sh`: 853 XCTest tests (3 skipped, 0 failures) + 5 swift-testing tests, `SWIFT GATE OK`.

## On-screen checklist (append to B5-21's 0–13; FKA ON, isolated profile, focus trace on)

Use a document with at least three layers; note the layer count before each step.

14. Click a Layers row (select a layer), Tab to that row's eye button (ring). Press ⌫: no layer is deleted,
    focus stays on the eye, no beep required either way. Press fn-⌫ (forward delete): same.
15. Same eye button focused, press Space once: the layer's visibility toggles exactly once; the canvas does not
    pan and the cursor does not become the hand. Hold Space for a second: still exactly one toggle, no pan.
    Release: nothing else happens.
16. Focus a toolbar tool button (or the titlebar sidebar toggle) with Tab: ⌫ does nothing (layer count
    unchanged); Space presses that button once (the tool / sidebar changes once), no pan.
17. Click the Layers list row itself (list focused, not a button), press ⌫: the selected layer is deleted exactly
    as before (⌘Z restores it).
18. Click the canvas with a layer selected and no pixel selection, press ⌫: behaves as on main (deletes the layer;
    with a marquee selection it clears the selection instead). Hold Space over the canvas and drag: pans as before.
19. Double-click a layer name (rename field) or a Properties text field: ⌫ deletes text and Space types a space;
    no layer is deleted, no pan.
20. With the eye button focused, press B then V: tools switch as before (B5-21 step 6 still holds).

## Note for the foreground run (from the 2026-09-30 background attempt)

- The inspector focus trace records only while the Tessera window is the key window, and it stops after 32 key presses. The foreground FKA-on run must rotate trace files per checklist section (A–F): pass a fresh `--inspector-focus-trace` path per launch, or relaunch between sections.
- Background-delivered keys produce an empty trace even though the app acts on them; see evidence/2026-09-30/keycheck-dd4953c4/GUI-RESULTS.md.
