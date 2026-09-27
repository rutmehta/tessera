# AppKit layout and hosting audit

## Scope and evidence

Read-only source review of `apps/mac/Sources/Tessera`, prioritizing `ValueSlider`, sidebar and layer outlines, custom-drawn views, and AppKit/SwiftUI hosting boundaries. Read `apps/mac/DESIGN_AUDIT.md` first. No application launch, foreground interaction, source edit, build, or screenshot capture was performed. Geometry below was checked with Python arithmetic; it is **source-proven geometry, not a claim of runtime visual reproduction**. All source paths below are relative to `apps/mac/Sources/Tessera/`.

The existing design audit describes historical fixes and captures, not a proof that today's states and sizes cannot clip. In particular its items 38 (three-way grading slider labels), 8 (sidebar folder disambiguation), and 44 (compare panes) need the distinctions below.

## Concrete defects

### A1 — Compare caption and decision fields overlap by 12 pt

**Priority: medium. Geometry certain; visible text collision depends on content.**

- `Compare/CompareView.swift:245–247` sets the caption to `x = 12, width = 0.6W`, while the badge starts at `x = 0.6W` and has width `0.4W - 12`.
- Consequently the caption ends at `0.6W + 12`: the text-field frames intersect over a **12 pt strip**, at every pane width. At W = 300, the caption occupies x = 12…192 and the badge x = 180…288. Their vertical frames overlap too.
- Long filenames use their allocated caption frame (`:157` truncates in the middle), and badges can contain both the decision and “Suggested” (`:174–177`). When both allocations fill, the fields have no actual separation. This is not merely a small-window or localization concern.
- Root cause: the left inset was added to the caption origin but not subtracted from its width; two independent percentage calculations do not partition the available width.
- Diagnosis target: long filename plus “Best 3 · Suggested”, especially at narrow compare widths. A shared partition boundary and explicit inter-field gap are needed; changing fonts alone does not remove the intersection.

### A2 — Color-wheel puck extends 4 pt outside the view at full saturation

**Priority: medium. Reachable endpoint geometry certain.**

- `Inspector/ColorWheelView.swift:30–38` makes the disc diameter `min(width, height) - 4`, leaving only a 2 pt inset on the limiting axis, and places the puck center on that disc's rim at saturation 100.
- `:64–71` draws a radius-5 puck with a centered 2 pt stroke, giving a **6 pt outer radius**. Only 2 pt is available: **4 pt of the painted puck is outside the view** at a cardinal endpoint on the limiting axis.
- Example: a 72 pt-high wheel has diameter 68. At the top or bottom cardinal hue, the outer puck reaches 4 pt beyond the 72 pt boundary. This remains true on the larger single-range wheel: increasing the wheel size does not change the 2 pt inset.
- Reachable callers: `Inspector/DevelopPanels.swift:266` (144 pt high), `:274` (72 pt high), `:280` (72 × 72 Global). Pointer values are assigned to `saturation` at `ColorWheelView.swift:118`; this is not an impossible state.
- Depending on ancestor clipping, the result is a cut-off puck or paint intruding into the neighboring region. The source proves overflow, not which compositor path wins.
- Root cause: the disc's visual inset is used as if it also accommodated the larger interactive handle and stroke. The handle envelope must inform either the disc inset or the handle travel radius.

### A3 — Endpoint strokes exceed their reserved insets in both slider and point-curve controls

**Priority: low polish defect. Exact painted-boundary overflow.**

- **Slider:** `Inspector/ValueSlider.swift:108–111` reserves half the 12 pt thumb at each track endpoint. `:161` therefore gives a minimum-value thumb whose left edge is exactly zero. The ring is inset by 0.5 pt (`:171`) but its dragging stroke is 2 pt (`:172`). Its painted left edge is `0.5 - 1 = -0.5 pt`; the maximum-value edge exceeds the right boundary by the same amount. The ordinary 1 pt stroke fits. The shadow (`:163–169`) has additional unreserved extent, but is not needed to prove the ring defect.
- **Point curve:** `Inspector/CurveEditorView.swift:59–62` reserves 4 pt between the plot and view edge. End knots are drawn with radius 4 (`:197–200`), then an unselected knot is stroked at 1.5 pt (`:205`). Painted extent is 4.75 pt: **0.75 pt outside the view** at a boundary knot. Selected knots are fill-only and do not have this extra stroke overflow (`:201–205`).
- These are small outline clipping/bleed problems, not a claim that whole controls disappear. Both arise from budgeting the path bounds instead of its stroke envelope.

