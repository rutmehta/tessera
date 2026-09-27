# B5-11 implementation status — live shapes, Pen / Direct Selection and vector masks

Branch `wp/B5-11` from main `c1f95e1` (+ brief `8a36653`). Steps 360–379 in `apps/mac/ACCEPTANCE.md` section
`## B5-11. Shapes, Pen and vector masks (B5-11)`; Sol copy in `acceptance.md`.

## FFI (`crates/tessera-ffi/src/document/vector.rs`, module `vector_shapes`)

Records: `ShapeLayerRecord { layer, model_json, transform: TransformMatrix, revision, live_kind, bounds: ShapeBounds?,
has_open_subpaths, vector_mask: VectorMaskRecord?, notes }`, `VectorMaskRecord { path_json, enabled, feather, density }`,
`ShapeHitRecord { layer, part: ShapeHitPart(Fill|Stroke), local_x, local_y }`, `ShapePathOperation` (Combine,
Subtract, Intersect, Exclude). `DocLayerKind::Shape` (the `kind_of` arm; the live-revision arm is untouched).

`DocumentSession` methods:
- `shape_layer(layer)`, `vector_mask(layer)` — reads (live draft while one is pending).
- `add_shape_layer(name, parent, index, model_json, transform)` — one node ("Rectangle Tool" … / "Pen");
  `created[0]` is the new id; `index` bottom-first, `None` on top.
- `set_shape_layer(layer, model_json, transform, interactive)` — full model + transform; drafts / one final node;
  a draft equal to the base records nothing. The vector mask stays in document space.
- `edit_shape_path(layer, command_json, interactive)` — `move_anchor`, `set_handle` (mirror), `insert_anchor`
  (de Casteljau), `delete_anchor`, `add_subpath`, `set_closed`, `set_fill_rule`, `set_path`; one object or an array;
  absolute positions against the committed path (coincident generated anchors merged first; TesseraCore does the
  same). Clears `live_shape`.
- `transform_shape_with_mask(layer, transform, interactive)` — the explicit linked gesture: `EditShape` +
  transformed `SetVectorMask` in ONE `Batch`.
- `boolean_shape_paths(layer, operands, operation)` — operand paths mapped through their affine and the target's
  inverse, `Path::boolean` in order, operands removed: one `Batch` node; clears `live_shape`.
- `set_vector_mask(layer, mask?, interactive)` — add / replace / delete next to the raster mask.
- `shape_hit_test(x, y, include_stroke, tolerance)` (topmost visible) and `shape_layer_hit_test(layer, …)` — inverse
  affine, `Stroke::outline` (dashes, caps, joins, alignment) then fill under the path's fill rule; geometric, not
  visible-alpha.
- Free function `shape_primitive_path(shape_json)`.
- Temporary (see below): `cancel_shape_preview()`, `convert_shape_to_pixels(layer)`.

Validation before mutation: JSON, finite / invertible transform, `validate_shape`, and Inside / Outside strokes on
open paths (clear error, which the engine would otherwise only hit at render time). Locks come from the compositor
(pixel / all block content, position blocks affine changes only).

## Affine conventions

- Engine `TransformMatrix` / compositor `Affine` / Swift `AffineTransform2D`: row-major `[a,b,c,d,e,f]`,
  `(a·x + b·y + c, d·x + e·y + f)`.
- `kurbo::Affine` (vector crate): `[a, d, b, e, c, f]` in that crate's layout — converted by `kurbo_affine` in Rust and
  `kurboCoefficients` / `init(kurbo:)` in Swift.
- `CGAffineTransform(a: a, b: d, c: b, d: e, tx: c, ty: f)` — `cgAffineTransform` / `init(_ cg:)`.
- Tests use skew + translation (Rust `kurbo_layout_matches_row_major`, `inverse_affine_hits_track_skew_and_translation`;
  Swift `testAffineLayoutsConvertExplicitlyWithSkewAndTranslation`), never identity only.
- Geometry is local level-0 pixels; vector masks and gradient / pattern coordinates are document pixels. The linked
  gesture moves the mask by `new ∘ old⁻¹`.

## Conversion / cancel: B5-10 plumbing (after merging wp/B5-10b, HEAD 70e33dc)

The temporary helpers are gone. `shape_source_edit` in vector.rs is a thin wrapper over B5-10's `source_edit`
(`SourceOps { preview: op.clone(), commit: Some(op) }`, shape ops being absolute) that adds the node's history label
after a final call. `Pending::Shape` / `Pending::VectorMask` are in `Pending::is_source`, so B5-10's
`cancel_source_preview` drops shape and mask drafts (and only live-source drafts). Conversion is B5-10's
`convert_to_pixels` (history "Convert to Pixels"). Swift: `DocumentVectorBackend` requires B5-10's
`cancelSourcePreview()` / `convertToPixels(id:)`, implemented once in `DocumentTextBackend.swift`; the duplicate
`AffineTransform2D` CG / TransformMatrix conversions the merge produced were removed from the vector files in favour of
B5-10's (identical layout). vector.rs constructs no compositor, so there is nothing to route through
`fonts::compositor` / `fonts::install` (conversion's own renderer is engine-owned, see B5-10b's NEEDS).
`document.rs` B5-11 edits: module / re-exports, `DocLayerKind::Shape`, the `kind_of` arm, the two `Pending` keys and
their `is_source` arm.

## Mac

