# M2-51 report: AI Denoise and Lens Blur on, Guided uncorrected view, model downloads, export warnings

Branch `wp/M2-51`. Swift only. No Rust edits. `build-ffi.sh` left the generated bindings unchanged.

## Gate

`cd apps/mac && ./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing`, with
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-51`. Exit **0**. **260 tests, 0 failures.** ThemeLint is green.
Summary: `evidence/gate-summary.log`.

## What changed

| Brief item | Implementation |
| --- | --- |
| 1. AI Denoise | `DevelopEngineGaps.aiDenoise` is removed. Ticking the toggle acquires `enhance/cfa-unet-fp32` through `ModelAcquisition` (`ModelDownloads.open` + `request`). While that runs, an inline `ModelProgressRow` shows Queued, then bytes (determinate once the total is known), then Ready or Failed with the reason and a **Retry** button. The recipe is written (`AI Denoise On`) only once the model is Ready. The engine's automatic CFA backend then renders it, so the loupe refines. A recipe that already carries AI Denoise acquires the model on appear and calls `session.refresh()` when it is Ready. Off always works. |
| Settings ▸ AI | New `Settings/ModelDownloadsSettingsSection.swift`: a **Develop models** section with the **Allow model downloads** toggle (`ai-allow-model-downloads`, default on, persisted in UserDefaults `ModelDownloadsAllowed`). Changing it reopens the downloader and forgets earlier failures. With downloads off, a failure reason gets the suffix "(model downloads are off in Settings ▸ AI)". |
| 2. Lens Blur | The `lensBlur`, `lensBlurDepth` and `lensBlurSubject` gaps are removed. **Apply** acquires the depth weights first. A failure is an inline warning and leaves the recipe unchanged. **Blur Amount** is enabled. The **Bokeh** control is a `MenuPicker` with every distinct aperture the engine accepts. The **Focal Range** strip draws the 256-bin `depthHistogram()` behind the band. **Visualize Depth** calls `setRenderDepthVisualisation` and is session-only. It is switched off when you change photo or turn Apply off. **Subject** acquires depth, U2Net and the SAM encoder/decoder, calls `focusLensBlurOnSubject()`, then records the returned range as one history step `Focal Range: Subject N–M` and refreshes the histogram. The Refine brushes stay disabled; their reason is shown in a warning `StatusLine`. The view model is `TesseraCore/Develop/LensBlurDepth.swift` (`LensBlurDepthModel`). |
| 3. Guided Upright | `UprightGuideTool.begin()` calls `setRenderUncorrected(true)` through `UncorrectedPlacement`. `end()` restores it on the same session. If the session closed on a photo switch, it is forgotten without a call. While the tool is armed, the loupe guide mapping uses no crop, because the uncorrected render drops geometry, including crop. A panel hint says the loupe is showing the uncorrected photo. |
| 4. Constrain Crop | Disabled, with `DevelopEngineGaps.constrainCrop` as the reason (warning `StatusLine` plus help tag). It can still be switched off if a recipe already has it on. The old "loupe does not draw Upright/Transform" note now appears only if the session reports those settings as ignored. |
| 5. Export warnings | The FFI `ExportReport` has no warnings accessor. However, the engine writes `<output>.tessera-warnings.txt` beside each committed file (`export::BatchReport::warnings`). `TesseraCore/Export/ExportWarnings.swift` reads those files off the main actor after the run. The toast headline gains "; N with warnings" and the details list `name: warning` after any failures. An unreadable warning file is reported, never treated as "no warnings". |
| DESIGN / ACCEPTANCE | DESIGN.md has a new "Model acquisition, Lens Blur depth tools (M2-51)" paragraph. ACCEPTANCE section X steps 170–176 are updated in place, and its appendix is updated. New section **AC** covers steps 520–529, with a verdict and an identifier appendix. |

## Tests

New file `Tests/TesseraCoreTests/ModelDepthExportTests.swift`:
- `ModelAcquisitionTests` (6): event → state mapping, labels and fractions, combined states, queued → bytes → ready, failure with reason and Retry, the allow-downloads setting persisting and reopening the downloader, a missing manifest, the standard `<support>/models` paths.
- `LensBlurDepthModelTests` (4), using a stubbed session and downloader: depth histogram binding (normalised, 256 bins, cleared on photo switch), missing weights giving an inline error with no engine call, the Visualize Depth toggle (and turning it off on the old session), the Subject action (acquires the segmentation weights, applies the range once, handles missing weights and "no subject").
- `UncorrectedPlacementTests`: uncorrected view on Guided enter and exit, idempotence, session switch, abandon, failure reporting.
- `ExportWarningsTests`: reading the warning files and the toast lines.
- `LensBlurExportWarningTests`: runs the **real engine**. It applies Lens Blur to the ARW fixture with no depth weights and exports. Result: exported 1, with the warning `Lens Blur skipped: Lens Blur depth model is not cached; download depth/anything-v2-small in Models`, and the toast reads "; 1 with warnings".
- `TransformLensBlurTests` expectations updated (apertures and aliases, subject label, remaining gaps).

## Files outside the brief's allow-list

- `apps/mac/Sources/Tessera/Agent/AISettingsView.swift`: a one-line insertion, `ModelDownloadsSettingsSection()`. Settings ▸ AI is defined there, so this was unavoidable (the same pattern as `KeywordsCaptionsSettingsSection`).

`AppCommands.swift` and `AppModel*.swift` are untouched. There are no new Photo/ files.

## Limitations and deviations

- **Nine apertures.** The engine has **eight** distinct apertures: circle, bubble, 5-blade, hexagon, octagon, ring, cat-eye, oval (`crates/pipeline-cpu/src/lens_blur.rs`). The other accepted ids (disc, five-blade, pentagon, cat_eye, "cat eye", anamorphic) are aliases. The picker offers the eight, and stored aliases read back correctly.
- **CFA model acquisition.** The CFA entries in the catalog are local artifacts with paths relative to the source manifest. They do not resolve from the engine's copy at `<support>/models/models.toml`, which is what `ModelAcquisition` uses by default, so on a normal install AI Denoise shows "Failed: <reason>". `TESSERA_MODEL_MANIFEST=<checkout>/crates/ml-runtime/models.toml` points acquisition at the original manifest. Even then, the artifacts `tools/orchestrate/wp/M3-16/artifacts/cfa-*.onnx` are absent here. This needs packaging (FFI or engine work), not UI work.
- **Manifest timing.** The catalog copy is written when a develop session opens. Before that, acquisition fails with "No model catalog … (open a photo in Develop first)". The panels are only enabled with an open session, so users do not hit this.
- **Visualize Depth render failures.** These still reach the app's existing `render_failed` path (status message), not the panel. The panel avoids them by acquiring the weights first.
- **Histogram refresh.** The histogram is computed on photo open (when Lens Blur is applied), on Apply and on Subject. It is not recomputed on every tone edit.
- **Not verified with real model inference.** Depth, segmentation and CFA weights are not present on this machine and no network download was exercised. The download UI was tested against a stubbed engine downloader. The real-engine test covers only the missing-weights export warning path.
- **No manual UI evidence.** No screenshots were taken and ACCEPTANCE was not run by hand.
- The Refine brushes (no engine API) and Constrain Crop (not rendered) remain disabled.
