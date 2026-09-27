# B5-09 implementation status: Remove tool and neural filters in the app

## FFI added, and why

The existing FFI could not take a mask from the selection or a stroke: `apply_raster_filter` (M5-29) takes masks only
as one float per canvas pixel inside `params_json`, and `remove_distractions` applies immediately without review. So
`crates/tessera-ffi/src/document/retouch.rs` (registered in `document.rs`, four lines) builds the requests engine-side
and calls M5-29's code; it does not reimplement removal, fill, detection or the neural filters.

- `remove_with_selection(layer, backend: Auto|PatchMatch|Lama, params_json)` and
  `content_aware_fill_selection(layer, params_json)`: the selection raster becomes the mask; the JSON is written
  directly (`0` / `1` / 3 decimals), not through a `serde_json::Value`; then `apply_raster_filter`.
- `begin_remove_stroke(layer, size, backend)` / `remove_stroke_points(points) -> dirty rect` /
  `remove_stroke_bounds` / `cancel_remove_stroke` / `end_remove_stroke(params_json)`: a canvas-sized u8 mask
  accumulated from hard round dabs (spacing 15 % of the diameter, the brush crate's `round_coverage` at hardness 1,
  kept where ≥ ½, which is exactly "pixel centre inside the circle", the rule of a non-anti-aliased elliptical
  marquee). An active selection limits the stroke.
- `detect_distractions(layer, params_json) -> DistractionScan` (review only, no history: the geometric detector's wire
  and face-box masks split into 8-connected suggestions with bounds, kept engine-side) and
  `remove_distraction_suggestions(layer, accepted, backend, params_json)` (only the accepted ones, one node; stale
  after any layer edit) plus `clear_distraction_suggestions`. Face boxes come from the caller or from ml-faces when
  its weights are cached; the scan result names the source and the "not segmentation" limitation.
- `neural_filter(layer, kind, params_json, destination: CurrentLayer|NewLayer|SmartFilter)`, `neural_filters()`
  (catalogue with controls). Current layer goes through `apply_raster_filter`. New layer and Smart filter (from a pixel
  layer) evaluate `filters::CompositorFilters` once and commit a single op (`AddLayer`, or the Convert for Smart
  Filters construction with the filter already in its list), so each is one history node. Skin Smoothing without
  `faces` uses the face detector (cached weights only) or the selection's bounds, else a clear error.
- `retouch_models()` (LaMa, DDColor, DRUNet, YuNet, SFace: installed?, cache path, source URL) and `cancel_retouch()`
  (its own cancel flag plus `cancel_filter`).
- Every call reports `RetouchResult { update, backend, note, millis }`: the backend that actually ran (`PatchMatch`,
  `LaMa`, `Content-Aware Fill`, `Skin Smoothing (CPU)`, `DDColor`, `DRUNet`, or `none` for an empty mask) and why it
  differs from the request (Auto without LaMa). History nodes are relabelled (`Remove`, `Content-Aware Fill`,
  `Remove Distractions`, the filter name) instead of the adapter id.
- Weights: only ever read from `<app support>/models/cache` (`resolve_cached_ref`, override
  `TESSERA_RETOUCH_MODEL_CACHE`). A cached model is installed into M5-29's adapter with `load_model` (no download,
  because it is cached). A missing one is the documented error: "<what> needs the <id> model weights, which are not
  installed. Tessera never downloads weights on its own: the file comes from <url> and is expected, hash-verified, at
  <path>". Nothing downloads, nothing is simulated.
- State per session lives in a module-level map keyed by the session's `Shared` address, with dead sessions pruned
  (session ids are only unique per engine, which broke parallel tests at first).

## App

- `Sources/TesseraCore/Document/Retouch/`: `DocumentRetouchBackend` protocol and value types, the engine and stub
  adoptions, and the UI-free state: `RemoveToolOptions` (params JSON, bracket sizes), `DistractionReview`,
  `NeuralSheetState` (allowed outputs per layer kind and selection, values, re-edit JSON keeping face boxes),
  `RetouchErrorPresentation`, `OutlineRequestGate`.
- `Sources/Tessera/Document/Retouch/`: `DocumentRetouch` (Remove tool, stroke capture on its own serial queue, busy
  state with Cancel/Esc, review, Content-Aware Fill, neural sheet model), `RemoveOverlayView` (stroke band and
  suggestion boxes, on-image set only, a subview above `ToolOverlayView`), `RetouchViews` (palette slot, options bar,
  Edit ▸ Content-Aware Fill, Filter ▸ Neural Filters…, the sheet), `RetouchSelfTest` (`--retouch-selftest[=]<dir>`).
- Delimited hooks: `DocumentTools.swift` (mouse, keys, tool change; the outline fix), `ToolsPalette.swift` (slot, options
  bar, title), `FilterMenus.swift` (menu item), `SmartFilterRows.swift` (neural rows re-edit in the Neural Filters sheet),
  `DocumentView.swift` (sheet modifier, status bar tool title), `AppCommands.swift` (3 lines, `// B5-09 begin/end`).
  KeyRouter, EngineDocumentBackend.swift and StubDocumentBackend.swift were not touched (adoption is in extensions).
