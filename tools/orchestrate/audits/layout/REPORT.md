# Tessera layout correctness audit

## Executive answer

**Keep the native stack. Fix its layout contracts and the way features are composed.** This audit found real failures in the use of SwiftUI/AppKit, not evidence that Swift is inherently incapable of laying out a photo editor. Swift is a language; SwiftUI and AppKit are UI frameworks. React would replace the view layer, not automatically solve width budgets, intrinsic-size overflow, text measurement, scrolling, or minimum window dimensions.

The highest-impact reproduction is document mode at a 1280 × 800 window. The real `ContentView` and its native split views, hosted by a non-activating audit runner, ask for **1318 × 817 pt of content** inside **1280 × 748 pt**. The split-view subtree is actually placed at **x = −19, y = −34**. This is not a cosmetic font problem: whole regions extend outside the host. The Properties heading runs into the toolbar and the right-hand Fill control is clipped. At 1440 × 900 and 1728 × 1117, that same default document state fits. Additional expanded sections and snapshots can increase its height again.

A counterfactual layout probe isolates the horizontal trigger: retaining `DocumentStatusBar` yields a 1318 × 817 fitting size; omitting only that row from the otherwise identical probe yields 960 × 817. The status bar's width demand is therefore expanding the whole workspace, while the inspector independently dictates the excessive height. See `budget-probe.swift` and `budget-probe.log`; this experiment changes only an audit fixture, not application source.

A second, smaller reproduction uses the production `ValueSlider` drawing code: the 80 pt-wide Highlights slider has effectively zero space between its title and −100 readout. Shared menus and custom keyword wrapping also have source-proven ways to exceed their proposed width. These defects can multiply across many panels even when every view uses the approved colors, fonts, and spacing tokens.

**The existing design system enforces visual consistency more than layout correctness.** `ThemeLintTests.swift:15–31` bans raw style values and even native segmented controls, but does not check containment, text fit, window-size composition, or unreachable controls. A perfectly token-compliant view can still be larger than its window.

## Scope, provenance, and important limits

**Audit baseline:** commit `84fc6f23abeea5365394bb1dff0449c2ac85f33b`. All 181 Swift-source hashes in `source-manifest.json` match that commit's files in `audited-source-84fc6f2.tar.gz`. File:line citations refer to this pinned baseline, not a moving branch. While the release build was running, other work advanced `main` to `14eda8254e73117d58daeec18b889470f6d69054` and changed source files; see `concurrent-source-changes.json`. The audit does not certify those later changes. The archived source preserves the cited lines even if current checkout line numbers drift.

All authored audit files are under `tools/orchestrate/audits/layout/`. No application source was intentionally edited. The explicitly requested `build-ffi.sh` generates/copies FFI bindings as part of its normal build; tracked-source diffs are checked separately. Source paths below are relative to `apps/mac/Sources/Tessera/` unless qualified. The actual design-system paths are `App/Theme.swift` and `App/Components.swift`, not the older top-level paths in the request.

Supporting evidence:

- [SwiftUI source inventory](swiftui-findings.md): systematic modifier inventory, sheet envelopes, detailed source findings.
- [AppKit source review](appkit-findings.md): manual layout, drawing, intrinsic sizing, native controls, and hosting bridges.
- [Build log](build.log), [isolated build log](isolated-build.log), [window runner log](window-probe.log), and `frames-*.json` native-frame dumps.
- [Historical evidence manifest](evidence-manifest.json): 440 PNGs discovered. A representative 132-image contact-sheet sample was generated, followed by full-size inspection of targeted files. This is not a claim to have pixel-audited all 440 screenshots.
- [Font measurements](font-metrics.json): actual AppKit SF font metrics, not estimated character widths.

### How fresh visual reproduction was done without stealing focus

`App/TesseraApp.swift:108` calls `NSApp.activate()` unconditionally. Therefore the requested `open -g -n ...` is **not a reliable guarantee of background behavior**. `--front` at `:173–176` does not bypass the earlier activation. The retouch self-test explicitly activates too (`Document/Retouch/RetouchSelfTest.swift:125–126`). I did not use those launch paths while the owner was working.

Instead, `window-probe.swift` links Tessera's compiled UI objects, excluding only `TesseraApp.swift.o` and its application startup. It hosts the real `ContentView`, with the same 960 × 600 minimum wrapper, in AppKit windows. The audit process sets activation policy to `.prohibited`, orders only its own windows to the back, does not make them key, sets exact outer window sizes, and captures each with `/usr/sbin/screencapture -l <windowNumber> -x`. Every final capture logs `frontmostIsProbe=false`. Windows are ordered out after capture.

Library reproduction uses the real engine with the generated 40-image sample shoot. RAW reproduction uses the real engine with `fixtures/raw` and reports five items. The document reproduction deliberately uses the app's existing stub-document backend with sample layers, exercising the same document layout without destructive edits to user documents. The runner's state is isolated through `TESSERA_APP_DIR` under this audit directory.

