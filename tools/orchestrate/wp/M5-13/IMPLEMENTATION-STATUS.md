# M5-13 implementation status: document mode UI over a stub backend

Branch `wp/M5-13`. Everything runs over `StubDocumentBackend`; the real `DocumentSession` (M5-09) is wired in M5-13b.

## Files

TesseraCore (`apps/mac/Sources/TesseraCore/Document/`)

| File | Contents |
| --- | --- |
| `DocumentBackend.swift` | `DocumentBackend` / `DocumentEngine` / `DocumentBackendListener` protocols and their records; `DocumentSurfaces` (RGBA8 IOSurfaces) |
| `StubDocumentBackend.swift` | `StubDocumentEngine` (new / open / from image, same path = same session) and `StubDocumentBackend` (snapshot history, interactive + commit, JSON `.tessera-doc`, flat export through ImageIO, thumbnails cached per revision, coalesced render queue into attached IOSurfaces) |
| `StubDocumentModel.swift` | Stub layer tree (procedural sample art, image files, merged content), masks, image cache |
| `StubCompositor.swift` | CPU compositor: 27 blend modes (COMPOSITOR.md §2), clipping, pass-through / isolated groups, masks, every adjustment, fills |
| `DocumentOutline.swift` | `LayerRecord` list → tree; `moving(_:into:at:)` for drops; `diff(from:to:)` = minimal outline edits (LIS-anchored, one move per drag); `backendMoves(to:)` → `move_layer` calls |
| `DocumentBlendModes.swift` | `DocBlendMode`: 27 modes, menu order, six Photoshop groups, backend strings |
| `DocumentAdjustments.swift` | `AdjustmentModel` / `FillModel`: JSON mirrors of `compositor::Adjustment` / `Fill` |
| `DocumentViewportMath.swift` | zoom / pan / fit / presets, `level = floor(log2(1/zoom))` clamped, visible rect, level rect |
| `DocumentKeyMap.swift` | document-mode key table (tools, Space, Tab, F, ⌫ and the ⌘ shortcuts) |

Tessera (`apps/mac/Sources/Tessera/Document/`): `DocumentController` (observed model per document, all edits),
`DocumentWorkspace` (tabs, New / Open / Edit in Layers / Save / Save As / Export Flat / close prompt, Tab, F),
`DocumentViewport` (CAMetalLayer EDR presenter, checkerboard shader, zoom / pan / pinch / scrubby / Space-pan,
marquee + marching ants), `LayersOutline` (NSOutlineView rows, drag and drop, context menu), `LayersPanel`,
`PropertiesPanel`, `DocumentHistoryPanel`, `DocumentView` (content, tool bar, zoom HUD, inspector, tabs, status bar),
`DocumentSheets` (New Document, Export Flat), `DocumentControls` (ValueSlider / NSColorWell / CurveEditorView bridges).

Changed: `AppModel` (`ViewMode.document`, `documents`, ⌘Z / ⌘A routing), `ContentView`, `KeyRouter` (document branch,
Space key-up), `AppCommands` (File, Edit, View, Layer, Select, Library ▸ Edit in Layers; mode-dependent shortcuts),
`TesseraApp` (Finder open, `--new-document`, `--open-document`), `Theme` (checker aliases), `Info.plist` (document
types, `dev.tessera.document` UTI), `Cull/AssistController.swift` (Swift 6.2.4 build fix), `DESIGN.md` §10 (additions),
`ACCEPTANCE.md` §U. Tests: `DocumentModeTests.swift`, `DocumentKeyRoutingTests.swift`.

## The protocol

`DocumentBackend` is M5-09's `DocumentSessionProtocol` (branch `wp/M5-09`, e050cdf / 92bef76) call for call, same labels
and throws. Records carry distinct Swift names so files importing both TesseraCore and TesseraFFI are not ambiguous:

| TesseraCore | TesseraFFI (M5-09) |
| --- | --- |
| `DocumentSummary` | `DocumentInfo` |
| `LayerRecord`, `LayerKindTag`, `LayerLockFlags`, `LayerGroupMode` | `LayerNode`, `DocLayerKind`, `LayerLocks`, `DocGroupMode` |
| `LayerProperties`, `NewLayerKind`, `LayerMaskInit` | `LayerPropsRecord`, `NewLayer`, `MaskInit` |
| `DocumentChange`, `DocHistoryEntry`, `CanvasRect` | `DocumentUpdate`, `DocHistoryItem`, `DocRect` |
| `DocFrame`, `DocViewportPlan`, `DocBitDepth` | `DocFrameInfo`, `DocSurfacePlan`, `DocDepth` |
| `DocExportFormat`, `DocExportColor` | `ExportFormat`, `ExportColor` |
| `DocumentBackendListener` | `DocumentListener` |
| `DocumentEngine` (3 calls) | `Engine.newDocument / openDocument / openDocumentFromImage` |

