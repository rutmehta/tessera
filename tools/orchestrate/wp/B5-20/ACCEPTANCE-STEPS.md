# B5-20. Adaptive Wide Angle (steps 460–479)

To merge into `apps/mac/ACCEPTANCE.md` by the integrator. Vanishing Point is out of scope for B5-20.
Automated coverage: `DocumentAdaptiveWideAngleTests` (9 tests), `crates/tessera-ffi/tests/document_adaptive_ui.rs`
(8 tests + 1 ignored timing), `crates/filters/tests/adaptive_smart_filter.rs` (6 tests) and
`--adaptive-wide-angle-selftest <dir>` (background launch only:
`open -g -n … --args --adaptive-wide-angle-selftest <dir> --nonactivating`).

Open `sample.dng` with Edit in Layers (document mode). Select the pixel layer.

460. Filter ▸ Adaptive Wide Angle… (⌥⇧⌘A) is listed after Camera Raw Filter…, enabled for a pixel layer or a
     smart object, disabled for adjustment/text/fill/shape/group layers and while the workspace is open.
461. The workspace sheet opens titled "Adaptive Wide Angle", subtitle "<layer> · W × H px · changes the layer's
     pixels" (smart object: "· adds a smart filter"). Left: the canvas (fit to the view, checkerboard behind
     transparency). Right: Correction (Perspective | Fisheye, Focal Length in mm, Scale %), Constraints, View
     (Preview (P), Show constraints) and the preview-resolution note ("Preview at 1/N …; OK renders W × H").
     Footer: status, Cancel, OK. Fits at 1440 × 900.
462. Opening changes nothing: with Perspective and no constraints the preview equals the layer. For a library
     image with EXIF FocalLengthIn35mmFilm the Focal Length starts there and "EXIF: N mm (35 mm equivalent)"
     with a Use button is shown; otherwise it starts at 24 mm with a hint.
463. Choose Fisheye and a short focal length (e.g. 12–16 mm): the preview shows the fisheye corrected to a
     rectilinear view (straight lines through the centre stay straight, edges stretch outward). Scale shrinks or
     enlarges the corrected view; transparent areas outside the source show the checkerboard.
464. Drag on the canvas along an edge: Preview turns off, the original is shown with the constraint drawn as a
     curve that follows the camera model (a fisheye bows it like the edge). ⇧-drag makes it Vertical or
     Horizontal by its dominant direction; otherwise Straight. A click without a drag adds nothing.
465. Click a line to select it (wider stroke): the Orientation picker (Straight | Horizontal | Vertical) and Remove
     Line appear; Delete removes it; Remove All clears every line. Changing Correction or Focal Length re-traces
     every line.
466. Preview (P) shows the corrected result with the constraints applied (the traced edges come out straight /
     level / plumb); unchecked shows the original with the lines. History records nothing while the sheet is up.
     Scroll pans, ⌘-scroll or pinch zooms, ⌥ double-click fits.
467. A line beyond the camera's field of view is refused with a red status line and not added.
468. Conflicting constraints (e.g. a vertical edge set Horizontal against a Vertical one) show a red error in the
     footer ("constraint residual exceeds tolerance; conflicting lines…"); OK then reports the error, the sheet
     stays open, History is unchanged.
469. Cancel (Esc): the sheet closes, the layer is unchanged, History has no new row.
470. Reopen, set Fisheye + constraints, OK (Return): "Applying Adaptive Wide Angle… N s · Esc cancels" while it
     renders at full resolution; then the sheet closes and History gains one row "Adaptive Wide Angle". Esc while
     applying stops it with no row.
471. Undo restores the layer exactly; Redo re-applies. With a marquee on a pixel layer, only the selected pixels
     change (the whole layer is corrected, written through the selection).
472. Filter ▸ Convert for Smart Filters, then Adaptive Wide Angle…, OK: one "Adaptive Wide Angle" smart filter row
     is added under the layer (name "Adaptive Wide Angle", not the id); the canvas shows the corrected layer.
473. Double-click the row (or its menu ▸ Edit Smart Filter…): the workspace reopens with the stored camera, focal
     length, scale and lines, subtitle "· re-editing smart filter 1".
474. Change Scale and OK: History gains one row; still one smart filter row (replaced in place, not duplicated).
     Disabling the row shows the original layer; enabling it restores the correction.
475. Save, close and reopen the .tessera-doc: the row, its appearance and its recipe round-trip; double-click still
     re-opens it with every line (params kept exactly, no format change).
476. Saving that document as .psd is refused as for any smart filter ("smart filters … are native-only; rasterize
     explicitly for PSD"); File ▸ Save Rasterized PSD Copy… saves, and the copy reopened matches the native
     composite. The document itself keeps its editable smart filter.
477. A layer larger than 4095 × 4095 px (e.g. a 6000 × 4000 image): Adaptive Wide Angle… does not open; the status
     reads "Adaptive Wide Angle supports layers up to 4095 × 4095 pixels (16,777,216 mesh vertices); this layer is
     6000 × 4000". History is unchanged.
478. A locked layer (Lock image pixels / Lock all) is refused ("the layer's pixels are locked"); a layer changed by
     another edit while the sheet is open is refused at OK ("changed") without a history row.
479. Older builds: a document with an Adaptive Wide Angle smart filter opens; the filter is an unknown id there
     (it does not render, as any unknown smart filter), and nothing is lost when this build reopens it.

Identifiers: `document.awa.canvas`, `.projection`, `.focal`, `.useExif`, `.scale`, `.orientation`, `.removeLine`,
`.removeAll`, `.preview`, `.showConstraints`, `.error`, `.cancel`, `.ok`.
