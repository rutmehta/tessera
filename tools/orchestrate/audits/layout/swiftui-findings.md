# SwiftUI layout audit — Tessera

## Scope and evidence limits

Read-only source audit of `apps/mac/Sources/Tessera`, with `apps/mac/DESIGN.md` and `apps/mac/DESIGN_AUDIT.md` as design references. Paths below are relative to `apps/mac/Sources/Tessera` unless otherwise stated. No application launches, builds, source edits, runtime measurements, or screenshot inspections were performed. The parent audit owns visual reproduction.

**Evidence labels:** **Static contradiction** means explicit layout bounds cannot all be satisfied in the stated configuration; it does not assert which edge SwiftUI clips or which ancestor expands. **Conditional defect** means the algorithm demonstrably has no accommodation for an identified input/bounds condition. **Risk** means the code creates pressure but actual truncation/overlap needs rendered measurement. A frame alone does not clip; overflow, ancestor clipping, layout expansion, and truncation are distinct possible outcomes.

The historical design audit is not evidence of today's rendering: e.g. its export-sheet “720 pt” fix (DESIGN_AUDIT.md:169–171) predates the current 760 pt grouped Form. Its prior “fixed” point-curve / grading claims need retesting against current code.

## Priority findings

### S1 — Document inspector cannot fit the permitted minimum height

**Static contradiction; high priority.** `Document/DocumentView.swift:97–120` builds one non-scrolling outer VStack. Properties has minimum height 192 (`:105`), Layers minimum 256 (`:112`), then a hairline, Channels, and expanded-by-default History. `Document/DocumentHistoryPanel.swift:15–27` fixes history's list at 144; `:29–53` adds Snapshots, every snapshot row, and the New Snapshot / memory row outside that list's scroller. `App/Components.swift:283–312` adds a 32 pt header, 16 pt bottom padding to expanded sections, and a 1 pt separator. Theme values: `App/Theme.swift:158–184`.

Even with Channels collapsed, a deliberately conservative subtotal is **675 pt**: Properties 192 + Layers 256 + inter-section hairline 1 + collapsed Channels 33 + History header 32 + history list 144 + History bottom padding 16 + separator 1. This excludes all Snapshots text, controls, spacing, and snapshot rows. The window explicitly allows a **600 pt** content minimum (`App/TesseraApp.swift:12–16`). The inspector's two priority settings cannot reconcile these minimums; local scrolling only scrolls list contents, not the outer stack. Expanded Channels additionally reserves up to eight 32 pt rows plus footer (`Document/Channels/ChannelsPanel.swift:18–31,76–77`). Snapshot accumulation adds uncapped 24 pt rows.

**Expected symptom:** inaccessible bottom history controls / vertically overflowing sections, or an unexpected effective window minimum. **Parent verification:** minimum-height document, expand History, then Channels; add snapshots. Distinguish clipping from a window refusing to shrink.

### S2 — Tether session row has a hard lower bound above a narrow center pane

**Static contradiction when detail width < 504 pt; high priority.** `Tether/TetherPanel.swift:131–181,210–213` puts a 200 pt session field and a 240 pt naming field in adjacent columns, a 12 pt inter-column gap, and a 24 pt icon menu with 4 pt gap. Horizontal gutters add 24. Thus the row needs at least **504 pt**, before any session album column. It has no horizontal scrolling or alternate vertical arrangement. The center budget at the declared 960 pt minimum window and ideal sidebar/inspector widths is at most **444 pt before dividers** (`App/TesseraApp.swift:14`; `App/Theme.swift:180–181`; `Shell/ContentView.swift:9–20,115–124`). Even minimum sidebar/inspector widths leave only 472 before dividers.

SwiftUI/AppKit may respond by expanding minimum window size rather than visibly overlapping; the numeric contradiction is conditional on both side panels remaining shown at these budgets. **Parent verification:** open tether with sidebar and inspector visible, narrow window, compare with inspector hidden; then connect a session with Albums visible.

### C1 — Shared menu style explicitly refuses horizontal compression

**Conditional defect mechanism; highest shared amplification.** `App/Components.swift:93–108` wraps every `ThemeMenuStyle` label and chevron in `.fixedSize()` on both axes, then padding and a fixed height. Parent `.frame(maxWidth: .infinity)` does not turn that intrinsic label into a truncating one. `MenuPicker` (`:130–138`) shortens strings by character count, not available pixel width. A 26-character wide-glyph profile can exceed a narrow row budget; direct `Menu` callers bypass even this shortening.

