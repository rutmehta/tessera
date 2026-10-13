# Tessera redesign — spec (2026-10-13)

Owner decision (Rut, 2026-10-12): direction **A + F** from the design-directions canvas, with the
burst-first Cull from the demo. This file is the contract for the implementation work packages.
The clickable demo is the reference for feel and behaviour: `docs/design/redesign-2026-10/demo/`
(open `index.html` from a local server; it is plain HTML).

## 1. What it is

**The photo and one panel.** Every mode is the photo (or the shoot), one context panel for what you
are doing, and one agent field. Nothing else is on screen by default.

Three modes in one segmented control, top centre: **Cull · Edit · Deliver**. They replace
Library / Edit photo / Review. The toolbar is the only bar: the folder name (opens the folder,
album and people list as a popover), the mode control, before/after, panel toggle, Export.

**Tab hides the panel.** With the panel hidden the app is "F": the photo fills the window, the
on-photo dial, mask ring, Try cards and agent field still work. New users start with the panel
shown; Tab is the only thing that changes between the two states. The state persists per mode.

**The agent field** sits bottom centre in every mode, with a mode-specific placeholder
("Reject the blurry ones in this burst" · "Say what you want, or point at it" · "Export the keeps
as web JPEGs"), a mic button, and ⌘K focuses it. Its answer is never a wall of text: it is a list
of **change cards** (what · where · how much), each with its own toggle, a before/after pair on
hover, and Keep / Undo all / Apply to burst. Kept changes become one grouped step in Edits with an
amount fader. Provider and guardrail settings live in Settings ▸ AI, not in a sheet.

## 2. Palette and type (tokens)

Dark is the only appearance during the redesign; light parity is restored afterwards. Replace the
graphite greys in `Theme.swift` with true black and warm white. Amber stays, and becomes
**user-customisable** (§6).

| Token | Value | Use |
| --- | --- | --- |
| `canvas` | `#0A0A0A` | Window ground, behind the photo |
| `panel` | `#161616` | Panel, toolbar fills, cards |
| `raised` | `#1C1C1C` | Buttons, fields, hover |
| `track` | `#2A2A2A` | Slider tracks, inactive toggles |
| `hairline` | white 8 % | Separators |
| `hairlineStrong` | white 14 % | Field and HUD outlines |
| `textPrimary` | `#F2EFE9` | |
| `textSecondary` | `#B5B1AA` | Values, secondary labels |
| `textTertiary` | `#8D8A84` | Hints, counts |
| `textQuaternary` | `#5A5852` | Disabled, key caps |
| `accent` | `#F0A52A` default, user-set | Selection, focus, the primary action, the agent mark |
| `onAccent` | `#1A1200` | Text on accent |
| `keep` / `reject` | `#6F9A60` / `#C0584A` | Decisions only |
| HUD | `rgba(10,10,10,0.86)` + hairlineStrong, radius 14 | Dial, mask chips, toasts |

Type: one family (system SF is fine; the demo uses Instrument Sans). Scale: 15/600 section
titles, 14/500 toolbar, 13 body and slider labels, 12 secondary, 11 key caps. **No 10 pt and no
uppercase tracked labels.** Values are tabular numerals.

Radius: 6 controls, 7–8 bars and cards, 12 agent field, 14 HUD, 999 chips. Slider: 4 pt track,
16 pt thumb, 26–28 pt row. Minimum control height 32.

Tooltips appear after 250 ms (`NSInitialToolTipDelay`).

## 3. Edit

- Panel groups: **Light** (Exposure, Shadows, Highlights, Contrast, then Whites, Blacks, Curve
  behind a disclosure), **Color** (Temperature, Tint, Vibrance, then Saturation, HSL, Grading),
  **Detail** (Sharpen, then Texture, Clarity, Dehaze, Noise), **Optics**, **Effects**. Histogram
  is a 40 pt strip at the top, clipping as the only colour.
- **Edits** stack at the bottom of the panel: every step (yours, the agent's, automatic) as a row
  with a toggle; an agent run is one grouped step with an amount fader.
- **Pointing at the photo**: a click picks the mask under the cursor (Subject, Sky, Background,
  else whole photo; the fast segmenter already exists), draws the ring, and opens the **dial**
  next to the cursor: one large control for the most relevant slider (Shadows on a face,
  Highlights on sky, Exposure elsewhere), chips to switch to Exposure / Warmth / Highlights /
  Skin, and the panel slider highlights so the two agree. Drag up/down on the dial. Esc closes.
- **Try cards** above the agent field: Relight face, Deeper sky, Film look, Clear the crowd,
  Black and white, plus "more from how you usually edit" (style profile). Each is a preview until
  Keep; each becomes a step.
- Tools strip left of the agent field: Masks, Crop, Heal. Masks shows the Subject / Sky /
  Background / Whole photo chips over the photo. Crop shows the overlay with aspect chips. Heal is
  click-to-heal.

## 4. Cull (burst-first)

- Left column: the agent's **triage** of the shoot, computed as the folder loads: Likely keepers,
  Bursts to decide, Suggested rejects (blurry · eyes closed · near-duplicates), Nothing to flag.
  Each row is a view. Progress through the bursts at the bottom.
- Centre: **one burst at a time**. The agent's pick large, with its reasons on the photo
  (sharpness bar, eyes, face score, score). The burst's frames as a strip below with the pick
  badged. ← → within the burst, ⌥← ⌥→ between bursts.
- **K** keeps the pick and rejects the rest, then advances. P / X on a single frame. When every
  frame of a burst is decided it advances by itself.
- A **grid view** of the same data (toggle in the header, G): thumbnails grouped by burst, pick
  badged, decisions marked, denser; clicking a burst opens it in burst view.
- Quick actions as chips above the agent field: keep the pick in every burst, reject the blurry
  ones, find closed eyes, show me <person>.

## 5. Deliver

What (Keeps · Selection · Whole shoot) · How (Web JPEG · Instagram 4:5 · Full size TIFF · Print)
· Where. Preview grid of the files with their output sizes and a size estimate. One Export button.
The agent field understands "export the keeps for Instagram".

## 6. Custom colours

Settings ▸ Appearance: **Accent** (swatches: Amber default, Coral, Cobalt, Teal, Rose, Lime, plus
a colour well) and **Ground** (Black default, Graphite, Navy). Both are tokens; nothing in the
views names a colour. `onAccent` is computed from the accent's luminance so text stays legible.
Keep and reject colours are not customisable. The choice persists in UserDefaults and applies live.

## 7. Engineering out of the UI

- No engine readouts in the status bar (`engine sink…`, `Opened … 1.85 s`); behind a Debug flag.
- Menu titles never contain shortcuts in parentheses; shortcuts go in the key-equivalent column.
- Panels contain no explanatory paragraphs; one tooltip each. Empty states: one sentence, one button.
- Badges reduced to the focus ring and the keep / reject mark.

## 8. Work packages

| WP | Scope | Touches |
| --- | --- | --- |
| UX-10 | Tokens (§2), custom colours (§6), engineering out (§7), tooltip delay | Theme.swift, Components.swift, Settings, status bar, AppCommands menu titles |
| UX-11 | Shell: three modes, one toolbar, folder popover, Tab panel toggle, agent field in every mode | ContentView, toolbar, KeyRouter, a new AgentField view |
| UX-12 | Edit panel groups + Edits stack + change cards for agent answers | PhotoEditInspectorView, DevelopPanels, Agent UI |
| UX-13 | Dial, mask ring, Try cards, tools strip | Loupe overlays, Masking |
| UX-14 | Cull burst-first + grid + triage | Cull views, AssistController |
| UX-15 | Deliver | Export sheet becomes a mode |

UX-10 and UX-11 first, in parallel; the rest follow. Every WP reinstalls to /Applications when
done (`apps/mac/Support/make-app.sh release`, then copy) and takes window screenshots
(`screencapture -l <window id>`) for review.
