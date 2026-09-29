# B5-18. Camera Raw Filter (steps 420–439)

To merge into `apps/mac/ACCEPTANCE.md` by the integrator (kept out of the shared file while
B5-13 / B5-15 / B5-19 / B5-20 are in flight). Automated coverage: `DocumentCameraRawTests` (16 tests)
and `--camera-raw-selftest <dir>` (background launch only: `open -g -n … --args --camera-raw-selftest <dir> --nonactivating`).

Open `sample.dng` with Edit in Layers (document mode). Select the pixel layer.

420. Filter ▸ Camera Raw Filter… (⇧⌘A) is listed after Neural Filters…, enabled for a pixel layer or a smart
     object, disabled for adjustment/text/fill layers and while an apply runs. In the library/Develop, ⇧⌘A is
     still Develop ▸ Auto Edit…; in documents it is Camera Raw Filter… (Auto Edit keeps its menu item).
421. The sheet opens titled "Camera Raw Filter", subtitle "Layer “<name>”" (", inside the selection" with a
     marquee). Left: 1:1 detail pane (label "1:1"), Amount 100 %, "Show before". Right: tabs Basic, Curve, HSL,
     Color Grading, Detail, Effects. Footer: Reset, Cancel, OK. Fits at 1440 × 900 (sheet 860 × 600 pt).
422. Basic: White Balance (Temp 6500 K, Tint 0), Tone (Exposure, Contrast, Highlights, Shadows, Whites, Blacks),
     Presence (Texture, Clarity, Dehaze, Vibrance, Saturation). All at neutral; opening changes nothing on the
     canvas (sharpening and colour NR start at 0 for rendered pixels).
423. Curve: Parametric Highlights/Lights/Darks/Shadows and the three split points (they cannot cross).
424. HSL: Hue, Saturation, Luminance × Red…Magenta (the Develop HSL mixer's eight bands).
425. Color Grading: Shadows/Midtones/Highlights/Global Hue, Saturation, Luminance; Blending 50, Balance 0.
     Detail: Sharpening (Amount 0), Noise Reduction, Color Noise Reduction (Color 0). Effects: Vignette, Grain.
     Every slider writes the same `DevelopSettings` path as the Develop panel of the same name.
426. Drag Exposure: the canvas preview follows (drags coalesced ~120 ms, the release at once); the detail pane
     updates latest-wins. History records nothing while the sheet is open.
427. "Show before" shows the layer without the filter on the canvas and in the detail pane (label "1:1 before");
     moving any slider turns it off.
428. Amount 50 % blends halfway between the original and the developed layer.
429. Cancel (Esc): the preview ends, the layer is unchanged, History has no new row.
430. Reopen (the last tab is remembered), set values, OK (Return): the sheet stays up with "Applying at full
     resolution… Cancel stops it." and disabled controls; when it lands the sheet closes and History gains one row.
431. Cancel while applying stops the apply; no row is added and the preview is cleared.
432. Undo restores the layer; Redo re-applies.
433. With a marquee on a pixel layer, OK changes only the selected pixels.
434. Filter ▸ Convert for Smart Filters, then Camera Raw Filter…: subtitle "Smart filter on “<name>”"; OK appends
     one camera_raw smart filter row under the layer.
435. Double-click the row (or its menu ▸ Edit Smart Filter…): the sheet reopens with the saved values, subtitle
     "Editing a smart filter of “<name>”"; the canvas previews the re-edit (the 1:1 pane is replaced by a note).
436. Change a value, OK: History gains "Edit Smart Filter"; still one smart filter row (replaced, not duplicated).
     Save, close, reopen the .tessera-doc: the row and its settings round-trip.
437. A recipe with settings the sheet has no control for (e.g. point curves from MCP) keeps them after re-edit.
438. Smart object with an active selection: Camera Raw Filter… does not open; the status says
     "Camera Raw Filter: Deselect first (⌘D). On a smart object it is a smart filter over the whole layer…".
439. A camera_raw smart filter whose recipe has AI mask components (subject, sky, background, person, object,
     landscape, depth) does not open: the status names the AI masks and says no AI mask provider is installed.
     Invalid values are impossible from the sheet (clamped); engine errors show in red in the sheet.

Identifiers: `document.cameraRaw.panel`, `.basic|curve|hsl|color_grading|detail|effects` (the scroll area),
`.<control path joined by .>` (e.g. `document.cameraRaw.tone.exposure`), `.amount`, `.before`, `.detail`, `.busy`,
`.reset`, `.cancel`, `.ok`.