Concrete pressure point: `Loupe/SoftProof.swift:158–165` gives Profile a 56 pt label, gaps, menu, and Other button inside the 264 pt minimum panel interior. The menu competes with indispensable siblings while refusing the proposal. Other narrow consumers include Effects (`Inspector/DevelopPanels.swift:383–392`), Crop (`:480–501`), schema choice rows (`Document/Filters/FilterSheets.swift:80–94`), and style label/value rows (`Document/Styles/LayerStyleEditors.swift:150–157`). Facet menus are **not the same uncontained failure**: `Library/FilterBar.swift:63–79` puts those in a horizontal ScrollView, so long active facet names principally increase scroll distance.

**Verify first:** long/wide ICC profile names in minimum-width Soft Proofing; schema choices with long current values. Do not call all menu uses broken—intrinsic sizing is appropriate in horizontally scrolling or sufficiently wide contexts.

### C2 — FlowRow wraps rows, but never constrains one oversized chip

**Conditional defect, directly established algorithm; high priority.** `Inspector/LibraryPanels.swift:136–155` measures each subview with `.unspecified`, returns the proposed container width, and places each child with its entire intrinsic size. A first item wider than the container does not wrap (`x > 0` / `x > bounds.minX` guards); even subsequent oversized items merely move to a new row and still exceed its width. `:20–26` feeds arbitrary applied keyword names into it. `KeywordChip` uses a fixed 20 pt height (`:67–81`).

For a keyword whose ideal chip width exceeds the available 264 pt panel interior, this layout reports containment while placing content outside its reported width. That is a source-proven width-contract failure, not a guessed screenshot. Exact clipping is ancestor-dependent. Suggested keyword chips also use this flow family; test a single very long unbroken keyword, not only many short tags.

### C3 — Status groups and document tabs have unbounded intrinsic width

**Conditional defect mechanism; high priority.** Library status locks complete groups using `.fixedSize()` at `Shell/ContentView.swift:355,387,394`; `stateText` includes the full basket target (`:409–416`), and the basket group repeats it (`:389–395`). Only the intervening status message is explicitly truncatable. Document status likewise fixes canvas/profile, zoom, tool, selection, render timing and open count (`Document/DocumentView.swift:226–254`). Both are single 24 pt rows without horizontal scrolling.

A sufficiently long album/profile or combined readout exceeds any finite detail width; truncating the message cannot solve the sum of noncompressible siblings. `DocumentTabs` (`Document/DocumentView.swift:147–165`) similarly fixes the whole list's ideal size; each title is capped (`:185–191`) but the number of tabs is not. `Shell/ContentView.swift:151–181` adds a fixed-size four-mode picker and retains the invisible thumbnail-slider slot even in document mode, with further action items at `:184–263`. **Toolbar overflow may absorb items:** inspect actual NSToolbar overflow before describing tab overlap. Status bars have no equivalent explicit overflow fallback in source.

### R1 — Generic filter sheet is undersized for its own scaffold and fixed preview

**Strong risk, not pixel-measured contradiction.** `Document/Filters/FilterSheets.swift:14–52`: 560 × 300 outer frame, 180 × 180 detail pane, 16 pt body padding, arbitrary schema controls in a non-scrolling VStack, and Reset/Cancel/OK footer. `App/Components.swift:415–442` requires header, two hairlines, and footer.

Preview + body vertical padding + 28 pt footer button + footer vertical padding + separators already consume **266 pt**, leaving **34 pt** for a header that has 24 pt vertical padding plus title and subtitle (with a 2 pt gap). Normal typography strongly implies overflow/compression; exact text metrics were not measured. More controls or error text worsen it. At 560 width, after padding, preview and gap, the controls get 332 pt; choice rows further reserve 96 pt label plus 8 gap. The related destructive-adjustment sheet correctly wraps its editor in a ScrollView (`:240–267`); do not conflate the two.

**Parent verification:** filter with a point/angle plus several controls, then an inline error. Capture preview bottom and footer, not just controls.

### R2 — Neural filters and merge warnings grow outside scroll containers

