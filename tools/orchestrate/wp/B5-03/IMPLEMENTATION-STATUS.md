# B5-03 implementation status: engine `DocumentSession` in document mode

Branch `wp/B5-03` (contains wp/B5-01 and wp/B5-02). Document mode now runs on the engine; the stub stays for
`--stub-library` and unit tests.

## What was wired

- `apps/mac/Sources/TesseraCore/Document/EngineDocumentBackend.swift`
  - Conversions both ways for every record and enum in the B5-02 name table (`DocumentInfo` ⇄ `DocumentSummary`,
    `LayerNode` ⇄ `LayerRecord`, `DocumentUpdate` ⇄ `DocumentChange`, `DocFrameInfo` ⇄ `DocFrame`, `DocSurfacePlan`,
    `LayerPropsRecord`, `LayerLocks`, `DocRect`, `NewLayer`, `MaskInit`, `DocDepth`, `DocLayerKind`, `DocGroupMode`,
    `ExportFormat`, `ExportColor`). `BridgeError.Failure` becomes `DocumentError` (not found / unsupported / io / invalid).
  - `DocumentHistoryIDMap`: the engine's base node is the UI's id 0 (`Opened`). It is left out of `historyItems()`, its
    children list no parent, and `checkoutHistory(0)` goes to it. The engine root is node 0, so today this is the
    identity map. If pruning removes node 0, the oldest retained parentless node becomes the base.
  - `EngineDocumentListenerAdapter`: `DocumentListener` → `DocumentBackendListener` on the main queue, coalesced. One
    drain is scheduled per burst and delivers the newest frame, the union of changed layer ids (first-seen order), the
    latest (mapped) head and every failure. `cancel()` drops pending callbacks. The scheduler can be injected for tests.
  - `EngineDocumentEngine.for(engine)`: one adapter per `Engine`, and one backend object per session id, so the same
    path or image gives the same tab (`install` dedupes by identity). `EngineDocumentBackend` makes one session call
    per protocol call. `close()` cancels the listener, clears it and closes the session.
- `DocumentWorkspace`: `engine` is an explicit override (tests), otherwise `selectEngine(library:policy:)`:
  - an engine-backed library → that library's engine;
  - policy `.engine` (the app sets it at launch unless `--stub-library`) → a standalone engine opened lazily in the
    app-support directory, so New Document works before a folder is open;
  - otherwise the stub.

  Edit in Layers opens engine images on their own engine with `openDocumentFromImage(imageId, developed: true)`.
  Engine opens (files, Edit in Layers) run off the main thread and then install the document. New Document gives the
  engine's blank canvas: one transparent pixel layer `Layer 1`, selected.
- `DocumentViewport`: `detachSurfaces()` before attaching a resized ring (and on close via the controller). Surfaces are
  sized to the visible level region and `setViewport` takes level pixels, as B5-01 documents.
- Render timing: `DocumentController.renderReadout` (`render: L1 2606 × 1734, 3.6 ms`, 10 Hz) appears in the document
  status bar with Debug ▸ Show Render Timing. `TESSERA_DOC_FRAME_LOG=1` logs every frame.
- `LayersOutline`: layer and mask thumbnails render on a background queue, newest request per slot only; the row
  keeps its previous image until the new one lands.
- `--document-selftest <dir>` (`DocumentSelfTest.swift`) runs ACCEPTANCE §U part 2 through the controller calls the
  UI makes, with step markers for screenshots and the listener's frame timing.
- Small fix: the title subtitle said "1 layers".

## Mismatches found and how they were resolved

