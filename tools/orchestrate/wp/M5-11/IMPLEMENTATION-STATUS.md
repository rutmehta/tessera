# M5-11 implementation status: layered editor tools (painting, selections, transform)

Branch `wp/M5-11` (contains M5-09, M5-13, M5-13b and main with the `brush` / `selection` crates of M5-05).

## What was built

### Engine (`crates/tessera-ffi/src/document/tools.rs`, an `impl DocumentSession` block)

- **Strokes.** `begin_stroke` snapshots the target raster (pixels, or the mask: one is created revealing all when the
  layer has none) and opens a `brush::Stroke` limited by the selection, on a scratch copy of the document that the
  viewport shows. `stroke_points` takes the samples of one display frame, rasterizes the new dabs, composites exactly
  their dirty rect into a live copy of the raster (recomputed from the stroke-start snapshot), applies the touched
  tiles to the scratch as one `DocOp::PaintTiles` and requests a frame only (rows and thumbnails refresh when the
  stroke ends). `end_stroke` applies the whole stroke (plus the new mask, if any) as **one** history node labelled
  `Brush Tool` / `Eraser` / `Clone Stamp` / `Healing Brush`. Transparency lock keeps alpha; pixel lock refuses.
  Clone / heal sample `p + (dx, dy)` of the source layer, or the composite with Sample All Layers. Pressure, tilt,
  timestamps, smoothing, symmetry (vertical, horizontal, dual, diagonal, radial, mandala), sampled tips (built-in Chalk
  and Square, imported `.abr`) come from the brush crate.
- **Selections.** Marquee (rect via the existing ramped rect, ellipse / row / column), lasso (free, polygon,
  magnetic + `magnetic_path` for the live wire), wand, quick selection, colour range, subject / sky / object through the
  engine's segmenter (`Engine::with_segmenter`, a small accessor added to `masks.rs`), all / none / inverse, modify
  (border, smooth, expand, contract, feather, computed on the selection's bounds only), refine edge (interactive on the
  scratch, one `Refine Edge` node, cancel restores), outlines (marching squares at a pyramid level ≤ 8 MP, cached per
  selection and level), save / load channels. Every call is one history node with Photoshop's label; `SelectionOp`
  combines raster-wise (`compositor::document::selection::combine`), "no selection" counting as empty.
- **Free Transform.** `begin_transform` captures pixel layers and linked masks, `set_transform` resamples on all cores
  (bilinear preview) into the scratch, `commit_transform` resamples with the chosen interpolation (nearest / bilinear /
  bicubic, premultiplied) as one `Free Transform` node, `cancel_transform` drops the preview.