**Data-dependent risks.** Neural sheet: `Document/Retouch/RetouchViews.swift:133–167` fixes 680 × 460 and uses a non-scrolling detail column beside a 220 pt list. `:198–249` appends slider rows, output choice, unavailable-output reasons, face hints, limitations, missing-model URL/cache path, and errors. Multiple explanatory texts have `.fixedSize(horizontal: false, vertical: true)`, preserving all wrapped height. Long paths/messages or a many-control spec can overrun the content allocation or displace footer without an escape scroll path.

Merge: `Photo/PhotoMergeSheet.swift:24–37,61,75–88` scrolls **options only**, not its fixed-width preview/warnings column. Repeated engine warnings and notes under the 400 × 300 preview in an 820 × 560 sheet can exhaust the left column. `SheetScaffold` itself does not scroll content or protect footer from oversized descendants (`App/Components.swift:431–441`). These are reasons to exercise error/unsupported states, not claims every ordinary sheet clips.

### R3 — Narrow inspector HStacks: point-curve, grading, decision chips

**Measurement-required risks.** All PanelSection content starts with 24 pt deducted from inspector width (`App/Components.swift:307–309`), leaving **264 / 272 / 356 pt** at min / ideal / max inspector.

- **Point Tone Curve:** `Inspector/DevelopPanels.swift:111–124` places two independent `.fixedSize()` segmented controls in one HStack, with Parametric/Point plus RGB/R/G/B/L in point mode. Both request ideal width; no fallback or scroll. Exact SF widths needed to prove whether their sum exceeds 264. This is a stronger candidate than the ordinary fill-based segmented picker.
- **Three-way Color Grading:** `Inspector/DevelopPanels.swift:271–278` puts three `ControlSlider`s side by side with two 12 pt gaps. Equal shares at minimum width are only **80 pt**. Shared AppKit `ValueSlider` supplies no intrinsic minimum width (`Inspector/ValueSlider.swift:59`) and draws full title at x=0 and full value at width−valueWidth with no collision/truncation check (`:129–138`). **Overlap condition is exact:** titleWidth + valueWidth > sliderWidth. Highlights / Midtones and signed extreme values are priority checks; no measured glyph-width claim is made here. Same drawing code backs document opacity/fill (`Document/LayersPanel.swift:20–30`) and angle/sliders in generated forms.
- **Selection:** `Inspector/InspectorView.swift:94–115,141–153` packs three decision chips or four marks plus basket into single HStacks. Titles have one-line truncation (so likely truncation rather than overlap); keys/swatches/padding retain width. Test a long basket name and grade captions at 288 pt.
- **Layers locks:** `Document/LayersPanel.swift:32–47` combines Lock, four fixed 20 pt icons, a minimum spacer and fixed 96 pt Filter field. It has much less spare width than the full inspector suggests, but no hard contradiction at 264 pt was established.
- **Masks:** `Inspector/MasksPanel.swift:77–106` puts every component glyph in an unbounded HStack beside a 56 pt thumbnail; many components consume name/control width. Test composite masks, not only one component.

### R4 — Center-pane filter and loupe chrome have no compact arrangement

**Risks.** `Library/FilterBar.swift:16–62` requires a minimum 180 pt rule field plus fixed-size match count, Clear and Save as Smart Album. Only the lower facet row scrolls. Narrowing center with both side panels on forces the top row to compete; diagnostic text can vanish/truncate before essential controls can shrink. The rule-field diagnostic correctly has `.lineLimit(1)` and a help tag (`:26–33`).

`Shell/ContentView.swift:426–456` fixes loupe info strip at 32 pt, but filename Text lacks a one-line policy while chips, proof state, info and Masks share its HStack. Long names can wrap into the fixed height or be compressed unpredictably. `Loupe/MaskToolbar.swift:15–38,105–119` puts many tools on one non-scrolling row and brush sliders on another; slider widths/readouts are fixed. The existing top padding of 36 (`:53`) **does** respect the intended 32 pt information strip: do not report the historical top-offset bug as still present absent screenshots.

### R5 — Document palette is vertically fixed while options scroll only sideways

**Risk near minimum height; certain finite-height exhaustion with additional shell strips.** `Document/Tools/ToolsPalette.swift:13–33` lays all tool groups vertically with no scroll. Core defines 14 slots (`apps/mac/Sources/TesseraCore/Document/Tools/EditorTools.swift:86–89`), plus Remove after Heal. The fixed 28 pt icons, separators and swatches make a tall column. `Document/DocumentView.swift:13–24` adds outer padding and an always-laid-out 28 pt ZoomHUD (opacity hides drawing, not layout). Tether/progress strips can further reduce center height (`Shell/ContentView.swift:19–20,60–68`). Capture palette bottom at minimum height. In contrast the options bar deliberately scrolls horizontally (`ToolsPalette.swift:166–171`), so its many `.fixedSize()` fields and toggles are not standalone overflow defects.

