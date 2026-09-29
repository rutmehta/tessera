# B5-21 handoff: keyboard Tab loop for native panel / inspector / toolbar controls

Branch `wp/B5-21` (Machine B), base main a32ce399. Swift only (no Rust, board.json or Cargo.lock changes).

## Problem (A's on-screen pass at a32ce399, FKA on)

`KeyRouter.handleDocument` mapped every plain Tab to `togglePanels` unless the first responder was `NSText`,
`NSTextField` or a `KeyOwningControl`. With Full Keyboard Access on, Tab from a Layers eye `NSButton`, a toolbar
button, the titlebar sidebar toggle or any SwiftUI control (first responder `SwiftUI.KeyViewProxy`) hid the panels
instead of moving focus, so History −/+/↺ could not be reached from the Layers list on screen. (⇧Tab was already
passed through; `DocumentKeyMap` maps only unmodified Tab.)

## Policy chosen

In document mode, Tab (key code 48) goes to the native key-view loop when `KeyRouter.panelViewHasKeyboard(in:)` holds:

- the window's first responder is an `NSView` attached to that window,
- it is not the window's `contentView`, and it is not hidden (`isHiddenOrHasHiddenAncestor`),
- it is not the `DocumentViewportView` or a view inside it.

Otherwise (focus on the canvas, the window itself, or nothing), Tab toggles the panels as before.

Why this and not "NSControl only" or marking buttons `KeyOwningControl`:
- The path from the Layers eye buttons to History under FKA runs through SwiftUI controls (Layers footer
  IconButtons and Menus, the History disclosure button). Their first responder is `SwiftUI.KeyViewProxy`, a
  plain `NSView` and not an `NSControl`, so an NSControl-only rule would still stop at the first SwiftUI control.
  Converting every panel button to `KeyOwningControl` would cost many edits and still miss the SwiftUI ones.
- The rule names no private class. A view is only first responder if it accepted it, and plain SwiftUI hosting
  views don't (checked: `NSHostingView.acceptsFirstResponder == false`; `KeyViewProxy` accepts only with FKA on).
  So a click on empty panel space doesn't take Tab away from the canvas.
- It changes only the Tab key in `handleDocument`, so nothing else moves: tool letters (both the
  `KeyOwningControl` path and the plain-button path through `DocumentTools.handleKey`), Space, ⌫, Q, text input,
  sheets/panels/modal (`isBusyWindow`), popover Escape, menus and the text-session keys all behave as before.
- The hidden-responder guard keeps Tab able to bring the panels back if a hidden view somehow stays first responder.

Trade-off, stated plainly: once focus is in a panel, Tab moves focus and does not hide the panels. To hide them
with Tab, click the canvas first (or use View ▸ Hide Panels). If the key-view loop reaches the canvas, the next Tab
toggles the panels again. This matches Photoshop (Tab in a panel moves between fields).

## Commits

- `3a7fdc72` B5-21 RED: `apps/mac/Tests/TesseraCoreTests/DocumentPanelTabTraversalTests.swift` (7 tests; 3 red on
  main: plain button, focus-proxy view, eye button → History chain).
- `89e1fcc5` fix: `apps/mac/Sources/Tessera/App/KeyRouter.swift` (`panelViewHasKeyboard(in:)` + one guard at the
  top of `handleDocument`, header comment).
- this handoff.

Tests (hosted `NSWindow`s ordered back; the app is never activated and no window is made key):
Tab/⇧Tab over a focused plain `NSButton` isn't consumed; the same for a non-control first-responder view (focus
proxy stand-in). Nothing focused and the `DocumentViewportView` still toggle panels both ways. A hidden focused
button still toggles. A text field keeps Tab and letters; B / V over a focused plain button still pick the tool.
The hosted DocumentInspector (Stack tab) goes from a Layers eye button through Tab to History − → + → ↺, and ⇧Tab
from ↺ goes back to +. Tab from `LayersOutlineView` reaches −, and the panels never hide on the way.

## Gates (worktree, 89e1fcc5)

- `apps/mac/build-ffi.sh`: OK.
- Focused: `DocumentPanelTabTraversalTests|InspectorFocusTraceTests|InspectorFocusAXBridgeTests|DocumentKeyRoutingTests|KeyFocusTests|DocumentHistoryKeyboardTraversalTests|DocumentHistoryHeightControlTests|DocumentDitherCheckboxTests|DocumentInspectorActionButtonTests|DocumentModeTests|ThemeLintTests|DocumentInspectorLayoutTests`:
  69 tests, 0 failures.
- Strict release build (`swift build -c release --product Tessera --scratch-path ~/.cache/tessera-strict-B5-21
  -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors`): exit 0, "Build of product 'Tessera' complete!".
- `tools/orchestrate/swift-gate.sh`: 837 XCTest tests (3 skipped, 0 failures) + 5 swift-testing tests, `SWIFT GATE OK`.

## On-screen checklist for A (FKA ON, isolated profile, focus trace on)

Open or create a document, Stack tab, History expanded with the height not at its default (so −, + and ↺ are all
enabled).

1. Click the canvas, press Tab: the panels hide (trace: DocumentViewportView, handled=true). Tab again: they come back.
2. Click a Layers row, then Tab: focus moves to the row's eye button (ring). Tab again: focus moves on
   (next eye button or the Layers footer controls), the panels stay visible, trace handled=false (NSButton / KeyViewProxy).
3. Keep pressing Tab: focus goes through the Layers footer and the History disclosure to History −, then +, then
   ↺ (focus ring on each); the panels stay visible the whole way. ⇧Tab from ↺ goes back to +.
4. With focus on + press Space: History grows by one row (the History H-act check from B5-16).
5. With focus on an eye button press B: the Brush tool is selected, focus stays and the panels stay.
6. Focus a toolbar tool button or the titlebar sidebar toggle (Tab / ⇧Tab to it): Tab moves focus and doesn't
   toggle the panels.
7. Click in the layer-name rename field or a Properties text field: Tab keeps the field behaviour (no panel toggle).
8. Click the canvas again, Tab: panels toggle (no regression). Hide the panels with focus on the canvas, then check
   that nothing in the hidden inspector keeps Tab (Tab brings the panels back).
9. Regression: Dither D2/D3 from B5-16 (Tab from Dither → Color header, panels stay).

Known, out of scope: Space over a focused plain eye/toolbar button still pans (document `panHold`) and does not
press the button. That's unchanged by this package.
