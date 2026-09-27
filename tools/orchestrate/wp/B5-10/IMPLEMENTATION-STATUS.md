# B5-10 implementation status: editable Type tool, Character/Paragraph inspector, 1440-pt inspector fix

Branch `wp/B5-10` (base 9bb77b7). All gate commands pass (see "Gate"). Self-test of acceptance 340–359 on a
20 MP document: 63 checks, `done, 0 failure(s)`.

## FFI (crates/tessera-ffi/src/document/text.rs, document.rs)

| Call | Behaviour |
| --- | --- |
| `text_layer(layer) -> TextLayerRecord` | live model JSON (the draft while one is pending), `TransformMatrix`, committed `revision`, `draft_pending`, `caret_editable`, `limitations` (missing font, warp / path / vertical caret, hyphenation, colour interpretation) |
| `available_text_fonts() -> [TextFontFamily]` | families of the shared snapshot, faces (PostScript name, weight, italic); macOS `.`-private families hidden |
| `layout_text(model_json) -> TextLayoutRecord` | the engine layout: glyph run, UTF-8 cluster, origin, advance, angle, `rtl`; lines with source range, glyph range, x, baseline, width, available width, caret ascent/descent; overflow; UTF-8 length. Path text is laid out on its path |
| `add_text_layer(name, parent, index, model_json, transform, interactive)` | `interactive`: a provisional draft on the scratch (created ids not reported); final call: one `Add Text` node, `created` = the new id |
| `set_text_layer(layer, model_json, transform, interactive, expected_revision)` | draft preview of the COMPLETE model, or one node: an `EditTextRuns` splice when only runs changed, `EditText` when paragraph/box/warp/path/transform changed, nothing when equal to the base |
| `edit_text_runs(layer, start_run, end_run, runs_json, expected_revision)` | direct half-open RUN-index splice as one node; rejects reversed / out-of-range ranges, stale revisions and a pending draft of the layer, with no change |
| `cancel_source_preview()` (document.rs) | drops only source drafts (`Pending::Text`), rebuilds the scratch for other pending keys, no history change |
| `convert_to_pixels(layer)` (document.rs) | `DocOp::ConvertToPixels` as one node "Convert to Pixels"; non-text/shape layers rejected before any flush |
| `text_run_splice`, `load_text_fonts_for_tests` | `#[doc(hidden)]` helpers (splice derivation used by the engine path; fixture fonts for deterministic tests) |

Draft lifecycle (document.rs `source_edit`): only one gesture owns the scratch — pending edits of other keys are
committed first as their own node; every preview rebuilds the scratch from the committed document plus the complete
draft (never replays relative run edits); `make` validates against the committed base before anything changes; a
failing final op restores the previous draft. The existing rules hold: unrelated edits, undo/redo/checkout and save
flush a pending text draft (tested).

Font snapshot: one process-wide `TextRenderer` (system fonts, discovered once) feeds `layout_text` and is cloned
into every compositor render.rs constructs (resident renderer, CPU fallback, thumbnails, `composite_raster`), in
delimited `B5-10` blocks; style routing untouched. `typography` added as a path dependency (Cargo.lock: one edge).

## Index-domain mapping (TesseraCore/Document/Text)

* **UTF-8 byte offsets** into the concatenated run text are canonical: model edits, layout clusters, caret and
  selection all use them.
* **UTF-16** only at the `NSTextInputClient` boundary: `TextIndexMap` builds scalar-start tables (UTF-8 ↔ UTF-16),
  rounding a surrogate-pair or mid-scalar offset down to its scalar; `utf16Range` / `utf8Range` convert NSRanges.
* **Run indexes** for splices and styles: `TextIndexMap.run(at:)` (the run before the caret wins at a boundary) and
  `TextRuns` split runs only at the requested offsets, which are caret stops.
* **Caret stops** (`TextLayoutIndex`) = Swift grapheme boundaries minus offsets inside a shaping cluster of the
  engine layout (ligatures such as `ffi`, clusters spanning graphemes); trimmed trailing spaces and separators stay
  valid stops. Caret x = leading edge of the cluster starting at the offset (right edge for RTL), else the trailing
  edge of the cluster ending there; a final separator gets a synthetic empty line. Hit testing picks the nearest line,
  then the cluster box, left/right half mapped to start/end with RTL clusters swapped. Selection rectangles are the
  union of cluster boxes per line. No CoreText relayout anywhere.
