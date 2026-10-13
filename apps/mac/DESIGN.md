# Tessera for Mac: design system

The single source of truth is `Sources/Tessera/App/Theme.swift` (tokens) and
`Sources/Tessera/App/Components.swift` (the shared controls). Views use nothing else:
`Tests/TesseraCoreTests/ThemeLintTests.swift` fails on raw colours, numeric padding / spacing /
radii, ad-hoc fonts, native segmented pickers and link buttons anywhere in `Sources/Tessera`.
The defects this system replaced are listed in `DESIGN_AUDIT.md`.

## 1. Identity

**A quiet, precise pro tool.** The photo is the loudest thing on screen; the interface is the
frame around it. Three ideas carry the identity:

* **Black and warm white.** The ground is true black (`#0A0A0A`) with two steps of near-black for
  panels and controls, and one warm-white ink family. Adobe's cool mid-greys, grey tiles and
  bevels are out. The person can choose a graphite or navy ground instead (§2.5).
* **Hairlines, not boxes.** Structure comes from 1 px separators and spacing. Panels have no
  borders, thumbnails have no tiles, cards are rare.
* **One accent, used sparingly.** The accent marks exactly three things: *content selection*
  (grid cells, list rows, history head), *focus* (the focused photo's ring, slider drag) and *the
  primary action* of a sheet, plus the agent mark. Controls' own modes (segmented controls,
  toolbar toggles) stay neutral.

Why amber by default: it is warm, like the ink, so the palette reads as one family; it is far
from every decision colour (sage keep, brick reject, slate basket) and from the macOS default
blue and Adobe's blue; and it carries continuity from the app's original focus ring. The accent
is the person's to change (§2.5); the decision colours are not.

Dark is the only appearance during the redesign (docs/design/redesign-2026-10/SPEC.md §2): View ▸
Appearance defaults to Dark. Every token still has a Light value, kept as it was until light
parity is restored.

## 2. Colour tokens

Hex values are sRGB. `Theme.Palette.*` are dynamic `NSColor`s (AppKit drawing); `Theme.*` are the
same colours as SwiftUI `Color`s. Contrast is WCAG 2 against the surface named. Dark values
below are for the default ground (Black) and accent (Amber); §2.5 lists the alternatives.

### 2.1 Surfaces

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `canvas` | `#0A0A0A` | `#E3E2DF` | Window ground: grid, compare, loupe surround, behind the photo |
| `panel` | `#161616` | `#F5F4F2` | Inspector, toolbar, filter bar, status bar, sheets, cards |
| `raised` | `#1C1C1C` | `#FFFFFF` | Fields, bordered buttons, selected segment, hover |
| `track` | `#2A2A2A` | `#CFCDC9` | Slider tracks, inactive toggles |
| `well` | `#0F0F0F` | `#E9E8E5` | Segmented tracks |
| `plotWell` | `#121211` | `#121211` | Scopes: histogram, curve, 1:1 detail (dark in both, like FCP scopes) |
| `hover` | white 6 % | black 5 % | Hover fill |
| `pressed` | white 11 % | black 9 % | Pressed fill, toolbar toggle on |
| `hairline` | white 8 % | black 11 % | 1 px separators, quiet outlines |
| `hairlineStrong` | white 14 % | black 18 % | Field and HUD outlines |
| `groupAlt` | white 3.5 % | black 4 % | Alternate burst groups in the grid |
| `hud` | canvas 86 % | `#FBFAF8` 92 % | Dial, mask chips, loupe tool bars, toast (with `hairlineStrong`, radius 14) |

`canvas`, `panel`, `raised`, `well` and `hud` follow the chosen ground (§2.5). The sidebar uses
the native source-list material (vibrancy) rather than a token.

### 2.2 Ink

| Token | Dark | Light | Contrast (on panel) | Use |
| --- | --- | --- | --- | --- |
| `textPrimary` | `#F2EFE9` | `#1D1C1A` | 15.8 / 15.5 | Titles, values, row text |
| `textSecondary` | `#B5B1AA` | `#5E5A54` | 8.5 / 6.2 | Values, labels, sub-headers, captions |
| `textTertiary` | `#8D8A84` | `#858079` | 5.3 / 3.6 | Hints, counts (never the only carrier of meaning) |
| `textQuaternary` | `#5A5852` | `#A19D96` | 2.5 / 2.5 | Disabled text, key caps |
| `textOnAccent` | `#1A1200` | `#FFFFFF` | 9.0 / 4.8 on accent | Primary button text; computed from the accent's luminance (§2.5) |

### 2.3 Accent and semantics

| Token | Dark | Light | Contrast (on panel) | Use |
| --- | --- | --- | --- | --- |
| `accent` | `#F0A52A` (user-set) | `#A8620C` | 8.7 / 4.5 | Selection, focus, primary action, the agent mark |
| `accentSubtle` | accent 18 % | accent 14 % | – | Selection fill |
| `keep` | `#6F9A60` | `#2F7A46` | 5.6 / 4.8 | Keep, grades (decisions only) |
| `reject` | `#C0584A` | `#B23B2F` | 4.1 / 5.4 | Reject, errors (decisions only) |
| `basket` | `#88A8CE` | `#3B6699` | 7.4 / 5.4 | Basket target album |
| `warning` | `#D8B55C` | `#8A690B` | 9.2 / 4.7 | Warnings (always with an icon) |

Semantic colours are desaturated to the same lightness family so no decision shouts. They are
never used for chrome, grammar or mode (the smart-album rule rails used to be blue / amber / red;
they are neutral now).

**On-image set** (`Palette.OnImage`, one set for both appearances, because it sits on photos):
keep `#80B38C`, reject `#D9786B`, basket `#87A8CF` fills with ink `#141312` (≥ 6:1); scrim
`#0F0E0D` at 62 % with text `#EDEBE6` for hints and status chips; guides white 90 % / 30 %.

**Marks** (keys 6–9, appearance-independent): rose `#CF7FA8`, citron `#CCBF62`, teal
`#62B5AF`, lilac `#A394D6`.

**Plots**: channel red `#E85C52`, green `#6BCC6E`, blue `#668FFA`; plot line `#E6E4E0`.

### 2.4 Focus rings

Custom controls show focus with the accent. Native AppKit fields and pop-ups draw the macOS
focus ring in the user's system accent: that is an accessibility preference and there is no
per-app override without an asset-catalog `AccentColor`, so it is left alone.

### 2.5 Appearance settings (SPEC §6)

Settings ▸ Appearance holds two choices, both persisted in user defaults and applied live
(`AppearanceSettings` in Theme.swift; `AppearanceSettingsView` in Settings/):

* **Accent**: six swatches or any colour from the system colour well. Default Amber `#F0A52A`;
  Coral `#FF7A66`, Cobalt `#4F7BFF`, Teal `#2FB3A6`, Rose `#E8709A`, Lime `#A6D24A`. The
  choice sets `accent` and `accentSubtle`; `textOnAccent` is computed from the accent's
  luminance (the warm ink `#1A1200` or the warm white `#F2EFE9`, whichever contrasts more; pure
  black or white if neither clears 4.5:1, so any colour stays legible). Keep and reject never
  follow the accent.