## Cross-cutting policy inventory and non-findings

- **Theme heights are fixed, not adaptive:** `App/Theme.swift:158–186` defines 20/24/28 pt controls, 32 pt sections/sliders/progress, 24 pt status, 72 pt filmstrip, and fixed width ranges. These are valid density tokens; they do not prove content fits. Font tokens are explicit point sizes (`:195–227`), so “Dynamic Type enlarged everything” is not an established explanation.
- **Buttons/chips:** ThemeButtonStyle explicitly uses one-line labels and bounded height (`App/Components.swift:33–38`); Chips similarly (`:252–258`); normal SegmentedPicker labels have one-line limits, flexible fill and help tags (`:180–197`). These mainly risk truncated labels, not intrinsically expanding every inspector. Forced `.fixedSize()` at a caller materially changes that conclusion.
- **InfoRow:** fixed 72 pt label, middle-truncated one-line value (`Components.swift:336–342`) is an explicit width policy. Label has no line limit; a long label may wrap, but the row itself has no fixed height.
- **Hint vs StatusLine:** Hint preserves wrapped height with vertical-only fixedSize (`Components.swift:370–373`); StatusLine (`:393–399`) has neither an explicit line limit nor intrinsic-height preservation. The same error string may wrap in one context and compress in another; test each fixed-height/container use.
- **ProgressStrip:** fixed 160 pt bar, title, counts/detail, current file and trailing actions inside 32 pt row (`Components.swift:472–489`). Only current filename has one-line middle truncation. Narrow center / long counts/detail may pressure title and trailing controls. Multiple simultaneous progress strips also reduce document palette/loupe height.
- **GeometryReader inventory:** shell `Shell/ContentView.swift:22,55` fixes ZStack to viewport without adding `.clipped()`; oversized overlay controls do not enlarge that geometric budget safely. Filters `Document/Filters/FilterSheets.swift:125,163,196` use geometry for dial, point pad, and detail rendering with explicit caller dimensions. PointPad centers a 12 pt puck at the edge for x/y=0 or 1 (`:173–175`): half-puck outside bounds is a drawing-edge risk, not a generic layout expansion. Suggested confidence `Inspector/UnderstandingPanels.swift:119–125` is an overlay and clipped to its rounded chip. Lens Blur `Inspector/LensBlurPanel.swift:263–298` uses bounded scope geometry; handle offsets are clamped, not negative layout-spacing hacks.
- **Offsets:** only three lexical SwiftUI `.offset(...)` calls in the scanned source. ColorSwatches (`Document/Tools/ToolsPalette.swift:54,61`) deliberately offsets a 20 pt square by 8 inside a 28 pt allocation, a consistent footprint. Lens Blur selection band and handles (`Inspector/LensBlurPanel.swift:283,288`) are in-scope positioning; no shell text alignment offset hack was found.
- **Other surfaces checked pragmatically:** photo inspector outer ScrollView (`Inspector/InspectorView.swift:11–46`) prevents the document-inspector vertical stack defect; keyword FlowRow remains a horizontal exception. People grid scrolls; detail header's 220 pt name field plus actions and fixed Move Targets column deserve narrow-center testing (`People/PeopleView.swift:277–295,306–339`). Tether incoming tiles explicitly clip fill images (`Tether/TetherPanel.swift:423–425`), intentional crop rather than lost controls. Compare, grid, sidebar, loupe renderer and document viewport are predominantly AppKit-backed; their internal geometry is outside this SwiftUI-focused report and should be correlated with the parent's AppKit audit.

## Sheet coverage / fixed-size envelope index

This is a source inventory, not a claim that every fixed sheet is defective. Scroll containers listed here are important mitigating evidence.

