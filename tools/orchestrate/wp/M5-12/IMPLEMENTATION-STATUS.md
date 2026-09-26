# M5-12 implementation status: filters and direct adjustments in the app

Branch `wp/M5-12` (base: M5-09 FFI, M5-13/M5-13b document UI, main with M5-06 filters and M5-07).

## What was built

### Engine (Rust)

- `crates/filters/src/registry.rs` (new; one `pub mod registry;` line in `lib.rs`): the Filter menu catalogue. 24 filters
  in the groups Blur, Sharpen, Noise, Distort, Stylize, Render, Other, each with a parameter schema in Photoshop units
  (px, %, levels, degrees, choices, points, toggles), a hand-written `schema_json()` (the crate has no serde), and
  `build(id, values, scale)` mapping values to `Effect` + `FilterParams`. Spatial values are divided by `scale = 2^level`
  so previews on a pyramid level match full resolution. Not listed: Lens Blur (needs a depth map), Oil Paint and Lens
  Flare (explicit placeholders in the crate), Camera Raw (needs a host processor).
- `crates/tessera-ffi/src/document/filters.rs` (new, `impl DocumentSession` + free functions; registered in
  `document.rs` as `mod filtering`).
- Small additive edits: `document.rs` (module, one `Shared.filters` field and its initialiser, stop in `shutdown`,
  `export_flat` bakes smart filters), `document/render.rs` (`present_frame` takes `&Arc<Shared>` and renders the
  *presented* document: three lines), `Cargo.toml` (`filters` dependency), `Cargo.lock`.