* **Ground**: Black (default) `#0A0A0A` / `#161616` / `#1C1C1C`, Graphite `#131312` / `#1C1B1A`
  / `#272624` (the pre-redesign set), Navy `#12151C` / `#1A1E27` / `#232833`, as canvas / panel
  / raised; `well` and `hud` follow. The loupe surround is the chosen canvas in linear light.

Views name tokens only. The ground- and accent-dependent `Theme.*` / `Theme.Palette.*` tokens are
computed properties that read the settings object: a SwiftUI body that uses them observes the
choice, and AppKit views get their `viewDidChangeEffectiveAppearance` pass and a redraw when it
changes. The Light values of these tokens are the pre-redesign ones and do not follow the choice.

## 3. Layout

### 3.1 Spacing: 8-pt grid, 4-pt micro steps

| Token | pt | Use |
| --- | --- | --- |
| `Space.hairline` | 1 | Separators, outlines |
| `Space.xxs` | 2 | Chip-to-edge on filmstrip, focus ring width |
| `Space.xs` | 4 | Icon-to-label, chip gaps, row micro spacing |
| `Space.s` | 8 | Control gaps, filter bar rhythm |
| `Space.m` | 12 | **Gutter**: panel edges, inspector content, grid edge, status bar |
| `Space.l` | 16 | Sheet edges, section bottom padding |
| `Space.xl` | 24 | Print preview inset |
| `Space.xxl` | 32 | Empty states |

Grid cells carry a 6 pt inset (`s − xxs`) and 4 pt spacing, so photos start on the 12 pt
gutter and sit 16 pt apart.

### 3.2 Radii

| Token | pt | Use |
| --- | --- | --- |
| `Radius.chip` | 4 | Chips, badges, inputs, list thumbnails, plot wells |
| `Radius.control` | 6 | Buttons, menus, segmented controls, grid selection, sidebar selection |
| `Radius.card` | 8 | Bars, toasts, cards, sheets |
| `Radius.field` | 12 | The agent field |
| `Radius.hud` | 14 | HUD: the dial, mask chips, toasts over the photo |
| `Radius.pill` | 999 | Pill chips |

No capsules on buttons. Circles only for slider thumbs, colour wells, accent swatches and dots.

### 3.3 Heights

| Token | pt | Use |
| --- | --- | --- |
| `Height.chip` | 16 | Badges on thumbnails and in chrome |
| `Height.small` | 20 | Inspector buttons and menus, facet menus, panel segmented controls |
| `Height.regular` | 24 | Search field, decision chips, sidebar rows, list rows, status bar |
| `Height.large` | 28 | Toolbar items, sheet footer buttons, HUD tools, sidebar headers |
| `Height.sectionHeader` | 32 | Inspector section headers, the loupe information strip |
| `Height.slider` | 42 | One slider row: `sliderText` 16 (label + value) over `sliderTrack` 26 (4 pt track, 16 pt thumb) |
| `Height.histogramStrip` | 40 | The Edit panel's histogram strip |
| `Height.filmstrip` | 72 | Filmstrip |

Widths: sidebar 200–300 (ideal 220), inspector 288–380 (ideal 296), label column 72.

### 3.4 Window structure

One toolbar (real `NSToolbar` via SwiftUI `.toolbar`, flat — no per-item glass on macOS 26), the
only bar (redesign SPEC §1, WP UX-11): Open Folder and the folder name (leading; the name opens
the source sidebar's content — folders, albums, people, culling lists — as a popover) ·
**Cull · Edit · Deliver** segmented (centre; ⌘1 ⌘2 ⌘3) · the search field (trailing; widens while
it has the keyboard, the facet menus hang off its filter button as a popover), thumbnail size
(Grid only), the panel toggle (Tab) and Export. Review stays reachable from the View and Develop
menus. Below: content (canvas) with the agent field bottom centre (`Shell/AgentField.swift`:
560 × 48, sparkle mark, the mode's placeholder, mic, ⌘K; in Edit the Masks / Crop / Heal strip
beside it) · progress strips · status bar · filmstrip. The source sidebar is hidden by default
(View ▸ Show Library Sidebar, persisted); the inspector is the one panel, hidden and shown by Tab
with the state remembered per mode (`Shell.panel.<mode>` in user defaults). With the panel hidden
in Edit the photo fills the window. Deliver covers the canvas with the export entry until UX-15.

**Cull** (SPEC §4, WP UX-14; `Cull/TriageColumn.swift`, `Cull/BurstReviewView.swift`): the panel
is the 260 pt triage column on the *left* (`Theme.Width.triageColumn`; Tab hides it; the right
inspector is for the loupe, Compare, Edit and Review). The centre is one burst: the focused
frame large (outlined by the accent when it is the pick, keep / reject once decided), an accent
"Agent's pick" pill top-left, the name top-right in a HUD pill, the signals bottom-left as HUD
pills (`hud` fill, `hairlineStrong` outline, capsule): a 48 pt sharpness bar, eyes, face, and
the score in the accent. The strip below is 172 × 116 cells (`Theme.Width.burstCell` ×
`Theme.Height.burstCell`, radius 4): a 2 pt accent focus ring, a filled accent **Pick** chip, an
18 pt keep / reject disc with P / X, name and score on a scrim. Actions are 36 pt
(`Theme.Height.burstAction`) with the key cap inside the button: the primary one is the K action.
The quick-action chips above the agent field are capsules (`panel` fill, `hairline` outline, 28 pt).
The grouped grid keeps the Library grid's cells; each burst's row block has a 28 pt header in
`labelMedium` secondary ink, tabular detail in tertiary ink and an accent "review" link.

### 3.5 Layout contracts (WP M2-56)

Tokens make controls look alike; these rules make them fit. `Shell/ShellLayout.swift` holds the
shell's budget and `ShellLayoutTests` checks it at 960 × 600 (the declared minimum content size),
1280 × 800, 1440 × 900 and 1728 × 1117, light and dark.

- **The window is the budget.** Each split-view column is a `containedColumn()`: it takes the size
  the split view gives it and clips (or scrolls) its own overflow. No subtree may raise the window's
  minimum, sit at a negative origin or slide under the toolbar.
- **Yield order when the window shrinks:** 1) the library sidebar collapses (the person's choice
  returns when it fits again); 2) the inspector narrows towards its 288 pt minimum; 3) the filmstrip
  hides when the canvas above it would be shorter than 320 pt. Below 1280 pt the toolbar's text
  buttons show icons only (titles stay in help and accessibility).
- **Text in a constrained row** truncates at a measured width (`lineLimit(1)` + a truncation mode,
  full text in help); never a character count, never two-axis `fixedSize()` on a text or menu group.
  Pop-up menus hug their title and give width back (`hugCompressible()`); chevrons, icons and
  numeric readouts do not compress.
- **Label / value pairs** reserve a gap (`Space.s`); the value is always whole, the label truncates
  (ValueSlider).
- **Rows with optional parts** remove them (`if`), not `opacity(0)`; AppKit rows collapse the
  constraint and its gaps when a part is hidden.
- **Status and action rows** offer a compact variant (`ViewThatFits`): the full row when it fits,
  else short labels or icons; a message never decides which variant fits (ideal width 0).