| Surface | Envelope / evidence | Main limitation or mitigation |
|---|---|---|
| Export | `Export/ExportSheet.swift:98`, 640 × 760 | Grouped Form at :56–72 provides scrolling; top preset row and bottom errors remain outside Form. |
| Print | `Print/PrintSheet.swift:20–32,62`, 900 × 660 | 400 pt preview beside grouped Form; form controls have a narrower actual budget than sheet width. |
| Lightroom import | `Import/LightroomImportSheet.swift:94`, 820 × 620 | Step-specific ScrollViews at :123,193,407,480; fixed mapping columns :320–332 deserve long-title checks. |
| Smart album | `Library/SmartAlbumSheet.swift:67–71,100`, width 620, rule scroll 180–320 high | Vertical scrolling protects height; nested rails consume width, fixed 120/84 pt picker pair at :182,188 leaves decreasing value-field width through depth 8 (:140). |
| Auto Edit | `Agent/AutoEditSheet.swift:76`, 560 × 600 | Form-driven content; error/long provider strings remain state-specific checks. |
| Agent review | `Agent/AgentReviewSheet.swift:46`, 720 × 560 | List-backed content; multi-action rows :62–99 can truncate text. |
| Defect sweep | `Cull/DefectSweepSheet.swift:72`, 560 × 520 | Fixed facet columns :86–99, list rows need long-filename checks. |
| Photo Merge / Enhance | `Photo/PhotoMergeSheet.swift:61`, 820 × 560; `Photo/EnhanceSheet.swift:90`, 520 × 600 | Merge scrolls options only; Enhance content ScrollView :19. |
| New / flat export / Save As document | `Document/DocumentSheets.swift:57,112,225`, 460 × 400 / 460 × 320 / 520 × 330 | Fixed form envelopes; filename/path length checks, no proven ordinary-state contradiction established. |
| Filters / destructive adjustments / blending | `Document/Filters/FilterSheets.swift:52,267,310`, 560 × 300 / 380 × 420 or 560 / 360 × 200 | Generic filter non-scroll risk R1; adjustment editor scrolls. |
| Tool sheets | `Document/Tools/ToolsSheets.swift:89,117,142,176` | Fixed token-derived sizes; warning text can change vertical requirements. |
| Channel sheets | `Document/Channels/ChannelSheets.swift:121,162,256,314`, token-derived 412 × 300 | Fixed label-column forms; native radio groups and spot notes require state-specific fit checks. |
| Neural filters | `Document/Retouch/RetouchViews.swift:167`, 680 × 460 | Non-scrolling detail; see R2. |
| Layer Style floating panel | `Document/Styles/LayerStyleInspector.swift:34–56` | Minimum 600 × 520; 216 pt effect column, scrollable editor and effect list (:39,:88). Minimum is not a fixed maximum. |

## Parent reproduction order (not performed here)

1. Minimum-height document: History expanded, Channels toggled, snapshots added. Capture entire inspector including bottom.
2. Minimum-width engine-backed library: both side panels, tether open; then hide inspector to separate center-width pressure from intrinsic control defects.
3. Inspector at 288: Soft Proof long ICC name, Point curve, three-way grading with extreme readouts, long keyword, long basket target.
4. Generic filter sheet with controls/error, neural missing-model detail, merge with multiple warnings. Include complete header/body/footer.
5. Document long profile + selection + render readout; library full decision state + long basket; many document tabs. Record actual toolbar overflow separately from clipped status text.

## Complete lexical modifier inventory

Inventory scans all 107 `.swift` files in `Sources/Tessera`; the table includes the 47 SwiftUI-importing files with matching constructs. Counts are occurrences, **not defect counts**. Frame entries include explicit width/height and minimum constraints (numeric, token, or geometry-derived), exclude pure max-only flexible frames, and are matched across line breaks with balanced parentheses. Text policy includes lineLimit, truncationMode, minimumScaleFactor, allowsTightening and layoutPriority; no minimumScaleFactor/allowsTightening occurrences were found. HStacks are candidates only; horizontal scrolling and flexible/truncating labels frequently make them safe. Comment/string lexical matches are not AST-validated.

Totals: **fixedSize: 65**; **fixed/min frames: 314**; **text policy: 68**; **GeometryReader: 6**; **offset: 3**; **HStack: 233**.

