# B5-v — Layered editor (document mode) on screen

Setup (never edit source files; capture only the Tessera window with the computer_use screenshot tool after every step; keyboard-first; identifiers are in the "Appendix: accessibility identifiers" sections near the end of apps/mac/ACCEPTANCE.md):
1. Build: `export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/B5-v; (cd apps/mac && ./build-ffi.sh && swift build && Support/make-app.sh release)`; last line `Built …/apps/mac/build/Tessera.app`.
2. `SCR="$(mktemp -d)"; export TESSERA_APP_DIR="$SCR/appdir"; swift apps/mac/Support/make-sample-folder.swift "$SCR/shoot" 12; cp -RL fixtures/raw "$SCR/raw"; defaults delete dev.tessera.app 2>/dev/null; true`.
Then perform every numbered step of sections V and W below exactly as written; report pass/fail per step with what you saw; list visual defects (overlap, spacing, off-theme colours, truncation, blank areas) separately; when a step needs terminal output, redirect it to a file and `cat` it.

## V. Document mode: layered documents (B5-02)

**Layers** (the fourth view-mode segment) is Tessera's layered editor: a viewport, and Properties, Layers and History
panels in the inspector. Since B5-03 documents run on the engine's `DocumentSession` (`EngineDocumentBackend`): the
engine of the open folder, or a standalone engine in the app-support directory when no folder is open. With
`--stub-library` they run on the **stub backend** (`StubDocumentBackend`): a new document opens with six sample layers
(Paper, Landscape with a soft elliptical mask, Vignette clipped to it, and a Grade group holding Curves 1 and
Hue/Saturation 1), rendered on the CPU. Part 1 runs on the stub; part 2 repeats the key steps on the real engine.
`--new-document` (test aid) creates a document at launch; `--open-document <file>` opens one.

### Part 1: over the stub backend

130. Quit Tessera. Build and launch:
     ```sh
     (cd apps/mac && swift build && Support/make-app.sh)
     open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-doc" --stub 200 --stub-library
     ```
     Choose **File ▸ New Document…** (⌘N). 📸 Expect a sheet `New Document` with Preset, Width `2400`, Height `1600`,
     Bit depth `8-bit | 16-bit | 32-bit float`, Colour profile `sRGB IEC61966-2.1`, the footer note `Stub backend: the
     document opens with sample layers` and an accent **Create** (`document.new.create`). Click **Create**.
131. 📸 Expect: the toolbar's view control on **Layers**, a tab `Untitled-1` (`document.tabs.0`) in the toolbar, the window
     subtitle `6 layers`; the viewport (`document.viewport`) shows a dusk landscape inside an ellipse on warm paper, a
     checkerboard in the transparent margin around the paper, and the canvas surround beyond it; the zoom chip
     (`document.zoomHUD`) briefly shows the fit zoom. The inspector shows **Properties** (`Grade`, `Group · Pass
     Through`), **Layers** and **History** (`Opened`, highlighted; `0 states · Zero KB`). The Layers outline lists, top to
     bottom: `Grade` (folder glyph, expanded) with `Hue/Saturation 1` and `Curves 1` indented, `Vignette` with the clipping
     glyph and a drop glyph, `Landscape` with a link glyph and a white ellipse mask thumbnail, `Paper` with a lock glyph.
     The status bar reads `2400 × 1600 px · 8-bit · sRGB IEC61966-2.1 | <zoom> | Move (V)`.
132. **Zoom and pan.** Press ⌘1: the chip reads `100 %` and the landscape fills the view at full size. ⌘= (View ▸ Zoom In) twice →
     `300 %` (pixels turn crisp: nearest-neighbour from 200 %), ⌘− → `200 %`, ⌘0 → the fit zoom. Hold ⌥ and drag right in
     the viewport: the zoom grows around the point you pressed (scrubby zoom). Pinch on a trackpad: zooms about the
     pointer. Two-finger scroll pans; hold Space and drag: the cursor is a closed hand and the canvas follows.
     Expect the image to stay sharp after each gesture settles and the status bar zoom to match the chip.
133. **Layers and live sliders.** Click `Landscape`. Properties shows `Pixel`, Bounds `96, 96 · 2208 × 1408 px` and Mask
     `On · linked`. Drag **Opacity** (`document.layers.opacity`) to about 40 %: the landscape fades live while dragging;
     on release History gains one row `Opacity 40 %` (`document.history.row.1`) and the tab shows the dirty dot. Choose
     **Multiply** from the blend pop-up (`document.layers.blendMode`: modes in six groups with dividers): History gains
     `Blend Mode`. Press ⌘Z twice: opacity and mode return; ⇧⌘Z once re-applies the opacity. Click `Opened` in History:
     the document returns to how it opened.
