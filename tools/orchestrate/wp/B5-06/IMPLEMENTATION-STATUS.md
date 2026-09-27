# B5-06 — implementation status

Branch `wp/B5-06`. Document-mode fixes from B5-v and Properties editors for every adjustment M5-26 / M5-28 added.

## B5-v findings

### Step 144: ⌘E in the grid did not open Edit in Layers

**Reproduction.** I ran the step sequence on this worktree's debug and release builds (`open -n …`, `--folder` holding a copy
of `sample.dng`, with and without the 12 sample JPEGs; step 143's ⌘N / Create first, then the Grid segment, then a click on
`sample.dng`, then ⌘E, sent through System Events), seven runs in all. ⌘E opened the document every time, so the failure
did not reproduce with System Events. The unit test below fails on the code before this change, where `KeyRouter.handle`
returned `false` for ⌘E and the key was left to the menu.

**Root cause (by inspection).** ⌘E belonged to two menu items: Layer ▸ Merge Down, bound all the time and listed earlier
in the menu bar, and Library ▸ Edit in Layers, whose key equivalent SwiftUI turns off in document mode and back on when
leaving it (`.shortcut(!docMode, …)`). The grid's ⌘E therefore depended on SwiftUI rebuilding the Library menu item after a
mode change. AppCommands already notes that SwiftUI can leave stale disabled state on menu items (see the undo comment).
When that rebuild was stale, as after B5-v's step 143 → Grid, the key went nowhere, or to the disabled Merge Down. Step 144's
engine-image path (`editInLayers` → `openDocumentFromImage`) works: once the command is called, it opens `sample` at
5212 × 3468.

**Fix.** `KeyRouter.handleEditInLayers`: outside document mode, and not in the People view, a text field, a sheet or a
panel, plain ⌘E calls `DocumentWorkspace.editInLayers(focusedItem)` directly. It is ignored while a document is still
opening. The route no longer depends on menu state. Merge Down's ⌘E is bound only when a document is shown
(`.shortcut(doc != nil, …)`).
Test: `DocumentKeyRoutingTests.testCommandEInGridRunsEditInLayers` covers ⌘E in the grid, after a trip through document
mode, in the loupe, passing through in document mode, ⇧⌘E ignored, and a focused text field keeping the key. Evidence:
`evidence/v144-cmd-e-after-new-document.png` (step 143, then Grid, then ⌘E: the `sample` tab, 5,212 × 3,468 px).

### Step 140: the Save As filename field could not be targeted

**Root cause.** Save As used `NSSavePanel`. Since macOS 11 the panel runs out of process: its content view is an
`NSRemoteView`, and I confirmed this with a probe (`panel.contentView` is `NSRemoteView`, and so is the first responder). The
app cannot set an identifier on the name field or reach it through accessibility.

**Fix.** File ▸ Save As… (and ⌘S on an unsaved document) now opens Tessera's own `SaveAsSheet` (DocumentSheets.swift,
presented from ContentView). Its controls:

