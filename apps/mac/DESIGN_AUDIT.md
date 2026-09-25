# Tessera for Mac: design audit (WP M2-23)

Audit of the shipped interface before the design-system overhaul. Every surface was captured
window-only (`screencapture -l <window>`) from a real run: the worktree build, a scratch
`--app-dir`, and a scratch folder of 48 generated burst JPEGs plus the five fixture raws
(`fixtures/raw`). The attached display is 1x (3840 × 2160 at native); no 2x display was
available, so every capture is 1x (see "Limits" below).

Screenshots are in `tools/orchestrate/wp/M2-23/evidence/`:

| Surface | Before | After (Dark) | After (Light) |
| --- | --- | --- | --- |
| Grid, sidebar, filter bar, status bar, filmstrip | `before/grid.jpg` | `after/dark/grid.jpg` | `after/light/grid.jpg` |
| Loupe + Histogram + Basic | `before/loupe-basic.jpg` | `after/dark/loupe-basic.jpg` | `after/light/loupe-basic.jpg` |
| Tone Curve | `before/loupe-tonecurve.jpg` | `after/dark/loupe-tonecurve.jpg` | `after/light/loupe-tonecurve.jpg` |
| HSL / Color | `before/loupe-hsl.jpg` | `after/dark/loupe-hsl.jpg` | `after/light/loupe-hsl.jpg` |
| Color Grading | `before/loupe-colorgrading.jpg` | `after/dark/loupe-colorgrading.jpg` | `after/light/loupe-colorgrading.jpg` |
| Detail | `before/loupe-detail.jpg` | `after/dark/loupe-detail.jpg` | `after/light/loupe-detail.jpg` |
| Effects, Crop & Straighten | `before/loupe-effects-crop.jpg` | `after/dark/loupe-effects-crop.jpg` | `after/light/loupe-effects-crop.jpg` |
| Soft Proofing, Presets, Snapshots, History | `before/loupe-proof-presets-history.jpg` | `after/dark/loupe-proof-presets-history.jpg` | `after/light/loupe-proof-presets-history.jpg` |
| Masks panel + loupe mask toolbar | `before/loupe-masks.jpg` | `after/dark/loupe-masks.jpg` | `after/light/loupe-masks.jpg` |
| Keywords, Metadata | `before/grid-keywords-metadata.jpg` | `after/dark/grid-keywords-metadata.jpg` | `after/light/grid-keywords-metadata.jpg` |
| Compare | `before/compare.jpg` | `after/dark/compare.jpg` | `after/light/compare.jpg` |
| Toast (K: keep best, reject rest) | `before/toast.jpg` | `after/dark/toast.jpg` | `after/light/toast.jpg` |
| Smart album sheet | `before/sheet-smartalbum.jpg` | `after/dark/sheet-smartalbum.jpg` | `after/light/sheet-smartalbum.jpg` |
| Lightroom import sheet | `before/sheet-import.jpg` | `after/dark/sheet-import.jpg` | `after/light/sheet-import.jpg` |
| Export sheet | `before/sheet-export.jpg` | `after/dark/sheet-export.jpg` | `after/light/sheet-export.jpg` |
| Print sheet | `before/sheet-print.jpg` | `after/dark/sheet-print.jpg` | `after/light/sheet-print.jpg` |
| Defect sweep sheet | `before/sheet-defects.jpg` | `after/dark/sheet-defects.jpg` | `after/light/sheet-defects.jpg` |
| Empty state | `before/empty.jpg` | `after/dark/empty.jpg` | `after/light/empty.jpg` |

`iteration-1/` holds the first post-implementation pass (Dark) that the review loop corrected:
truncated toolbar segments ("Lou…", "Co…"), menu labels picking up the accent tint, sub-headers
touching the next label, the mask hint drawn under the mask toolbar, an amber-filled active
compare pane and accent-filled segmented controls in sheets. The second pass (both appearances)
then fixed washed-out scopes in Light (scopes now keep a dark well in both appearances), a brown
light-mode accent (raised to `#A8620C`, 4.75:1 with white text) and the remaining system pop-ups
in the inspector (now `MenuPicker`). `after/` is the state after the third pass.