134. **Adjustments.** Click `Curves 1`: Properties shows a channel control (RGB / Red / Green / Blue) and the curve editor
     (`document.properties.curves.editor`) with an S-curve. Drag the upper point up: the picture brightens live, one
     `Curves` history row on release. Click **Layers ▸ New ▸ Adjustment Layer ▸ Levels**: `Levels 1` appears above the
     selected layer and is selected; drag **Gamma** (`document.properties.levels.gamma`) to 2.00: midtones lift live.
     Switch the channel to **Blue** and drag Output white down: the image turns yellow. Add **Hue/Saturation** from the
     footer's adjustment menu (`document.layers.addAdjustment`), tick **Colorize**: the image becomes monochrome in one hue.
     Repeat quickly for Exposure, Posterize (4 levels = visible banding), Threshold (black and white), Channel Mixer
     (Monochrome) and Invert (`Invert has no settings.`).
135. **Fills.** **Layer ▸ New ▸ Fill Layer ▸ Gradient**: a black-to-white gradient covers the canvas; Properties shows
     Linear / Radial, a gradient preview, two stops with colour wells and positions, **Add Stop** and **Reverse**. Click
     **Reverse**: white-to-black. Set the layer's blend mode to **Soft Light**. **Layer ▸ New ▸ Fill Layer ▸ Solid Color**,
     pick a colour in its well (`document.properties.fill.color`): the canvas takes that colour; one `Solid Color` history
     row a moment after you stop picking. Delete it with ⌫ (the Layers outline focused) or the trash button.
136. **Structure.** Select `Vignette` and `Landscape` (⌘-click) and press ⌘G: one history row `Group Layers`, a new
     `Group 1` holds both. ⇧⌘G: they return. ⌘J on `Paper`: `Paper copy` above it. ⌘E (Merge Down) on `Paper copy`: it
     merges into `Paper`. Drag `Curves 1` out of `Grade` to the top of the list: one `Move Layer` history row, the row
     animates to its new place; drag it back into `Grade`. Double-click a layer name, type `Sky`, Return: renamed. Click an
     eye: the layer hides (name dimmed); ⌥-click an eye: only that layer shows. Right-click a row: the menu mirrors the
     Layer menu (Rename, Duplicate, Delete, Group, Ungroup, Merge Down, Flatten, clipping, mask items). Lock buttons
     (`document.layers.lock.*`) toggle the lock glyph on the row.
137. **Selection and masks.** Press **M**, drag a rectangle over the sun: marching ants run around it and the status bar
     shows `Selection W × H`. Select `Vignette` and **Layer ▸ Layer Mask ▸ From Selection**: its mask thumbnail appears;
     the vignette now shows only inside the rectangle. ⇧-click that mask thumbnail: a red cross, the mask is off. ⌘D:
     the ants disappear. **V** returns to Move (clicking the canvas with Move explains that moving pixels arrives later).
138. **Panels and screen modes.** Press **Tab**: sidebar and inspector hide; Tab again restores them. Press **F**: the
     window goes full screen; F: panels hide too; F: back to standard. Culling keys do nothing here: press **X**, **P**,
     **1**, **G**, **E** and the arrows — the document, the mode and the library selection stay as they are.
139. **History and snapshots.** Click **New Snapshot…** (`document.history.newSnapshot`), keep `Snapshot 1`, Save: it is
     listed under Snapshots. Make two edits, click **Restore** on the snapshot: the document returns to it and History
     gains `Snapshot “Snapshot 1”` (undoable). The memory line reads `<n> states · <size>`.
140. **Save, reopen, close.** ⌘S on the new document opens Save As; save `Poster.tessera-doc` into `$SCR`. The tab title
     becomes `Poster.tessera-doc`, the dirty dot goes. Choose **Save As…** with a `.psd` name: the status bar reads
     `Save As: Saving as PSD / PSB needs the engine (B5-03); save as .tessera-doc`. **File ▸ Export Flat…** (⇧⌘E): format
     PNG / JPEG / TIFF, quality for JPEG, colour space; export `Poster.png` into `$SCR` and check it opens in Preview with
     transparent margins (JPEG: white). Make one edit and press ⌘W: `Do you want to save the changes made to
     “Poster.tessera-doc”?` with Save…, Cancel, Don’t Save; choose Don’t Save: the tab closes and the viewport shows the
     `No document` empty state with New Document… and Open Document…. **File ▸ Open Document…** (⇧⌘O) `Poster.tessera-doc`:
     the saved layers come back. In Finder, choose Open With ▸ Tessera on `Poster.tessera-doc` (with the app running):
     it becomes the current tab (a document already open is not opened twice).