- Name (`document.saveAs.name`, first responder when the sheet opens, text selected, Return saves)
- Format pop-up (`document.saveAs.format`: Tessera Document / Photoshop PSD / Large Document PSB; follows the name's extension)
- Where (`document.saveAs.folder`), with **Choose…** (`document.saveAs.choose`, an NSOpenPanel for folders)
- Cancel (`document.saveAs.cancel`) and Save (`document.saveAs.save`)

The sheet asks before replacing an existing file. The default folder is the document's own folder, then the last Save As
folder, then the open library folder, then ~/Documents.

Evidence: `evidence/v140-save-as-sheet.png` and `evidence/v140-save-as-ax.txt`. The AX dump shows the app's focused element
is `AXTextField document.saveAs.name` with the value `sample.tessera-doc`. I typed `Adjustments.tessera-doc` into the sheet,
pressed Return, and it saved. Test: `DocumentAdjustmentAnalysisTests.testSaveAsNames` covers the name and extension logic.
The §V identifiers appendix lists the new identifiers.

### Step 142: stale test count

The step now prints only the last `Executed` line, expects `with 0 failures` rather than an exact count, and names the 13
XCTest suites the filter runs. The count at B5-06 is 101.

## Adjustment editors (item 2)

- **Model** (TesseraCore/Document/DocumentAdjustments.swift, DocumentAdjustmentModels.swift): `AdjustmentModel` has a case for
  each of the 23 `compositor::Adjustment` variants, with the same field names, serde tags and serde defaults. Decoded numbers
  are rounded to f32, so the engine's shortest-decimal output and values the app computes decode to the same model.
  `Kind.allCases` follows Photoshop's order. `layerMenuSections` and `imageMenuSections` hold Photoshop's menu groups.
- **Round-trip proof.** One shared fixture, `crates/tessera-ffi/tests/fixtures/adjustments.json`, has 25 objects covering all
  23 variants.
  - Rust (`crates/tessera-ffi/tests/adjustment_json.rs`): serde reads and writes each object back identically, and each one
    validates. A `match` with no wildcard fails to compile when a new variant is added. `DocumentSession.layers()` emits
    exactly the fixture shape and names each layer with Photoshop's name.
  - Swift (`DocumentAdjustmentJSONTests`): one test per variant (23) adds each fixture object on a real engine document. It
    decodes the exact JSON `layers()` returns, checks it equals the fixture model, sends `model.json` back with
    `set_adjustment_json`, and asserts the engine's string is byte-identical. Two more tests check fixture coverage and
    re-encoding, and that every kind's neutral value round-trips.
- **Editors** (Tessera/Document/AdjustmentEditors.swift) follow the existing Properties layout. Sliders update live while
  dragging and add one history row on release.
  - Brightness/Contrast: sliders and Use Legacy.
  - Vibrance: two sliders.
  - Color Balance: Shadows / Midtones / Highlights picker, three sliders, Preserve Luminosity.
  - Black & White: six sliders, Auto, Default, Tint with a colour well.
  - Photo Filter: pop-up of the engine's 20 swatches plus Custom, colour well, Density, Preserve Luminosity.
  - Gradient Map: the fill layer's gradient-stops editor, now a shared `GradientStopsEditor`, plus a Method pop-up, Dither
    and Reverse.
  - Selective Color: Reds … Blacks pop-up, CMYK sliders, Relative / Absolute.
  - Desaturate: no settings.
  - Equalize: Analyze Again.
  - Auto: Tone / Contrast / Color, Clip %, read-only black / white / gamma, Analyze Again.
  - Match Color: source-layer pop-up, Luminance, Color Intensity, Fade, Neutralize.
  - Replace Color: colour well, Use Foreground (after picking with the Eyedropper), Fuzziness, Hue / Saturation / Lightness.
  - Color Lookup: Load 3D LUT… for .cube / .3dl, file-name row, Reset.
  - Shadows/Highlights: Amount / Tone / Radius for each range, then Color, Midtone and the Black / White clips (the pair is
    kept below 1).
  - HDR Toning: Method pop-up; Local Adaptation sliders and a toning curve; Exposure and Gamma; Equalize Histogram with
    Analyze; Highlight Compression.
- **Frozen parameters.** The engine exposes no constructor for them over FFI, and engine-api stays unchanged, so
  TesseraCore/Document/AdjustmentAnalysis.swift ports `auto_from_histogram`, `equalize_from_histogram`, the Lab statistics
  behind `match_color_from_pixels`, `HdrToning::equalize_from_histogram`, and the CUBE / 3DL loaders. The analysis reads the
  backend's 256 px thumbnails:
  - An adjustment layer analyses the composite below it. `DocumentController.compositeSamples(neutralizing:)` sets the layer
    to identity Levels on the interactive path for the read and restores it; no history row is added (tested).
  - Match Color's source is that layer's own thumbnail.
  - New Equalize, Auto and Match Color layers are analysed when they are added.
- **Menus.** Layer ▸ New ▸ Adjustment Layer and the Layers footer menu list Photoshop's four groups, then the kinds Photoshop
  offers only as commands (Shadows/Highlights, HDR Toning, Desaturate, Match Color, Replace Color, Equalize, Auto).
  Image ▸ Adjustments lists Photoshop's five groups. Invert, Desaturate and Equalize apply at once, measured on the layer.
  Match Color opens with another pixel layer as its source. Shortcuts:
  - Image ▸ Adjustments: Levels ⌘L, Hue/Saturation ⌘U, Color Balance ⌘B, Black & White ⌥⇧⌘B, Invert ⌘I, Desaturate ⇧⌘U.
  - Image menu, new commands: Auto Tone ⇧⌘L, Auto Contrast ⌥⇧⌘L, Auto Color ⇧⌘B. They are measured on the selected pixel
    layer and applied directly, with History rows named after the command.
  - Debug's grid-benchmark ⇧⌘B is now bound only outside document mode.
- **Stub backend.** StubCompositor renders these kinds as in adjust.rs: Brightness/Contrast, Color Balance, Black & White,
  Photo Filter, Gradient Map, Desaturate, Equalize, Auto, Replace Color and Color Lookup. Vibrance, Selective Color, Match
  Color, Shadows/Highlights and HDR Toning pass through unchanged on the stub.
- **FFI.** `adjustment_title` now names every variant ("Desaturate" and "HDR Toning" used to fall into the `_ =>
  "Adjustment"` wildcard, which is gone). That is the only change to src/document.rs.

On screen (engine, `sample.dng`), the evidence in `evidence/`:

- `adj-01-color-balance.png`: Cyan – Red drag, live, one `Color Balance` row.
- `adj-02-gradient-map.png`: Reverse and a stop-position drag, two rows. ⌘Z undid one.
- `adj-03-black-white.png`: Yellows drag, Auto, Tint.
- `adj-04-reopened.png`: saved through the new sheet as `Adjustments.tessera-doc`; after quitting, `--open-document` brought
  back all four layers and the same look.
- `adj-05-more-layers.png`, `adj-06-equalize.png`: Match Color, Selective Color, Photo Filter, Auto, Shadows/Highlights,
  HDR Toning and Equalize were added through the menu; none crashed.

ACCEPTANCE.md: steps 140, 142 and 144 are revised; §V Part 4 (steps 161–165) covers Color Balance, Gradient Map and
Black & White (add, drag, undo, save / reopen) and the other editors; the verdict now includes 161–165; the identifiers
appendix gains `document.saveAs.*` and the new `document.properties.<kind>.*` identifiers.

## Tests

- `swift test`: `Executed 217 tests, with 0 failures (0 unexpected)`, plus `✔ Test run with 5 tests in 2 suites passed`.
- `swift test --filter "Document|ThemeLint"`: `Executed 101 tests, with 0 failures`.
- `xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath ~/.cache/tessera-derived-data-B5-06 build`:
  `** BUILD SUCCEEDED **`.
- `Support/make-app.sh`: `Built …/apps/mac/build/Tessera.app`.
- `cargo test -p tessera-ffi --release`: every binary `ok`; the new `adjustment_json` binary reports `2 passed`.
- `cargo clippy --release -p tessera-ffi --all-targets -- -D warnings`: clean.
- `cargo fmt -p tessera-ffi -- --check`: clean.

## Deviations and notes

- **Color Lookup: no stored path, no dither.** The compositor's `ColorLookup` holds only `size` and `data`, and COMPOSITOR.md
  §4 says the samples are stored, not an external file name; there is also no dither field. The source file's path is kept
  for the session (`AdjustmentEditorState.lookupFile`) and shown in Properties; after a reopen the row reads `Embedded n³
  table`. There is no Dither checkbox. Adding either would need a compositor change, which is outside this package's paths.
  A 3DL's output scale is inferred as the smallest of 1, 1023, 4095 and 65535 that holds every value; the engine takes it
  as an argument.
- **Black & White Auto** is a documented native heuristic: a least-squares fit of the slider mix to the image's Rec. 601
  luma, pulled toward the defaults. It is not Adobe's algorithm.
- **Match Color's Neutralize** is not a serde field. It is stored as a zero source a*/b* mean and re-derived from the source
  layer when toggled.
- **Auto's clip** is not stored in the adjustment (the engine freezes only black / white / gamma). The Clip slider state
  lasts for the session and defaults to 0.1 %.
- **Save As is Tessera's sheet**, not the system panel. The system panel cannot be scripted because it runs out of process.
- **Shared-cache race.** During one `build-ffi.sh` run, uniffi-bindgen read a `libtessera_ffi.dylib` that a sibling agent
  (B5-07, layer styles) had just written to the shared `CARGO_TARGET_DIR`, so the generated bindings briefly contained its
  API. I re-ran `build-ffi.sh` until the bindings diff was empty and the archive had no `layer_styles` UniFFI symbols. The
  committed bindings are unchanged from main.
- **Visual defects seen but out of scope.** They were already present before this package (seen on the unmodified build): on a 1440 pt-wide window, the document-mode inspector content is wider than its column, so the right edge
  of Properties and Layers is clipped (Fill value, long hints). AppKit controls in the Properties scroll view draw under the
  transparent toolbar when scrolled; this is the macOS 26 scroll-under-toolbar behaviour, and I left it unchanged.