- **Custom drawing** budgets the whole painted envelope: stroke halves, handle radii, text rects.

## 4. Type

SF Pro only, at six sizes and three weights (regular, medium, semibold; never bold). Every
numeric readout uses monospaced digits (`.monospacedDigit()` / `monospacedDigitSystemFont`).
SF's built-in size-specific tracking is used as is; the only manual tracking is −0.3 on 22 pt.
No uppercase labels: sentence or title case everywhere.

| Size | Token (SwiftUI / AppKit) | Weights | Use |
| --- | --- | --- | --- |
| 11 | `Fonts.caption*` / `NSFonts.caption*` | R / M / S, numeric, mono | Inspector labels and values, chips, captions, status bar, hints |
| 12 | `Fonts.label*` / `NSFonts.label*` | R / M / S, numeric, mono | Buttons, sidebar rows, section headers, filter field |
| 13 | `Fonts.body*` / `NSFonts.body*` | R / M / S, numeric | Edit panel slider labels and values, change cards, the agent field |
| 15 | `Fonts.title` | S | Sheet titles, Edit panel section titles |
| 17 | `Fonts.headline` | S | Empty-state titles |
| 22 | `Fonts.display` | S, −0.3 tracking | Reserved (large numerals, onboarding) |

## 5. Components

All in `Components.swift` unless noted.

**Buttons** (`ThemeButtonStyle`): rectangular, radius 6, heights 20 / 24 / 28.
*Bordered*: raised fill, `hairlineStrong` outline. *Borderless*: no fill until hover. *Primary*:
accent fill, `textOnAccent`, one per sheet, rightmost. *Destructive*: borderless with reject ink.
States: hover fill (`hover`), pressed fill (`pressed`), disabled 40 % opacity, keyboard focus ring.
`.sheetButton(primary:)` is the 28 pt sheet footer variant.

**Toolbar items** (`ToolbarButtonStyle`, `ToolbarToggleStyle` in ContentView): 28 pt, flat,
icon + label; toggle on = `pressed` fill + primary text.

**Segmented control** (`SegmentedPicker`): `well` track with a hairline, selected segment raised
with a 1 pt shadow; 20 pt in panels, 24 pt in the toolbar and sheets. Neutral, never accent.
Segments may be icon-only when width is tight (Color Grading ranges), with the name in the help tag.

**Menus** (`ThemeMenuStyle`, `IconMenuStyle`): bordered pull-down with a tertiary chevron; active
filters use the accent-subtle fill and a 50 % accent outline.

**Slider** (`ValueSlider`, AppKit `NSControl` on the < 16 ms path): 42 pt row = a 16 pt text
line (label 13 pt primary left, value 13 pt tabular right: secondary at the default, accent +
medium once changed) over a 26 pt track row; 4 pt track in `track`, fill from the default to the
value in `textSecondary` (accent while dragging); a 1 px tick marks the default of bipolar
controls; 16 pt disc thumb with a hairline and soft shadow, 2 px accent ring while dragging.
`dialHighlighted` (the loupe dial targets this control) paints the row `accentSubtle` with an
accent hairline. Drag is relative, ⌥ is 10× finer, double-click resets, a click on the track
away from the thumb jumps there.

**Chips** (`Chip` in SwiftUI; `BadgeOverlayView.drawChip` on thumbnails): one family — 16 pt
(on images) or 20 pt (panels), radius 4, 11 pt medium, sentence case, 6 pt side padding.
*Filled* = a fact about the photo (decision, grade, mark, basket). *Outlined* (on a scrim over
images) = a hint or derived status (Suggested, Edited · In 2 albums, ΔE). Single-glyph chips are
square (marks, filmstrip). The filmstrip shows single glyphs only (K, 1–3, X, B, 6–9).

**Decision chips** (`DecisionChip`, Selection panel): 24 pt, radius 6; off = raised + hairline;
on = semantic outline at 70 %, 16 % tint and semantic ink. Key caps in tertiary tabular type.

**Sidebar rows** (`SidebarCell`, `SidebarRowView`): source list with vibrancy, 24 pt rows, 28 pt
title-case headers, 8 pt swatch, 12 pt title, tertiary tabular count right-aligned, selection
fill `accentSubtle` at radius 6.

**Inspector sections** (`PanelSection`): 32 pt header, 12 pt semibold title (primary when open,
secondary when closed), a chevron that rotates 90°; content at the 12 pt gutter with 16 pt
bottom padding; a hairline between sections. `SubHeader` groups controls inside a section
(11 pt medium secondary, 12 pt above, 4 pt below). `InfoRow` uses the 72 pt label column.

**Icon buttons** (`IconButton`): 20 / 24 / 28 pt square, radius 6; on = accent glyph over
`accentSubtle`.

**Fields**: native rounded text fields at `.small` in panels; the rule field sits in
`FieldContainer` (24 pt, radius 6, reject outline when the rule has an error).

**Sheets** (`SheetScaffold`): header (15 pt semibold title, 11 pt secondary subtitle, optional
trailing accessory), hairline, content, hairline, footer (secondary text left, actions right,
primary rightmost). 16 pt edges, `panel` background, accent tint for native controls.

**Toast** (`ToastView`): HUD material, radius 8, 12 pt message, borderless "Undo" in the accent
with the ⌘Z key cap in tertiary. Enters from and exits to the bottom.

**Progress strips** (`ProgressStrip`): 32 pt, hairline above, title · bar · counts · current
file · Cancel.

**Empty states** (`EmptyStateContent`): 22 pt symbol in tertiary, 17 pt title, 12 pt message,
28 pt actions with the primary first.

**HUD bars** (`HUDBackground`): mask toolbar and brush settings; 8 pt radius, hairline, soft
shadow; kept below the loupe's 32 pt information strip so they never cover its text.

**Grid cells** (`ThumbnailCell`): photo on the canvas; selection = accent-subtle fill at radius
6; focus = 2 px accent ring; rejected photos at 35 % opacity; caption = 11 pt name (secondary) and
11 pt tabular group (tertiary) on one baseline.

**Assist pills** (`BadgeOverlayView`): a pre-filled decision is an *outlined* chip over the scrim
(`Keep?` in keep ink, `Reject?` in reject ink; `K?` / `X?` on the filmstrip), never a filled
decision chip, until Y confirms it. **Face strip** (`FaceStrip`, loupe only): a filmstrip-height
panel row under the loupe; square close-ups at radius 4 with a hairline (accent on hover), a focus
dot in keep / warning / reject and an eyes glyph whose *shape* also changes (eye, eye with warning,
eye slashed), plus a text legend so colour is never the only signal; click opens a popover.
**Agent groups** (History panel): an outlined `AI` chip in the accent, the group name and its
amount, a `ValueSlider` "Amount" (0–100 %) and per-step checkboxes with the rationale in
secondary ink; the review queue and Auto Edit sheet use `SheetScaffold` and the confidence chip
(warning below 40 %, secondary to 70 %, keep above).