141. **Several documents and Edit in Layers.** **File ▸ Open Folder…** `$SCR/shoot` (step 2; stub items have no files to
     edit), select a photo in the grid, press ⌘E (**Library ▸ Edit in Layers**): a new tab named after the photo with one pixel layer (on the stub, JPEGs open as decoded; RAW files use
     macOS's own rendering). Switch between tabs: each keeps its zoom and position. Open `Poster.png` (Open Document…):
     one pixel layer named `Poster`.
142. **Tests.**
     ```sh
     (cd apps/mac && swift test --filter "Document|ThemeLint" 2>&1 | grep "Executed")
     ```
     Expect `Executed 41 tests, with 0 failures` (32 from B5-02, 9 engine-adapter tests from B5-03).

### Part 2: over the real engine (B5-03)

Work on a copy of the fixture: `mkdir -p "$SCR/shoot" && cp fixtures/raw/sample.dng "$SCR/shoot/"` (the app writes
sidecars next to photos; never point it at `fixtures/raw`). Turn on **Debug ▸ Show Render Timing** for the readout.

143. **Blank document.** Launch without `--stub-library`:
     `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-doc" --folder "$SCR/shoot"`. ⌘N, **Create**: the
     sheet has no stub footer; one transparent pixel layer `Layer 1` (checkerboard over the whole canvas), selected,
     Properties `Pixel`, History `Opened` only, subtitle `1 layer`, status bar without `(stub backend: sample layers)`.
144. **Edit in Layers.** In the grid select `sample.dng`, press ⌘E. The status bar reads `Edit sample.dng in Layers…`
     while the engine develops it (about 1.5–3 s), then a tab `sample` with one pixel layer `sample`, Properties
     `Bounds 0, 0 · 5212 × 3468 px`, status bar `5,212 × 3,468 px · 16-bit · sRGB IEC61966-2.1` and
     `render: L1 <w> × <h>, <ms>` (📸 `evidence/engine-01-edit-in-layers.png`).
145. **Adjustment layer.** Footer adjustment menu ▸ **Exposure**: a layer `Exposure 1` (engine names are numbered per
     kind, as in Photoshop: `Levels 1`, `Hue/Saturation 1`, `Color Fill 1`) above `sample`, History `New Layer Exposure 1`.
     Drag Exposure to +1.00: the photo brightens live; one `Exposure` row on release (📸 `engine-02-adjustment.png`).
146. **Opacity drag.** Select `sample`, drag **Opacity** to 40 %: the photo fades over the checkerboard live and the
     readout stays in single-digit milliseconds (budget < 16 ms at the viewport level); on release one row `Opacity 40 %`
     (📸 `engine-03-opacity-drag.png`). The layer thumbnail does not re-render during the drag (opacity is not part of it).
147. **Undo.** ⌘Z: opacity returns to 100 % (📸 `engine-04-undo.png`); ⇧⌘Z re-applies it. Click `Opened`: the document
     returns to one layer. **M** and a marquee drag: the ants hug the dragged rectangle exactly (bounds are pixel-exact),
     `Selection W × H` in the status bar, and History gains `Rectangular Marquee`; ⌘D adds `Deselect`.
148. **Save and reopen.** ⌘S → Save As `SelfTest.tessera-doc`: the dirty dot goes (📸 `engine-05-saved.png`). ⌘W, then
     **Open Document…** it: the same layers and the Exposure settings come back (📸 `engine-06-reopened.png`).
149. **Export Flat** (⇧⌘E) PNG, sRGB: a 5212 × 3468 PNG (📸 `engine-07-exported.png`). **Save As…** `SelfTest.psd`: the
     save succeeds (the engine writes PSD/PSB); close and open the PSD: `Exposure 1` (an adjustment layer) over `sample`,
     History `Opened` (📸 `engine-08-psd-open.png`). A document with fill layers cannot be saved as PSD yet (the status
     bar shows the engine's message).
150. **Scripted run.** The same flow through the controller calls the UI makes, with step markers and the frame timing
     of the listener, then quit:
     ```sh
     apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" --app-dir "$SCR/appdir-doc" \
       --document-selftest "$SCR/doc-out" 2>&1 | grep document-selftest
     ```
     Expect every `check … ok`, `opacity drag: frames 61, render median <16 ms`, and `done, 0 failure(s)`.
     `TESSERA_DOC_FRAME_LOG=1` prints every frame (`doc-frame: epoch … L1 … render … ms`) during manual drags.

### Part 3: filters, Image ▸ Adjustments and smart filters (B5-05)

Same scratch copy as part 2 (`$SCR/shoot/sample.dng`). In the grid select `sample.dng`, ⌘E (Edit in Layers).

151. **Filter menu.** The menu bar has **Image** and **Filter** in document mode. Filter lists **Last Filter** (⌃F,
     disabled until a filter was applied), **Convert for Smart Filters**, then Blur, Sharpen, Noise, Distort, Stylize,
     Render and Other, built from the engine's `list_filters()` (Gaussian Blur…, Box Blur…, Motion Blur…, Radial Blur…,
     Surface Blur…, Unsharp Mask…, Smart Sharpen…, Add Noise…, Reduce Noise…, Median…, Dust & Scratches…, Pinch…,
     Spherize…, Twirl…, Wave…, Ripple…, Polar Coordinates…, Emboss…, Find Edges, Solarize…, Clouds…, Difference
     Clouds…, High Pass…, Offset…). With an adjustment layer selected the groups are disabled.
152. **Gaussian Blur preview.** Filter ▸ Blur ▸ **Gaussian Blur…**: a dialog `Gaussian Blur`, `Layer “sample”`, a 1:1
     detail pane (drag it to move) and **Radius**. Drag Radius: the canvas blurs live at the viewport level and the
     detail pane follows; History does not change and the document stays as it was. Untick **Preview**: the canvas shows
     the original, the pane still shows the filter. **Reset** returns Radius to 2.0 px
     (📸 `tools/orchestrate/wp/B5-05/evidence/filters-01-gaussian-dialog.png`).
153. **Apply and undo.** Radius 12, **OK**: the status bar reads `Applying Gaussian Blur…`, then
     `Gaussian Blur applied (<s> s)`; History gains one row `Gaussian Blur` (📸 `filters-02-gaussian-applied.png`).
     ⌘Z restores the sharp photo (📸 `filters-03-gaussian-undone.png`). ⌃F applies Gaussian Blur 12 px again without a
     dialog; ⌘Z. With a marquee selection only the selection is filtered.
154. **Levels via Image ▸ Adjustments.** Image ▸ Adjustments ▸ **Levels…** (⌘L): the Levels editor of the Properties
     panel in a sheet; moving Input black / Gamma previews live on the canvas (📸 `filters-04-levels-dialog.png`).
     **OK**: one History row `Levels`, the pixels of `sample` change, no adjustment layer is added
     (📸 `filters-05-levels-applied.png`). Invert (⌘I) applies at once.
155. **Smart filter.** Filter ▸ **Convert for Smart Filters**: `sample` becomes a smart object (Properties `Smart
     Object`, History `Convert to Smart Object`). Filter ▸ Blur ▸ Gaussian Blur…, Radius 10, OK: History `Gaussian Blur`,
     and a row **Gaussian Blur** appears under `sample` with an eye, a white mask thumbnail and a blending-options button
     (📸 `filters-06-smart-filter-on.png`). Click its eye: the photo is sharp again, History `Disable Smart Filter`
     (📸 `filters-07-smart-filter-off.png`); click again: blurred, `Enable Smart Filter`
     (📸 `filters-08-smart-filter-on-again.png`). Double-click the row: the dialog reopens with Radius 10 and previews the
     re-edit; OK records `Edit Smart Filter`. The blending-options button sets mode and opacity (`Smart Filter Blending
     Options`). Save As `.tessera-doc`, close, reopen: the smart filter row and the blurred look come back; Export Flat
     bakes the smart filter at full resolution.
156. **Scripted run.**
     ```sh
     apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" --app-dir "$SCR/appdir-filters" \
       --filter-selftest "$SCR/filter-out" 2>&1 | grep filter-selftest
     ```
     Expect every `check … ok`, `gaussian preview latency (value → frame, viewport 5212 × 3468 at L1): n 12, median …`
     and `done, 0 failure(s)`.

## Verdict (document mode)

PASS when steps 130–142 (stub), 143–150 (engine) and 151–156 (filters) meet their expectations. Record the stub render time on a
large window (drag Opacity on `Landscape` at 100 %) as an observation; the stub renders on the CPU and is not held to the
engine's budget.

## W. Layered editor tools: painting, selections, transform (B5-04)

Engine backend (not `--stub-library`), a scratch copy of `fixtures/raw/sample.dng` in `$SCR/shoot`, opened with
Library ▸ Edit in Layers (⌘E). The tools palette sits on the left of the canvas, the options bar across the top.

151. **Palette and keys.** V M L W B E S J G C T I H Z select Move, Rectangular Marquee, Lasso, Quick Selection, Brush,
     Eraser, Clone Stamp, Healing Brush, Gradient (placeholder), Crop (placeholder), Type (placeholder), Eyedropper,
     Hand, Zoom; ⇧M / ⇧L / ⇧W cycle Elliptical Marquee, Polygonal / Magnetic Lasso, Magic Wand / Object Selection;
     right-click a palette slot lists its group. The options bar follows the tool. X swaps and D resets the swatches.
152. **Brush stroke visible and undoable.** B, foreground red (click the foreground swatch), Size 80. Drag across the
     photo: the stroke follows the pointer while dragging, the outline circle shows the brush size, and History gains
     one `Brush Tool` row (📸 `tools-2-brush-stroke.png`). ⌘Z removes it (📸 `tools-3-brush-undo.png`), ⇧⌘Z restores
     it. With Debug ▸ Show Render Timing the status bar shows `Brush Tool: N frames, median … ms` and the render readout.
     [ ] resize the brush, ⇧[ ⇧] change hardness, 1–0 set opacity (4 then 5 = 45 %); ⌃-drag (or ⌥-right-drag) shows the
     HUD: right = larger, up = harder. With a tablet, pressure narrows the stroke (Size pressure is on by default).
     ⇧-click draws a straight line from the last stroke. Symmetry ▸ Vertical mirrors about the centre with a guide.
153. **Eraser to transparency.** E, Size 200, drag over the photo: the checkerboard shows through; one `Eraser` row
     (📸 `tools-4-eraser.png`).
154. **Clone and heal.** S, ⌥-click a source, paint elsewhere: the source crosshair follows the brush and the pixels are
     copied (aligned); one `Clone Stamp` row. J does the same, blended into the surroundings (`Healing Brush`).
155. **Wand selection outline.** ⇧W until Magic Wand, Tolerance 24, click the wall: marching ants follow the region's
     outline (not its bounding box) and the status bar shows `Selection W × H`; one `Magic Wand` row
     (📸 `tools-5-wand.png`). ⇧-click adds, ⌥-click subtracts, ⇧⌥ intersects (also the four options-bar buttons).
     Marquee, ellipse, lasso, polygonal (click points, double-click or Return closes, Esc cancels) and magnetic lasso
     (edge-snapping path while moving) combine the same way; ⌫ clears the selected pixels of a pixel layer.
156. **Subject selection.** Select ▸ Subject (first run loads the on-device model): the subject's outline appears; one
     `Select Subject` row (📸 `tools-6-subject.png`). Select ▸ Sky, Color Range…, Inverse (⇧⌘I), Modify ▸ Expand… /
     Contract… / Border… / Smooth… / Feather…, Select and Mask… (⌥⌘R: Overlay / On Black / On White preview, sliders
     update live, OK records one `Refine Edge` row, Cancel restores), Save Selection… / Load Selection ▸ work.
157. **Transform commit.** Select the photo layer, ⌘T: a box with eight handles and the reference point. Drag a corner
     (⇧ keeps proportions, ⌥ scales about the centre), drag outside the box to rotate (⇧ snaps 15°), drag inside to move;
     the options bar shows X, Y, W %, H %, angle and skew and accepts typed values (📸 `tools-7-transform-preview.png`).
     Return commits one `Free Transform` row (📸 `tools-8-transform-commit.png`), Esc cancels. The Move tool (V) drag
     moves the layer (one `Free Transform` row); Edit ▸ Transform ▸ Flip / Rotate apply directly.
158. **PSD save and reopen.** File ▸ Save As… `ToolsSelfTest.psd`, close, File ▸ Open Document… the PSD: the same
     layers, with the painted, erased and transformed pixels (📸 `tools-9-psd-reopen.png`).
159. **Brushes and colour panels.** With a painting tool the inspector shows Color (foreground / background wells) and
     Brushes (Hard / Medium / Soft Round, Chalk, Square and imported tips with previews, Import Brushes… for `.abr`,
     size / hardness / spacing / angle / roundness). The options bar's brush button opens the same preset list.
160. **Scripted run.**
     ```sh
     apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" --app-dir "$SCR/appdir" \
       --tools-selftest "$SCR/out" 2>&1 | grep tools-selftest
     ```
     Expect every `check … ok`, `brush stroke: … frame render … median <16 ms`, and `done, 0 failure(s)`.