**This is a real-view integration harness, not a normal release-app launch.** It forces sizes programmatically, so it establishes what happens when those sizes are presented, not whether every ordinary mouse resize would permit them. A normal SwiftUI window may refuse a size below the effective minimum instead of clipping. Either behavior exposes the mismatch with the declared 960 × 600 minimum. Release startup, focus management, every interactive sheet state, and every feature combination are not certified by these captures.

Initial `cacheDisplay` attempts on an offscreen full window produced missing/native-layer artifacts. Files named `host-*` are rejected diagnostic captures and must **not** be treated as evidence of app rendering defects. Per-window compositor captures named `window-*` supersede them. The small slider and scaffold component captures are valid, separately labeled probes.

### Capture matrix

The names encode outer window dimensions in points, not bitmap dimensions. Titlebar/toolbar consume 52 pt in the final runner. `screencapture` includes window shadow; all saved images are downscaled to at most 1400 px wide. Scaling a saved image is not a substitute for resizing the window: the runner actually set each size before capture.

| State | 1280 × 800 | 1440 × 900 | 1728 × 1117 |
|---|---|---|---|
| 40-image engine library | `shots/window-library-1280x800-{dark,light}.png` | `shots/window-library-1440x900-{dark,light}.png` | `shots/window-library-1728x1117-{dark,light}.png` |
| Engine RAW fixture folder | `shots/window-raw-1280x800-{dark,light}.png` | `shots/window-raw-1440x900-{dark,light}.png` | `shots/window-raw-1728x1117-{dark,light}.png` |
| Sample layered document | `shots/window-document-1280x800-{dark,light}.png` | `shots/window-document-1440x900-{dark,light}.png` | `shots/window-document-1728x1117-{dark,light}.png` |

The engine-library and RAW states fit at the requested sizes in the final matrix. A partly visible Keywords row at the bottom of their inspector is ordinary content inside an outer ScrollView, **not an inaccessible-control defect**. Ellipsized status strings and hidden scroll content are not counted as overlap. The document state does not fit the smallest size. Absence of a defect in these default states does not clear long names, expanded panels, multiple jobs, or error states.

## A. Catalogue of findings

Evidence grades: **V** = observed in a fresh runner/component capture and/or measured live frames; **G** = deterministic source geometry; **C** = conditional overflow contract with an explicit triggering input; **R** = risk needing a further state-specific reproduction. A screenshot reference marked “context only” is not proof of that risk. These are finding sites, not a count of every affected pixel, repeated component instance, or possible application state.

### Confirmed and conditional findings