**Tether panel** (`TetherPanel`, File ▸ Tethered Capture…): docked under the filter bar on `panel`
with a hairline below, not a sheet, so culling continues beside it. 32 pt header (title, `Test camera` outlined chip in
warning ink for the test aid, connection dot + camera, battery / frames-left readouts), session and naming fields in
`FieldContainer` (reject outline and an inline `StatusLine` when the template is invalid, a mono live example otherwise),
one accent **Capture** (the panel's primary action) and neutral interval menus, then the **Incoming** strip at filmstrip
height: 3:2 tiles at radius 4 with a HUD badge bar (sequence, focus dot keep / warning / reject, eyes glyph only when
faces were found), a filled decision chip once decided, rejected tiles at 35 %, accent ring on the focused frame and
`Downloading` placeholders for shutter requests on their way.

**People view** (`PeopleView`, sidebar ▸ People): on the canvas, a 32 pt panel header (title, tabular counts, a
borderless **Refit**); tiles like grid cells (no box: face crop at radius 4 with a hairline, accent on hover; selection =
accent-subtle fill at radius 6; a drop target adds a 2 px accent ring), the name in 12 pt medium or a small native name
field, a tertiary tabular photo count and an outlined `Confirmed` chip in keep ink. The detail view reuses the header
(back chevron, name field, counts, bordered **Confirm All** / **Split**), 96 pt face chips with a confirm seal on a HUD
square (keep ink when confirmed) and a `panel` **Move to** column of 28 pt drop-target rows. The toolbar's **Merge** is a
plain toolbar button. The sampled-clustering footnote is a 24 pt tertiary caption with an info glyph.

**Scopes** (`HistogramView`, `CurveEditorView`, `DetailPreviewView`): `plotWell`, radius 4,
channel colours composited additively. The Lens Blur **focal range strip** (`FocalRangeStrip`) is a scope
too: `plotWell`, a near → far ramp from `plotGuide` to `plotGrid`, the in-focus band in `accentSubtle`
(it is a selection) between two 2 pt `plotLine` handles, `Near` / `Far` in 11 pt `plotText`.

**Export sheet additions** (M2-46): no new colour, size or font. The format bar is the sheet's 24 pt
`SegmentedPicker` with six segments; format-specific rows (Quality, Bit depth, AVIF Speed, JPEG XL "Lossless", DNG
"Linear 32-bit float") follow it, and anything the engine cannot do is a disabled native control with a `Hint` giving
the reason (HDR, lossy JPEG XL, colour space for JPEG XL / DNG) or a warning `StatusLine` (no watermark on DNG). The
Watermark section uses a None / Text / Graphic `SegmentedPicker`, native fields, `ColorPicker` (the user's colour is
data) and the **anchor grid** (`AnchorPicker`): a `well` track with a hairline, 3 × 3 cells, the chosen cell raised
with the 1 pt shadow and a `textPrimary` dot, the others a `textTertiary` dot (neutral, like segmented controls). The
**placement preview** is a 240 × 160 scope well (`plotWell`, radius 4, `plotGuide` hairline frame) with the watermark
drawn in its own font and colour, or the engine's 480 px render once requested, and an on-image scrim chip naming which.

**Photo Merge and Enhance sheets** (M2-50): no new colour, size or font. Both are `SheetScaffold` sheets. Photo
Merge puts a 400 × 300 **merge preview well** (`plotWell`, radius 4, `plotGuide` hairline frame while empty, the
engine's ≤ 512 px JPEG fitted inside, an on-image scrim chip `Engine preview · W × H`, 60 % opacity while re-rendering
with a small spinner in the header) left of a vertical hairline and the options column: `SubHeader` groups, native
checkboxes, 24 pt `SegmentedPicker`s for Deghost, Projection and frames per bracket, a sheet `ValueSlider` for Boundary
Warp. Engine and exposure-spread warnings are warning `StatusLine`s under the preview; the engine's notes (approximate
preview, chosen projection) are `Hint`s. The footer's leading slot says why Merge is dimmed, else names the output.
Enhance is a single column in the same grammar; Raw Details is a disabled checkbox with its reason, and the missing
model error is an error `StatusLine`. A running job is a `ProgressStrip` above the status bar (indeterminate while a
model downloads, because the engine reports only start and ready).

**Transform / Lens Blur panels** (M2-48): the Upright bar (`UprightModeBar`) is a 20 pt
`SegmentedPicker`-look track with six icon segments; the chosen segment adds its name (the only way six
modes fit the 288 pt inspector). Per-group resets are borderless 20 pt **Reset** buttons on the
`SubHeader` line (`GroupHeader`). Controls the engine cannot render yet stay visible but disabled
(40 %) with a `StatusLine` warning naming the gap. Guided Upright guides in the loupe use the on-image
set: 1.5 pt `OnImage.guide` over a 3 pt `OnImage.shadow`, dashed (4 / 3) while drawing, the selected
guide in the accent, 7 pt square ends in `OnImage.text` with an `OnImage.ink` outline, and the
bottom scrim hint.

**Model acquisition, Lens Blur depth tools** (M2-51) add no colour, size or font. A control that needs pinned
weights acquires them on first use and shows the state inline under itself (`ModelProgressRow`): a small linear
`ProgressView` (determinate once the size is known) over a `captionNumeric` line in `textSecondary`
(`Depth model: Downloading 12 MB of 99 MB`); a missing / unobtainable model is a warning `StatusLine` with the
reason and a borderless 20 pt **Retry**. The recipe changes only once the model is ready. Settings ▸ AI has a
**Develop models** section with **Allow model downloads** (default on). Lens Blur's eight apertures use a
`MenuPicker` pop-up (too many for a `SegmentedPicker`); the depth histogram is drawn in the Focal Range scope well as
`plotLine` bars at 35 % behind the band; **Subject** is a bordered 20 pt button beside the **Visualize Depth**
checkbox; a busy estimate is a mini spinner with a caption. While Guided Upright is armed the loupe shows the
uncorrected frame (session only) with a `Hint` saying so. Remaining gaps (Refine brushes, Constrain Crop) keep the
disabled-with-`StatusLine` rule above.

## 6. Motion

* 150–200 ms ease-out for things that appear (toast, popovers); `Theme.Motion.appear`.
* Enter and exit along the same edge (`Theme.Motion.transition(from:)`); with Reduce Motion the
  transition is an opacity fade only.
* No animation on anything keyboard-driven or repeated hundreds of times a session: culling,
  navigation, mask tool toggles (M), view mode changes. The previous 120 ms mask-toolbar
  animation was removed for this reason.
* Hover and pressed states change fills instantly (no transition), on pointer-down.

## 7. Density

Pro density: 11 pt as the working size in panels; 32 pt slider rows; 24 pt list rows; chrome on
thumbnails only when a fact is set (decision, mark, basket, derived status) or the frame is the
group's suggested best; the filmstrip at 72 pt; a one-line status bar in three groups.

## 8. Do and don't