* The host converts the engine's row-major `[a,b,c,d,e,f]` affine explicitly (`AffineTransform2D` ⇄
  `CGAffineTransform` tested with translation and skew).

## IME behaviour

`TextInputView` (an invisible `NSView`, `NSTextInputClient`, `KeyOwningControl`) is first responder during a
session. Marked text lives in the draft (`TextEditSession.setMarked`) so the canvas preview shows it (accent
underline); the model before composition is kept: `cancelComposition` (Esc while composing) restores it exactly,
`insertText` / `unmarkText` commit it. Marked text never reaches history on its own: history records whole drafts
only — Apply records one node containing the committed composition (tested: cancel leaves no node, commit + Apply =
one node, one ⌘Z removes it). Candidate windows are placed with `firstRect(forCharacterRange:)` from the engine
layout; `characterIndex(for:)` uses the same hit test. Focus moving to an inspector field commits the composition.

## Keys

While the view is first responder the KeyRouter monitor leaves every key alone (KeyOwningControl): T V X D Q,
digits, Space and ⌫ type/delete text; `performKeyEquivalent` handles ⌘A / ⌘C / ⌘X / ⌘V / ⌘Z / ⇧⌘Z / ⌘Return
before the menu, and `DocumentController.undo/redo/selectAll` route to the text while editing. Keypad Enter or
⌘Return applies; Esc cancels (after the IME had its chance); Return inserts a newline.

## Canvas and inspector

Click = point text (baseline at the click, via a probe layout), drag = area text, click on text = resume editing at
the hit offset, drag = selection, double/triple click = word/all, ⇧-click extends; eight box handles resize area text
(rewrap, no bitmap scaling); ⌘-drag inside moves, outside rotates (⇧ snaps 15°) the source affine. Every completed
box/move/rotate/Character/Paragraph edit first records pending typing, then its own labelled node; slider drags
preview live. Tool change, document switch and clicking elsewhere apply; closing a document drops the session.
Properties ▸ Character / Paragraph / Text box (+ Source text editor for warp/path/vertical, limitations, Apply /
Cancel, Convert to Pixels); Layer ▸ Convert Text to Pixels; options bar defaults for new text.

## 1440-pt inspector clipping (reproduced and fixed)

Reproduced with every panel expanded (evidence `340-before-fix-{1280,1366,1440}.png`): the window content stayed
~1450 pt wide, so the sidebar was pushed past the left edge and the inspector's right edge (Fill value, filter field,
limitation text) was cut. Diagnosis from the running app (split-view constraints): on macOS 26 the floating sidebar
and the inspector overlay the detail column, and the split view requires the detail's minimum PLUS both overlays,
then adds the inspector column again; the detail's minimum was its ideal width (status bar message and fixed
labels, ~800 pt). Fix (ContentView, delimited): document mode's detail column gets an explicit 384 pt minimum/ideal
(`ContentView.documentDetailMinWidth`), the status bar's canvas label truncates. The inspector minimum (288 pt) is
unchanged; library modes untouched. After: the split host equals the window at 1280/1366/1440/1512/1600 pt (self-test
checks, `340-after-fix-*.png`); B5-06/07/08 panels regressed via `DocumentInspectorLayoutTests` (every adjustment and
fill kind, group, pixel, text incl. warped with limitations: no section wider than 288/296/320/380 pt).

## Typing latency on a 20 MP document (5472 × 3648, keystroke → presented frame)

In-app, Fit (level 1, 2736 × 1824), Helvetica 274 px, 43 keys at 8/s (`typing-latency-20mp.log`): first key 46 ms,
**median 1072 ms, p95 1844 ms, max 1980 ms**. Frame render grows linearly with glyph count (46 ms at 1 glyph →
1155 ms at 43): the compositor re-lays out and rasterizes every glyph for every output tile of a full-canvas damage
(NEEDS.md 1). The preview FFI call itself is < 0.1 ms; the Rust bench `typing_preview_latency_20mp` (level 2, no
photo layer, 20 keys) gives preview + frame median 73.7 ms, max 122 ms. Previews coalesce (one in flight), so the
draft never falls further behind than one frame.