| ID / impact | Instance and evidence | File:line cause and mechanism |
|---|---|---|
| D01 / high / V | **Document Properties/header intrudes into toolbar at 1280 × 800.** See `shots/window-document-1280x800-dark.png` and light equivalent. Native subtree height 817 exceeds host height 748, with its top at −34. At the larger two sizes the same default state fits. | `Document/DocumentView.swift:97–120`: non-scrolling outer inspector with Properties min 192 and Layers min 256, followed by Channels and History. `Document/DocumentHistoryPanel.swift:15–53` adds a fixed 144 pt history list and snapshot content outside that list. `App/Components.swift:283–312` adds section headers/padding. `App/TesseraApp.swift:14` declares only 600 pt minimum content height. Local list scrolling cannot shrink the outer sum. |
| D02 / high / V | **Document Fill readout/knob and far-right layer actions clip at the right edge.** Same small-window shots; `frames-document-1280x800-dark.json` places a 1318 pt split subtree at −19 inside a 1280 pt host. This is global horizontal overflow, not an isolated Fill font error. | **Isolated trigger: `DocumentStatusBar`, `Document/DocumentView.swift:223–264`, especially `.fixedSize()` groups at :227,230,233,240,245,254.** In `budget-probe.log`, the same document shell fits at 1318 × 817 with this row and 960 × 817 without it. `Shell/ContentView.swift:67–68` installs the row into the detail column, then :115–124 adds the inspector. The noncompressible status width propagates through the split hierarchy. The paired controls at `Document/LayersPanel.swift:20–30` are victims of the shifted root; widening or shrinking Fill alone would miss the cause. |
| D03 / medium / V | **Highlights label and −100 readout run together at minimum three-way width.** `shots/probe-sliders-dark.png`, light equivalent. At width 80, title = 53.0234375 pt and value = 27 pt, yielding −0.0234375 pt nominal gap. At ideal 82.67 width the gap is only 2.64 pt; this is crowding, not proof of overlapping glyph ink. At 110.67 it separates well. | `Inspector/ValueSlider.swift:129–138`: two unbounded `draw(at:)` calls, no reserved inter-label gap or text rectangles. `:59` advertises height but no intrinsic minimum width. `Inspector/DevelopPanels.swift:271–278` allocates three controls across a narrow inspector; `App/Components.swift:307–309` consumes the gutters. |
| D04 / medium / G | **Compare caption and decision frames intersect by 12 pt at every pane width.** No screenshot of the long-filename + combined-badge trigger was captured. At pane width 300, caption spans 12…192, badge spans 180…288. Actual text collision depends on both fields filling their allocations. | `Compare/CompareView.swift:245–247`: caption origin gets the 12 pt inset but caption width remains `0.6W`; badge starts at `0.6W`. `:157,174–177` allow middle-truncated filename and decision plus Suggested. One shared partition calculation, not independent percentages, is required. |
| D05 / medium / C | **Themed menu can paint/layout wider than a narrow row.** Long/wide ICC profile names in Soft Proof and long schema choices are the triggers. Not visually reproduced; source proves refusal of horizontal compression, not that all menu labels fail. | `App/Components.swift:93–108` applies two-axis `.fixedSize()` to menu plus chevron. `:130–138` truncates by character count, not measured width. `Loupe/SoftProof.swift:158–165` also reserves a 56 pt label and Other action. `.frame(maxWidth: .infinity)` on a parent does not override a descendant's intrinsic demand. |
| D06 / medium / C | **One oversized keyword chip escapes FlowRow's reported width.** Trigger: an unbroken keyword whose intrinsic chip width exceeds the inspector interior. No trigger screenshot; ordinary Keywords screenshots are context only. | `Inspector/LibraryPanels.swift:136–155` measures with `.unspecified`, reports the container width, then places the unbounded measured width. Wrapping to another row does not reduce an oversized item's width; the first item is not wrapped at all. `:20–26,67–81` feed arbitrary keyword strings into fixed-height chips. |
| D07 / low / G | **Full-saturation color-wheel handle extends 4 pt outside its view.** No endpoint screenshot; source arithmetic proves paint overflow, not whether a particular ancestor clips or allows bleed. | `Inspector/ColorWheelView.swift:30–38,64–71`: disc inset is 2 pt but outer puck radius including stroke is 6 pt. At a cardinal rim endpoint the extra 4 pt is unreserved. Callers at `Inspector/DevelopPanels.swift:266,274,280` include both small and large wheels. |
| D08 / low / G | **Dragging slider endpoint ring extends 0.5 pt outside its allocation.** No dragging screenshot. Keep this separate from the much more serious text collisions. | `Inspector/ValueSlider.swift:108–111,161,171–172`: travel accounts for thumb radius, but the 2 pt drag stroke around a path inset by only 0.5 crosses the edge. Ordinary 1 pt ring fits. |
| D09 / low / G | **Unselected point-curve endpoint knot stroke extends 0.75 pt outside the view.** No endpoint screenshot. | `Inspector/CurveEditorView.swift:59–62,197–205`: 4 pt plot inset versus 4 pt knot radius plus half a 1.5 pt stroke. Selected fill-only knots do not have that extra stroke. |
| D10 / medium / G | **Hidden sidebar badge retains a blank width slot, causing avoidable folder-name truncation.** Long repeated folder names at sidebar minimum are the relevant trigger; not photographed in this audit. | `Sidebar/SidebarView.swift:623–647,675–678`: ordinary constrained siblings, not arranged stack subviews. Hiding badge/detail does not deactivate the 16 pt badge constraint or surrounding gaps. Title/detail have deliberately lower compression resistance, so they pay for the invisible slot. This is premature truncation, not overlapping frames. |

### Additional bounded risks and non-reproduced states

