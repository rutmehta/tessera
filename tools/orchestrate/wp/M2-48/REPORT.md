# M2-48 report: Develop panels for AI Denoise, Transform/Upright and Lens Blur

Branch `wp/M2-48`. No Rust was edited. `build-ffi.sh` regenerated identical bindings, so none are committed.

## What was built
- **Detail ▸ AI Denoise** (`AIDenoiseSection` in `Inspector/LensBlurPanel.swift`, placed at the top of Noise Reduction).
  - Toggle and Amount map to `denoise.method = {kind: neural, model: enhance/cfa-unet-fp32@a138…, joint_demosaic: false}` and `denoise.amount`. Switching off removes the model members.
  - Both are **disabled**, with a warning StatusLine, because of an engine gap. The toggle stays enabled only to switch off an AI Denoise a recipe already has.
  - The classic NR sliders are unchanged.
- **Transform panel** (`Inspector/TransformPanel.swift`, between Detail and Effects).
  - Upright bar (Off/Auto/Guided/Level/Vertical/Full): the SegmentedPicker look with icons; the selected segment shows its name. Each segment has a help tag, an accessibility label and an identifier.
  - Each non-Guided mode is one history step and clears the guides.
  - **Guided** arms a loupe tool (`Loupe/LoupeUprightGuides.swift` plus hooks in `LoupeToolOverlay.swift`):
    - Draw up to 4 guides, drag their endpoints, or select one and press ⌫. Return, Esc or Done leaves the tool.
    - Each finished gesture is one history step.
    - The recipe is written as Guided only when there are 2–4 guides, because the engine rejects fewer. A stored Guided that drops below 2 guides becomes Off.
    - Guides are stored in sensor-normalised coordinates through `MaskSpace`, so crop and orientation are handled.
  - Manual sliders: Vertical, Horizontal, Rotate ±10°, Aspect, Scale 50–150 %, Offset X and Offset Y.
  - Constrain Crop, and a Reset per group.
  - A warning, driven by the engine's `ignoredSettings`, says the loupe does not draw Upright or Transform yet.
- **Lens Blur panel** (after Effects):
  - Apply, Blur Amount, and Bokeh limited to the engine ids circle, hexagon and octagon.
  - A Focal Range strip, drawn as a scope: drag a handle or the band; one commit on release.
  - Visualize Depth and Subject-aware toggles.
  - A Refine row with Focus and Blur brushes and a `Later` chip.
  - Everything is **disabled** with the reason shown (engine gaps below). Apply stays enabled only to remove a lens blur a recipe already has.
- **Core models**: `TesseraCore/Develop/TransformControls.swift` and `LensBlurDenoiseControls.swift`, with the gap reasons collected in `DevelopEngineGaps`.
- **`DevelopController`**: `ignoredSettings` is refreshed after each recorded commit, plus a new `ignores(prefix)` helper.
- **`ControlSlider` fix**: it now honours SwiftUI `.disabled`. Before this, disabled develop sliders stayed draggable, including HDR headroom.
- **Documentation**:
  - `ACCEPTANCE.md` section X: steps 161–167, a verdict and "Appendix: accessibility identifiers (M2-48)".
  - `DESIGN.md` §5 notes.
- **Screenshot aid**: `TESSERA_SELFTEST_UPRIGHT=guided`.

## Verified
- **Gate** (`./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing`) exited 0.
  - XCTest: **Executed 186 tests, with 0 failures**.
  - Swift Testing: 5 tests in 2 suites passed.
  - ThemeLint green.
- **New tests** (7, all passing) in `Tests/TesseraCoreTests/TransformLensBlurTests.swift`:
  - 5 pure JSON-patch/model tests.
  - `TransformSessionTests.testTransformDragsAndUprightButtonsAreOneHistoryStepEach` (real ARW session):
    - Coalesced drags of Vertical, Rotate and Scale give exactly one history entry each.
    - The Upright buttons and a two-guide Guided commit are one entry each, and the values reach `getSettingsJson` unchanged.
    - The Transform reset works.
    - `ignoredSettings` reports the gap.
  - `testUprightAutoChangesTheRenderedFrame` (real engine, fixtures/raw ARW, 640 px PNG export): Off vs Auto gives `640x426 -> 640x426, 43.4% of pixels changed`.
- **Screenshots** in `evidence/`:
  - Captured with `screencapture -l <windowid>` of the Tessera window only, from a make-app.sh build with `--app-dir` on a scratch copy of the ARW.
  - Downscaled to 1200 px, each under 1 MB.
  - Files: `transform-panel.png`, `transform-guided.png`, `detail-ai-denoise.png`, `lens-blur-panel.png`.

## Engine gaps
1. **The develop viewport does not render Upright, Transform or constrain_crop.** `tessera-ffi` `renderable_with` keeps only crop and straighten, so neither the loupe nor the thumbnails show the correction. Export renders it (the test proves this). Once the viewport renders it, the loupe must show the unwarped frame while guiding.
2. **AI Denoise cannot be rendered or exported.** The session needs `configureCfaDenoise` with a per-camera noise calibration the app lacks. Export injects no denoiser, so it would fail every export. There is no download or progress API for denoise weights, and the CFA models are local artifacts.
3. **Lens Blur needs depth inference.** `effects.lens_blur` fails the session and export renders. There is no depth plane in the FFI, so there is no histogram and no Visualize Depth. There is no subject-aware field in the schema and no refine-brush API. Only 3 bokeh shapes exist; Boost and cat-eye are runtime options that are not in the recipe.
4. The guide axis is inferred by the engine; the schema has no explicit axis.

## Not verified
- Hand interaction in the running app (guide drawing and dragging, ⌫/Esc/Return, Tab focus) was not exercised by a person or computer-use pass. It is covered at model and session level, and the guided screenshot comes from the screenshot aid.
- The light appearance was not captured.
- ACCEPTANCE steps 161–167 have not been run by a verifier.
- The Upright export test takes about 70–100 s.
- Launching the app for screenshots added scratch folders to `dev.tessera.app` RecentFolderPaths/LastFolderPath. These were restored to the previous single `shoot` entry.
