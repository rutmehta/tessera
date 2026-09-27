# B5-10c implementation status: Type tool fixes from the on-screen verification

Branch `wp/B5-10c` (base wp/B5-11 @ 1255578). Shape/vector code untouched. No Rust change (text.rs was read; the
defects are host-side), bindings regenerate unchanged.

## Root causes and fixes

| # | Defect | Root cause | Fix |
|---|---|---|---|
| 1 | ⌘Return stops applying after a box handle drag | `DocumentViewportView.mouseDown` makes the viewport first responder before forwarding to the tools. The select / ⌘-move paths called `focus()` again, but the handle-resize path did not, so `TextInputView` lost the keyboard. ⌘Return then reached nothing: KeyRouter ignores ⌘ keys, `TextInputView.performKeyEquivalent` requires first responder, and no menu item has ⌘Return. Keypad Enter still worked because `DocumentTools.handleKey` maps it to `.commit` | `DocumentText.mouseUp` gives the keyboard back to `TextInputView` after any gesture while a caret-editable session exists. New `DocumentText.routeSessionKey` (a delimited `// B5-10c` hook at the top of `KeyRouter.handle`) makes ⌘Return / keypad Enter apply and Esc cancel whichever view of the document window is first responder. Exceptions: `TextInputView` itself (the IME goes first), a field editor with marked text, and other windows, panels or sheets |
| 2 | A click just right of the last glyph starts new text | The hit test used the union of glyph boxes inset by 4 px (4 canvas px, under 1 pt at Fit). Both the idle hit test and the in-session "inside" test used it | New `TextHitRegion` (TesseraCore): per-line box from the first glyph origin to the last advance / line width, ascent to descent. It is widened by a **trailing margin of 0.5 × line height, at least 12 view points**, on both horizontal ends (the trailing end of RTL / right-aligned lines is the left one), plus 4 px vertically. Area text keeps box + 4 px. Used for both paths, so resume wins over create |
| 3 | Stale status hint | Hints were one-off `say()` calls: `beginNew` said "Point/Area text", `beginExisting` said nothing, so "Area text: …" or "Type: cancelled" stayed | `DocumentText.hint` is computed from the session (point / area / limitation). `session`'s `didSet` publishes it to the status bar whenever it changes (an error said during the same state stays until the state changes). The explicit `say` calls were removed |
| 4 | Latency p95 76,844 ms after 10 keys | `keyAt` was set on each keystroke and never cleared, and every preview captured it. Box-handle / move / rotate previews (e.g. step 347) therefore recorded "latency" since the last keystroke, which can be minutes. Background frames were also delayed by App Nap and occlusion | New `TypingLatencyMeter` (TesseraCore). Only keystrokes (insert, marked text, deletions) are measured. A preview carries the oldest unsent keystroke once; commits carry a leftover one; gesture and inspector previews carry none. The meter measures to the renderer's completed frame (FrameInfo epoch ≥ the preview's). Time while the window is not visible or the app is not active is excluded (NSApplication active/hide and NSWindow occlusion/miniaturize notifications), and keystrokes or frames that arrive while inactive are not counted. Readout: `Keystroke → rendered frame: median … · p95 … (n keys, inactive time excluded)`. The background self-test uses wall clock and labels it `wall clock` |
| 5 | Area-text outline solid | `stroke(…, dashed: !isParagraph)` followed DESIGN.md, which said solid, but ACCEPTANCE 342 said dashed | `DocumentText.frameDashed` is dashed (4 / 3) for both kinds. DESIGN.md is updated |
| 6 | Selection ~5 px past the ink | By design (advance box, matches Photoshop) | Documented in DESIGN.md ("Type tool fixes (WP B5-10c)") so verifiers do not flag it |

Hooks outside Text/**: `KeyRouter.swift` (4 lines, `// B5-10c begin/end`). DESIGN.md: one word changed (area box
"solid" → "dashed too since B5-10c") plus a new B5-10c bullet. ACCEPTANCE.md `## B5-10.`: notes on 342, 343 and 347.

## Tests (each written first and seen failing)

New `apps/mac/Tests/TesseraCoreTests/DocumentTextSessionTests.swift` (8 tests). They use a real engine document, a
`DocumentViewportView` in an off-screen window, synthesized mouse events into the viewport, and keys through `KeyRouter`.
Before the fix: `Executed 8 tests, with 26 failures` (6 of the 8 tests failed; the two pure TextHitRegion / meter tests
only exercise new types). After: `Executed 8 tests, with 0 failures`.

* 1: `testCommandReturnAppliesAfterABoxHandleDrag`, `testApplyAndCancelKeysReachTheSessionWhateverViewIsFocused`
  (⌘Return / keypad Enter / Esc × viewport / text field / nothing focused)
* 2: `testClickJustRightOfTheLastGlyphResumesEditing`, `testHitRegionIsTheLineBoxPlusTheTrailingMargin`
* 3: `testStatusHintFollowsTheSession`
* 4: `testSessionMeasuresKeystrokesNotGestures` (before: 2 measured for 1 keystroke), `testLatencyCountsKeystrokesOnlyAndExcludesInactiveTime`
* 5: `testTextFrameIsDashedForPointAndAreaText`

Self-test extended (`TESSERA_TEXT_SELFTEST`, 8 new `B5-10c …` checks). After 347: keyboard back on the text, ⌘Return
applies after the handle drag, and ⌘Return applies with the viewport focused. It also checks the point-text hint at
348 and after 352's cancel, and that a click right of the last glyph resumes with the caret at the end and adds no layer.

## Gate (CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-10c)

* `cargo test --locked --release -p tessera-ffi`: exit 0, 237 passed, 0 failed, 10 ignored
  (`document_text_ui`: `test result: ok. 18 passed; 0 failed; 1 ignored`). A first run, made while swift test and
  xcodebuild were running at the same time, failed the load-sensitive `develop::export_batch_does_not_starve_slider_drag`.
  It passes on the quiet rerun. No crate was changed.
* `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: exit 0
* `cargo fmt --all -- --check`: exit 0
* `./build-ffi.sh`: exit 0, bindings unchanged; `swift build`: exit 0
* `swift test`: `Executed 319 tests, with 0 failures (0 unexpected)`
* xcodebuild Debug: `** BUILD SUCCEEDED **`
* Self-test on a 1200 × 800 document (`make-app.sh debug`, `open -g -n`, own window captured with
  `screencapture -x -o -l`): 71 checks, `done, 0 failure(s)`. 347-resized-box.png shows the dashed area box and the
  relabelled readout. The app stayed in the background and only its own PID was quit.

## Needs on-screen verification

* Step 347 with a real mouse: drag a box handle, type, press ⌘Return. It should apply with no click on Apply.
* A real click just right of a point text's last glyph, at Fit and at 200 %, should resume editing.
* The status bar hint after area → point editing and after Esc → a new edit.
* The latency readout with the window active and frontmost. It should be tens to a few hundred ms on 1200 × 800,
  never seconds. Also check it while the app is in the background: keys typed while inactive are reported as
  "not counted".
* The dashed area box, and the selection advance box described in DESIGN.md.

## Not done / out of scope

* Toolbar overlapping the options bar and inspector, content under the title bar after a resize, and the `--app-dir`
  folder registry leak (the sidebar in the capture still lists other folders). These were reported to Machine A.
* Frame completion is timed when the FrameInfo reaches the main thread (`DocumentController.listenerFrame`).
  Timestamping on the listener thread would need an edit outside the allowed paths. Inactive time is excluded instead.