- The Remove tool is not a `DocumentTool` case (that enum is outside the allowed paths): it rides on the Healing Brush
  slot (the document's tool reads Healing Brush; ⇧J toggles), and syncs the heal brush's size/hardness while on so
  `ToolOverlayView` draws the Remove outline; the heal options are restored when it ends.
- Outline fix: `refreshOutline` starts a generation for every refresh including the synchronous clear
  (`OutlineRequestGate`), so an outline request still in flight when the selection is cleared is dropped.

## Backends on this machine

No weights are installed (`~/Library/Application Support/Tessera/models/cache` is empty; the self-test's app dir too):
LaMa (`remove/lama`), DDColor (`filters/ddcolor`), DRUNet (`enhance/drunet-color`), YuNet / SFace are all absent.
So Remove runs PatchMatch (Auto reports the fallback, LaMa reports the missing-model error), Colorize and JPEG
Artifact Removal show the missing-model message, Skin Smoothing (no weights) runs on the CPU with face boxes from
the selection. Nothing was downloaded. Photo Restoration is not exposed by M5-29's adapter, so it is not in the panel.

## Measured (sample.dng, 5212 × 3468, 16-bit, M4 Max, Debug app build over the release FFI archive)

- Remove stroke (about 130 px brush, 51 points), Auto → PatchMatch: **7.3 s engine time** (6.9 s in the previous run).
- Content-Aware Fill of a 5 % × 5 % marquee: 3.2 s.
- Remove Selection on 80 % × 80 % of the canvas: still running after 1.5 s, cancelled; history unchanged.
- Skin Smoothing (face box from a 522 × 486 selection) to a new layer: 2.5 s.
- Remove Distractions on sample.dng: 0 suggestions (no thin straight lines; no face detector weights).
- Most of that is the M5-29 path at full resolution (PatchMatch over the whole canvas and a JSON mask of 18 M values);
  not changed here (filters.rs / the adapter are outside the allowed paths).

## Tests and gate (all run from the worktree root, `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-08`)

- `cargo test -p filters -p ml-filters -p tessera-ffi --release`: exit 0, every `test result: ok`, 0 failed. New:
  `tests/document_retouch_ui.rs` `test result: ok. 8 passed; 0 failed; 0 ignored` (deterministic PatchMatch removal
  from a selection and one undo step; stroke mask equals the equivalent hard ellipse selection and removes identically;
  Auto without LaMa → PatchMatch with the note; missing LaMa / DDColor / DRUNet / YuNet give the documented error with
  history and pixels unchanged; stale layer ids and a closed document error cleanly; cancellation of Remove and of a
  neural new-layer apply leaves history, layers and pixels unchanged; Content-Aware Fill and the three neural
  destinations are one node each, smart filter re-edit; distraction review removes only the accepted suggestion).
  tessera-ffi lib `42 passed`, filters lib `35 passed`, ml-filters lib `7 passed`.
- `cargo clippy -p tessera-ffi --release --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`: clean.
- `(cd apps/mac && ./build-ffi.sh && swift build && swift test)`: `Executed 195 tests, with 0 failures (0 unexpected)`
  and `Test run with 5 tests in 2 suites passed` (new `DocumentRetouchTests`: 9 tests, including ThemeLint over the new
  views).
- `xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath
  "$HOME/.cache/tessera-derived-data-B5-09" build`: `** BUILD SUCCEEDED **`.
- engine-api unchanged; no NEEDS.md.

## Evidence

`evidence/retouch-selftest.log` (`done, 0 failure(s)`, 11 checks ok) and window-region screenshots (downscaled to
1440 px): `retouch-01-remove-stroke` (the band while dragging), `02-remove-applied` (one `Remove` row), `03-remove-undone`,
`04-reopened` (saved `.tessera-doc` reopened), `05-content-aware-fill`, `06-slow-job-running` (spinner, seconds, Cancel),
`07-slow-job-cancelled`, `08-distractions-review` (nothing found on sample.dng), `09-neural-colorize` and
`10-neural-jpegArtifactRemoval` (missing-model messages with source and path, Apply disabled), `11-neural-skin-no-faces`,
`12-neural-skin-new-layer`. Another application's floating panel was on screen over the top centre of the window
during the run; it covers part of the options bar in the shots.

## Deviations and notes

- ACCEPTANCE §Y uses steps 320–333 (B5-09's reserved range).
- Launching the app with a bare directory path after `--retouch-selftest` sometimes produced no main window (the same
  happened with other path-valued flags); `--retouch-selftest=<dir>` avoids it. The self-test starts from the Filter
  menu's Neural Filters item (built at launch), because TesseraApp.swift is outside the allowed paths.
- README's launch-argument table (not in the allowed paths) does not list `--retouch-selftest`.
- Remove on smart objects works from a stroke without a selection (the adapter keeps smart retouch filters unmasked);
  with a selection it reports the adapter's error. New layer output needs a pixel layer.
- No distraction suggestions were found on sample.dng, so the review boxes are shown in the tests, not in a screenshot.
