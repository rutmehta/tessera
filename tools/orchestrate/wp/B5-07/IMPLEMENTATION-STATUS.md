# B5-07 implementation status: Layer Style inspector and global light

## FFI (crates/tessera-ffi/src/document/styles.rs, registered in document.rs)

`impl DocumentSession`:
- `layer_styles_json(layer) -> String`: the `LayerStyles` JSON of the live state.
- `set_layer_styles_json(layer, json, interactive) -> DocumentUpdate`: `DocOp::SetProps` with the live props and
  only `styles` replaced. The JSON is validated (`LayerStyles::validate`) first. With `interactive`, the edit goes on
  the scratch path under the layer's props key, so style and opacity drags fold into one net node on `commit`.
  The call is refused on Lock All layers, adjustment layers and pass-through groups (the renderer cannot draw
  styles on those two kinds).
- `layer_style_summaries() -> Vec<LayerStyleSummary{layer, effects: [StyleEffectSummary{index, kind, enabled}]}>`:
  every styled layer in `layers()` order, effects top first. This is the side query the Layers outline uses;
  `LayerNode` is unchanged.
- `global_light() -> GlobalLightRecord{angle, altitude}` and `set_global_light(angle, altitude, interactive)`
  (`DocOp::SetGlobalLight`, validated; the interactive key is a sentinel that cannot be a layer id).