1. **Thumbnails re-rendered on every opacity step (Rust fix).** `LayerNode.revision` included the layer's own
   `props_rev`, so each opacity or blend step changed the thumbnail key. The UI (and the engine's own thumbnail cache)
   then re-rendered a 20 MP layer's thumbnail on the main thread: 57 ms per slider tick. The opacity drag's main-thread
   cost was 58.6 ms median per step. `layer_revision` now covers what thumbnails show: content, mask, and for groups
   their children including the children's properties. The layer's own properties no longer count, and thumbnails
   ignore them anyway. The main-thread cost per step is now 0.9–1.1 ms. Test: `thumbnails_are_cached_per_revision`
   was extended to cover interactive and committed opacity, blend, rename, a group's child vs own props, and a mask.
2. **Selection bounds were tile-granular (Rust fix).** `info().selection_bounds` returned stored 256-px tiles, so the
   marching ants and `Selection W × H` would have been wrong. It is now pixel-exact: a scan of the stored tiles, cached
   per selection raster through a `Weak` in session state, so `info()` stays cheap on every tick. The
   `tessera_doc_round_trip_and_same_path_same_session` assertions were updated to cover a tile-corner rect, clipping,
   clear, undo, and history nodes.
3. **Default layer names (Rust fix).** The engine named new layers by layer id (`Exposure 2`, `Hue Saturation 4`,
   `Fill 7`). They are now numbered per kind as Photoshop does: `Exposure 1`, `Hue/Saturation 1`, `Color Fill 1`,
   `Gradient Fill 1`, `Layer 2`, `Group 1`. The same applies to `group_layers`. Test:
   `default_layer_names_are_numbered_per_kind`.
4. **History base.** The engine lists root node 0 (`New Document`/`Open`) as a history item and the UI lists `Opened`
   separately. Resolved in the adapter (map above); neither side's convention changed.
5. Checked and matching without changes: `set_selection_rect` / `clear_selection` are history nodes
   (`Rectangular Marquee`, `Deselect`); interactive + `commit(label)` gives one node per drag; `plan_surface`,
   `set_viewport` and frames use level pixels; straight-alpha RGBA8; blend-mode names and order (`DocBlendMode` equals
   `blendModeNames()`, plus `pass_through`); every `AdjustmentModel` and `FillModel` JSON the Properties panel writes is
   accepted and reads back.

Bindings were regenerated with `build-ffi.sh`; only doc comments changed in `TesseraFFI.swift`. engine-api is unchanged.

## Measured frame time (running app, listener `FrameInfo.render_ms`)

`--document-selftest` on `sample.dng` (5212 × 3468, 16-bit, Metal Apple M4 Max), window 1440 × 900 @2x, viewport level 1
(2606 × 1734 texels), 61 interactive opacity steps at display rate:

| Run | Opacity drag render_ms | Main thread per step |
| --- | --- | --- |
| evidence run | median **3.57 ms**, p90 4.43, max 9.64 (61 frames) | median 0.89 ms |
| final build | median 2.28 ms, p90 5.88, max 8.78 | median 0.98 ms |
| before fix 1 | median 3.65 ms, p90 5.03 | median 58.6 ms (thumbnail re-render) |

Exposure adjustment drag: median 3.4–3.9 ms. Target < 16 ms: met. The cold first frame after Edit in Layers is 35 ms,
and 67 ms after opening the PSD. Edit in Layers takes 1.7–3 s to the first frame (full-resolution develop).

## Acceptance §U part 2 (engine)

`evidence/document-selftest.log`: Edit in Layers → one 16-bit pixel layer `sample` (5212 × 3468, source image id set),
Exposure 1 added and dragged (one `Exposure` row), opacity 100 → 40 % (one `Opacity 40 %` row), ⌘Z restores it, save
`.tessera-doc`, close and reopen (same layers and adjustment JSON), export flat PNG (5212 × 3468), Save As PSD, open the
PSD (`Exposure 1` adjustment over `sample`): `done, 0 failure(s)`. Screenshots (Tessera window region only):
`evidence/engine-01-edit-in-layers.png` … `engine-08-psd-open.png`. ACCEPTANCE §U was updated: intro, step 130 now
passes `--stub-library` for the stub part, part 2 rewritten as steps 143–150 with the engine's behaviour (blank
`Layer 1`, per-kind names, exact marquee bounds, PSD save, the scripted run), and step 142 expects 41 tests.

## Tests

- `cargo test -p tessera-ffi --release`: all green, 96 passed / 6 ignored (`tests/document.rs`: 11 passed, 1 ignored;
  one new test). `cargo clippy -p tessera-ffi --release --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`:
  clean.
- `swift test`: `Executed 133 tests, with 0 failures` (124 before + 9 in `EngineDocumentBackendTests`) and
  `Test run with 5 tests in 2 suites passed`. New tests: enum round trips (every value, both ways), record round trips
  field by field, blend-mode strings vs the engine (each set and read back, `pass_through`), the history id map
  (including pruning), listener coalescing (manual scheduler) and main-thread delivery, a real session through the
  adapter (blank canvas, every adjustment and fill kind, interactive + commit, selection nodes, undo, checkout 0,
  errors, same path = same backend), frames reaching the listener on main, and engine-vs-stub selection in the
  workspace (including an `EngineLibrary`).
- `xcodebuild … -derivedDataPath ~/.cache/tessera-derived-data-B5-02 build`: BUILD SUCCEEDED. `Support/make-app.sh`: built.

## Deviations and notes

- Part 2 was run through `--document-selftest`, which drives the controller calls the sliders, menus and ⌘Z make (the
  opacity slider's `setOpacity(_, final:)` path at display rate), not synthetic mouse events. Screenshots come from
  that run. During the test the window is raised to floating level so `screencapture -R` sees only Tessera. A first
  capture attempt caught another app in front of the window; those images were deleted and not kept.
- The standalone engine used when no folder is open is a second `Engine` on the app-support directory. Folder opens
  already create one `Engine` per folder, so this matches existing practice.
- The thumbnail render itself (~57 ms for a 20 MP layer) is unchanged. It now happens only on content changes and
  off the main thread.
- A group's thumbnail still follows its children's properties, so dragging a child's opacity re-renders the group's
  thumbnail. It renders in the background and is coalesced to the newest request.
- Not done: Photoshop-verified PSD round trip (no Photoshop here); EDR document frames (M5-08); rulers and the Move
  tool as in B5-02.
