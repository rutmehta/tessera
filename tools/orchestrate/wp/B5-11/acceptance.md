# B5-11 acceptance (Sol)

- App bundle: `/Users/rutmehta/Developer/lightroom/.worktrees/B5-11/apps/mac/build/Tessera.app` (after integration: the
  coordinator's combined build).
- Real engine, new 5472 × 3648 8-bit sRGB document (20 MP); no library fixture is needed. For a PSD round trip use a
  scratch folder.
- Scripted evidence: `tools/orchestrate/wp/B5-11/run-vector-selftest.sh` (background launch, own-window captures).
- Record `verdict.json` with every number 360–379; screenshots under `tools/orchestrate/wp/B5-11/evidence/`.

The numbered expected states are the ACCEPTANCE.md section, reproduced:

### B5-11. Shapes, Pen and vector masks (B5-11)

Engine backend, this worktree's app (`apps/mac/build/Tessera.app`, `Support/make-app.sh debug`). New document
**5472 × 3648 px, 8-bit, sRGB** (File ▸ New; 20 MP). Only scratch folders. The scripted run of every step is
`tools/orchestrate/wp/B5-11/run-vector-selftest.sh` (launches with `open -g -n … --new-document
--vector-selftest=<dir>`, never activates the app, captures its own window with `screencapture -l`); its log and 27
captures are in `tools/orchestrate/wp/B5-11/evidence/`. Known gaps: tools/orchestrate/wp/B5-11/NEEDS.md.

360. **Tools.** The palette has three new slots: Pen (P), Path Selection / Direct Selection (A, ⇧A cycles) and
     Rectangle / Ellipse / Polygon / Line (U, ⇧U cycles; right-click lists the group). Press **U** and drag on the canvas.
     📸 A row with the `square.on.circle` kind glyph named `Rectangle 1` appears; Properties ▸ Kind reads **Shape**
     (never Fill); History reads **Rectangle Tool**.
361. **Rounded rectangle.** Options bar ▸ Radius `40`, drag another rectangle: all four corners rounded. In Properties
     untick **Same radius for all corners** and drag Top right / Bottom right / Bottom left independently: the corners
     follow live during the drag; each release is one **Edit Shape** node.
362. **Ellipse.** ⇧U to Ellipse; drag with ⇧⌥ from a point: a circle centred on the press point. Without modifiers the
     box corner follows the pointer.
363. **Polygon / star.** ⇧U to Polygon; options Sides `6`, tick Star, Inset `50 %`; drag from the centre (⇧ snaps the
     angle to 15°): a 12-point star. In Properties drag Sides to 8 and Star inset: the geometry regenerates live.
364. **Line.** ⇧U to Line, Weight `14`, drag (⇧ snaps to 45°): a stroke-only line (Properties ▸ Fill **None**). With
     Path Selection (A) click just beside the line (within the stroke): the line is selected.
365. **Paint.** Rectangle ▸ Properties ▸ Fill ▸ **Gradient**; change Style, Start / End colours, Angle: the canvas
     follows. Move the shape with Path Selection: the gradient stays anchored to the document (the shape moves across
     it; Hint in Properties says so). Fill ▸ Solid and a colour: the colour applies.
366. **Stroke.** Stroke ▸ Solid, Width `18`, Align **Inside**, Caps **Round**, Corners **Bevel**, Dashes `60, 30`, Dash
     offset `12`: each visible on the canvas; the values survive save / reopen (step 378).
367. **Pen.** P, click three points, drag on the second to pull symmetric handles (⌥ breaks them), click the first
     point: the path closes and one **Pen** node adds a custom shape. Return finishes an open path; Esc discards the
     draft; ⌫ removes the last point.
368. **Direct Selection.** ⇧A to Direct Selection (or A twice). Click an anchor (filled square = selected), drag it: one
     **Move Anchor Point** node. Drag a direction point: the opposite handle mirrors; with ⌥ it stays (one **Move
     Direction Point** node each).
369. **Insert / delete.** ⌥-click a segment: **Add Anchor Point**; ⌫ deletes the selected anchor (**Delete Anchor
     Point**); with the Pen, clicking a selected shape's anchor deletes it and a segment adds one. Drag a corner of a
     live rectangle: Properties changes to **Custom Path** and later paint edits no longer regenerate the rectangle.
370. **Path operations.** Draw two overlapping rectangles, select both rows, Layer ▸ Combine Shapes ▸ **Combine /
     Subtract Front Shape / Intersect Shape Areas / Exclude Overlapping Shapes** (also Path Selection's Combine menu):
     the front shape merges into the back one with correct holes; each is **one** history row; ⌘Z restores both layers.
371. **Hits at any view.** At Fit, 100 % (⌘1) and after panning, Path Selection clicks select the shape under the
     pointer, including thin stroke-only lines; the overlay path, anchors and box stay on the geometry. Give a shape a
     skewed transform (Path Selection box: drag a side handle, then rotate outside the box): clicks inside the skewed
     shape still hit it.
372. **Affine handles.** Path Selection: drag a corner handle (⇧ keeps proportions, ⌥ from the centre), inside to move,
     outside to rotate: one **Transform Shape** node per drag. Start another drag and press Esc before releasing: the
     shape returns and History is unchanged.
373. **Vector mask next to a layer mask.** Layer ▸ Layer Mask ▸ Reveal All, then make a marquee and Layer ▸ Vector Mask
     ▸ Current Selection (or Properties ▸ Add Vector Mask / From Selection): the raster mask thumbnail stays; the vector
     mask clips outside the marquee; Properties shows the Vector Mask section.
374. **Mask controls.** Untick Enabled (whole shape shows), tick it again; drag Density 100 → 50 % (outside shows at half
     opacity; one **Vector Mask Density** node on release), Feather 40 px (soft edge; one **Vector Mask Feather**
     node). ⌘Z steps back through each.
375. **Fixed versus linked mask.** Path Selection, drag the shape: the mask stays in place (the shape slides under it).
     Tick **Move vector mask with shape** (options bar or Properties) and drag again: shape and mask move together as
     one **Transform Shape and Vector Mask** row; one ⌘Z restores both.
376. **Errors.** Lock pixels on the shape and change its colour: the status bar says the content is locked and nothing
     changes. Lock position only: moving fails (`position is locked`), recolouring works. Select the line and choose
     Align ▸ Outside: `Inside and Outside alignment need a closed path; …`.
377. **Convert.** Layer ▸ Rasterize Shape (or Properties ▸ Convert to Pixels) on the masked shape: the row becomes a
     pixel layer with both masks still applied once (appearance unchanged), history row **Convert to Pixels** (B5-10's
     shared conversion); ⌘Z restores the live shape exactly.
378. **Reopen.** Save As `.tessera-doc` and `.psd` into a scratch folder, close, reopen each: every shape is a Shape row
     with its live controls (rectangle radii, star, line, custom paths, dashes) and its vector mask. Properties ▸
     Interchange states that in a PSD the extra vector mask is a raster user mask plus Tessera's private tvMk record
     (other apps see the combined raster mask). A shape with an imported pattern fill shows the warning that PSD save
     does not support pattern shape fills, and Save As `.psd` fails with that reason (native save works).
379. **Inspector and neighbours.** At 1440 pt window width 📸 the shape Properties (Shape, Fill, Stroke, Vector Mask,
     Interchange) scroll inside the Properties pane; the Remove tool (⇧J) still activates and deactivates. Add B5-07
     layer styles (Drop Shadow, Stroke) to a shape: two history rows, the row shows FX and its effects, the layer stays
     a live Shape; Convert to Pixels keeps the styles and ⌘Z restores the styled live shape.

### Verdict (B5-11 shapes, Pen and vector masks)

PASS when steps 360–379 meet their expectations. Known engine limitations listed in NEEDS.md (PSD reopen of a shape
with both a full-canvas raster mask and a vector mask on large documents; slow previews of stroked shapes) are
recorded, not failures of the host.

### Appendix: accessibility identifiers (B5-11)

| Identifier | Element |
| --- | --- |
| `document.tool.rectangleShape` · `ellipseShape` · `polygonShape` · `lineShape` · `pen` · `pathSelect` · `directSelect` | Palette slots (the slot shows the group's current tool) |
| `document.shape.inspector` · `document.shape.convert` | Shape Properties section, Convert to Pixels |
| `document.shape.rect.width` · `rect.height` · `rect.linkRadii` · `rect.radius` · `rect.radius0…3` | Rectangle controls |
| `document.shape.ellipse.width` · `ellipse.height` · `polygon.sides` · `polygon.radius` · `polygon.rotation` · `polygon.star` · `polygon.inset` · `line.length` · `line.angle` · `fillRule` | Other live parameters |
| `document.shape.fill.kind` · `fill.color` · `fill.gradientKind` · `fill.start` · `fill.end` · `fill.angle` | Fill |
| `document.shape.stroke.kind` · `stroke.color` · `stroke.width` · `stroke.alignment` · `stroke.cap` · `stroke.join` · `stroke.miter` · `stroke.dashes` · `stroke.dashOffset` | Stroke |
| `document.shape.mask.add` · `mask.enabled` · `mask.density` · `mask.feather` · `mask.linked` · `mask.delete` | Vector mask |
| `document.option.fillColor` · `document.option.strokeColor` · `document.option.width` · `document.option.radius` · `document.option.sides` · `document.option.weight` | Options bar |
