# B5-11b implementation status — shapes, Pen and document-mode fixes from on-screen verification

Branch `wp/B5-11b` from main `cf9dc6f` (+ brief `8dd2ad1`). One test per item was written first and run against the
unfixed code (Swift: public API scaffolds that kept the old behaviour; Rust: the text.rs fix reverse-applied):
`Executed 12 tests, with 38 failures` (every new Swift case failed) and `auto_named_text_layer_follows_its_first_line_until_renamed`
failed with `left: "Hi" right: "HiZQ"`. With the fixes all pass.

## Root causes and fixes

1. **A twice stayed on Path Selection.** `ToolKeyMap.action` returned the current tool for a repeated letter in its
   group (Photoshop behaviour for M / L / W), so only ⇧A cycled. New `DocumentTool.keyRepeatCycles` (Path / Direct
   Selection only): a plain A cycles too. Test: `DocumentVectorTests.testToolRoutingKeysAndGroups`.
2. **Stroke took the fill colour.** The inspector's paint picker seeded a new stroke from
   `paint?.representativeColor ?? options.fillColor`. Now `ShapeToolOptions.newStrokeColor(fill:foreground:)`: the
   foreground colour when it differs from the fill, else black / white by the fill's Rec. 709 luminance
   (`DocumentVector.newStrokePaint(for:)`).
3. **Keyboard slider steps made several Edit Shape rows.** Every final call of a slider recorded a node and
   `ValueSlider` sends a final on Return, blur and each mouse-down, so one keyboard adjustment could end in several
   finals. `DocumentVector.setSource` / `setMask` now treat edits made without a mouse button down as a keyboard
   adjustment: drafts (and finals) stay live and ONE node is recorded 0.6 s after the last step (`commit(label:)`
   of the pending draft), or at once when any other vector edit, canvas click or tool change comes first. A mouse
   edit of the same control supersedes the adjustment (its final records the net change). Mask sliders get their
   engine label (`Vector Mask Density` / `Feather`).
4. **Pen hid the previous anchor's handles.** The draft drew handles for the last anchor only. `PenDraft.handleAnchors`
   now returns the last segment's two anchors (closing segment when closed).
5. **No vector-mask thumbnail.** The Layers row had one mask tile (raster). A separate tile shows the vector mask
   (host-drawn from the mask path, cached per layer revision, crossed when disabled).
6. **Rejected recolour left the well showing it.** A failed engine edit changed no observable model value, and the
   active `NSColorWell` stays linked to the colour panel still holding the rejected colour. Failures now bump
   `DocumentVector.rejections`; the inspector's colour wells are keyed on it, so they rebuild from the model (which
   also ends the panel link). In the off-screen harness the well value already reverted without the fix (no colour
   panel), so the fail-first evidence is the rejection signal; the panel interplay needs on-screen verification.
7. **Tool letters swallowed by sliders / Layers list.** `KeyRouter.isBusyWindow` ignored every key while any
   `KeyOwningControl` had focus. In document mode a `ToolKeyMap` `.tool` action now runs first when the focused
   key owner is not text input (`TextInputView`); text fields and their field editors still keep letters. Arrows,
   Return / Esc and ⌫ still go to the control.
8. **Path Selection empty click did not deselect.** A click outside the box started a (zero) rotation. A click
   without a drag (or with nothing selected) now deselects the path (no box, no outline; the layer stays selected);
   clicking the shape selects it again; a drag outside still rotates.
9. **Stale status hints.** Hints were said once on events (Esc's "Pen path discarded" stayed after Return; the
   Remove tool's message stayed after choosing Move). `DocumentTools.hint(for:)` derives the hint from the tool and
   its session (Type session, Pen path; nil while Remove is on) and `publishHint()` shows it on every tool change,
   Pen path start / end and text-session change (B5-10c's text hint now goes through it). Every tool has
   `DocumentTool.idleHint` (the placeholder notice for Crop).
10. **Position 0,0 / Bounds "Whole canvas".** The inspector showed the transform's translation (identity for shapes
    drawn in document space) and Properties the engine's `affected_bounds` (none for shapes). Both now use
    `DocumentVector.displayBounds` — the engine's shape bounds, the affine box during a Path Selection drag, the live
    path during a Direct Selection drag.
11. **Typing + Edit Text for a resized new area text.** A box-handle mouse-down committed the draft as "Typing" to
    create the layer; the resize could not be recorded yet (the id arrived later), so Apply said "Edit Text". Box /
    move / rotate gestures on a new layer no longer pre-commit: they fold into its first apply, one **Add Text**;
    an inspector edit that must flush a new layer's draft labels it "Add Text" too.
12. **Area-text hint after apply.** With no session the text hint was nil and nothing was said. The Type tool's idle
    hint is published when the session ends (item 9's mechanism).
13. **Text layer name did not follow its text.** Rust `text.rs`: `with_auto_name` batches a `SetProps` rename with
    the text edit when the layer's name is still the auto name of its old first line (so it undoes with the edit);
    a user rename (any other name) sticks. Applies to drafts committed via `commit`, final `set_text_layer` and
    `edit_text_runs`. No FFI signature change (bindings unchanged).

## Tests

- Rust `cargo test --locked --release -p vector -p tessera-ffi`: 37 test binaries, **259 passed, 0 failed,
  10 ignored**; new `document_text_ui::auto_named_text_layer_follows_its_first_line_until_renamed`.
- clippy `-p tessera-ffi --all-targets -D warnings`: clean. `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- Swift `swift test`: `Executed 338 tests, with 0 failures (0 unexpected)` and
  `✔ Test run with 5 tests in 2 suites passed`. New `DocumentVectorVerifyFixesTests` (11 cases: items 2–13 through an
  engine document, an off-screen viewport, synthesized events, KeyRouter, `LayersOutlineController`, a hosted
  `ShapeInspector`) and item 1 in `DocumentVectorTests`.
- xcodebuild Debug: `** BUILD SUCCEEDED **`. `build-ffi.sh`, `swift build`: 0.
- `--vector-selftest` (`run-vector-selftest.sh`, `open -g -n`, own-window `screencapture -l`, own PID only):
  89 checks ok, `done, 0 failure(s)`, 33 captures in `evidence/`; the 18 `11b-*` checks drive the real dash-offset
  `ValueSlider` with key events, U / ⇧U / Z through `KeyRouter` with the slider / Layers list focused, the real fill
  `NSColorWell` on a locked shape, the Layers row cells, the Pen, Path Selection and the Type tool.

## Needs on-screen verification

- The colour panel interplay of item 6 (a real NSColorPanel sending the rejected colour); item 3 with a real keyboard
  and the 0.6 s idle; tool letters with real keyboard focus in sliders / the Layers list (item 7); the vector-mask
  tile's look at real row size; the hint text changes in the status bar; Pen handle drawing with a real mouse.

## Deviations / notes

- `ValueSlider` / `DocSlider` / `DocColorWell` (outside the allow-list) are unchanged; coalescing and the well
  rebuild live in `DocumentVector` / `ShapeViews`.
- Keyboard-vs-mouse is decided by `NSEvent.pressedMouseButtons` (a slider drag always has the button down); callers
  can pass `keyboard:` explicitly (the self-test's scripted drags pass `false`).
- `KeyRouter`'s DocumentKeyMap `.tool` path also publishes the hint.
