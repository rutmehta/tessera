# B5-09v — Remove tool, Content-Aware Fill, Remove Distractions, Neural Filters (with real models)

Setup (never edit source files; capture only the Tessera window with the computer_use screenshot tool after every step; keyboard-first; identifiers are in the accessibility-identifier appendices near the end of apps/mac/ACCEPTANCE.md):
1. Build: `export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/B5-09v; (cd apps/mac && ./build-ffi.sh && swift build && Support/make-app.sh release)`; last line `Built …/apps/mac/build/Tessera.app`.
2. `SCR="$(mktemp -d)"; export TESSERA_APP_DIR="$SCR/appdir"; swift apps/mac/Support/make-sample-folder.swift "$SCR/shoot" 12; cp -RL fixtures/raw "$SCR/raw"; defaults delete dev.tessera.app 2>/dev/null; true`.
This Mac HAS the LaMa, DDColor and DRUNet model weights cached (earlier engine tests downloaded them), so where a step expects a missing-model error, instead report whether the real model ran and what the result looked like (object removed cleanly? colours plausible? artifacts reduced?), with timing if shown.
Perform every numbered step of section Z below exactly as written; report pass/fail per step with what you saw; list visual defects separately; when a step needs terminal output, redirect it to a file and `cat` it.

## Z. Remove tool, Content-Aware Fill and neural filters (B5-09)

Engine backend (not `--stub-library`), a scratch copy of `fixtures/raw/sample.dng` in `$SCR/shoot`, opened with
Library ▸ Edit in Layers (⌘E), the photo layer selected. Model weights are never downloaded by these steps: with no
weights installed (the default), expect the missing-model messages below; where a model is installed by hand in
`<app support>/models/cache/<sha256>.onnx` (Settings do not offer a download), expect the result instead.

320. **Remove tool.** Press ⇧J (or click the palette slot under the Healing Brush, icon `eraser.line.dashed`): the slot
     turns amber, the options bar reads `Remove` with Size, the Auto · PatchMatch · LaMa picker, Expand, Remove
     Selection and Remove Distractions…; the status bar reads `Remove (⇧J)`. Without LaMa installed the bar shows
     `LaMa not installed` (its help names `remove/lama`, the Hugging Face URL and the cache path). [ ] change the size.
321. **Remove an object by a stroke.** Size about 1/40 of the long edge, Auto. Drag over a small object: a translucent
     red band at brush width follows the pointer (📸 `retouch-01-remove-stroke.png`). On release the options bar shows a
     spinner, the elapsed seconds and Cancel; then the object is filled from its surroundings, History gains exactly one
     `Remove` row and the status bar reads `Remove: PatchMatch, N s (LaMa (remove/lama) is not installed, so Auto used
     PatchMatch)` (📸 `retouch-02-remove-applied.png`). With LaMa installed it reads `Remove: LaMa, …`.
322. **Undo, redo, reopen.** ⌘Z restores the object (one step; 📸 `retouch-03-remove-undone.png`), ⇧⌘Z removes it
     again. File ▸ Save As… a `.tessera-doc`, close it, open it again: the removal is there (📸 `retouch-04-reopened.png`).
323. **Remove with LaMa, weights missing.** Choose LaMa and stroke again: nothing changes, no History row, and the options
     bar shows the warning `Remove needs a model that is not installed`; its help (and the status bar) name `remove/lama`,
     `https://huggingface.co/Carve/LaMa-ONNX/…` and the expected cache file. Nothing is downloaded.
324. **Remove Selection.** Make a marquee around an object, click Remove Selection: one `Remove` row; outside the
     selection nothing changes.
325. **Edit ▸ Content-Aware Fill.** Make a small marquee, Edit ▸ Content-Aware Fill (disabled without a selection or on
     an adjustment layer): the selection is filled from its surroundings; one `Content-Aware Fill` row
     (📸 `retouch-05-content-aware-fill.png`).
326. **Start and cancel a slow job.** PatchMatch, marquee most of the image, Remove Selection: the options bar shows the
     spinner, a counting `Removing… N s` and Cancel (📸 `retouch-06-slow-job-running.png`). Click Cancel (or press Esc):
     the bar returns to the options, the status bar reads `Remove cancelled`, History and the pixels are unchanged
     (📸 `retouch-07-slow-job-cancelled.png`).
327. **Remove Distractions: review first.** Deselect (⌘D), click Remove Distractions…: boxes appear over the canvas, one per
     suggestion, with chips `Wire-like line` / `Face box`; the bar reads `N of N suggestions selected`, All, None, the note
     `geometric suggestions, not person segmentation`, Cancel and Remove Selected.
     No History row yet. Click a box: it turns dashed and its chip reads `(kept)`. Remove Selected (or Return) removes only
     the boxes still selected: one `Remove Distractions` row. Esc cancels the review. Without the face detector weights
     (`opencv/yunet`) no face boxes are suggested, and `sample.dng` has no thin straight wire, so there the bar reads
     `Nothing found` and the status bar names the missing face model (📸 `retouch-08-distractions-review.png`).
328. **Neural Filters: Colorize.** Filter ▸ Neural Filters…: a sheet lists Skin Smoothing, Colorize and JPEG Artifact
     Removal (the last two with a `No model` chip when their weights are missing). Choose Colorize: its
     Saturation / Artifact Reduction sliders, the Output picker, the note `CPU-only DDColor; …`, and the warning
     `Colorize needs the filters/ddcolor model, which is not installed.` with the Hugging Face URL and cache path; Apply is
     disabled (📸 `retouch-09-neural-colorize.png`). With the model installed: Apply colorizes as one `Colorize` row.
329. **Neural Filters: JPEG Artifact Removal.** Same, naming `enhance/drunet-color` (📸 `retouch-10-neural-jpegArtifactRemoval.png`).
330. **Neural Filters: Skin Smoothing.** Needs no weights. Without a selection and without the face detector the sheet says
     `The face detector (opencv/yunet) is not installed: select a face first …` (📸 `retouch-11-neural-skin-no-faces.png`);
     Apply then fails with `Skin Smoothing needs face boxes …` and records nothing. Cancel, make an elliptical marquee
     around a face, reopen, Output **New layer**, Apply: a new layer `<layer> (Skin Smoothing)` above the photo, one
     `Skin Smoothing` row (📸 `retouch-12-neural-skin-new-layer.png`).
331. **Outputs.** On a pixel layer Output offers Current layer, New layer and Smart filter (Smart filter only without a
     selection; the sheet lists why an output is unavailable). **Smart filter** converts the layer into a smart object
     with the filter in the same single History row; the filter appears under the layer in the Layers panel.
     Double-click that row: the Neural Filters sheet reopens on the same filter with its values (Output fixed to Smart
     filter); OK records `Edit Smart Filter`. On a smart object, New layer is unavailable.
332. **Outline after deselect.** Make a large wand selection and press ⌘D immediately: the marching ants disappear and do
     not come back when the outline computation finishes.
333. **Scripted run.**
     ```sh
     open -n --stderr "$SCR/retouch.log" apps/mac/build/Tessera.app --args --folder "$SCR/shoot" \
       --app-dir "$SCR/appdir" --front --retouch-selftest="$SCR/out"; grep retouch-selftest "$SCR/retouch.log" 2>&1 | grep retouch-selftest
     ```
     Expect the step lines above, every `check … ok`, the measured `remove stroke: backend … engine … ms` and
     `done, 0 failure(s)`.