### A4 — Hidden sidebar badge still consumes width and causes premature text truncation

**Priority: medium density/legibility defect; not an overlapping-frames claim.**

- `Sidebar/SidebarView.swift:623–625` adds the title, detail, badge, and count as ordinary sibling subviews, **not arranged subviews of a stack**.
- The required chain at `:638–647` is title → 4 pt → detail → 4 pt → 16 pt badge → at least 4 pt → count → 8 pt trailing inset.
- `:675–678` hides absent detail/badge without deactivating constraints or changing the badge width. `isHidden` on an ordinary constrained view does not collapse those constraints.
- Thus even an unbadged recent-folder row retains the invisible badge's **16 pt box plus its adjoining 4 + 4 pt spacings** between detail and count. With the other fixed slots, the row reserves at least **54 pt before any title, detail, or count text** (2 + 8 + 8 + 4 + 4 + 16 + 4 + 8), excluding outline indentation and any source-list margins.
- Both title and detail have deliberately low horizontal compression resistance (`:627–628`), with detail lower still. The reserved invisible badge therefore steals space from the parent-folder text used to disambiguate repeated folder names. Hiding the detail also leaves the chain spacings in place.
- Root cause: visibility changes are not reflected in the Auto Layout width budget. Compression priorities prevent arbitrary overlap but cannot reclaim phantom slots. Some empty-column alignment may be intentional; the specific cost is that absent badges still reduce space for disambiguation. Validate long recent-folder names at the 200 pt sidebar minimum before deciding which columns should collapse.

## Conditional defects and risks requiring runtime sizing/content evidence

### R1 — Three-way ValueSlider titles have no collision policy

- `Inspector/ValueSlider.swift:135–138` draws the title at x = 0 and the formatted value at `W - valueWidth`, using unbounded `draw(at:)` for both. Collision occurs exactly when **titleWidth + valueWidth > W**. There is no clipping rectangle, truncation, reserved gap, or title-aware intrinsic width (`:59` reports no intrinsic width).
- `Inspector/DevelopPanels.swift:271–278` puts three such controls in an HStack with two 12 pt gaps; `App/Components.swift:306–309` adds 12 pt panel gutters on both sides. `App/Theme.swift:180–181` permits a 288 pt inspector with a 296 pt ideal width. If equally allocated, each slider gets **80 pt at minimum**, **82.67 pt at ideal**, and **110.67 pt at the 380 pt maximum**, before any additional scroll-view reduction.
- The risky strings are “Highlights”, “Midtones”, and signed multi-digit luminance values. No native font measurement or actual SwiftUI allocation was performed in this pass, so do **not** present a particular numeric-value collision as screenshot-confirmed. The collision predicate and missing policy are certain; evaluate them with actual font metrics and minimum-width frames.
- This revisits `DESIGN_AUDIT.md:142–143`: renaming “Lum” to the range name does not, by itself, solve width budgeting.

### R2 — Layer-row nesting can consume the name column, but ordinary top-level rows are not proven broken

- `Document/LayersOutline.swift:65–72` uses 32 pt rows, 12 pt indentation per level, and disables expansion-driven outline-column resizing. The horizontal NSStackView is pinned on all sides (`:650–673`); its fixed eye/link buttons are 20 pt and thumbnails/glyph 24 pt. The name is explicitly compressible and tail-truncated (`:636–643`).
- A plain masked pixel row already needs **120 pt excluding its name**: eye 20, thumbnail 24, link 20, mask 24, gaps 4 + 8 + 4 + 8, and trailing inset 8. Clipping indicator, style badge, kind, and lock can add more, while each ancestor consumes indentation.
- A sufficiently deeply nested decorated row must eventually exhaust the name space or force compression/detachment/constraint recovery. The actual outline cell width, disclosure reservation, and NSStackView behavior need runtime inspection; there is no basis here to claim all layer rows currently clip.
- Unlike SidebarCell, LayerRowCell uses arranged subviews. Its hidden thumbnail/glyph/mask alternatives (`:700–718`, `:735`) should not be reported as the sidebar's phantom-slot bug without observing stack behavior.