TesseraCore `Document/Vector/`: `ShapeModels.swift` (Codable mirror of the vector crate JSON, opaque pattern paints),
`ShapeGeometry.swift` (affine layouts, primitives, cubic maths, containment, targets, commands, tool maths, Pen draft),
`ShapeToolOptions.swift`, `DocumentVectorBackend.swift`, `EngineDocumentBackend+Vector.swift`. `LayerKindTag.shape`.
DocumentTool cases `rectangleShape, ellipseShape, polygonShape, lineShape, pen, pathSelect, directSelect` (U / P / A).
Tessera `Document/Vector/`: `DocumentVector.swift` (canvas controller, coalesced drafts, overlays, keys),
`ShapeViews.swift` (inspector + options bar), `VectorSelfTest.swift`. Hooks in `// B5-11` lines/blocks of
DocumentTools, ToolsPalette, ToolOverlayView, PropertiesPanel, LayersOutline, DocumentView, AppCommands (Layer ▸
Vector Mask, Combine Shapes, Rasterize Shape), KeyRouter (doc comment), DocumentKeyMap, EditorTools, the two
backend kind mappings. ⌘T on a shape layer switches to Path Selection's affine box.

## Tests (after the merge, HEAD 70e33dc + this commit)

- Rust `crates/tessera-ffi/tests/document_vector_ui.rs`: **17** named cases —
  `test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (plus 1 unit test in `vector.rs`).
- Swift `DocumentVectorTests`: **16** cases — `Executed 16 tests, with 0 failures (0 unexpected)`.
- Gate: `git diff --check` 0; `cargo test --locked --release -p vector -p compositor -p psd -p tessera-ffi` 0
  (90 test binaries, 554 passed, 0 failed, 20 ignored); clippy `-D warnings` 0; `cargo fmt --check` 0;
  `build-ffi.sh` 0; `swift build` 0; `swift test`: `Executed 311 tests, with 0 failures (0 unexpected)` and
  `✔ Test run with 5 tests in 2 suites passed`; xcodebuild `** BUILD SUCCEEDED **`; `make-app.sh debug` 0;
  `codesign --verify --deep --strict` 0.
- `--vector-selftest` (evidence/vector-selftest.log): 71 checks ok, `done, 0 failure(s)`, 27 own-window captures;
  step 379 now adds B5-07 Drop Shadow + Stroke to a shape (two nodes, still a live Shape, styles kept through Convert to
  Pixels, undo restores the styled shape): `vector-27-379-inspector-1440-styles.png`.

## Measured drag latency (20 MP, 5472 × 3648, 8-bit, Metal M4 Max, fit view, debug build)

Latency = synthesized pointer event → first presented frame whose epoch includes that preview.
- Before the merge: fill-only ellipse handle drag **median 517 ms, p90 581 ms, max 860 ms** (51 previews → 50 frames);
  dashed Inside-stroked custom shape ≈ 5.5 s per frame.
- After the merge (this commit): fill-only **median 541 ms, p90 591 ms, max 975 ms** (52 previews → 34 frames);
  dashed stroked shape ≈ 13.3 s for its single preview frame (the document then renders on the CPU compositor because
  B5-07 styles on the document require it, per the engine's log line).
- Engine preview calls ≈ 0.3 ms; the cost is frame rendering (CPU rasterization, full-canvas damage). The host overlay
  follows the pointer; pixels lag (NEEDS.md 3).

## Limitations

- PSD: a shape with a vector mask AND a full-canvas raster mask on a large document saves but cannot be reopened
  (tvMk bridge > 64 MB); pattern shape fills make PSD save fail (stated in the inspector). NEEDS.md 1–2.
- Vector-mask path editing on canvas is not offered (masks are added from the canvas or the selection bounds, then
  enabled / density / feather / moved); Direct Selection edits the shape path.
- Colour: shape colours are document samples; a non-sRGB / F32 document gets an explicit inspector note, no
  conversion.
- Rotation / skew in Path Selection uses the axis-aligned bounds at gesture start as the box (like a fresh Free
  Transform), not a persistent oriented box.
- Layers summary bounds for shapes read "Whole canvas" (engine `affected_bounds`, NEEDS.md 4).
- Needs on-screen verification (not done: the app was never brought to the front): real mouse / tablet feel and
  cursor changes, the NSColorWell colour panel, keyboard focus routing of U / P / A / Return / Esc / ⌫ from a real
  keyboard (tested through `ToolKeyMap` and `DocumentTools.handleKey` with synthesized events), and the options bar
  overlapping the window toolbar in the background-window captures (the options bar sits under the toolbar in
  B5-09's frontmost captures; likely a non-key-window toolbar layout, not changed here).

## Deviations

- `apps/mac/Tests/TesseraCoreTests/EngineDocumentBackendTests.swift` (not in the allow-list): one line adds `.shape` to
  the list of FFI kinds; `testEnumsRoundTripEveryValue` asserts that list covers every `LayerKindTag`, so the mandated
  new case would otherwise fail it.
- The Swift 6.2.4 watermark fix is in the merged base; nothing was applied by hand.
- ACCEPTANCE section titled per the coordinator's correction (no section letter).
- Evidence caveat: `vector-26-378-psd-reopened.png` shows the session saved as `VectorSelfTest.psd` (opening a path a
  session already saved to returns that session); the PSD round trip itself is verified through a separate engine
  (`.psd: shapes reopen as Shape rows`, `rectangle source and mask intact`).
- `tools/orchestrate/wp/B5-11/run-vector-selftest.sh` added (evidence runner).