| Do | Don't |
| --- | --- |
| `Theme.Space.gutter` for panel edges | `.padding(14)` |
| `.buttonStyle(.theme(.bordered, height: Theme.Height.small))` | `.controlSize(.small)` on a system capsule button |
| `SegmentedPicker` | `.pickerStyle(.segmented)` (accent-filled system capsule) |
| `Chip(text: "Keep", color: Theme.keep, style: .outlined)` | `Text("KEEP").font(.system(size: 9.5, weight: .bold))` |
| Accent for selection, focus, the one primary action | Accent for toggle state, warnings, grammar, links |
| `Theme.warning` + an icon for warnings | Colour alone as the signal |
| `Hairline()` | `Divider().opacity(0.5)`, white 8 % strokes |
| Sentence / title case | UPPERCASE tracked micro-labels |
| `Theme.Palette.OnImage.*` for anything drawn over a photo | Dynamic panel colours on a photo |
| `NSColor.cgColor(for: view)` when setting a layer colour | `Theme.Palette.x.cgColor` in `init` (misses appearance changes) |

## 9. Implementation notes

* `Theme.Palette.dynamic(dark:light:)` builds an `NSColor(name:dynamicProvider:)`; layer-backed
  AppKit views resolve it in `updateLayer` / `draw` or with `cgColor(for:)` and refresh in
  `viewDidChangeEffectiveAppearance`.
* `Theme.loupeBackgroundLinear` is the canvas colour in linear light for the current appearance;
  the Metal presenter reads the token on each draw (presentation code unchanged).
* `AppearancePreference` stores View ▸ Appearance in user defaults (default Dark);
  `--appearance dark|light` overrides it for one run (screenshots).
* `AppearanceSettings.shared` (Theme.swift) holds the accent (`AccentColor`, 0xRRGGBB hex) and
  the ground (`Ground`) in user defaults and rebuilds its `tokens` on every change; the
  ground- and accent-dependent `Theme.Palette` tokens are computed properties over it (§2.5).
* Engine readouts (render timing, the open summary's RAW / group counts and scan time) appear in
  the status bar only with Debug ▸ Show Engine Readouts (`ShowRenderReadout`), SPEC §7.
* Menu titles never carry a shortcut in parentheses; single keys route through `KeyRouter` and
  are listed in docs/shortcuts.md (`tools/orchestrate/shortcut-audit.py --write-doc`).
* Tooltips appear after 250 ms: `NSInitialToolTipDelay` is registered at launch.
* The toolbar opts out of macOS 26's per-item glass with `sharedBackgroundVisibility(.hidden)`
  (`flatToolbarItem()`); macOS 15 ignores it.

## 10. Document mode (WP B5-02)

Layered documents reuse the system above; nothing here adds a colour, size or font.

* **Toolbar**: a fourth segment, **Layers**, in the view-mode control. In document mode the
  navigation area holds the **document tabs**: a `well` track with a hairline like
  `SegmentedPicker`; the current tab is raised with the 1 pt shadow, 11 pt title (medium when
  current), a 6 pt `textSecondary` dot for unsaved changes, and an `xmark` close glyph on the
  current or hovered tab; a `+` opens New Document.
* **Viewport**: the `canvas` surround; the transparency checkerboard alternates `checkerLight`
  (`thumb`) and `checkerDark` (`plotText`), aliases of existing tokens, in 8 pt squares, light
  in both appearances as in every image editor. The marquee's marching ants are the on-image
  pair (`OnImage.text` under `OnImage.ink` dashes, 4 / 4, 30 fps). Tools (Move, Rectangular
  Marquee) sit in a top-left HUD bar of 28 pt `IconButton`s; the zoom percentage appears in a
  HUD chip at the bottom for 1.2 s after each zoom change.
* **Inspector**: Properties (a `PanelSection` in its own scroll view), Layers (fixed 32 pt
  header, then the panel), History (`PanelSection`). The Layers panel: blend mode as a 20 pt
  `ThemeMenuStyle` pop-up (the 27 modes in Photoshop's six groups with dividers, Pass Through
  first for groups), Opacity and Fill `ValueSlider`s side by side, 20 pt lock `IconButton`s
  (on = accent glyph over `accentSubtle`), a dimmed filter field placeholder; the outline;
  a 28 pt footer of icon buttons (add, mask, adjustment left; group, delete right).
* **Layer rows** (`NSOutlineView`, 32 pt, 12 pt indentation per level): eye (secondary; slashed
  and tertiary when hidden, the name tertiary too), the clipping glyph for clipped layers,
  24 pt thumbnail on the checkerboard at radius 4 with a `hairlineStrong` outline (adjustment
  layers show their SF Symbol instead), a link glyph and the mask thumbnail (a 2 pt `reject`
  cross when the mask is off), 12 pt name, tertiary kind and lock glyphs. Selection is the
  list-row fill: `accentSubtle` at radius 6.
* **History rows** follow the develop History panel: 24 pt, current row `accentSubtle`, undone
  states in tertiary ink; snapshots below with borderless Restore; the memory line in tertiary
  tabular type.
* **Status bar** in document mode: canvas size · depth · profile | zoom | tool | selection size,
  then the message.
* **Tools (WP B5-04)**: the tools palette is a vertical HUD column of 28 pt `IconButton`s at the canvas's top left, one
  slot per tool group in Photoshop's order, then the foreground / background swatches (user colours, radius 4,
  `hairlineStrong` outline) with swap and default. The options bar is a HUD bar beside it (scrolls sideways when
  narrow): the tool's icon and name, then compact small native fields with 11 pt captions, checkboxes, the neutral
  selection-mode `SegmentedPicker` (icons) and borderless actions. On-canvas feedback uses only the on-image set:
  marching ants (`OnImage.text` under `OnImage.ink` 4 / 4 dashes), brush outline and guides in `OnImage.guide` over
  `OnImage.shadow`, hardness ring and symmetry guides in `OnImage.guideFaint`, square transform handles in
  `OnImage.text` with `OnImage.ink`, and the HUD readout as a scrim chip. Select and Mask previews tint outside the
  selection with `OnImage.reject` at 50 % (Overlay), `OnImage.ink` (On Black) or `OnImage.text` (On White).
* **Filter dialogs** (WP B5-05) are `SheetScaffold` sheets: the filter name as title, the layer as
  subtitle; content is a 180 pt 1:1 detail pane on `plotWell` (radius 4, a `1:1` chip in the
  on-image pair) left of the controls generated from the filter's schema: `ValueSlider` rows for
  numbers (unit in the value), a 28 pt angle dial (`well`, hairline, `textPrimary` needle) beside
  the slider for angles, `SegmentedPicker` for up to three choices and a `ThemeMenuStyle` pop-up
  beyond, a `well` point pad with a hairline cross for centres, checkboxes for toggles. Footer:
  Preview checkbox left; Reset, Cancel, OK (primary) right. Errors are an inline `StatusLine`.
  Image ▸ Adjustments sheets host the Properties editors unchanged. No new colours or sizes.
* **Smart filter rows** sit under their smart object in the Layers outline (last applied on top):
  eye (tertiary when off, name tertiary too), a 20 pt mask thumbnail on the checkerboard, 11 pt
  name (with mode and opacity when not Normal 100 %), and a blending-options glyph; double-click
  re-opens the filter dialog. They are not selectable as layers.