| ID | Trigger / symptom to test | Code cause and evidence status |
|---|---|---|
| R01 | Tether session fields with both side panels at minimum window width | `Tether/TetherPanel.swift:131–181,210–213`: 200 pt session field + 240 pt naming field + gaps/menu/gutters needs at least 504 pt before an album column. The declared 960-wide shell cannot leave that much center space while retaining both panels. Static budget contradiction; actual response may be a larger minimum rather than overlap. No screenshot. |
| R02 | Long basket/profile names, multiple selected items, render/selection readouts in status bars | `Shell/ContentView.swift:355,387,394,409–416`; `Document/DocumentView.swift:226–254`: noncompressible groups in a fixed-height, non-scrolling row. A sufficiently long name must exceed finite width. Final default engine states fit; see window library/raw shots as controls, not failures. |
| R03 | Many document tabs plus full toolbar | `Document/DocumentView.swift:147–165,185–191`; `Shell/ContentView.swift:151–181,184–263`: tab titles are capped but aggregate tab count is not, and the entire group is fixed-size. A thumbnail-slider slot remains laid out even in document mode. Native toolbar overflow may absorb items, so no assertion that every crowded toolbar overlaps. No stress screenshot. |
| R04 | Generic filter sheet with more schema controls or error text | `Document/Filters/FilterSheets.swift:14–52`, `App/Components.swift:415–442`: 560 × 300 envelope, 180-high preview, body padding, header/footer, no body scroller. Historical `shots/historical-B5-05-filters-01-gaussian-dialog.png` has footer buttons extremely close to the lower boundary. **A fresh geometry-only scaffold fixture did not reproduce clipped buttons** (`shots/probe-scaffold-{dark,light}.png`), so ordinary Gaussian Blur clipping is not confirmed. More-control/error states remain a risk. |
| R05 | Neural filter with long model path, limitations, unavailable-output hints, and errors | `Document/Retouch/RetouchViews.swift:133–167,198–249`: fixed 680 × 460 sheet, non-scrolling detail column, multiple texts preserving wrapped height. Historical `shots/historical-B5-09-retouch-11-neural-skin-no-faces.png` shows readable main controls and footer; this is a negative control, not proof of overflow. |
| R06 | Merge with many backend warnings | `Photo/PhotoMergeSheet.swift:24–37,61,75–88`: options scroll but the 400 × 300 preview/warning column does not; 820 × 560 outer envelope. No populated-warning screenshot. |
| R07 | Point-curve header at minimum inspector width | `Inspector/DevelopPanels.swift:111–124`: both the Parametric/Point and RGB/R/G/B/L segmented controls are fixed at ideal size in one HStack. Needs actual composed width measurement. No confirmed screenshot. |
| R08 | Narrow center filter and loupe chrome, long filename, many mask components | `Library/FilterBar.swift:16–62`; `Shell/ContentView.swift:426–456`; `Loupe/MaskToolbar.swift:15–59,105–119`; `Inspector/MasksPanel.swift:77–106`. These retain fixed fields/icons/tools with no compact alternative; loupe filename lacks a one-line policy in a 32 pt strip. Lower filter facets and tool options already have horizontal scrolling and are not the same defect. No stress screenshot. |
| R09 | Deeply nested decorated layer rows | `Document/LayersOutline.swift:65–72,636–673,700–735`: 12 pt indent per level and fixed icon/mask/style/lock furniture eventually exhaust name width. Ordinary rows deliberately truncate and use NSStackView; do not misdiagnose them as the sidebar hidden-slot bug. No deep-nesting screenshot. |
| R10 | Very narrow loupe / long histogram backend message | `Loupe/LoupeToolOverlay.swift:153–170,248–249`; `Inspector/HistogramView.swift:34–40,102–105`: unbounded drawn strings; crop chip has a height threshold below which it extends outside the canvas. No trigger screenshot. |
| R11 | Minimum-height document plus extra job/tether strips | `Document/Tools/ToolsPalette.swift:13–33`; `Document/DocumentView.swift:13–24`; `Shell/ContentView.swift:19–20,60–68`: non-scrolling vertical palette loses available height. Hidden ZoomHUD uses opacity, retaining layout space. No combined-state screenshot. |
| R12 | Very large image coordinates / backing-scale transition | `Inspector/DetailPreviewView.swift:48–71`: 88 pt coordinate label and explicit layout-dependent backing-size updates. Test long coordinates and 1×↔2× move; no confirmed failure. |

### Historical screenshot interpretation

`shots/historical-B5-08-channels-1-panel-overlay.png` shows toolbar/inspector interference and clipped shell edges consistent with an oversized content subtree. It is useful corroboration of D01/D02, but its original window dimensions and exact source revision were not verified. `shots/historical-B5-05-filters-01-gaussian-dialog.png` also has left/right shell-edge clipping; do not count those crops as independent filter-specific failures.

`shots/historical-M2-48-lens-blur-panel.png` has a partly visible Refine section at the bottom. Because the photo inspector has an outer ScrollView (`Inspector/InspectorView.swift:11–46`), that observation alone is normal scrolling, not proof that Refine is unreachable. Some historical screenshots contain another application's overlay or incomplete window framing. Those occlusions are not attributed to Tessera's layout code.

## B. Root-cause patterns, ranked

Counts below are catalogue entries assigned to one primary cause, not modifier counts. Confirmed/conditional entries and unverified risks are deliberately separated. This prevents a list of possible stress cases from masquerading as dozens of reproduced defects.

| Rank by D-entry count | Pattern | Confirmed/conditional sites | Additional risk sites |
|---|---|---|---|
| 1 | Manual drawing/partition math does not reserve the full text/paint envelope | 5: D03, D04, D07, D08, D09 | R10, R12 |
| 2 | Intrinsic-size/visibility contracts ignore the available width | 4: D02, D05, D06, D10 | R02, R03, R07, R08, R09 |
| 3 | Independently reasonable panel minimums are composed into an impossible window budget | 1: D01 | R01, R04, R05, R06, R11 |

**Priority is not identical to count.** Fix pattern 3 first: a single expanding subtree moves entire regions into the toolbar and outside the window. Then remove shared width-refusal behavior in menus, status groups, and chips. Treat the half-pixel endpoint issues as polish after usable controls are guaranteed.

The lexical source inventory found 65 `fixedSize` calls and 314 fixed/min-frame occurrences in its SwiftUI-focused scan, but these are **not 379 defects**. Many are correct image/handle/icon geometry, deliberate intrinsic sizing in a scroll view, or minimum constraints rather than hard maximums. Similarly, lack of `.minimumScaleFactor` is not a defect by itself; making every label tiny would hide the problem rather than solve it.

### What is not supported by the evidence