How it works: the compositor stores smart filters but does not render them, and previews are not document state. The
render thread therefore presents a derived document: smart objects with enabled smart filters replaced by a baked pixel
layer, the previewed layer replaced by a proxy pixel layer (the filtered viewport level, each level pixel repeated
`2^level` times, so the pyramid shows it exactly at that level; outside the previewed region the layer's own tiles), and
for adjustment previews the adjustment clipped directly above the layer (GPU, no CPU work). Previews and bakes run on a
per-session worker thread, latest wins (a new preview sets the running one's cancel flag). Sources and stack prefixes
are cached, so a slider tick only re-runs the edited filter. Neighbourhood filters run in parallel 256–1024 px blocks
with real-neighbour halos; resampling filters run premultiplied. Bakes run at the viewport level and are refined at
level 0 for the visible region when the view is at 100 % or closer; export bakes at full resolution.

### FFI (all on `DocumentSession` unless noted)

| Call | Notes |
| --- | --- |
| `list_filters() -> Vec<FilterInfo{id, group, name, params_schema_json}>` | free function, from the registry |
| `preview_filter(layer, filter_json, region: Option<DocRect>)` | viewport level, no history, async, latest wins |
| `preview_smart_filter(layer, index, filter_json, region)` | re-edit preview of smart filter `index` |
| `preview_adjustment(layer, adjustment_json)` | Image ▸ Adjustments preview (clipped adjustment, GPU) |
| `clear_preview()`, `filter_error()`, `cancel_filter()` | |
| `filter_detail(layer, filter_json, x, y, w, h) -> FilterDetail{surface_id, width, height, level}` | 1:1 pane (IOSurface) |
| `apply_filter(layer, filter_json) -> DocumentUpdate` | one node; pixels inside the selection, or smart filter appended with the selection as mask |
| `apply_adjustment(layer, adjustment_json) -> DocumentUpdate` | one node; `compositor::Adjustment` JSON, same maths as the adjustment layer |
| `smart_filters(layer) -> Vec<SmartFilterRecord>` | index, id, name, enabled, filter JSON, opacity, blend mode, has mask |
| `set_smart_filter(layer, index, SmartFilterEdit::{Enabled, Params, Blending})` | one node each |
| `remove_smart_filter(layer, index)`, `smart_filter_mask_thumbnail(layer, index, max_px)` | |
| `convert_for_smart_filters(layer)` | Filter ▸ Convert for Smart Filters (same id, props, mask) |

Smart filters are stored in `compositor::SmartFilter` (`name` = filter id, `params` = `{filter, opacity, blend,
mask_png?}`; masks are canvas-sized 8-bit PNG, base64), so they round-trip through `.tessera-doc` unchanged.

### App (Swift)

- TesseraCore `Document/Filters/`: `FilterSchema.swift` (schema → `FilterControl`, values, `FilterSettings` JSON,
  `FilterMemory` for Last Filter and per-filter last values, persisted in user defaults), `DocumentFiltersBackend.swift`
  (second protocol), `EngineDocumentBackend+Filters.swift`, `StubDocumentBackend+Filters.swift` (lists the catalogue,
  applies nothing: "Filters need the engine").
- Tessera `Document/Filters/`: `DocumentFilters.swift` (controller, sheet models, off-main-thread applies),
  `FilterSheets.swift` (generated filter dialog with 1:1 detail pane, angle dial, point pad; Image ▸ Adjustments sheet
  hosting the Properties `AdjustmentEditor`; smart filter blending options), `FilterMenus.swift` (Filter menu from the
  catalogue with Last Filter ⌃F and Convert for Smart Filters; Image ▸ Adjustments with ⌘L / ⌘U / ⌘I),
  `SmartFilterRows.swift` (rows under smart objects: eye, mask thumbnail, name, blending button; double-click re-edits;
  context menu), `FilterSelfTest.swift` (`--filter-selftest <dir>`, started from `DocumentFilters.init`, no TesseraApp hook).
- Shared files, small marked blocks: `AppCommands.swift` (2 menus), `DocumentWorkspace.swift` (`let filters`),
  `DocumentView.swift` (sheet modifier), `LayersOutline.swift` (smart filter hooks marked `WP M5-12`),
  `PropertiesPanel.swift` (`AdjustmentEditor.onEdit`, 4 lines), `EngineDocumentBackend.swift` (`change` no longer
  `private`), `DESIGN.md` §10, `ACCEPTANCE.md` §U part 3 + identifiers, `README.md` (launch argument).
- Accessibility identifiers: `document.filter.<id>.<key>` / `.detail` / `.preview` / `.reset` / `.cancel` / `.ok`,
  `document.adjustment.<kind>.*`, `document.layers.smartFilter.<layer>.<index>.*`, `document.smartFilter.blending.*`.

## Preview latency (Gaussian blur, viewport level)

- Worker time (filter + proxy build), `cargo test --release -p tessera-ffi --test document_filters bench -- --ignored`,
  20 MP layer (5472 × 3648), warm source: **L2 1368 × 912 median 17.6 ms**, **L1 2736 × 1824 median 35.7 ms**
  (cold first preview 28 / 58 ms). Full-resolution apply (radius 8): 410 ms.
- In the app (`--filter-selftest`, `sample.dng` 5212 × 3468 16-bit, window 1440 × 900 @2x, whole image visible at L1,
  value change → listener frame, 12 radius steps): **median 111.5 ms, p90 230.6 ms, max 411.6 ms**
  (`evidence/filter-selftest.log`). The frame's `render_ms` is 45–60 ms of that: the proxy is a level-0 pixel layer, so
  the resident renderer uploads and re-mips the changed tiles. An earlier smart-object proxy was slower in the app
  (median 222 ms): the compositor resamples smart objects on the CPU, single-threaded. Apply at full resolution in the
  app: 0.82 s.

## Tests

- `cargo test -p tessera-ffi -p filters --release`: all green (26 result lines, 0 failed). New: `filters` registry unit
  tests (2, in the lib's 30), `tessera-ffi` lib unit tests for base64 and filter JSON (2, in 34),
  `tests/document_filters.rs` 6 passed + 1 ignored bench: catalogue; preview leaves history unchanged and matches the
  crate at level 1 (latest wins, region); apply adds one node, selection-only, undo, locks; `apply_adjustment` Invert
  and Levels equal the adjustment layer; smart filters render, toggle off/on, blending, re-edit, masked second filter,
  save/load round trip, export bake, delete; 1:1 detail crop.
  `test result: ok. 6 passed; 0 failed; 1 ignored` (document_filters), `test result: ok. 30 passed; 0 failed` (filters
  lib), `test result: ok. 34 passed; 0 failed` (tessera-ffi lib), `test result: ok. 11 passed; 0 failed; 1 ignored`
  (document).
- `cargo clippy --release -p tessera-ffi -p filters --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`: clean.
- `swift test`: `Executed 140 tests, with 0 failures` (133 + 7 in `DocumentFiltersTests`: schema → control mapping for
  every kind, the engine catalogue, filter JSON, Last Filter memory with persistence, the engine adapter, the stub, the
  controller: dialog, reset, OK → one node, ⌃F, Levels sheet, non-pixel targets) and `Test run with 5 tests in 2 suites passed`.
- `xcodebuild -scheme Tessera … -derivedDataPath ~/.cache/tessera-derived-data-M5-12 build`: `** BUILD SUCCEEDED **`.
- engine-api unchanged.

## Evidence

`evidence/filter-selftest.log` (`done, 0 failure(s)`) and screenshots of the Tessera window region:
`filters-01-gaussian-dialog.png` (dialog, live canvas preview, detail pane), `02-gaussian-applied`, `03-gaussian-undone`,
`04-levels-dialog`, `05-levels-applied`, `06-smart-filter-on` (row under the smart object), `07-smart-filter-off`,
`08-smart-filter-on-again`.

## Deviations and notes

- `crates/filters/src/lib.rs` gained one line (`pub mod registry;`); the brief allows only the new file, but the
  module cannot be used otherwise.
- Smart filter masks are per filter (the filters crate's `SmartFilter` model), created from the selection at apply
  time; painting them arrives with the brush tools. Photoshop has one mask for the stack.
- "Every `Adjustment` variant" is read as the compositor's adjustment-layer `Adjustment` (the eight kinds the Properties
  editors cover). The filters crate's extra direct adjustments (Brightness/Contrast, Vibrance, Photo Filter, …) are not
  in the menu yet.
- Merge Down / Flatten of a smart object ignore its smart filters (they composite the live document); export bakes
  them. Layer and composite thumbnails show smart objects without filters.
- A smart object whose filters are all off renders through the compositor's CPU smart-object path (~340 ms per change at
  L1 for 18 MP): compositor cost, not changed here.
- Filter dialogs are sheets and cover the middle of the canvas; the preview is visible around them.
- The stub backend lists the catalogue but cannot apply filters (it has no pixel storage).