* **Channels (WP B5-08)** is a collapsible `PanelSection` between Layers and History. Rows are 32 pt like layer rows:
  eye (secondary; slashed and tertiary when hidden), a 24 pt thumbnail at radius 4 with a `hairlineStrong` outline
  (grey planes; the RGB row shows the composite), 12 pt name (double-click renames in place), and on the right a
  tertiary lock glyph on the read-only RGB / Red / Green / Blue rows or, on spot rows, a 12 pt ink swatch (the user's
  colour, radius 4, `hairlineStrong`). The highlighted channel is the list-row fill (`accentSubtle`, radius 6); the
  Quick Mask channel carries an outlined `Temporary` chip. Footer (28 pt): load as selection, save selection, Quick
  Mask (an `IconButton`, on while active), then new channel / spot channel and delete. The Save / Load Selection,
  Channel Options and New Spot Channel sheets are `SheetScaffold` forms with the 72 pt label column, a small native
  channel pop-up and native radio groups for the operation; spot controls always carry the `Hint` that spot colour is
  preview-only. The canvas preview uses the user's channel colours (alpha default red 50 % over masked areas, spot
  ink at its solidity) and `OnImage.ink` behind a single visible channel when the colour components are hidden.
* **Remove tool and neural filters** (WP B5-09) add no colour, size or font. The Remove tool is a 28 pt palette
  `IconButton` directly under the Healing Brush (⇧J switches between them). Its options bar follows the tools bar:
  Size and Expand fields, a neutral `SegmentedPicker` for Auto / PatchMatch / LaMa (a tertiary "LaMa not installed"
  caption when Auto or LaMa cannot use it), borderless Remove Selection and Remove Distractions…; while an apply runs,
  a small spinner, the elapsed seconds and a bordered Cancel; errors are an inline `StatusLine` (warning for a missing
  model, error otherwise). On the canvas only the on-image set: the stroke in progress is an `OnImage.reject` band at
  55 % and brush width; distraction suggestions under review are boxes, accepted ones a 1.5 pt `OnImage.guide` over a
  3 pt `OnImage.shadow` with a 22 % reject tint inside, kept ones a dashed `OnImage.guideFaint`, each with a scrim
  chip naming it ("Wire-like line", "Face box"). The review bar says what the detector is: geometric suggestions, not
  person segmentation. **Neural Filters…** is a `SheetScaffold` sheet: a 220 pt list of filters (the chosen one on
  `accentSubtle`, an outlined warning chip "No model" where weights are missing), a hairline, then the
  chosen filter's `ValueSlider` rows, an Output `SegmentedPicker` with only the outputs the layer allows (the others
  listed in tertiary with the reason), face-box and limitation notes in caption type, and for a missing model a
  warning `StatusLine` naming the model with its source URL and cache path in selectable text. Footer: Reset, Cancel,
  Apply (primary, disabled while the model is missing). Nothing offers a download.
* **Layer Style** (WP B5-07) is a floating panel (`NSPanel`, utility style), not a modal sheet: edits apply live
  and each gesture is one history node, so there is no Cancel. Its content uses `SheetScaffold` (title *Layer
  Style*, the layer as subtitle, a `Locked` outlined warning chip when Lock All is on; footer: effect count left,
  **Done** primary right). Left, a 216 pt `panel` column: *Blending Options*, a hairline, then every effect kind
  in the engine's stacking order (top first) as 24 pt rows (checkbox, 12 pt title; absent kinds in secondary
  ink, hidden effects in tertiary; the selected row `accentSubtle` at radius 6; repeatable kinds carry a 20 pt
  "+" `IconButton`), and a 28 pt footer with an `fx` add menu and delete. Right, the editor generated from
  `style_effects_schema_json()` exactly like filter dialogs: `ValueSlider` rows, the 28 pt angle dial beside
  angles, colour wells, `ThemeMenuStyle` pop-ups for blend modes and long choices, `SegmentedPicker` up to three.
  Contour / jitter / texture appear only as `InfoRow`s under a "Kept, not rendered" `SubHeader`, never as
  controls. The Global Light panel is the same scaffold with the dial and Angle / Altitude sliders.
* **Styled layer rows**: an `fx` SF Symbol glyph after the name (secondary ink, like the kind glyph); under
  non-group layers, effect rows as smart filter rows are drawn (eye, 11 pt name, tertiary when hidden), after any
  smart filter rows. The Properties panel lists effects under a *Layer Style* `SubHeader` (eye glyph, 11 pt name,
  tertiary size / angle readout) with a bordered *Edit…*. No new colours or sizes.
* **Type tool and text (WP B5-10)**: on canvas only the on-image set: the text frame (point text: dashed layout bounds;
  area text: the box, dashed too since B5-10c) in `OnImage.guide` over `OnImage.shadow`, eight square box handles like the transform
  handles, the selection as the accent at 35 % over the glyph boxes (content selection), marked IME text underlined in
  the accent, the caret a 1.5 pt accent line over a 3 pt `OnImage.shadow` line, blinking. The options bar holds the
  new-text font pop-up, size field and the neutral alignment `SegmentedPicker` (icons), then borderless Cancel and
  bordered Apply while editing. Properties ▸ Character / Paragraph / Text box are `SubHeader` groups: 72 pt label
  column rows with `ThemeMenuStyle` pop-ups (font, style), `ValueSlider` rows (units in the value, `(mixed)` in the
  title when a selection mixes values), the minimal colour well and a kerning checkbox; the alignment segments; the
  box kind as `InfoRow`s with a borderless convert action. Limitations are `warning` triangle + secondary caption
  lines at the top. Document mode's detail column has a 384 pt minimum so the sidebar and the inspector (288 pt
  minimum, unchanged) always fit the window; the status bar's canvas label truncates in the middle.
* **Type tool fixes (WP B5-10c)**: the text frame is dashed 4 / 3 (1 pt `OnImage.guide` over a 2 pt `OnImage.shadow`)
  for point and area text alike; box handles stay solid squares. The selection highlight covers each cluster's
  **advance box** (origin to advance, ascent to descent of the line), not the ink: it extends past the glyph ink by
  the side bearings (≈ 2–3 canvas px each side for 48 px Helvetica, ~5 device px at 200 %). This is intended and
  matches Photoshop; verifiers should not flag it. A click resumes an existing point text within its line boxes
  widened by a trailing margin of half the line height (at least 12 pt on screen) on both horizontal ends and 4 px
  vertically (`TextHitRegion`); area text within its box + 4 px. The status bar hint is derived from the session
  (`Point text: …` / `Area text: …` / the limitation) whenever it changes. The options-bar latency readout reads
  `Keystroke → rendered frame: median … · p95 … (n keys, inactive time excluded)`.
* **Shapes, Pen and vector masks (WP B5-11)** add no colour, size or font. Palette: Pen, Path / Direct Selection and a
  shape slot (Rectangle, Ellipse, Polygon, Line) as 28 pt `IconButton`s. Options bar: fill / stroke checkboxes with
  small colour wells, compact fields (Width, Radius, Sides, Inset, Weight), and a tertiary caption with the modifier
  keys; Path Selection adds the "Move vector mask with shape" checkbox and a Combine pull-down. On the canvas only the
  on-image set: paths and the affine box as `OnImage.guide` over `OnImage.shadow`, anchors as 7 pt squares (hollow
  `OnImage.text`, filled `OnImage.guide` when selected, `OnImage.ink` outline), direction points as 6 pt circles on
  guide lines, the vector mask outline dashed in `OnImage.guideFaint`. Properties for a shape: `SubHeader` groups
  (Live <kind> or Custom Path, Fill, Stroke, Vector Mask, Interchange) of `DocSlider` rows, neutral `SegmentedPicker`s
  (paint kind, alignment, caps, fill rule), `ThemeMenuStyle` pop-ups (gradient style, corners), a dash field, `Hint`s
  for behaviour and warning `StatusLine`s for the engine's interchange and colour limitations.