### R3 — Full-width draw-at text in loupe hints / histogram error states is unbounded

- `Loupe/LoupeToolOverlay.swift:159–170` sizes the hint to its measured string plus 24 pt and centers it. It cannot fit when `textWidth + 24 > bounds.width`; no wrapping or truncation exists. The long targeted-adjustment instruction at `:153` is a useful narrow-loupe test.
- `Inspector/HistogramView.swift:34–40` likewise centers an arbitrary placeholder with `draw(at:)`; `.unavailable(let why)` passes the reason through at `:102–105`. An unusually long backend reason will exceed the inspector width. Typical short placeholders are not a proven defect.
- Crop dimensions have a separate height threshold: `Loupe/LoupeToolOverlay.swift:23–26` fits to 80% of the view, while `:248–249` puts a 20 pt chip 6 pt below the crop. For a height-limited centered crop, the chip bottom is `0.9H + 26`; at H = 200 it reaches 206 (6 pt overflow), and below H = 260 it cannot fully fit. Whether supported window/panel combinations reach that canvas height belongs to the parent sizing pass.

### R4 — Detail-preview coordinate label and backing-scale invalidation deserve a targeted check

- `Inspector/DetailPreviewView.swift:51` fixes the coordinate label to 88 pt, but `:71` embeds unbounded integer x/y strings. Test large-image coordinates rather than treating the fixed width alone as proof of clipping.
- The preview refreshes `contentsScale` and surface size only in layout (`:48–55`) and schedules layout when attached (`:58–64`). It does not explicitly handle `viewDidChangeBackingProperties`, unlike the Metal viewport bridges. Check a same-point-size move between 1× and 2× screens; do not label it a confirmed failure because AppKit may independently invalidate layout.

## Positives / non-findings

- **Hosting geometry is generally explicit.** `Shell/ContentView.swift:22–55` supplies the canvas ZStack with GeometryReader dimensions, so the initial 800 × 600 construction size in `Loupe/LoupeView.swift:19` is not evidence of a fixed-size canvas bug.
- **Metal overlays track size correctly at the bridge.** `Loupe/MetalLoupeView.swift:65–68,92–103` resizes the overlay in both axes and responds to frame/backing changes. `Document/DocumentViewport.swift:166–173,230–245` does the same for document overlays and guards drawable dimensions to at least one pixel.
- **Native controls use appropriate intended row heights.** ValueSlider advertises 32 pt (`Inspector/ValueSlider.swift:59`), matching the examined grading and LayersPanel callers; the layer outline's 32 pt row fits 24 pt thumbnails. No broad vertical row-height mismatch was established.
- **Sidebar compression is deliberate.** Title/detail truncate, count and badge resist horizontal compression (`Sidebar/SidebarView.swift:604,619,627–630`), and the outline scrolls vertically (`:153–157`). This prevents blaming every long title or fixed badge for overlap; the confirmed concern is retained hidden width.
- **Layer outline has the right basic structure.** Recycled native rows, name compression, a vertically scrollable document view (`Document/LayersOutline.swift:16–25`), and a 128 pt minimum outline region (`Document/LayersPanel.swift:54–55`) are reasonable. Deep nesting remains the conditional stress case above.
- **The direct NSHostingView panel has a minimum-size guard.** `Document/Styles/DocumentStyles.swift:243–253` creates a resizable NSPanel, installs the hosting root, and sets its content minimum to the supplied size. `:236` uses 640 × 560 for the Layer Style inspector. No missing frame constraint or clipping defect was established at this bridge.
- **Grid layout recomputes against the scroll viewport.** `Grid/UniformGridLayout.swift:37–58,120–124` derives cell dimensions from the viewport and invalidates on the relevant extent change. `Grid/ThumbnailCell.swift:157–164` resizes image and overlay together. Thumbnail status-chip layout subtracts the basket width (`:268–279`) and tiny chips are skipped (`:311–312`), avoiding a blanket “all custom badges overlap” diagnosis.

## Handoff

Prioritize a narrow compare pane with long caption + combined badge, full-saturation grading-wheel endpoints, and minimum-width three-way grading. Then test long recent-folder names and a deeply nested, masked/styled/locked layer. The first two have direct geometric proofs; the three-way text case needs actual font/frame measurements. Keep micro-stroke overflow separate from usability-level text collisions. Historical audit screenshots were not inspected in this source-only pass.
