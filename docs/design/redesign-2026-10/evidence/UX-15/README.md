# UX-15 collisions: evidence

FOLLOWUPS items 18–25. Rendered in-process by `apps/mac/Tests/TesseraCoreTests/CollisionEvidenceTests.swift`
(`TESSERA_UX15_EVIDENCE=<dir> TESSERA_UX15_SHOOT=<folder> swift test --filter CollisionEvidenceTests`
from `apps/mac/`) on a copy of the five CC0 fixture RAWs (`fixtures/raw`, copied to a scratch
folder; never the owner's volume), window 1385 × 868 pt, dark, the panel shown. The window is
drawn through `cacheDisplay` at 2× when `screencapture` has no permission, so the Metal loupe
can be black in those renders (the automation shell's limitation, not the app's). The `before/`
run had `screencapture`; the `after/` run did not: in `after/` the loupe is black, the Library
grid's collection view and macOS 26's inspector material do not draw at all (`02`, `03` show the
canvas and a colour-gradient placeholder where the inspector material is). Those two states are
verified by `ThumbnailRetryTests` and the inspector source instead; re-run the test with screen
recording allowed for the xctest host to get full renders. `before/` is
commit `a04b8848` (the test alone on main), `after/` the package's final commit. PNGs are
downscaled to 1400 px wide.

| File | Item | What to look at |
| --- | --- | --- |
| `01-cull-toast.png` | 20 | Before: the "Opened shoot" toast sits on the burst view's action row (on the owner's shoot, on the chip row). After: one row above the chips, in the band the burst view keeps clear. |
| `02-cull-grid.png` | 23 | Before: "No Smart Preview" text label inside the cell, "review" as a bare link in the row header. After: a corner glyph (tooltip carries the text; the fixtures have no Smart Previews, so no glyph shows here), a **Review** button. Thumbnails all drawn. |
| `03-cull-loupe-panel.png` | 22 | Before: Assist, Agent Edit, Keywords and Metadata as paragraphs, a Photo Edit section. After: one line each, Keywords and Metadata collapsed, no Photo Edit. |
| `04-edit-opening.png` | 19 | 0.4 s after choosing Edit. Before: a "Rendering…" capsule in the histogram strip and the "Finish saving…" toast over the photo. After: the strip is empty, the Edits readout says "Rendering…" (the reason after 2 s is unit-tested; the fixtures open faster). |
| `05-edit-ready.png` | — | Edit once the engine's frame is up, for reference. |
| `06-deliver-switching.png` | 18 | 0.3 s after choosing Deliver with the session open. Before: Deliver's columns over the Edit panel, Try cards and tool strip. After: Deliver alone (the session is saved and closed first). |
| `07-deliver-settled.png` | 18 | Deliver after the switch settled. |

Items 21 (no "Measuring sharpness…" bar; the Cull column's line has Stop), 24 (`BridgeError`
messages) and 25 (the mode control's hit target and hover) have no screenshot of their own: 21
shows in `01` (no bar at the bottom), 24 and 25 are covered by `BridgeErrorDescriptionTests` and
the `SegmentedPicker` change.