* **Warp, Perspective Warp, Puppet Warp, Content-Aware Scale (WP B5-12)** add no colour, size or font. On canvas only
  the on-image set: the child canvas the stage clips to as a dashed `OnImage.guideFaint` rectangle; the warp net's
  iso-curves, perspective planes and the content-aware box as 1 pt `OnImage.guide` over a 3 pt `OnImage.shadow`;
  tangents, the puppet mesh (0.5 pt) and, in Warp mode, the layout planes (dashed) in `OnImage.guideFaint`; anchors,
  plane vertices and box handles are the square transform handles, warp tangents 6 pt dots; puppet pins 10 pt dots in
  `OnImage.text` with an `OnImage.ink` ring, the selected pin in the accent (content selection), a pin rotation as a
  faint ring with a guide tick; readouts (Layout / Warp, dimensions) are scrim chips. The options bar replaces the
  tool's options while a session is open: the operation's icon and name, then its controls (preset `MenuPicker`
  and Bend field; Grid menu; the split mode as an icon `SegmentedPicker`; Layout / Warp segments; Mode pop-up,
  Density segments, Expansion and Rotate fields, Show Mesh; W / H / Amount fields and the Protect pop-up listing saved
  alpha channels only), the interpolation pop-up, an `info.circle` glyph whose help lists the limitations, the
  tertiary latency readout, then borderless Reset and Cancel and bordered Apply (`Apply…` when Apply converts the
  layer, which asks in a standard alert). Transform stages appear as ordinary smart filter rows named after the
  operation; double-click re-opens the editor.
* **B5-11b** adds no colour, size or font. The Layers row gets a vector-mask thumbnail (the layer mask's checker tile
  size; white inside the path, mid-grey outside, crossed when disabled). The status bar always shows the current
  tool's idle hint (`DocumentTool.idleHint`) or its session hint (text session, Pen path), replacing the previous
  tool's message on every tool change.


## 11. Library and Photo Edit workspace (first redesign slice)

Precision Graphite is the provisional continuation of the existing graphite/amber
identity. This slice changes navigation and target clarity, not the global palette
or Document's editing model. The recovered exploration and approved implementation
scope are preserved under `docs/design/workspace-redesign/` at the repository root.

Library contains Grid, Loupe and Compare arrangements, library sources/filters and
selection/metadata properties. E/Return remains Library Loupe inspection. **Edit
photo** / D enters editing; the header names the focused photo and explicitly says
**Editing 1 photo**. Selection-based Library actions show their actual affected
count independently. Synthetic fixtures say **Preview only · STUB**.

Photo Edit keeps the native loupe and uses fixed Develop/Masks inspector tabs with
existing controls (the Develop tab's groups: §13). Global parameters target **Whole photo**; mask parameters name
the selected mask. The source sidebar and filter controls yield to the canvas.
Back to Library restores the prior arrangement, stable photo selection/focus and
browser anchor; G explicitly chooses Grid on return. A source/filter/folder change
leaves editing rather than silently changing its return scope. Normal context
switches do not rescan the library or create a document.

Photo Edit Undo/Redo targets the current photo history exclusively. Text and
numeric controls retain keyboard priority; bare culling decisions cannot act on
an unseen Library selection while editing. Escape reaches active tool cancellation
before returning to Library. Y in Library still confirms all **visible** suggestions,
with that scope named; changing its shortcut meaning is a separate migration.

The generated [keyboard shortcut reference](../../docs/shortcuts.md) lists menu equivalents,
routed keys, mode boundaries and macOS reservations. `ShortcutIntegrityTests` checks it
against the command definitions and key maps during the Swift gate.

Layered documents remain a distinct workspace with their existing tabs, tools,
save/dirty state and history. The Library-side handoff is **Open in Layers…** and explains that adjustments become pixels while the RAW
and its recipe remain separate. An already-open copy reopens unchanged, without
refreshing from later photo edits. Cmd-E still invokes this Library action (with its
disclosure) and remains Merge Down inside Document. No live-RAW/shared-graph promise
is implied. Machine B owns Document internals.

Use the existing M2-56 containment/yield rules at all four declared sizes and both
appearances. The target header truncates long names with full help/accessibility
labels and remains outside the photo pixels. No target label performs engine work.

## 12. Navigable Review (UX-02a)

Review is a session-local destination beside Library and Photo Edit. The Review
count and Develop menu remain available for empty and all-reviewed queues. Auto
Edit completion updates the queue and toast without presenting a modal or changing
the workspace. Queue relaunch persistence belongs to UX-02b and is not shipped by
this navigation slice.

The queue retains its existing failed-first/confidence order. A stable image ID
owns list selection and scroll anchor; a generation-scoped instruction draft
survives Review → Edit photo → Review. The inspector names one photo, distinguishes
user review status from the critic's result, and exposes Accept, Accept & next,
Redo with instruction, Revert group and Edit photo. Accept & next waits for a
successful current-target callback before using the queue's next-pending rule.
The preview is labelled Current preview; there is no before/after baseline.

Review owns selection without changing the Library source, filter or multi-photo
selection. Edit photo resolves the queued image independently of visible Library
rows. Back unwinds Edit → Review → Library, restoring the original native browser
anchor after layout. A deliberate folder/source change leaves this navigation.
Actions resolve stable image IDs only in the captured EngineLibrary instance;
a queue from another library session stays visible with unavailable actions.

Review consumes cull letters and owns its arrows after text, numeric, sheet and
Document guards. Escape cancels a draft before leaving Review. Undo/Redo in Review
do not consume Library history; the inspector directs the user to Edit photo for
recipe history. Accept's learned feedback is not advertised as undoable.

## 13. Edit panel (redesign SPEC §3, WP UX-12)

`Inspector/PhotoEditInspectorView.swift`. The Develop tab of the one panel in Edit, top to
bottom (the Masks tab is unchanged; M and the loupe mask tools switch to it):

* **Histogram strip** (`HistogramView`, 40 pt): the luminance histogram as one quiet fill in a
  plot well; the shadow / highlight clipping marks (`warning`) are its only colour. No title.
* **Sections** (`EditSection`): a 15/600 title with an optional readout at the right in 12 pt
  tertiary tabular (Light: the target, "Whole photo" · Color: the colour temperature · Detail:
  "Sharpen 40"), the everyday controls, then a borderless **More / Less** disclosure
  (`develop.panel.<Section>.more`, remembered per section in `EditPanel.<Section>.more`) with the
  rest of the group. Sections are not collapsible. Agent and Edits use a 13/600 title.
  * **Light**: Exposure, Shadows, Highlights, Contrast · More: Whites, Blacks, Tone Curve, HDR.
  * **Color**: Temperature, Tint, Vibrance · More: Saturation, HSL, Grading.
  * **Detail**: Sharpen · More: Texture, Clarity, Dehaze, the sharpening radius / detail / masking
    with the 1:1 preview, AI Denoise and noise reduction.
  * **Optics**: Crop & Straighten · More: Upright / Transform, Lens Blur. Lens corrections from
    the maker notes are automatic (an auto step in Edits).
  * **Effects**: Vignette and Grain amounts · More: the rest of both, Presets, Soft Proofing,
    Snapshots.
  * **Agent** (`AgentChangesSection`, only after a run from the agent field finished on this
    photo): "n changes · preview" and one **change card** per step of the plan (radius 8,
    hairline; raised on hover): the control in 13 pt medium, its value in the accent, "Whole
    photo" / "On Sky" in 12 pt tertiary, a mini switch (`develop.agent.toggle.<entry>`, the same
    step toggle as the Edits stack: a recorded step), dimmed at 45 % while off. The pointer over a
    card shows the photo without that change (the group's 0 % values for the controls the step
    names, through the interactive patch path; nothing is recorded). Below: **Keep** (primary,
    fills the row; accepts the queue entry, the group stays as one grouped step in Edits),
    **Undo all** (reverts the group, one step) and **Apply to burst (n)** (the same instruction on
    the other n frames of the burst; those go to Agent Review). Multi-photo runs keep using the
    review queue.
  * **Edits** (`EditsSection` + `HistoryPanel`): "n steps" and a borderless Reset; then every
    step newest first as a 24 pt row with a mini checkbox toggle, the label in 12 pt and who
    made it in tertiary (by hand · agent · auto · preset; the time is in the tooltip); an agent
    run is one grouped step (`AgentGroupSection`: `AI` chip, name, amount readout, the Amount
    fader, its sub-steps with their toggles and rationales, Redo with instruction); the Original
    row at the bottom returns to the unedited photo. One store: the develop controller's history.