| File | fixedSize | Fixed/min frame | Text policy | GeometryReader | offset | HStack |
|---|---|---|---|---|---|---|
| `Agent/AISettingsView.swift` | — | 41, 47, 56, 145 | 130 | — | — | 39, 45, 76, 121, 143 |
| `Agent/AgentPanels.swift` | 89, 162 | 27, 29, 78 | 22 | — | — | 20, 37, 73, 148, 165, 173 |
| `Agent/AgentReviewSheet.swift` | — | 46, 67 | 71, 80, 82 | — | — | 62, 70, 89, 126 |
| `Agent/AutoEditSheet.swift` | — | 76 | — | — | — | 85 |
| `App/Components.swift` | 106, 150, 373 | 38, 108, 151, 190, 202, 227, 258, 299, 337, 358, 383, 478, 489 | 35, 186, 255, 338, 422, 484 | — | — | 93, 175, 180, 288, 336, 353, 417, 433, 472, 507 |
| `App/TesseraApp.swift` | — | 14 | — | — | — | — |
| `Cull/AssistPanels.swift` | 26 | 116 | — | — | — | 14, 30, 41, 56, 91, 103 |
| `Cull/DefectSweepSheet.swift` | — | 39, 72, 90, 92, 97, 99, 123 | 126, 128 | — | — | 30, 86, 114 |
| `Cull/FaceStripView.swift` | — | 27, 50, 122, 126, 137, 191 | — | — | — | 15, 18, 38, 49, 124, 193, 198, 206 |
| `Document/AdjustmentEditors.swift` | — | 51, 64, 68, 77, 93, 489, 501, 508, 509 | — | — | — | 62, 144, 163, 207, 242, 270, 352, 452, 495, 519 |
| `Document/Channels/ChannelSheets.swift` | 52 | 28, 121, 162, 256, 314 | — | — | — | 26, 77 |
| `Document/Channels/ChannelsPanel.swift` | — | 23, 28, 77, 96, 127, 154 | 117 | — | — | 47, 91 |
| `Document/DocumentHistoryPanel.swift` | — | 26, 42, 76, 83 | 36, 79 | — | — | 34, 45, 74 |
| `Document/DocumentSheets.swift` | 30, 76 | 57, 85, 112, 225 | 196 | — | — | 81, 192 |
| `Document/DocumentView.swift` | 165, 227, 230, 233, 240, 245, 254 | 18, 78, 105, 112, 136, 154, 162, 182, 201, 259, 264 | 106, 113, 189, 190, 236, 250 | — | — | 15, 56, 131, 147, 180, 223 |
| `Document/Filters/FilterSheets.swift` | — | 19, 52, 70, 74, 78, 83, 94, 100, 102, 112, 174, 202, 267, 284, 296, 310 | — | 125, 163, 196 | — | 17, 72, 81, 98, 282 |
| `Document/LayersPanel.swift` | — | 24, 30, 44, 55, 106, 132 | — | — | — | 16, 20, 32, 70 |
| `Document/PropertiesPanel.swift` | — | 18, 131, 255, 259 | — | — | — | 16, 171, 253 |
| `Document/Retouch/RetouchViews.swift` | 30, 37, 44, 63, 66, 86, 181, 216, 225, 229, 237, 247 | 23, 139, 167, 185, 208 | 179 | — | — | 137, 147, 176 |
| `Document/Styles/LayerStyleEditors.swift` | — | 111, 121, 127, 145, 153, 157, 166, 214, 222 | — | — | — | 23, 117, 151 |
| `Document/Styles/LayerStyleInspector.swift` | — | 36, 38, 56, 125, 163, 193, 211, 217, 222, 233, 276, 283, 288 | 151 | — | — | 34, 138, 175, 209, 235, 272 |
| `Document/Styles/LayerStyleMenus.swift` | — | 82 | — | — | — | 73, 89 |
| `Document/Tools/ToolsInspector.swift` | — | 29, 33, 37, 90, 115 | 117 | — | — | 27, 44, 130 |
| `Document/Tools/ToolsPalette.swift` | 126, 141, 171, 180, 205, 219, 226, 262, 267, 319, 322, 327, 330, 332 | 29, 47, 61, 121, 164, 169, 195, 243, 301, 317, 347, 357 | — | — | 54 | 63, 114, 175, 344 |
| `Document/Tools/ToolsSheets.swift` | — | 33, 89, 117, 142, 176 | — | — | — | — |
| `Export/ExportSheet.swift` | 149, 219, 252, 416 | 23, 54, 98, 167, 209, 233, 324, 379, 429 | 50, 112, 337 | — | — | 27, 110, 127, 164, 206, 226, 274, 298, 321, 335, 375, 395, 401 |
| `Export/ExportWatermarkViews.swift` | 104 | 33, 34, 67, 83, 105, 123 | — | — | — | 16 |
| `Import/LightroomImportSheet.swift` | — | 94, 174, 321, 322, 332, 390, 466, 470, 504 | 221, 275, 431 | — | — | 16, 173, 219, 244, 320, 352, 377, 426, 430, 503 |
| `Inspector/DevelopPanels.swift` | 115, 121 | 129, 225, 266, 267, 268, 269, 274, 275, 280, 281, 284, 285, 349, 351, 352, 353, 357, 362, 365, 392, 394, 398, 429, 492, 493, 501, 577, 660, 665, 688, 692, 713 | 558, 620, 704 | — | — | 111, 132, 215, 271, 279, 338, 383, 459, 480, 494, 557, 584, 618, 658, 682, 695 |
| `Inspector/InspectorView.swift` | — | 13, 143, 153, 196 | 148, 188 | — | — | 94, 99, 106, 141, 174 |
| `Inspector/LensBlurPanel.swift` | — | 31, 115, 126, 134, 153, 163, 282, 287, 296 | 236 | 263 | 283, 288 | 102, 118, 140, 154, 216, 290 |
| `Inspector/LibraryPanels.swift` | 191 | 78, 95, 112, 198, 222 | 101, 199, 200, 225 | — | — | 39, 68, 93, 197, 221 |
| `Inspector/MasksPanel.swift` | — | 102, 137, 178, 182, 192 | 88, 194, 204 | — | — | 19, 77, 90, 147, 170, 190, 203 |
| `Inspector/TransformPanel.swift` | — | 196, 210, 279, 294 | 272, 286 | — | — | 183, 244, 266, 270 |
| `Inspector/UnderstandingPanels.swift` | 50, 228 | 113, 122, 159, 190 | — | 119 | — | 22, 46, 77, 81, 158, 172, 189, 209 |
| `Library/FilterBar.swift` | 40, 47, 53, 58, 167 | 22, 172, 174 | 30, 31, 125, 152 | — | — | 16, 64, 171, 178 |
| `Library/RuleTextField.swift` | — | — | 122, 125 | — | — | — |
| `Library/SmartAlbumSheet.swift` | 129 | 71, 100, 119, 182, 188 | — | — | — | 116, 121, 172 |
| `Loupe/MaskToolbar.swift` | — | 37, 42, 46, 59, 98, 117, 119 | — | — | — | 15, 41, 80, 106, 115 |
| `Loupe/SoftProof.swift` | — | 159, 168 | — | — | — | 158, 167, 174 |
| `People/LibrarySettingsView.swift` | — | 23 | — | — | — | — |
| `People/PeopleView.swift` | — | 72, 119, 139, 159, 190, 223, 295, 313, 339, 357, 369, 381, 389, 428, 452, 460 | 189, 380, 455 | — | — | 64, 99, 144, 193, 277, 306, 450 |
| `Photo/EnhanceSheet.swift` | — | 29, 90 | 74 | — | — | — |
| `Photo/PhotoMergeSheet.swift` | — | 29, 61, 165, 169, 180, 199, 221 | 45 | — | — | 24, 145, 163, 209 |
| `Print/PrintSheet.swift` | 108, 151, 176 | 22, 62 | — | — | — | 20, 73, 94, 119, 165 |
| `Shell/ContentView.swift` | 159, 355, 387, 394 | 55, 75, 175, 310, 348, 391, 400, 406, 456, 517 | 372, 445, 461, 492, 508 | 22 | — | 171, 185, 339, 341, 346, 360, 383, 389, 426, 491, 495 |
| `Tether/TetherPanel.swift` | — | 53, 76, 85, 104, 118, 142, 167, 237, 260, 273, 288, 353, 377, 389, 401, 424, 436, 462, 473 | 18, 25, 106, 146, 186 | — | — | 45, 52, 83, 103, 132, 143, 159, 196, 228, 284, 314, 348, 355, 399, 454 |

### fixedSize interpretation

The table locates every call. Vertical-only preservation: `App/Components.swift:373`; `Agent/AgentPanels.swift:89,162`; `Inspector/LibraryPanels.swift:191`; `Inspector/UnderstandingPanels.swift:228`; `Document/Tools/ToolsPalette.swift:171`; `Document/Retouch/RetouchViews.swift:225,229,237,247`. Remaining calls are unqualified two-axis intrinsic sizing. The risk depends on whether the parent can scroll, truncate siblings, or grow; the modifier itself is not a defect.
