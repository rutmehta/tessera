# B5-21 + B5-25 on-screen keyboard checklist — Machine B, background computer-use run

- Build: /Applications/Tessera.app = local/keycheck dd4953c4 (main 74f76332 + B5-25 07b449ee)
- Launch: `open -n --stderr keycheck/app.log /Applications/Tessera.app --args --app-dir keycheck/app --folder keycheck/library --inspector-focus-trace keycheck/focus-01.jsonl`
- PID 60851 for the whole run (checked after every step; no restart)
- FKA: `defaults read -g AppleKeyboardUIMode` -> "does not exist" (default 0 = Full Keyboard Access OFF)
- Driver: computer-use app_* tools only (background; Tessera was never activated/raised; frontmost app stayed Claude/Messages)
- Fixture: 3 PNGs; document "sample" (Library ▸ Open in Layers…, the ⌘E item) + Layer ▸ New ▸ Layer ×2 -> L = 3 (Layer 2, Layer 1, sample)

## Trace caveat (applies to every row)

focus-01.jsonl is **0 bytes**. InspectorFocusTrace only records events whose `input.eligible` is true, which needs
`ownedKeyWindow` (event.window === NSApp.keyWindow). Background-delivered keystrokes reach the window while the app
is inactive, so NSApp.keyWindow is nil and no record is written (KeyRouter still handles the key — panels toggled).
So the sequence / nativeType / handled columns are "—" for all rows. Instead, the focused element was read before and
after each key with System Events `AXFocusedUIElement` of the process (read-only), and results were judged from
app_screenshot. Separately: the trace also stops after 32 records (`events >= 32` -> traceEnd marker), so even a
foreground run needs several trace files for this checklist.

Key loop observed with FKA off: Layers outline (`document.layers`) -> History − (`document.history.height.decrease`)
-> + (`…increase`) -> ↺ (`…reset`, only while enabled, i.e. height ≠ default) -> Document canvas (`document.viewport`).
Eye buttons, the toolbar and the titlebar sidebar toggle are not in the loop, so preconditions "ring on an eye /
toolbar / titlebar control" can't be set up without changing FKA (not allowed).

## Results

| step | result | trace seq | nativeType | handled | note |
|---|---|---|---|---|---|
| 1 | PASS | — | — | — | Grid: clicked sample.png, Library ▸ Open in Layers…, sheet "Open in Layers" confirmed, no canvas click. Tab hid inspector+sidebar; Tab again restored. Repeated via File ▸ New Document… (Create, 2400×1600): Tab hid, Tab restored. AX focus = document.viewport. |
| 2 | PASS | — | — | — | Clicking the document tab in the titlebar was not delivered in the background (no switch). Switched instead via Library ▸ Open in Layers… on sample.png (opened a 3rd tab "sample"; note: it opened a new tab, not the existing sample tab). Without clicking canvas: Tab hid panels, Tab restored. |
| 3 | PASS | — | — | — | ⇧Tab with canvas focused: panels unchanged. |
| 4 | PASS* | — | — | — | Stack tab, History expanded. H0 = 192 pt after one + press (*the + was pressed via AXPress, a raw mouse click on + didn't register in background). Default = 168 pt. |
| 5 | BLOCKED | — | — | — | Clicked Layer 1 row: AX focus = AXOutline document.layers. Tab -> History − (not the row's eye). FKA off: eye NSButton not in key loop. Panels stayed. |
| 6 | PASS | — | — | — | From Layers: Tab -> −, Tab -> +, Tab -> ↺ (3 Tabs, with ↺ enabled); panels never hid. ⇧Tab from ↺ -> +. (At default height ↺ is disabled and + Tab -> canvas, panels stay.) |
| 7 | PASS | — | — | — | Space on +: 192 -> 216 (one step), no pan. Return on +: 216 -> 240 (one step, not two). Tab -> ↺, Space: 240 -> 168 (default). Observation: ↺ becomes disabled at default, focus fell to the canvas, so the next Tab hid the panels (viewport-owned Tab — expected, not a FAIL). |
| 8 | BLOCKED | — | — | — | FKA off: no toolbar control or titlebar sidebar toggle in the key loop (↺/+ -> canvas). |
| 9 | BLOCKED | — | — | — | Eye can't be focused (FKA off). Substitute on a focused panel button (History +, SwiftUI): ⌫ -> L stays 3, focus stays on +; fn-⌫ -> same. |
| 10 | BLOCKED | — | — | — | Eye can't be focused (FKA off); hold-Space also not possible with app_key (no key-hold). |
| 11 | BLOCKED | — | — | — | Titlebar toggle / toolbar control can't be focused (FKA off). (History +/− covered under 9/12.) |
| 12 | PASS* | — | — | — | Marquee drawn (M, drag on canvas). Click Layer 1 row, Tab -> History −, ⌫: selection stays, L stays 3. Hand tool (H), click canvas (focus = viewport), ⌫: History "Clear" (pixels inside the selection cleared on the selected layer, count stays 3); marching ants remained until Select ▸ Deselect. *Substitute panel button = History − (eye not focusable). Undo Clear afterwards. |
| 13 | PASS* | — | — | — | Focus on History + (eye not focusable): B -> Brush selected, focus stayed on +, panels stayed; V -> Move. |
| 14 | PASS | — | — | — | Click Layer 1 row (focus = document.layers), ⌫ -> Layer 1 deleted, L 3 -> 2; Edit ▸ Undo restored L = 3. |
| 15 | PASS (⌫) / BLOCKED (Space-drag) | — | — | — | No pixel selection, click canvas (Hand tool), ⌫ -> selected layer "sample" deleted, L 3 -> 2; Edit ▸ Undo restored 3. Space-hold + drag not possible with background tools. |
| 16 | BLOCKED | — | — | — | Couldn't enter rename: Layer ▸ Rename Layer… produced no visible field in background; a double-click on the row opened the Layer Style window instead (and one double-click resolved to AXPress on Layer 1's eye -> hid it; undone). Then the screen locked (~20+ min) and actions were refused. Dither part N/A (FKA off). |
| 16b | N/A | — | — | — | FKA off. |
| 17 | N/A | — | — | — | FKA off. |
| 18 | BLOCKED | — | — | — | Screen locked; also needs ring on an eye (FKA off). |
| 19 | BLOCKED | — | — | — | Screen locked; also needs ring on an eye (FKA off). |
| 20 | BLOCKED | — | — | — | Screen locked before this step. |
| 21 | BLOCKED | — | — | — | Screen locked before this step. |
| 22 | PASS | — | — | — | Screen still locked, so quit via Apple event `tell application id "dev.tessera.app" to quit` (app_menu unavailable while locked); exited cleanly, no save prompt, `pgrep -x Tessera` empty. focus-01.jsonl (0 bytes, see caveat) and app.log archived. |

No FAIL observed.

## app.log

```
2026-09-30 18:09:09.516 Tessera[60851:45824218] Sparkle updates not configured for this build
document: frames on the Viewport path (L0, requested Rect { x0: 0, y0: 0, x1: 652, y1: 434 })
document: frames on the Viewport path (L0, requested Rect { x0: 0, y0: 0, x1: 2400, y1: 1600 })
```