Every control of the former Basic / Tone Curve / HSL / Grading / Detail / Transform / Effects /
Lens Blur / Crop / HDR / Soft Proofing / Presets / Snapshots / History panels is still reachable
and keeps its accessibility identifier; only the BasicPanel's duplicate Snapshots menu
(`develop.basic.snapshots`, `.newSnapshot`, `.restoreSnapshot.n`) was retired in favour of
Effects ▸ More ▸ Snapshots. Panels carry no explanatory text: one tooltip per control.

**Agent status row** (`AgentStatus`, `Shell/AgentField.swift`): right of the agent field in Edit,
in a 170 pt slot mirroring the tools strip's: a 28 pt dot (accent once there are agent edits,
`track` otherwise) and "n edits by agent · review" (review shows the panel on Develop), "Agent is
working…" while the run goes, "No agent edits yet" otherwise.

**Dial highlight API** (for the loupe dial, UX-13): `AppModel.highlightedDevelopControl:
DevelopPanelControl?` (`.basic(BasicKey)` for Exposure / Shadows / Highlights / Temperature …,
`.control(DevelopControl)` for any other section slider). The panel passes it down as the
`highlightedDevelopControl` environment value; `ControlSlider` and `AdjustmentSlider` compare
themselves and set `ValueSlider.dialHighlighted`. Set it when the dial opens on a control, nil
when it closes. Controls behind a More are not revealed automatically.

## 14. Pointing at the photo (UX-13)

Edit's on-photo layer (SPEC §3): the dial, the mask ring, the Masks / Crop / Heal tools of the
strip, the Try cards and before / after. Everything draws over the native loupe; nothing is
added to the panel.

**The dial.** A click on the photo (not a drag, not with Crop, Heal, a mask brush, the HSL
picker or the detail target armed) picks the mask under the cursor with the existing AI masks:
Subject if the point is inside the subject mask, Sky if inside the sky, otherwise Background,
and the whole photo when the segmenter found nothing. The three masks are created on first use
as one history step ("Subject, Sky and Background masks") and found again by their single
Subject / Sky / Background component, so a photo that already has them gets no second set.
While they segment the dial says so and re-targets the click when they finish. The ring is the
mask's outline (marching squares on the engine's mask thumbnail, dashed accent, 1.5 pt over a
soft shadow) with an accent chip naming it. The HUD is 236 pt wide, radius 14, the HUD ground
(canvas at 86 %) with `hairlineStrong`: title and value (17 pt semibold, accent, tabular), a
120 pt knob whose conic fill is the value (drag up or down), chips for Exposure · Shadows ·
Warmth · Highlights (+ Skin when a detected face is under the cursor) and one line, "Applies to
the subject only." Default control: Shadows on the subject, Highlights on the sky, Exposure
elsewhere. The value is a local adjustment of that mask (Warmth is the local Temperature, Skin
is local Texture) or the matching panel slider on the whole photo, coalesced per frame while
dragging and one history step on release, so the Edits stack shows it.
`AppModel.highlightedDevelopControl` names the slider the dial drives; the panel highlights it.
Esc closes; a click elsewhere re-targets. `PhotoDial` owns all of this (Loupe/PhotoDial.swift);
the hit-test and outline are `MaskRaster` in TesseraCore.

**Masks tool (strip).** Shows the Subject · Sky · Background · Whole photo chips top centre
(a 999-radius pill of chips, the selected one filled with the accent) and the ring; a chip
targets that mask for the dial and selects its group in the Masks panel. M still opens the
brush / gradient / range toolbar; the chips move under it while it is up.

**Crop tool.** The existing crop overlay (rule-of-thirds guides by default) with an aspect pill
at the top: 3:2 · 4:5 · 1:1 · 16:9 · Free, the same `CropAspect` values as the panel.

**Heal tool.** Click-to-heal: each click appends one heal operation to the recipe's
`locals.retouch` list (a round brush spot, feather 50, healed from a source 2.6 radii beside it,
inside the frame), committed as the step "Heal spot". The develop pipeline's registered retouch
renderer draws it (`brush::render_retouch`: additive explicit-source heal / clone spots). Spots
show as accent circles while the tool is active. A Smart Preview session keeps the operation in
the recipe and reports it as a render notice (retouch needs the complete sensor).

**Try cards.** A row of 132 × (64 + 28) cards above the agent field in Edit: Relight face,
Deeper sky, Film look, Clear the crowd, Black and white, and "More from how you usually edit".
Each thumbnail is a real render of this photo with the look, through the open session's own
pipeline: `DevelopController.renderLooks` attaches a card-sized surface, renders each look as an
interactive patch, restores the live state with the patch's inverse and re-plans the loupe
ring; renders are cached per photo. Mask looks render once the AI masks exist (the dial, the
Masks tool or the card itself creates them). A click applies the look as the step
"Try · <name>" (amber border, "on"); clicking again undoes that step when it is still the head,
else records "Remove <name>". "Clear the crowd" quietens the background mask (darker, softer,
less colour): the engine's object removal exists in Layers only. The style card runs the style
profile as an agent edit (reviewed like any other run), since the profile has no synchronous
recipe. Tokens: `Theme.Width.tryCard`, `Theme.Height.tryThumbnail` / `tryCaption`.

**Before / after.** A toolbar button left of Panel ("Before"), held, and the `\` key show the
original through the history checkout the Edits stack uses (no step is recorded); releasing
returns to the head step. Nothing happens with no applied step.