## Summary

The shipped UI had no system: 13 font sizes (8, 9, 9.5, 10, 10.5, 11, 11.5, 12, 13, 14, 15, 17,
34), 8 corner radii (1, 1.5, 2, 3, 4, 5, 6, 7), 18 spacing values, two competing accents (the
app's amber and the macOS system accent, purple on this Mac), hard-coded greys that only work in
Dark, and a forced Dark appearance. On macOS 26 every toolbar item and bordered button renders
as a Liquid Glass capsule, which is the "bubbles" the owner saw. Uppercase tracked micro-labels,
grey tiles behind every thumbnail and saturated all-caps badges read as a 2012 Lightroom clone.

## Defects

Status: **Fixed** means the after-captures show the fix; the fix is described in one line.

### Title bar and toolbar (`before/grid.jpg`)

1. Every toolbar item is a separate floating glass capsule (Open Folder…, the view picker, the
   thumbnail slider in its own capsule, Auto-advance, Inspector): the "bubble" look.
   **Fixed**: one flat toolbar; items opt out of the per-item glass (`sharedBackgroundVisibility(.hidden)`), flat 28 pt buttons.
2. Auto-advance and Inspector "on" state is a filled capsule in the *system* accent (purple),
   a second accent next to the amber focus ring. **Fixed**: neutral pressed fill + primary text; the accent is not used for toolbar state.
3. Toggles are text-only; the brief asks for icon + label items. **Fixed**: SF Symbol + label (`arrow.right.to.line`, `sidebar.right`, `folder`).
4. The thumbnail-size slider stays in the toolbar (greyed, as a floating knob) in Loupe and
   Compare. **Fixed**: visible only in Grid; its slot keeps its width so nothing shifts.
5. Grid / Loupe / Compare is a system segmented capsule with the system accent selection.
   **Fixed**: `SegmentedPicker` (well track, raised selected segment, icons + labels).
6. Opening Color Grading widens the inspector from 280 to 340 pt (the five-segment picker's
   minimum width) and pushes the toolbar items (`before/loupe-colorgrading.jpg`): a layout jump.
   **Fixed**: icon segments for the four ranges; the inspector keeps its width.

### Sidebar (`before/grid.jpg`, `before/empty.jpg`)

7. The selected row uses the system accent (purple text on a grey slab). **Fixed**: `SidebarRowView` draws the accent-subtle fill at radius 6; text keeps its colour.
8. Recent folders are indistinguishable ("raw, raw, raw, nef, raw"); subfolders are indented
   with leading spaces in the title. **Fixed**: parent folder shown as tertiary detail ("raw run2"); real indentation via a constraint.
9. Section headers are 10 pt semibold, uppercase, tracked; the inspector uses a different header
   treatment. **Fixed**: 11 pt semibold title case, tertiary; same casing as the inspector.
10. Rows are 22 pt with 2 pt inter-cell spacing (off the 8-pt grid). **Fixed**: 24 pt rows, 28 pt headers, no inter-cell spacing.
11. The basket badge is 9 pt monospaced bold blue text in a 1 pt blue outline, a one-off.
    **Fixed**: 16 pt chip, 11 pt medium, hairline outline.
12. The "+" add menu is a text "+" in a pop-up button, 1–2 pt off the header baseline.
    **Fixed**: `plus` symbol, centred on the header label.
13. A solid painted background hides the source-list material. **Fixed**: native sidebar material shows through.

### Filter bar (`before/grid.jpg`)

14. The rule field is 11.5 pt monospaced in a square bezel — the only square field in the app,
    reading as a terminal — and its height differs from the facet buttons. **Fixed**: `FieldContainer` (24 pt, radius 6, magnifier), 12 pt SF.
15. Eight facet menus are system capsules (pill overuse); active facets tint the whole capsule.
    **Fixed**: `ThemeMenuStyle` 20 pt, radius 6; active = accent-subtle fill + outline.
16. "Save as Smart Album…" is a disabled bordered capsule floating at the far right.
    **Fixed**: borderless button beside Clear.
17. 10 pt horizontal padding, 5/6 pt vertical rhythm (off grid), no separator below.
    **Fixed**: 12 pt gutter, 8 pt rhythm, 1 px hairline.

### Grid and filmstrip (`before/grid.jpg`, `before/toast.jpg`)

18. Every thumbnail sits on a raised grey tile (0.14 white, radius 3, 8 pt inset): the
    Lightroom look. **Fixed**: photos on the canvas; alternate burst groups get a 3.5 % fill.
19. Selection = lighter grey tile *and* focus = 2 pt amber border, both at once, on a radius the
    rest of the app does not use. **Fixed**: selection = accent-subtle fill, focus = 2 px accent ring, radius 6.
20. Badges are 9.5 pt bold ALL CAPS (KEEP, REJECT, GOOD 2, BEST 3, SUGGESTED, PORTFOLIO 2026) in
    saturated fills with 85 % black text, in three shapes (filled, outlined, numeral).
    **Fixed**: one chip family: 16 pt, radius 4, 11 pt medium, sentence case, desaturated fills.
21. The basket chip spells the album name in capitals across the photo. **Fixed**: sentence case, max 55 % of the width, truncated.
22. Caption: 10.5 pt name and 10 pt monospaced group label on different baselines (y + 1 hack).
    **Fixed**: 11 pt name + 11 pt tabular group on one baseline.
23. Filmstrip 92 pt tall with 8 pt cell insets and the same grey tiles; 8 pt bold letter
    badges. **Fixed**: 72 pt, 2 pt insets, same chip family.
24. Empty state: the filmstrip keeps a blank 92 pt band under the status bar (`before/empty.jpg`).
    **Fixed**: no filmstrip without photos.

### Status bar (`before/grid.jpg`)

25. Seven items at 14 pt spacing with no grouping; the message truncates mid-sentence; basket in
    link blue; "Auto-advance on" duplicates the toolbar. **Fixed**: three hairline-separated groups (where / what happened / session), tertiary message, basket swatch, no duplicate.

### Loupe (`before/loupe-basic.jpg`, `before/loupe-masks.jpg`)

26. The "Masks" entry is a floating pill over the photo's top-right corner. **Fixed**: bordered 20 pt button in the loupe's information strip.
27. The mask toolbar overlaps the filename / display-info line (text visible through the bar),
    and the "Pick a mask tool" hint is drawn underneath the toolbar. **Fixed**: 32 pt reserved strip; toolbar below it; hint moved to the bottom.
28. Mask toolbar: selected tool is an amber square with a black glyph, radius 5 and 7 (off
    scale), white-12 % separators. **Fixed**: `IconButton` (accent glyph on accent-subtle), HUD radius 8, hairline separators.
29. Shortcut hints laid out with runs of spaces. **Fixed**: middle-dot separated, tertiary 11 pt.
30. Crop dimensions are white text straight on the surround (invisible on a light surround).
    **Fixed**: scrim chip.

### Inspector (`before/loupe-*.jpg`, `before/grid-keywords-metadata.jpg`)

31. Section headers: 10 pt uppercase tracked labels with "–" / "+" glyphs of different widths
    (the glyph column wobbles). **Fixed**: 32 pt header, 12 pt semibold title case, rotating chevron.
32. Sub-headers ("White Balance", "Tone") are 10 pt tertiary (≈ 2.6:1) and touch the next label.
    **Fixed**: 11 pt medium secondary (≥ 6:1), 12 pt above / 4 pt below.
33. Sliders: 3 × 10 pt bar thumb (hard to hit), 3 pt grey track, 30 pt rows (off grid), value in
    secondary grey. **Fixed**: 12 pt disc thumb, 2 pt track, default tick, 32 pt rows, tabular value that brightens when changed.
34. Reset / Snapshots and every panel button are system capsules. **Fixed**: 20 pt radius-6 buttons and menus.
35. Segmented pickers (Parametric/Point, Hue/Saturation/Luminance, Perceptual/Relative) fill the
    selected segment with the system accent (purple). **Fixed**: neutral `SegmentedPicker`.
36. Selection panel chips: 22 pt, monospaced 10 pt keys, coloured outlines on every chip, marks
    as empty coloured squares. **Fixed**: 24 pt `DecisionChip`; on = semantic outline + tint; marks show a swatch.
37. Tool buttons (targeted-adjustment scope, straighten) are 22 × 20 with a 7 % white fill.
    **Fixed**: 20 pt `IconButton`.
38. Color Grading 3-way: "Lum" sliders crammed so "+0" runs into the next "Lum"; "Global" label
    floats above its slider. **Fixed**: range names as slider titles; Global aligned to its wheel.
39. Soft proofing labels embed shortcuts with double spaces ("Soft proofing  (S)"); the gamut
    swatch is the only colour well. **Fixed**: shortcuts in help tags; label column aligned.
40. Masks: the Masking switch uses the system accent; row selection is white 9 %; component
    glyphs 9 pt. **Fixed**: checkbox in the accent, accent-subtle row selection, 11 pt glyphs.
41. Keywords: "New…" is a link-blue button (a third accent); chips have "×" text glyphs.
    **Fixed**: borderless button; `xmark` symbol; dashed outline for mixed keywords.
42. Metadata: field labels 64 pt wide, file facts 96 pt wide in the same panel. **Fixed**: one 72 pt label column.
43. Histogram, curve and 1:1 preview wells are hard-coded 8 % grey with radius 3 / 4.
    **Fixed**: `plotWell` token (dark in both appearances, like scopes), radius 4.

### Compare (`before/compare.jpg`)

44. Panes are grey tiles with a 2 pt amber outline for the active side; 6 pt gaps.
    **Fixed**: neutral panes, 2 px accent ring, 12 pt gutter, 8 pt gap.
45. "SUGGESTED" in caps in the keep colour even when nothing is decided. **Fixed**: sentence case.

### Toast (`before/toast.jpg`)

46. Radius 7, #303030 at 96 %, "Undo ⌘Z" as one amber string. **Fixed**: HUD material, radius 8, "Undo" in accent + "⌘Z" tertiary.

### Sheets (`before/sheet-*.jpg`)

47. Five sheets, five header layouts (title sizes, subtitle placement, padding 12–16).
    **Fixed**: `SheetScaffold`: one header, hairlines, one footer.
48. Primary actions are inconsistent: Export / Print / Reject are grey, Smart Album "Create" is
    system purple. **Fixed**: 28 pt primary button in the accent, always rightmost; Cancel left of it.
49. Export: content clipped under the footer at 680 pt; the template field is monospaced next to
    capsule token buttons. **Fixed**: 720 pt; 20 pt token buttons.
50. Smart album: the AND / OR / NOT rails are coloured blue / amber / red — semantic colours
    reused for grammar; "−" text buttons. **Fixed**: neutral rail (dashed for NOT); `minus` icon buttons.
51. Print: "Margins: top … bottom … left … right" as a run-on row; hard-coded blue margin guide.
    **Fixed**: labelled 2 × 2 grid; guide in the accent.
52. Defect sweep: "All / None" link buttons; plain centred empty text. **Fixed**: borderless buttons; `EmptyStateContent`.
53. Lightroom import: warnings use the accent amber; ΔE badges are capsules with black text.
    **Fixed**: `warning` token with an icon; outlined chips.

### Global

54. `NSApp.appearance = .darkAqua` and `.preferredColorScheme(.dark)`: no Light appearance.
    **Fixed**: follows the system; View ▸ Appearance (System / Dark / Light).
55. 13 font sizes, bold weights, 8 radii, 18 spacing values (see Summary). **Fixed**: 6 sizes × 3 weights, 3 radii, 7 spacings, all in `Theme`; `ThemeLintTests` fails on raw literals.

## Limits

* Keyboard focus rings on native AppKit controls (text fields, pop-ups) follow the macOS accent
  the user picked in System Settings (purple on this Mac). There is no public per-app API short of
  an asset-catalog `AccentColor`, which the SwiftPM bundle does not have; it is also an
  accessibility preference. Documented in DESIGN.md §3.2.
* 2x captures: the only display attached is a 4K panel running at native 1x, and changing the
  display mode is a system setting, so every capture is 1x.