## Tests

* Rust `crates/tessera-ffi/tests/document_text_ui.rs`: 18 cases (+1 ignored latency bench): insertion ids/order and
  groups, provisional drafts, run splices vs whole model, mixed-style insert/delete, invalid run bounds/stale
  revision/pending draft, UTF-8 clusters with combining marks and non-BMP, ligature cluster, bidi visual order + rtl,
  one-node typing group with exact undo/redo, cancel/no-op/other-key drafts, flush on unrelated edit/undo/save, locks
  and singular/NaN affines and row-major skew, conversion keeps id/opacity/styles/raster+vector masks with matching
  composite and exact undo, session renderer = CPU reference with the fixture font (injection), native reopen, PSD
  reopen with mixed runs and no TySh resurrection, limitations (warp, vertical, missing font, 32-bit colour, fonts).
* Swift `DocumentTextTests`: 14 cases; `DocumentInspectorLayoutTests`: 2 cases (1440 pt and adjacent widths).
* App self-test (`TESSERA_TEXT_SELFTEST`, Document/Text/TextSelfTest.swift): 63 checks over steps 340–359.

## Deviations

* `add_text_layer` has an `interactive` flag; `set_text_layer` / `edit_text_runs` take `expected_revision`
  (the plan's names were proposals; the contract requires revision checks and draft previews of new layers).
* Screenshots: early reproduction captures (`340-before-fix-*`) were window-region captures; all later evidence uses
  `screencapture -x -o -l <window>` of this PID's window while the app stays in the background (`open -g -n`); the
  self-test routes keys through KeyRouter, key equivalents and the first responder in-process (no focus taken).
* ACCEPTANCE section titled `## B5-10. Type tool and text layers` per the coordinator.

## Limitations (explicit)

* No canvas caret on warped, path or vertical text (labelled Source text editor); vertical composition and
  dictionary hyphenation unsupported; no font fallback (tofu for missing glyphs, missing families are errors and
  reported); live text colours are sRGB bytes written into document samples (reported for non-sRGB / 32-bit).
* Arrow keys move logically in bidi text (clicks, carets and selection are visual); RTL direction per glyph is
  derived from cluster order + strong-RTL script ranges (NEEDS 5); caret heights are size-based (NEEDS 6).
* No on-canvas scale/skew for text (move, rotate, box resize only); paragraph settings apply to the whole layer.
* Typing latency on 20 MP with large text is dominated by engine tile rasterization (NEEDS 1).
* Compositors constructed in filters.rs / io.rs / tools.rs do not get the shared snapshot (outside the allow-list,
  NEEDS 2); they rediscover the same system fonts.
* After a PSD reopen the Channels thumbnails showed black in one capture (359); not investigated (B5-08 area).

## Needs on-screen verification

* Step 351 with a real Japanese input source (candidate window placement, IME Esc) — exercised here only through
  `NSTextInputClient` calls.
* Real mouse/trackpad feel of drag-select, box handles and ⌘-drag rotation, and the caret blink, on an active window.

## Gate (this worktree, CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-10)

* `git diff --check` (excluding generated bindings, which uniffi emits with trailing spaces): exit 0
* `cargo test --locked --release -p typography -p compositor -p psd -p tessera-ffi`: exit 0, 483 passed, 0 failed,
  19 ignored (pre-existing ignores + the latency bench)
* `cargo test … --test document_text_ui`: `test result: ok. 18 passed; 0 failed; 1 ignored`
* clippy `-D warnings`: exit 0; `cargo fmt --all -- --check`: exit 0; build-ffi: exit 0, bindings unchanged after regen
* `swift build`: exit 0; `swift test`: `Executed 247 tests, with 0 failures`; `--filter DocumentTextTests`:
  `Executed 14 tests, with 0 failures`; `--filter DocumentInspectorLayoutTests`: `Executed 2 tests, with 0 failures`
* xcodebuild Debug: `** BUILD SUCCEEDED **`; `make-app.sh debug`: exit 0; `codesign --verify --deep --strict`: exit 0