- No evidence that GeometryReader is universally wrong. The shell uses it to allocate a canvas, and most geometry readers are bounded scopes, dials, or point controls.
- No epidemic of `.offset` or absolute text positioning in SwiftUI. The inventory finds only three lexical offsets; the principal shell problem is negotiated minimum size, not dozens of magic offsets.
- No basis to say all custom NSViews lack intrinsic size or Auto Layout. ValueSlider advertises its row height; sidebar and layer rows have constraints; native outlines and grid reuse are appropriate.
- No proven universal `NSHostingView.sizingOptions` bug. The floating style panel creates a resizable NSPanel with an explicit content minimum (`Document/Styles/DocumentStyles.swift:243–253`). The runner retains default hosting sizing options in its final matrix. Disabling sizing options is not a general fix for overflowing descendants.
- Dynamic Type did not cause the measured failures. Fonts are explicit point-size tokens (`App/Theme.swift:195–227`). The missing policy is how to support larger text/localized content, not evidence that Dynamic Type silently enlarged this session's labels.
- Absence of `layoutPriority` is not a universal cause. The document inspector already assigns priorities; priorities cannot make incompatible minimums fit.

## C. The framework question

### Is the team using the frameworks incorrectly?

**In specific, important places, yes.** Two-axis `fixedSize()` is being used as if it guaranteed a label fits its parent; it does the opposite when the ideal width exceeds the proposal. A custom FlowRow claims a bounded width while placing an unbounded child. Two draw-at text runs have no separation contract. Several independently fixed/minimum regions are combined without a whole-window budget and tested only in roomy/default states.

But the hybrid architecture itself is sound. Native outlines/collection views for large data, a Metal viewport, AppKit controls for hot input paths, and SwiftUI for inspector/forms/chrome are all defensible. The source contains several correct implementations of those boundaries. A wholesale rewrite would discard those working pieces while preserving the product's underlying layout decisions unless those decisions are changed explicitly.