- `copy_layer_styles(from)`: an application-wide clipboard, like Photoshop's; it records no history.
  `can_paste_layer_styles()`, `paste_layer_styles(to: Vec<LayerId>)` (one `Batch` node, all or nothing, "Paste
  Layer Style") and `clear_layer_styles(layer)` (one node, "Clear Layer Style").
- Free function `style_effects_schema_json()`: per kind, the title, rank, repeatable / global-light / PSD /
  behind flags, the serde defaults, fields (key, type, UI range, unit, display scale, options) and metadata
  (kept-not-rendered) fields, listed top first, plus the Scale Effects field and limits. It is a free function
  so the stub backend and unit tests can read it without a session.

**render.rs CPU fallback** (`// B5-07 begin/end`, approved by the coordinator). When the resident renderer
returns `Unsupported` (styled layers), frames go through a lazily created CPU `Compositor` via `cpu_present`,
and `read_level` goes through `render_level_rgba`. The GPU path is unchanged for documents without styles.
Switches between GPU and CPU are logged once to stderr. `DocFrameInfo` has no backend field, so the log is the
only record.

## Effects covered

All 11 `StyleEffect` kinds: Drop Shadow, Inner Shadow, Outer Glow, Inner Glow, Bevel & Emboss, Satin, Color /
Gradient / Pattern Overlay, the generic Overlay (shown only when present), and Stroke. Each has an editor
generated from the schema: sliders, an angle dial, colour wells, blend modes, choices, and a fill for stroke
(Color / Gradient / Pattern) and overlays. Use Global Light works as in Photoshop: editing the angle of an effect
that uses it moves the document's light, and turning it off keeps the current angle and altitude as the effect's
own. Scale Effects is in Blending Options, next to the layer's blend mode, Opacity and Fill Opacity. Repeatable
kinds (Stroke, Inner / Drop Shadow, the overlays) get "+" (the new one goes directly above), up to 10 per kind
and 64 per layer. The list follows the engine's stacking order and never offers reordering across kinds.

**Metadata only, not working controls:** contour and jitter (`EffectShape`) on shadows, glows, satin and bevel,
and bevel texture, which is kept in the PSD `lfx2` descriptor only. They are listed under "Kept, not rendered".
Gradient stops can be reversed and switched linear / radial, but not edited stop by stop. Pattern contents show
their size only.

## App

- The **Layer Style inspector** is a floating `NSPanel` (Styles/LayerStyleInspector.swift, LayerStyleEditors.swift).
  It follows the primary layer and has no Cancel: a drag is live and records one node on release, colour wells
  debounce 400 ms, and each click is one node.
- It is reachable from **Layer ▸ Layer Style ▸** (Blending Options…, each effect…, Copy / Paste / Clear Layer
  Style, Global Light…), from the **fx footer button** in the Layers panel, from **double-clicking a layer row
  away from its name** (a double-click on the name still renames), from the Properties summary, and from the
  effect rows.
- **Global Light…** is a small panel with a dial and Angle / Altitude sliders; a drag records one node.
- **Layers outline** (delimited hooks in LayersOutline.swift): an fx glyph on styled rows, and effect sub-rows
  (eye toggles, double-click edits, context menu) after any smart filter rows. Groups get the glyph only; see
  deviations. The layer context menu gained Blending Options… and Copy / Paste / Clear Layer Style.
- **Properties panel**: a compact Layer Style summary (one hook line) with an Edit… button.
- **Backends**: the `DocumentStylesBackend` protocol is adopted by `EngineDocumentBackend` and by
  `StubDocumentBackend` (an in-memory side table; it keeps styles but does not draw effects). Both adoptions
  live in new files, so EngineDocumentBackend.swift and StubDocumentBackend.swift are untouched.
- **`--styles-selftest <dir>`** (Styles/StylesSelfTest.swift) runs the ACCEPTANCE steps and prints step markers
  for screenshots. It is not listed in README.md, which is outside the allowed paths.

## Tests

- `crates/tessera-ffi/tests/document_styles_ui.rs`: `test result: ok. 8 passed; 0 failed; 1 ignored`. The ignored
  test is the bench. Covered: every kind round-trips (non-default values and contour / jitter metadata);
  top-first order including repeats; unrelated props (opacity, fill, blend, mask, locks) are preserved; Lock
  All, adjustment layers and pass-through groups refuse; an interactive drag is one node, and so is a
  global-light drag; copy / paste / clear are one node each; a global light change moves the shadows of two
  layers and leaves a local one alone (the session render matches the CPU compositor); `.tessera-doc` and PSD
  round trips; a PSD with bevel is refused rather than dropped; a styled document presents frames on the
  default backend and matches the CPU composite.
- Unit test in styles.rs: the schema is top first and every kind's defaults validate.
- `apps/mac/Tests/TesseraCoreTests/DocumentStylesTests.swift`: 7 tests, 0 failures. They cover the schema, JSON ↔
  model for every kind, repeatable order and limits, Global Light propagation in `StyleLightModel`, the engine
  adapter, the stub, and the `DocumentStyles` controller (one node per gesture, undo).
- Gate: `cargo test -p compositor -p psd -p tessera-ffi --release` all ok; clippy `-D warnings` clean; `cargo
  fmt --check` clean; `swift build` and `swift test` (XCTest: Executed 186 tests, with 0 failures; Swift Testing:
  5 tests passed); xcodebuild `** BUILD SUCCEEDED **`.
- In the app, `--styles-selftest` passed 15 checks (`done, 0 failure(s)`); screenshots are in `evidence/`
  (01–08 and `-panel` variants) with `styles-selftest.log`.

## Performance of the styled CPU fallback (M4 Max, 1368 × 912 viewport)

| Document | Level | Styled frame (drop shadow 20 + stroke 8) | Unstyled frame |
| --- | --- | --- | --- |
| 1000 × 700 (self-test) | L0 | 0.55–1.0 s | – |
| 1024 × 768 | L0 | 0.77–0.84 s | 12 ms |
| 2048 × 1536 | L0 | 13–26 s | 15 ms |
| 4896 × 3264 (16 MP) | L1 | about 108 s | 19 ms |
| 5472 × 3648 (20 MP) | L2 | fails: "style alpha canvas exceeds CPU pixel limit" | 24 ms |

The cost comes from the compositor, not the FFI: `emit_styles` renders full-canvas effect planes for every tile
job. That work, and GPU-resident styles, is written up for Machine A in NEEDS.md.

## Deviations

- render.rs was edited for the CPU fallback, outside the original allowed paths but approved by the coordinator.
- Effect rows are not shown under groups: the outline's structural diff indexes group children, so group styles
  show only the fx glyph (Properties and the inspector list them).
- A double-click on a layer row away from the name now opens Layer Style for every layer. Before, it renamed
  non-group layers and expanded or collapsed groups; groups still expand with the disclosure triangle.
- Copy Layer Style copies the effects and Scale Effects, not the blending options (fill, Blend If).
- The global-light interactive key reuses `Pending::Props(u64::MAX)`, because document.rs could take module
  registration only.
- Styled documents at 16 MP and larger cannot be drawn (NEEDS.md), so Sol's steps use a 1000 × 700 document.
- With Fill 0 %, the shadow shows through the interior: the engine has no "Layer Knocks Out Drop Shadow".
- During one self-test run another process sent key events to this app (a "Move sample.dng to the Trash?"
  alert appeared). The app was quit without confirming, and the self-test was rerun on an empty scratch library
  folder (`--folder`).