- **Fill / clear / eyedropper.** `fill_selection` (colour at an opacity, or the Content-Aware placeholder: a harmonic
  membrane fill from the selection's boundary, ≤ 0.5 MP), `delete_selection` (to transparency, or the background colour
  on a Background layer), `sample_color` (composite via the CPU compositor's tile, or a layer; 1×1 / 3×3 / 5×5).
- `io::selection_bounds` now handles selections whose absent tiles are selected (Select All, Inverse): before, any
  such selection reported the whole canvas.

### App

- TesseraCore `Document/Tools/`: `DocumentToolsBackend` (records with TesseraCore names, table in the file header),
  `EngineDocumentBackend+Tools`, `StubDocumentBackend+Tools` (geometric subset: every selection as its bounding rect,
  rectangular outline; painting and image tools report that they need the engine), `EditorTools.swift` (tool titles,
  keys, groups and palette order; `ToolKeyMap`; `BrushHUDMath` with bracket steps, ⌃-drag and opacity digits;
  `SelectionModifiers`; `PressureCurve`; `FrameStrokeCoalescer`; `ToolColors`), `FreeTransformMath.swift`
  (`AffineTransform2D`, `FreeTransformModel`: `T(ref+t)·R·Skew·S·T(−ref)`, handle drags with ⇧ / ⌥, rotation with
  15° snap, hit test).
- Tessera `Document/Tools/`: `DocumentTools` (options, colours, every tool's gestures on the viewport, stroke capture
  with tablet pressure / tilt coalesced per frame on a serial engine queue, selections, Free Transform and Move,
  Select / Edit commands, keys, brushes), `ToolOverlayView` (marching ants from the outline, gestures, brush outline
  and hardness ring, symmetry guides, clone source, HUD chip, transform box, Select and Mask preview modes),
  `ToolsPalette` (palette, swatches, options bar, brush preset popover), `ToolsInspector` (Color and Brushes
  sections), `ToolsSheets` (Select and Mask, Color Range, Modify, Fill, Save Selection), `ToolsMenus` (Select menu,
  Edit ▸ Fill / Clear / Free Transform ⌘T / Transform ▸), `ToolsSelfTest` (`--tools-selftest`).
- Shared files, small delimited blocks: `DocumentViewport` (tool drag case and forwarding, overlay, coordinate helpers,
  tracking area, right mouse), `DocumentView` (palette + options bar instead of the M5-13 tool bar, inspector sections,
  sheets, stroke readout), `AppCommands` (Select menu body, Edit group, ⇧⌘I only outside document mode for Import
  Lightroom Catalog), `KeyRouter` (tool keys first in document mode), `TesseraApp` (`--tools-selftest`),
  `DocumentKeyMap` (`DocumentTool` gains the new cases), `EngineDocumentBackend.change` made internal (the tools
  extension uses the history map through it). `ACCEPTANCE.md` §V, `README.md`, `DESIGN.md` §10 bullet.

## FFI list

`DocumentSession`: `begin_stroke`, `stroke_points`, `end_stroke`, `cancel_stroke`, `set_clone_source`,
`select_marquee`, `select_lasso`, `magnetic_path`, `select_wand`, `select_quick`, `select_color_range`,
`select_subject`, `select_sky`, `select_object`, `select_all`, `select_none`, `select_inverse`, `modify_selection`,
`refine_edge`, `cancel_refine_edge`, `selection_outline`, `save_selection`, `load_selection`, `selection_channels`,
`begin_transform`, `set_transform`, `commit_transform`, `cancel_transform`, `fill_selection`, `delete_selection`,
`sample_color`. Free functions: `brush_tips`, `import_abr`, `brush_tip_preview`. Records / enums: `PaintColor`,
`ToolPoint`, `StrokeTarget`, `StrokeTool`, `PaintSymmetry`, `PaintBrush`, `StrokeSample`, `StrokeFrame`, `SelectionOp`,
`MarqueeShape`, `LassoKind`, `SelectionModify`, `RefineEdgeParams`, `OutlinePolyline`, `TransformMatrix`,
`TransformInterpolation`, `TransformInfo`, `SelectionFill`, `BrushTipInfo`, `BrushTipImage`. engine-api unchanged.

## Measured dab latency

`cargo test -p tessera-ffi --release --test document_tools -- --ignored --nocapture` (5212 × 3468 16-bit layer,
three 2606 × 1734 surfaces at level 1, 120 frames × 6 samples, Apple M4 Max):

| Brush | `stroke_points` median / p90 | frame render median / p90 | dab → pixels in the surface median / p90 |
| --- | --- | --- | --- |
| 30 px | 0.56 / 0.89 ms | 1.83 / 2.74 ms | 2.39 / 3.73 ms |
| 100 px | 1.54 / 2.11 ms | 2.16 / 2.81 ms | 3.61 / 4.98 ms |
| 300 px | 4.68 / 6.20 ms | 2.66 / 5.32 ms | 7.55 / 11.25 ms |

In the running app on `sample.dng` via Edit in Layers (`evidence/tools-selftest.log`, 87 px brush, 181 frames through
the viewport's mouse path): `stroke_points` median 0.64–0.84 ms, frame render median 1.9 ms, p90 2.9–3.0 ms. Target
< 16 ms: met. Single outliers (first frame of a stroke, or frames that coincide with a model reload) reach 14–270 ms.

## Tests

- `cargo test -p tessera-ffi --release`: all green, 105 passed / 7 ignored (`tests/document_tools.rs`: 9 passed,
  1 ignored bench). `cargo clippy -p tessera-ffi --release --all-targets -- -D warnings`: clean.
  `cargo fmt --all -- --check`: clean.
- `(cd apps/mac && ./build-ffi.sh && swift build && swift test)`: `Executed 148 tests, with 0 failures` (133 before +
  15 in `DocumentToolsTests`) and `Test run with 5 tests in 2 suites passed`. ThemeLintTests green.
- `xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath
  ~/.cache/tessera-derived-data-M5-11 build`: BUILD SUCCEEDED.
- `--tools-selftest` on a scratch copy of `sample.dng`: 13 checks ok, `done, 0 failure(s)`; screenshots (Tessera window
  region only) `evidence/tools-1-edit-in-layers.png` … `tools-9-psd-reopen.png`.

## Deviations and notes

- **Signatures** follow the brief with additions the UI needed: `select_marquee` / `select_lasso` take feather and
  anti-alias, wand / quick take `sample_all` and the op, `refine_edge` takes `interactive`, `delete_selection` takes the
  background colour, plus `cancel_stroke`, `magnetic_path`, `cancel_refine_edge`, `selection_channels`, `sample_color`.
  `set_clone_source(layer, dx, dy)` is session state; the UI keeps the offset for aligned cloning.
- **Image-driven selections** (wand, quick, colour range, magnetic lasso, refine edge) analyse the finest pyramid
  level of ≤ 6 MP (≤ 3 MP for the segmentation models) and upsample the mask bilinearly; on sample.dng that is level 1,
  so their edges are accurate to ~2 px. Marquees, lassos and modify work at full resolution.
- **Selection channels** are session state (the `.tessera-doc` format and PSD writer have no alpha-channel field).
- **Brush tips** cross the bridge as small grey previews (`BrushTipImage`, ≤ 256 px), the one pixel buffer in the
  API; the tip library is process-wide (imports last for the run).
- **GPU dab rasterizer** (`brush::gpu`) is not used: it creates its own wgpu device (the session keeps one Metal
  device) and reads back per batch; the CPU path meets the budget above.
- **Transform** covers pixel layers (with linked masks); adjustment, group, text and smart-object layers are refused
  with a message. Warp / perspective / distort are not implemented. The preview is bilinear, the commit uses the
  options bar's interpolation.
- **Placeholders**: Gradient (G) fills the selection with the foreground colour; Crop (C) and Type (T) explain
  themselves; Content-Aware Fill is a membrane fill limited to 0.5 MP.
- **Object Selection** is click-to-select (point prompt); the box and lasso prompt modes are not wired.
- **Select and Mask** is a sheet (not a workspace) with Overlay / On Black / On White / Marching Ants previews drawn
  from the refined outline; output is the selection only (no decontaminate / new layer).
- The inspector column is clipped on a 1440 pt window with the sidebar open, as in M5-13b's screenshots (pre-existing
  layout); the new options bar scrolls sideways instead of widening the canvas.
- Clone / heal "Sample All Layers" reads the full-resolution composite at stroke start (≈ 0.3 s on 18 MP).
