# Tessera for Mac: design system

The single source of truth is `Sources/Tessera/App/Theme.swift` (tokens) and
`Sources/Tessera/App/Components.swift` (the shared controls). Views use nothing else:
`Tests/TesseraCoreTests/ThemeLintTests.swift` fails on raw colours, numeric padding / spacing /
radii, ad-hoc fonts, native segmented pickers and link buttons anywhere in `Sources/Tessera`.
The defects this system replaced are listed in `DESIGN_AUDIT.md`.

## 1. Identity

**A quiet, precise pro tool.** The photo is the loudest thing on screen; the interface is the
frame around it. Three ideas carry the identity:

* **Graphite, not grey.** Surfaces are warm neutrals (a hint of red-yellow in every step), from a
  near-black canvas to raised controls. Adobe's cool mid-greys, grey tiles and bevels are out.
* **Hairlines, not boxes.** Structure comes from 1 px separators and spacing. Panels have no
  borders, thumbnails have no tiles, cards are rare.
* **One accent, used sparingly.** Warm amber marks exactly three things: *content selection*
  (grid cells, list rows, history head), *focus* (the focused photo's ring, slider drag) and *the
  primary action* of a sheet. Controls' own modes (segmented controls, toolbar toggles) stay
  neutral.

Why amber: it is warm, like the graphite, so the palette reads as one family; it is far from
every decision colour (sage keep, brick reject, slate basket) and from the macOS default blue and
Adobe's blue; and it carries continuity from the app's original focus ring.

Dark is the primary appearance. Light is complete, not an afterthought: every token has both
values and the app follows the system (View ▸ Appearance overrides it: System / Dark / Light).

## 2. Colour tokens

Hex values are sRGB. `Theme.Palette.*` are dynamic `NSColor`s (AppKit drawing); `Theme.*` are the
same colours as SwiftUI `Color`s. Contrast is WCAG 2 against the surface named.

### 2.1 Surfaces

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `canvas` | `#131312` | `#E3E2DF` | Grid, compare, loupe surround |
| `panel` | `#1C1B1A` | `#F5F4F2` | Inspector, filter bar, status bar, sheets |
| `raised` | `#272624` | `#FFFFFF` | Fields, bordered buttons, selected segment, cards |
| `well` | `#0E0E0D` | `#E9E8E5` | Segmented tracks |
| `plotWell` | `#121211` | `#121211` | Scopes: histogram, curve, 1:1 detail (dark in both, like FCP scopes) |
| `hover` | white 6 % | black 5 % | Hover fill |
| `pressed` | white 11 % | black 9 % | Pressed fill, toolbar toggle on |
| `hairline` | white 9 % | black 11 % | 1 px separators, quiet outlines |
| `hairlineStrong` | white 16 % | black 18 % | Control outlines, slider tracks |
| `groupAlt` | white 3.5 % | black 4 % | Alternate burst groups in the grid |
| `hud` | `#1A1918` 88 % | `#FBFAF8` 92 % | Loupe tool bars, toast |

The sidebar uses the native source-list material (vibrancy) rather than a token.

### 2.2 Ink

| Token | Dark | Light | Contrast (on panel) | Use |
| --- | --- | --- | --- | --- |
| `textPrimary` | `#ECEAE6` | `#1D1C1A` | 14.3 / 15.5 | Titles, values, row text |
| `textSecondary` | `#A7A39C` | `#5E5A54` | 6.8 / 6.2 | Labels, sub-headers, captions |
| `textTertiary` | `#7C7872` | `#858079` | 3.9 / 3.6 | Hints, counts, key caps (never the only carrier of meaning) |
| `textOnAccent` | `#17130B` | `#FFFFFF` | 8.3 / 4.8 on accent | Primary button text |

### 2.3 Accent and semantics

| Token | Dark | Light | Contrast (on panel) | Use |
| --- | --- | --- | --- | --- |
| `accent` | `#E2A04A` | `#A8620C` | 7.7 / 4.5 | Selection, focus, primary action |
| `accentSubtle` | accent 18 % | accent 14 % | – | Selection fill |
| `keep` | `#80B38C` | `#2F7A46` | 7.2 / 4.8 | Keep, grades |
| `reject` | `#D8786C` | `#B23B2F` | 5.6 / 5.4 | Reject, errors |
| `basket` | `#88A8CE` | `#3B6699` | 7.0 / 5.4 | Basket target album |
| `warning` | `#D8B55C` | `#8A690B` | 8.7 / 4.7 | Warnings (always with an icon) |

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
| `Radius.card` | 8 | HUD bars, toasts, cards, sheets |

No capsules anywhere. Circles only for slider thumbs, colour wells and dots.

### 3.3 Heights

| Token | pt | Use |
| --- | --- | --- |
| `Height.chip` | 16 | Badges on thumbnails and in chrome |
| `Height.small` | 20 | Inspector buttons and menus, facet menus, panel segmented controls |
| `Height.regular` | 24 | Search field, decision chips, sidebar rows, list rows, status bar |
| `Height.large` | 28 | Toolbar items, sheet footer buttons, HUD tools, sidebar headers |
| `Height.sectionHeader` | 32 | Inspector section headers, the loupe information strip |
| `Height.slider` | 32 | One slider row |
| `Height.filmstrip` | 72 | Filmstrip |

Widths: sidebar 200–300 (ideal 220), inspector 288–380 (ideal 296), label column 72.

### 3.4 Window structure

Toolbar (real `NSToolbar` via SwiftUI `.toolbar`, flat — no per-item glass on macOS 26):
Open Folder (leading) · Grid / Loupe / Compare segmented (centre) · thumbnail size (Grid only),
Auto-advance, Inspector (trailing; icon + label). Below: filter bar (panel, hairline) · content
(canvas) · progress strips · status bar · filmstrip. Sidebar left, inspector right.

## 4. Type

SF Pro only, at six sizes and three weights (regular, medium, semibold; never bold). Every
numeric readout uses monospaced digits (`.monospacedDigit()` / `monospacedDigitSystemFont`).
SF's built-in size-specific tracking is used as is; the only manual tracking is −0.3 on 22 pt.
No uppercase labels: sentence or title case everywhere.

| Size | Token (SwiftUI / AppKit) | Weights | Use |
| --- | --- | --- | --- |
| 11 | `Fonts.caption*` / `NSFonts.caption*` | R / M / S, numeric, mono | Inspector labels and values, chips, captions, status bar, hints |
| 12 | `Fonts.label*` / `NSFonts.label*` | R / M / S, numeric, mono | Buttons, sidebar rows, section headers, filter field |
| 13 | `Fonts.body*` | R / M | Rare body text |
| 15 | `Fonts.title` | S | Sheet titles |
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

**Slider** (`ValueSlider`, AppKit `NSControl` on the < 16 ms path): 32 pt row; label 11 pt
secondary left, value 11 pt tabular right (tertiary at default, primary + medium when changed);
2 pt track in `hairlineStrong`, fill from the default to the value in `textSecondary` (accent
while dragging); a 1 px tick marks the default of bipolar controls; 12 pt disc thumb with a
hairline and soft shadow, 2 px accent ring while dragging. Drag is relative, ⌥ is 10× finer,
double-click resets, a click on the track away from the thumb jumps there.

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

**Scopes** (`HistogramView`, `CurveEditorView`, `DetailPreviewView`): `plotWell`, radius 4,
channel colours composited additively.

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
* `AppearancePreference` stores View ▸ Appearance in user defaults; `--appearance dark|light`
  overrides it for one run (screenshots).
* The toolbar opts out of macOS 26's per-item glass with `sharedBackgroundVisibility(.hidden)`
  (`flatToolbarItem()`); macOS 15 ignores it.

## 10. Document mode (WP M5-13)

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
