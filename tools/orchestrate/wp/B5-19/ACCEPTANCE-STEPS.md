## B5-19. Auto-Align Layers, Auto-Blend Layers and Photomerge into layers

Engine backend (not `--stub-library`). Make two or three overlapping photos: export `fixtures/raw/sample.dng` as
PNG and crop it into `left.png` (left 60 %) and `right.png` (right 60 %), e.g.
`sips -s format png sample.dng --out photo.png` then `sips -c <h> <w> --cropOffset 0 <x> photo.png --out left.png`.
Import the folder so the crops are library photos as well as files.

440. **Menus.** In document mode with one layer selected, Edit ▸ Auto-Align Layers… and Auto-Blend Layers… are
     disabled. File ▸ Automate ▸ Photomerge… is enabled in the library and in document mode.
441. **Photomerge from the library.** Select `left.png` and `right.png` in the grid, File ▸ Automate ▸ Photomerge…:
     the sheet lists both under Source Files, Layout radio (Auto, Perspective, Cylindrical, Spherical, Collage;
     no Reposition), Blend Images Together (on, disabled), Seamless Tones and Colors, Content-Aware Fill Transparent
     Areas, Vignette Removal and Geometric Distortion Correction (disabled, with the lens-calibration note).
442. **Sources.** Add Files… offers JPEG / PNG / TIFF only and adds a file (duplicates are dropped), the minus
     button removes one; with one source OK is disabled and the sheet says "Choose two or more photos to merge".
     Photos totalling more than 200 megapixels are refused before anything is read: the status bar says
     "Photomerge: … limited to 200 megapixels in total; these have N megapixels…".
443. **Busy / Cancel.** OK (Layout Auto): the busy sheet shows an indeterminate spinner, "Photomerge…", the note
     that reading can be cancelled but aligning / blending cannot, and a Cancel button (`stack-busy-cancel`). The
     status bar reads "Photomerge…". With several large photos, Cancel while reading: the button reads
     "Cancelling…", the sheet closes, the status bar reads "Photomerge was cancelled", and no tab / history row is
     added. Auto-Align / Auto-Blend busy sheets have no Cancel.
443a. **Colour.** Photomerge two Display P3 photos (e.g. iPhone JPEGs) into a new document: the document's colour
     profile is Display P3 (not sRGB) and colours match the originals. Into an open sRGB document the
     photos are converted (no oversaturated / washed-out layers).
444. **Result.** A new tab `Untitled` opens with one layer per photo, named after each photo, each with a layer mask;
     the canvas is the panorama's size. History lists exactly one row, `Photomerge`, after the opened state.
445. **Undo / redo.** ⌘Z returns to the empty opened state in one step; ⇧⌘Z restores the merged layers.
446. **Editable sources.** Each merged layer is a smart object holding the original photo; its mask is editable with
     the brush like any mask (paint black to hide, undo restores).
447. **Photomerge into the current document.** With a document open, Photomerge… offers "Add to the current
     document"; with it on, OK adds the photos as layers of that document (one `Photomerge` History row), growing its
     canvas; other layers keep their place.
448. **Content-Aware Fill.** Photomerge again with Layout Collage and Content-Aware Fill Transparent Areas on: one
     extra `Content-aware fill` layer covers only the transparent corners; turning its eye off shows them again.
449. **Auto-Align selection rules.** In a document with three pixel layers holding the crops at the same position,
     select two: Edit ▸ Auto-Align Layers… is enabled. Group one of them, lock another's position, or select a fill
     layer: the item is disabled (choosing it via the Layers context shows the reason in the status bar, e.g.
     "Auto-Align works on top-level layers; “right” is inside a group" / "“right” is locked").
450. **Auto-Align sheet.** Projection radio with a line of help per choice, Reference pop-up listing the selected
     layers bottom first (the bottom layer by default), lens toggles disabled with the note.
451. **Align.** OK with Collage: the busy sheet, then the layers overlap at the right place (the seam lines up), the
     canvas grows to fit both; History `Auto-Align Layers`. Hiding the reference shows the other layer offset by the
     crop offset.
452. **Undo align.** ⌘Z restores the original canvas size and both layers at the origin in one step.
453. **Auto-Blend Panorama.** With both aligned layers selected, Edit ▸ Auto-Blend Layers…: Blend Method Panorama /
     Stack Images (with help), Seamless Tones and Colors, Content-Aware Fill Transparent Areas. OK (Panorama): each
     layer gets a mask; in the overlap the masks are complementary (no visible seam); History `Auto-Blend Layers`;
     one ⌘Z removes all masks.
454. **Stack Images.** Two exposures of the same scene, one focused near and one far (or one crop blurred on the left
     half, the other on the right): Auto-Blend Layers ▸ Stack Images masks each layer to its sharp half.
455. **Errors change nothing.** Auto-Blend with a locked layer selected (via the menu after locking it while the sheet
     is open) reports the reason; History and the canvas are unchanged.
456. **Save and reopen.** File ▸ Save As… `Pano.tessera-doc`, close, reopen: the merged layers, their masks and
     sources are back and the composite is identical.
457. **Layered PSD.** File ▸ Save Rasterized PSD Copy… `Pano.psd`, open it: the layers keep their masks (transforms
     rasterized); the composite matches.
458. **Stub.** With `--stub-library`, Auto-Align / Auto-Blend on two unlocked pixel layers report "… needs the engine
     (the stub backend has no pixels)"; invalid selections report the same reasons as the engine.
459. **Scripted run.**
     ```sh
     open -g -n apps/mac/build/Tessera.app --args --nonactivating --app-dir "$SCR/appdir"
     # or, for the log:
     TESSERA_STACK_SELFTEST="$SCR/out" apps/mac/build/Tessera.app/Contents/MacOS/Tessera --nonactivating \
       --app-dir "$SCR/appdir" 2>&1 | grep stack-selftest
     ```
     Expect every `check … ok` and `done, 0 failure(s)` (Photomerge of two synthetic crops into a new document, one
     history node, undo / redo, save as `StackSelfTest.tessera-doc`).

## Verdict (B5-19 stack)

PASS when steps 440–459 meet their expectations. Known limitations: alignment and blending cannot be cancelled and
report no progress (engine ask); lens corrections need explicit per-layer calibrations (library lens profiles are not
mapped), so both toggles are disabled; Photomerge always blends (no "Blend Images Together" off); Reposition is
withheld because the engine mis-registers it (see HANDOFF.md); source transparency is dropped.

## Appendix: accessibility identifiers (B5-19)

| Identifier | Element |
| --- | --- |
| `stack-align-sheet` · `stack-align-layout` · `stack-align-reference` · `stack-lens` | Auto-Align Layers sheet |
| `stack-blend-sheet` · `stack-blend-method` · `stack-blend-tones` · `stack-blend-fill` | Auto-Blend Layers sheet |
| `photomerge-sheet` · `photomerge-layout` · `photomerge-sources` · `photomerge-fill` · `photomerge-into-current` | Photomerge sheet |
| `stack-busy` · `stack-busy-cancel` | Busy sheet (Cancel only for Photomerge) |