Apple documents exactly the behavior at issue: [fixedSize(horizontal:vertical:)](https://developer.apple.com/documentation/swiftui/view/fixedsize(horizontal:vertical:)) can cause a view to exceed its parent's bounds. [ProposedViewSize](https://developer.apple.com/documentation/swiftui/proposedviewsize) describes layout negotiation, and [Layout](https://developer.apple.com/documentation/swiftui/layout) requires the custom container to measure and place children consistently. A `.frame` is not automatically a clipping boundary or a coercion of all descendants.

### What established Mac pro apps do structurally

The useful comparison is their workspace behavior, not a guess at proprietary implementation languages. Native desktop applications can contain substantial custom cross-platform code; “native” does not mean every control is AppKit or that all five products use SwiftUI. Lightroom Classic is a cross-platform desktop application, not a model for a pure native AppKit/SwiftUI interface, and that distinction does not make it a web app either.

- **Capture One:** task/tool tabs, collapsible tool groups, movable/resizable regions, customizable and saved workspaces. Complexity is distributed by task rather than permanently stacked into one inspector. [Official workspace customization](https://support.captureone.com/hc/en-us/articles/5473687650717-How-do-I-customize-my-Workspace-and-rearrange-the-new-tool-tab-icons), [resizable areas/workspace guidance](https://captureone.com/blog/maximizing-image-area-custom-workspace).
- **Pixelmator Pro:** central canvas, separate Layers and Tools areas, context-dependent Tool Options, hideable interface elements, configurable toolbar. Its guide explicitly says smaller displays may require scrolling to see all tool options. [Interface overview](https://support.pixelmator.com/pixelmator-pro-user-guide/pixelmator-pro-basics/interface-overview).
- **Final Cut Pro:** separate browser, viewer, timeline, sidebar and inspector; show/hide and resize those regions, with task-specific/saved workspace arrangements. The inspector can change height instead of forcing all tools into one immutable vertical stack. [Arrange the main window](https://support.apple.com/guide/final-cut-pro/arrange-the-main-window-ver2a27194eb/mac).
- **Photomator:** a photo-centered workspace with tool-specific options and optional filmstrip/presets, rather than every tool competing for permanent space. [Interface overview, including Mac controls](https://support.pixelmator.com/photomator-user-guide/get-started/interface-overview).
- **Affinity:** a studio/panel organization around the document canvas, with customized panel arrangements rather than one ever-growing fixed inspector. [Studio customization reference](https://www.affinity.studio/help/workspace-customizing-studios/). The fetched current support landing page was sparse; no claim is made about its internal UI framework or a specific version's exact minimum dimensions.

Common principle: **reserve canvas space, make panels responsible for their own overflow, expose one relevant toolset at a time, and make the workspace reducible.** These are architectural decisions available in both native and web UI frameworks.

### Would React / Electron / Tauri help?

React is a UI model, not a layout engine. CSS Flexbox/Grid plus browser DevTools and Playwright could make layout debugging and snapshot automation more familiar to a web-oriented team. That is a real productivity benefit. It does not prevent the CSS equivalents of this audit's bugs: `min-width:auto`, `white-space:nowrap`, absolute coordinates, fixed-height dialogs, and overflowing flex children.

Electron supplies Chromium and a process architecture with extra memory/runtime cost. Tauri can retain Rust code and use the system webview with a smaller distribution, but platform webview behavior and native integration remain engineering work. Neither can simply turn an existing native Metal photo viewport into a zero-cost DOM element. A GPU photo editor still needs deliberate pixel ownership, texture/IOSurface or other presentation bridges, color management, EDR/HDR handling, input latency, native dialogs, accessibility, and lifecycle management. WebGPU can do serious image work, but adopting it or bridging the existing renderer is a separate graphics project, not a layout repair.

**Recommendation:** retain Rust/Metal + AppKit/SwiftUI. Rebuild the small shared layout foundations and the document workspace, add reproducible layout tests, and only reconsider a web shell if a measured prototype demonstrates a team-speed or cross-platform advantage that justifies the integration cost. Do not migrate frameworks to escape width arithmetic. Also do not force SwiftUI onto the synchronous slider/renderer path merely to make the UI “all one framework.”

## D. Remediation plan

The following examples describe future changes only. They are not applied to application source by this audit.

### Layout rules to adopt

1. Every reusable component declares a contract: minimum usable width, expansion axis, compression order, text policy, overflow policy, and whether it can be used inside a fixed-height row. Token compliance alone is insufficient.
2. Fixed sizes are appropriate for icons, swatches, image previews, and hit targets. Text-bearing controls use a measured/negotiated width or a documented one-line truncation policy with full accessible text/help. Do not use character counts as pixel budgets.
3. Default labels may compress; numeric values and essential actions remain visible. Reserve a gap. Use `layoutPriority` only after defining which item is expendable. Do not solve width pressure by applying `fixedSize()` to all siblings.
4. Long descriptions may wrap and preserve vertical height **inside a scroller**. Sheet header/footer stay outside that scroller. A sheet must have a minimum usable body viewport and a real screen-height limit.
5. Every independently growing inspector region either scrolls, belongs to a resizable split, or is task-switched/collapsible with a tested minimum. Do not sum multiple large minimum heights under a non-scrolling VStack.
6. On shrinking width: remove optional text, switch to a compact row or overflow menu, or hide a nonessential panel. Do not let the whole root center an oversized subtree outside the window.
7. Custom drawing budgets include glyph widths, handle radii, centered strokes, and intended shadows. Controls publish useful intrinsic dimensions or participate explicitly in representable sizing.
8. Overlays are for canvas annotations or deliberately floating tools, not general form layout. Allocate any fixed chrome through a real layout region or safe-area inset; avoid duplicate top-padding assumptions.
9. Keep toolbar groups small enough to overflow as useful units. Remove hidden layout slots conditionally instead of setting opacity to zero when preserving space has no purpose.
10. Declare a macOS text-size/accessibility strategy. Test longer translations, wide glyphs, RTL, user names and paths. Do not blindly transplant iOS Dynamic Type assumptions or shrink important labels using `minimumScaleFactor` as a universal fix.

### Wrong versus right examples

#### Intrinsic menu title versus an allocated row

```swift
// Wrong in a constrained inspector: the parent cannot compress this label.
HStack { Text(profileName); Image(systemName: "chevron.down") }
    .fixedSize()
    .frame(height: 20)

// Better contract: the title is the compressible region, not the chevron.
HStack(spacing: Theme.Space.xs) {
    Text(profileName)
        .lineLimit(1)
        .truncationMode(.middle)
        .frame(maxWidth: .infinity, alignment: .leading)
    Image(systemName: "chevron.down")
        .fixedSize()
}
.frame(minWidth: 0, maxWidth: .infinity)
.help(profileName)
.accessibilityLabel(profileName)
```

The enclosing row must still reserve space for its label and Other action. A modifier on this label cannot repair an impossible enclosing width budget.

#### Fixed sheet body versus scrollable content with stable actions

```swift
// Wrong for arbitrary schema/error content.
VStack { header; generatedControls; footer }
    .frame(width: 560, height: 300)

// Better: body can overflow without displacing the actions.
VStack(spacing: 0) {
    header
    ScrollView { generatedControls.frame(maxWidth: .infinity) }
        .frame(minHeight: 180, maxHeight: .infinity)
    footer.fixedSize(horizontal: false, vertical: true)
}
.frame(minWidth: 560, idealWidth: 640, minHeight: 420)
```

The actual sheet/window presentation must impose an available-screen maximum height; the snippet is the component contract, not a complete presentation controller.

#### Adaptive row rather than noncompressible siblings

```swift
ViewThatFits(in: .horizontal) {
    HStack { essentialStatus; detailedCounts; basketControl }
    HStack { essentialStatus; overflowMenu }
}
.frame(maxWidth: .infinity, alignment: .leading)
```

Test both branches with long strings. A fallback containing the same unbounded name is not a fallback. Use native menus/help/accessibility to expose omitted detail.

#### AppKit label/value drawing

```swift
// Wrong: independent origins, no separation rule.
title.draw(at: .zero, withAttributes: titleAttributes)
value.draw(at: NSPoint(x: bounds.width - valueWidth, y: 0),
           withAttributes: valueAttributes)

// Better: reserve value plus a real gap, then draw/truncate title in its rectangle.
let gap = Theme.Space.s
let labelWidth = max(0, bounds.width - valueWidth - gap)
let labelRect = NSRect(x: 0, y: 0, width: labelWidth, height: labelHeight)
let paragraph = NSMutableParagraphStyle()
paragraph.lineBreakMode = .byTruncatingTail
var attributes = titleAttributes
attributes[.paragraphStyle] = paragraph
title.draw(in: labelRect, withAttributes: attributes)
value.draw(at: NSPoint(x: bounds.width - valueWidth, y: 0),
           withAttributes: valueAttributes)
```

If the label becomes meaningless at the minimum width, use a stacked compact variant or one grading range at a time. Do not arbitrarily abbreviate all important labels.

#### Manual compare partition

```swift
// Wrong: inset was not removed from the percentage width.
caption.frame = NSRect(x: inset, y: y, width: width * 0.6, height: h)
badge.frame = NSRect(x: width * 0.6, y: y, width: width * 0.4 - inset, height: h)

// Better: partition the usable span once, accounting for a gap.
let usable = max(0, width - 2 * inset - gap)
let captionWidth = usable * 0.6
caption.frame = NSRect(x: inset, y: y, width: captionWidth, height: h)
badge.frame = NSRect(x: caption.frame.maxX + gap, y: y,
                     width: usable - captionWidth, height: h)
```

For FlowRow, measure oversized children using the actual maximum row proposal, place with the same proposal, and ensure the child accepts that constraint. For sidebar badges, change/collapse the constraint or use an arranged-subview visibility policy. For wheel/curve handles, inset by the complete painted envelope, not only the path radius.

### Work packages and acceptance criteria

Effort is a planning estimate in focused engineering days, not a promised schedule. These packages are recommendations, not newly created tasks or implemented fixes.

| Package | Size | Scope and acceptance |
|---|---|---|
| L0 — Safe reproducible harness | 1–2 days | Add a real `--background-audit` launch contract that bypasses all activation/order-front paths, separate app state, deterministic fixture setup, appearance/window-size hooks, and capture manifest. Explicitly preserve actual window size, root content size, backing scale and revision. Replace ad hoc cached-object linking in this audit with a supported target. |
| L1 — Document workspace sizing | 2–3 days | Rebuild `DocumentInspector` as a correctly budgeted resizable/scrollable or tabbed workspace. Properties, Layers, Channels, History and Snapshots must remain reachable at supported minimum sizes. No ancestor split frame outside root bounds. Include Channels expanded, many snapshots and extra progress strips. |
| L2 — Shared width contracts | 2–3 days | Rebuild ThemeMenuStyle/MenuPicker, FlowRow/KeywordChip, ValueSlider's label/value region. Remove character-count layout. Prove 288/296/380 inspector widths with long names and numeric extremes. Preserve keyboard, AX labels and hot-path latency. |
| L3 — Shell compact behavior | 1–2 days | StatusBar, DocumentStatusBar, document tabs, toolbar, filter and tether rows. Specify compact/overflow variants and which panels yield width. Eliminate retained invisible slots. Test minimum window and large counts/names, not only default values. |
| L4 — Sheet envelopes | 1–2 days | Generic filter, neural and merge sheets: stable actions, scrollable growing body, readable errors and paths. Retest ordinary cases before changing their dimensions; scaffold-only probe did not prove a baseline Gaussian failure. |
| L5 — Manual geometry polish | 0.5–1 day | Compare partition, wheel/curve/slider stroke envelope, hidden sidebar slots. Unit tests for rectangle intersections and endpoint paint bounds; screenshot long names, full saturation and extreme values. |
| L6 — Continuous layout gate | 2–3 days | Multi-size/appearance snapshot suite, frame/AX diagnostics, text stress fixtures, source lints with documented exceptions. Run in an isolated macOS GUI session or non-activating test mode, not by stealing the developer's desktop. |

Suggested dependency order: L0 first; L1 and L2 can then proceed independently with agreed contracts; L3/L4 consume those contracts; L5 is independent; L6 starts with L0 and gains cases from every package. One owner should control the shared Theme/Components contracts to avoid conflicting fixes.

### Lint and test strategy

- Keep Theme lint, but add SwiftSyntax-based checks for unconditional two-axis `fixedSize()` on text/menu groups outside known scrolling contexts; fixed-height text containers without a text policy; generic sheet bodies without overflow treatment; and custom Layout measurement/placement using incompatible proposals. Flag for review with rationale, not an indiscriminate ban on every frame.
- Do not ban native controls solely for visual uniformity if a custom replacement loses sizing, keyboard or accessibility behavior. Allow native Form/List/Outline/toolbar components where they solve the task. A Form is not required for the GPU canvas.
- Snapshot full windows at **960 × 600 (declared minimum), 1280 × 800, 1440 × 900, 1728 × 1117**, light/dark, and supported backing scales. Record whether the normal app rejects/clamps a requested size. Never silently call a clamped screenshot the requested size.
- Add inspector-width tests at 288/296/380 independently of outer window dimensions. Expand every section individually and representative combinations. Include many snapshots/layers/masks/tabs, active jobs/tether, and every sheet's error/empty/missing-model states.
- Stress text with long filenames, wide ICC profile names, unbroken keywords, large counts/negative readouts, accented/CJK strings, RTL and pseudo-localized expansion. Test the adopted larger-text preference at the same time.
- Capture AX role, identifier, title/value, enabled state and frame. Normalize to window coordinates. For visible actionable siblings, flag positive-area intersections above a tolerance unless a documented overlay/group relation allows them. Exclude ancestor–descendant containment, hidden views, scroll content outside its viewport, shadows, selection overlays and intentional image annotations. Require access to footer actions and full numeric values.
- AX frames alone are insufficient: `ValueSlider` exposes one slider but draws title/value internally, so the accessibility tree cannot see that collision. Instrument custom-control label/value rectangles in tests, add numeric geometry assertions, and retain pixel snapshots. Native-view frame dumps similarly do not reveal every SwiftUI Text frame.
- Add containment checks for the root split-view bounds against the host. The measured negative origins in this audit are a particularly high-signal regression test. Check nested coordinate conversion rather than comparing raw local origins.
- Assert visible strings don't unexpectedly ellipsize where exact numbers/actions matter. Intentional ellipsis for filenames is acceptable when full text is accessible; it should be recorded as a policy, not a failed test.
- Existing `PeopleLayoutTests.swift:19–38` is a useful start, but it calls `makeKeyAndOrderFront` and silently returns if its outline lookup fails. Make missing required elements fail, make execution background-safe, and cover document mode. Do not run that test on an occupied desktop unchanged.

## Reproduction commands and deliverables

The requested command was actually started with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/audit-layout`; see `build.log`. It finished Rust/FFI but encountered another process's SwiftPM lock. The audit stopped its own waiting process rather than terminating the other worker. A separate scratch Swift build was started under this audit directory; final build status is recorded in the verification appendix below.

The sample generator ran successfully:

```sh
swift apps/mac/Support/make-sample-folder.swift tools/orchestrate/audits/layout/sample-shoot 40
```

The non-activating full-view runner and the small component probes are retained with their logs. Their source documents the alternate startup and fixture choices. Re-link `run-window-probe.py` only against a completed, consistent build, not object files changing under another worker. One attempted link was explicitly rejected because an object changed during linking; it was not used as evidence. The successful final run captured all matrix cells and exited zero.

Do not treat the app-build command, a contact sheet, or one screenshot as a layout pass. The deliverable is the source diagnosis plus measured/captured defects, with risks kept visibly separate.

## Verification appendix

| Check | Actual result |
|---|---|
| Requested Rust/FFI build in `/Volumes/betterSSD/tessera-cache/target/audit-layout` | Completed; UniFFI generation ran and the archive was reported arm64. Original chained build then waited on another worker's SwiftPM lock. The audit stopped only its own waiting process. |
| Isolated Swift debug build | **Exit 0**, `Build complete! (128.84s)` in `isolated-build.log`. Scratch directory: `tools/orchestrate/audits/layout/swift-build`. |
| Isolated optimized release/package attempt | **Exit 1**. Swift rejected `apps/mac/Sources/Tessera/Document/Tools/ToolsPalette.swift` because it was modified during compilation. `release-build.log` preserves the exact error. The adapted packaging script changes only build/output paths and otherwise follows `Support/make-app.sh`. **No completed release bundle or release-launch result is claimed.** |
| Final full-view runner | **Exit 0**, linked against the completed isolated debug build. 18 compositor captures succeeded and every capture logged `frontmostIsProbe=false`. Library/RAW data loaded through the engine with 40/5 items respectively. |
| Focus-safe component probes | Slider and scaffold probes compiled and ran with **exit 0**; actual SF measurements and dark/light PNGs retained. Scaffold-only fixture did not reproduce baseline footer clipping. |
| Counterfactual root-width probe | **Exit 0**. With DocumentStatusBar: 1318 × 817 fitting size. Without it: 960 × 817. This isolates horizontal propagation without modifying production code. |
| Evidence validation | `python3 tools/orchestrate/audits/layout/validate-evidence.py` **exit 0**. Verifies all 18 named PNGs, dimensions ≤1400 px wide, successful capture/focus checks, native overflow frames, slider metrics, catalogue counts and final marker. See `validation.json` and `capture-manifest.json`. This is evidence consistency, **not a layout-pass verdict**. |
| Catalogue | 10 confirmed/conditional finding sites (3 fresh observed/measured, 5 deterministic geometry, 2 conditional width contracts), plus 12 explicitly separated risk entries. Counts are not affected-control totals. |
| Source preservation | Audit authored no application-source edits. Final `git diff --name-only -- apps/mac/Sources` was empty, but the branch had advanced through other workers' commits. The pinned archive/hash comparison is the reproducibility boundary, rather than pretending the moving checkout stayed unchanged. |

### Remaining verification boundary

The diagnosis and evidence report are complete for the pinned baseline. A normal release-app launch using `open -g` was intentionally not performed because startup explicitly activates the app; the alternate real-view runner is documented above. Release packaging remains unverified because of concurrent source mutation. A clean frozen checkout and supported non-activating application launch hook are required to finish that release-path check. Long-name/error-state risks in the catalogue remain follow-up reproduction targets, not disguised passes or confirmed screenshots.

RESULT: DONE
