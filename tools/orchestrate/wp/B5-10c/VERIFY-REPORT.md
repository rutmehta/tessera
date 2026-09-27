# B5-10 on-screen verification (Machine B verifier, 2026-09-27)

Build: wp/B5-10 @ cc70faa, `make-app.sh release`, CARGO_TARGET_DIR=~/.cache/tessera-target/verify.
Launch: `open -g -n` with a scratch `--app-dir` and a 1200×800 scratch PNG (a copy of fixtures/raw/sample.dng), window 1440×900 pt.
Driver: Codex background computer use (tools/orchestrate/cu/cu_client.py, CU_ALLOW=dev.tessera.app), bound with
`cua.getApp(<this build's .app path>)`, so the two other agents' Tessera instances were never touched. Claude computer
use `request_access` for Tessera was auto-denied twice; the user saw no prompt. Focus watcher (frontmost app and cursor,
sampled 4×/s) during every run: the frontmost app stayed Claude and the cursor never moved.
Evidence: `screencapture -x -o -l <own window>` after each chunk.

| Step | Result | Seen | Evidence |
|---|---|---|---|
| 340 | FAIL (overlap) | No right edge clipped at 1280/1366/1440/1512. But at every width the window toolbar is drawn over the document options bar (Size field, Cancel/Apply, hint and latency readout unreadable) and over the top of the inspector (the "Properties" heading and Assist/Auto Edit overlap). Scrolled Properties content passes under the toolbar, and clicks on the Style pop-up hit Auto Edit (it opened the Auto Edit sheet; dismissed) until Properties was scrolled. After an AX window resize the content also moved under the title bar: the traffic lights overlap "All Photos" and the "Library" heading is cut. History's last rows need scrolling. | v-bgcheck-after.png, v-340-1440.png, 340-w1280/1366/1440/v-1512.png |
| 341 | PASS | T selects Type; the options bar shows Helvetica/48/alignment/hint. Click then 'Hello World' gives a draft box and a provisional "Hello World Aa" row, no History. ⌘Return adds one `Add Text`; the text row is selected; Properties shows Character/Paragraph/Text box. No caret is drawn while the window is inactive (Codex saw one on its own capture at 200 %). | v-341-editing.png, v-341-applied.png |
| 342 | PASS (minor) | Drag gives a box with 8 handles and wraps inside 300 px; overflow hidden; `Add Text`; Text box 300×160. The outline is solid, not dashed as the spec says. | v-342-area.png |
| 343 | PASS | Drag across the Regular/Bold boundary selects (Style shows "Mixed"); X replaces it and the outer runs keep their styles; one `Edit Text` row. (The `Herld` result came from the CU API's `Delete` = forward delete, not an app bug.) | v-343-mixed.png |
| 344 | PASS | Double-click selects "World" (highlight = line height, sidebearings); Style▸Bold changes only "World"; one `Font Style` row. | v-344-bold.png |
| 347 | PASS + BUG | Handle drags rewrap at the same 48 px (300×160 → 300×340 → 200×340; overflow hidden), one `Resize Text Box` per release. BUG (reproduced twice): after a handle drag, ⌘Return no longer applies (Apply/Cancel and handles stay); keypad Enter or the Apply button works. Likely the first responder moves off TextInputView after the handle drag. | v-347-resize.png, v-353-undo.png |
| 348 | PARTIAL | At 200 % and 100 % the caret sits between e and r, and selection edges are ~5 px (retina) outside the ink on both sides (advance box, consistent). Panning via scroll didn't move the view (not verified). ⌘-drag rotate/move: BLOCKED (the CU API has no modifier drag; synthetic CGEvent posting was refused by the auto-mode classifier). | v-348-zoom200.png, v-348-rotated.png |
| 349 | partial | Arrows step correctly over plain Latin (Left×3 = 3 stops); ligature/combining/emoji not exercised on screen. | v-349-arrows.png |
| 351 | BLOCKED | No Japanese input source enabled (AppleEnabledInputSources: U.S. only + palette/press-and-hold/ironwood). Nothing changed. | — |
| 352 | PASS | t v x d q 1 5 Space typed into the text; tool, colours, opacity and layers unchanged; ⌘A/⌘C/→/⌘V duplicated the text; ⌘Z removed the uncommitted draft ("Undo typing"); Esc cancelled with no History. | v-352-keys.png |
| 353 | PASS | Typing ' ok' + ⌘Return gives one `Edit Text`; Edit▸Undo restores "Herld" with runs; Redo restores "Herld ok". | v-353-undo.png |
| 354 | partial | Esc cancels with no History row and status "Type: cancelled". Tab switch not tested. | v-349-arrows.png |
| 345, 346, 350, 355–358 | not run on screen | Covered by the in-process self-test. | — |
| 359 | see 340 | Inspector screenshot at 1440 is v-340-w1440.png; the Colour note shows (it scrolls under the toolbar). | v-344-bold.png |

Other observations:
- The latency readout showed "p95 76844.7 ms (10 keys)" on a 1200×800 doc. Keystrokes arrived seconds apart in the background, so it probably counts time until a frame is presented to an inactive or occluded window. Worth checking the measurement.
- The status bar hint is stale: "Area text: …" while editing point text, and "Type: cancelled" while a new edit is active.
- The sidebar of a fresh `--app-dir` instance lists other agents' folders (shoot b509b, b506, run…): folder registry leaks outside --app-dir (UserDefaults?).
- A click just right of a point text's last glyph starts a new text layer instead of resuming (hit test = glyph box only).