Conventions the UI relies on: `layers()` is flat pre-order with siblings top first, `index` = compositor child index
(0 = bottom); groups report `blendMode == "pass_through"` in pass-through; `historyHead == 0` and
`checkoutHistory(id: 0)` mean "as opened"; frames carry `canvasRect` (level 0) plus `width × height` valid texels
top-left in the surface; surfaces are RGBA8 sRGB straight alpha; `setViewport` takes the region in level pixels.

## Tests

`swift test`: `Executed 124 tests, with 0 failures` (XCTest; 93 before this WP, 31 added) and `Test run with 5 tests in
2 suites passed`. New suites: DocumentOutlineTests 8, DocumentBlendModeTests 3, DocumentViewportMathTests 4,
DocumentKeyMapTests 3, DocumentAdjustmentModelTests 2, StubDocumentBackendTests 7, DocumentKeyRoutingTests 4;
ThemeLintTests stays green. `xcodebuild … -derivedDataPath ~/.cache/tessera-derived-data-M5-13 build`: BUILD SUCCEEDED.
`Support/make-app.sh` builds `apps/mac/build/Tessera.app`; File ▸ New Document shows the six stub layers
(`evidence/new-document-sheet.png`, `evidence/document-mode-stub.png`). Drag reorder, rename, opacity drag and ⌘Z were
also exercised with real mouse events in the running app.

## What M5-13b does to wire the real session

1. Merge `wp/M5-09` (bindings regenerated by `build-ffi.sh`).
2. Add `EngineDocumentBackend.swift` in TesseraCore: `extension DocumentSession: DocumentBackend` (or a thin wrapper)
   converting records field by field with the table above, plus `final class EngineDocumentEngine: DocumentEngine`
   over `Engine`. The listener adapter wraps a `DocumentBackendListener` in a `DocumentListener`.
3. Set `DocumentWorkspace.engine` to the engine adapter when the library is engine-backed (keep the stub for
   `--stub-library` / stub runs); `editInLayers` already passes the engine image id when the engine is not the stub.
4. Verify against the engine: history node 0 for "as opened" (the Opened row calls `checkoutHistory(id: 0)`),
   `plan_surface` semantics (the viewport allocates surfaces the size of the visible level region and calls
   `set_viewport` in level pixels, which matches M5-09's doc), and that `set_selection_rect` / `clear_selection` create
   history nodes (the UI already expects this).
5. Run ACCEPTANCE §U part 2.

## Deviations and notes

- **Names**: TesseraCore records are renamed (table above) instead of reusing M5-09's names; identical names would be
  ambiguous in every file importing both modules (M5-09 flagged this).
- **Protocol grew to M5-09's final API**: besides the brief's calls it has `id`, `layer`, `setSelectedLayers`,
  `renameLayer`, `setLocks`, `groupLayers`, `ungroupLayer`, `setMaskDensity`, `setMaskLinked`, `setFillOpacity`,
  `maskThumbnail`, `historyMemoryBytes`, `setListener`, and `setFillJson(interactive:)`. ⌘G / ⇧⌘G are therefore one
  history node each.
- **Stub rendering** is CPU and coarsens its sampling beyond ~0.9 M texels per frame (0.22 M while dragging), so edges
  look stepped at large window sizes; it is not held to the engine's 16 ms budget. `New Document` on the stub opens the
  six-layer sample instead of a blank canvas (so the UI has something to show); the engine gives a blank document.
- **Stub files**: `.tessera-doc` written by the stub is JSON only the stub reads; PSD/PSB save is refused with a clear
  message; PSD/PSB open shows the flattened composite as one layer (no PSD reader in Swift).
- **Rulers** (optional in the brief) are not implemented. **Move tool** only explains that moving pixels arrives with
  transforms (the compositor does not model translation yet). The layer **filter row** is a disabled placeholder.
- **Checkerboard colours** are aliases of existing tokens (`thumb`, `plotText`), documented in DESIGN.md §10; no new colours.
- **Shortcut clashes** resolved by mode: ⌘E is Edit in Layers in the library and Merge Down in document mode; ⇧⌘S is New
  Snapshot (develop) outside document mode and Save As inside; ⇧⌘E Export… vs Export Flat…; ⇧⌘N stub items vs New Layer.
  Open Document is ⇧⌘O because ⌘O is Open Folder. Zoom In is ⌘= (the menu shows ⌘=).
- **Swift toolchain**: the machine has Swift 6.2.4 (not 6.3); `AssistController.analyze` did not compile on it and is
  fixed in its own commit.
- **Screenshots** are of the Tessera window region (`screencapture -x -R`) so other apps on screen are not captured.
