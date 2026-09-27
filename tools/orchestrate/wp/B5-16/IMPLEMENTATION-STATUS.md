# B5-16 implementation status: document inspector layout (tabbed, budgeted) and the M2-56 handoff H1–H12

Branch `wp/B5-16`, based on wp/B5-11b merged with main 1650de5. The package fixes the text and button overlap in
document mode. Colours and type still use the existing Theme tokens; this package changes layout and structure only.
`apps/mac/DESIGN.md` §10 and the Theme lint remain authoritative, and both are updated and passing.

## How each handoff item was resolved

| # | Resolution | Where |
|---|---|---|
| H1 (D01) | `DocumentInspector` is rebuilt as a 32 pt tab row, then a hairline, then the tab's own content, then History. Tab row: a 20 pt `SegmentedPicker` with **Stack · Properties · Channels**; ⌃1, ⌃2 and ⌃3 switch tabs. History is a collapsible pane at the bottom. The column minimum is set by `DocumentInspectorBudget`: tab bar 32 + hairlines 2 + the Stack tab's minimum 290 + History header 32 + History body minimum 80 = **436 pt**, below the 548 pt column. The History body is `historyHeight(requested:column:)`: it is never smaller than its own minimum, and it never pushes the tab content below the tab's minimum. The selected tab is stored on `DocumentWorkspace.inspectorTab` and persisted in `DocumentInspector.tab`. | `Document/DocumentView.swift`, `TesseraCore/Document/DocumentInspectorLayout.swift`, `Document/DocumentWorkspace.swift` |
| H2 | Removed the fixed 144 pt list. States and snapshots now share one scroller that fills the pane. New Snapshot… and the memory line are a fixed 28 pt row below that scroller, so they stay visible. The pane height changes when you drag the hairline above the History header (row-resize pointer; double-click restores 168 pt). The height is persisted in `DocumentInspector.historyHeight`. The expanded/collapsed state reuses the old `InspectorPanel.History` key, which the self-tests already set. | `Document/DocumentHistoryPanel.swift`, `DocumentView.swift` |
| H3 | Channels has its own tab. The list's minimum is 4 rows (up to 8 before) and it grows to fill the tab. The footer stays whole at the bottom. | `Document/Channels/ChannelsPanel.swift` |
| H4 (D02) | Opacity and Fill sit side by side only when the panel interior is at least 300 pt wide (`DocumentLayersRow.slidersSideBySide`, fed by `onGeometryChange`). Otherwise each gets its own row. At the 288–380 pt inspector widths the interior is 264–356 pt, so they are stacked up to about a 324 pt inspector. | `Document/LayersPanel.swift` |
| H5 (D02) | The Filter field is `minWidth` 48 / `idealWidth` 96 / `maxWidth` 96 inside a `ViewThatFits`. When the lock row cannot fit even 48 pt, the field becomes an icon pull-down that holds the same placeholder and help. The `document.layers.filter` identifier is kept. | `LayersPanel.swift` |
| H6 | Fixed as part of H1: Properties, Color and Brushes scroll inside the Properties tab, and the Layers footer lives inside the Stack tab. The "Color/Layers" and "FX/Channels" headers no longer share a boundary; compare the before and after screenshots at 960 × 600. | `DocumentView.swift` |
| H7 (R02/L3) | `DocumentStatusBar` is a `ViewThatFits` with a full row and a compact row. The compact row shows the canvas size only, the tool without its key, the selection as W × H, and drops the stroke and render readouts; the full strings are in help tags. The message has ideal width 0, and `n open` stays. This is the same pattern as the library `StatusBar`. | `DocumentView.swift` |
| H8 (R03/L3) | `DocumentTabs` shows at most 3 tabs, or 1 in a compact toolbar (below 1280 pt). The current document is always one of them (`DocumentTabStrip.visible`). The rest are in a `+n` pull-down (`document.tabs.overflow`). `.fixedSize()` is gone from the strip. The compact cap was added after the 960 × 600 capture with 8 documents: with 3 tabs, the whole strip fell into the toolbar's overflow chevron. | `DocumentView.swift`, `DocumentInspectorLayout.swift` |
| H9 (L3) | The close button's `opacity(0)` slot is kept on purpose, so titles do not shift on hover. There is a comment at the site, and DESIGN.md §10 records it as the one documented exception to the §3.5 rule "rows remove optional parts". | `DocumentView.swift`, `DESIGN.md` |
| H10 (R11) | `ZoomHUD` is an `.overlay(alignment: .bottom)` of the viewport representable and takes no layout height. | `DocumentView.swift` |
| H11 (R5) | The tools palette is a `ViewThatFits(in: .vertical)`: the plain column when it fits, otherwise the same column in a vertical `ScrollView` with indicators hidden, inside the same HUD background. The palette and options bar are also pinned top-leading in the full canvas instead of sitting in a VStack with a spacer. Before this, the palette at 960 × 600 extended under the toolbar and the Move tool was hidden (see before/after at 960 × 600). | `Document/Tools/ToolsPalette.swift`, `DocumentView.swift` |
| H12 (R04/R05, L4) | New helpers `documentSheetBody()` (a vertical `ScrollView` body; the header and footer stay outside it) and `documentSheetFrame(width:height:ideal:)` (fixed width; the old height becomes the minimum and default height; `maxHeight: .infinity`). They are applied to the filter, Blending Options, Neural Filters (list and detail scroll separately), Select and Mask, Color Range, Modify Selection, Fill, Save/Load Selection, Channel Options and New Spot Channel sheets. Image ▸ Adjustments already scrolled. New Document, Export Flat and Save As use a grouped `Form`, which scrolls on its own, so only their frames changed. The filter sheet now opens at 348 pt (minimum 300) so its 180 pt detail pane is whole; see `evidence/selftests/filter-1.png`. The Layer Style / Global Light panels were already min-sized `NSPanel`s and are unchanged. | `Document/DocumentSheets.swift`, `Filters/FilterSheets.swift`, `Retouch/RetouchViews.swift`, `Tools/ToolsSheets.swift`, `Channels/ChannelSheets.swift` |

Accessibility identifiers: every identifier used by the existing ACCEPTANCE steps and self-tests is kept
(`document.properties`, `document.channels`, `document.history`, `document.layers*`, `document.history.*`,
`document.tabs.*`). New: `document.inspector.tabs`, `document.inspector.stack`,
`document.inspector.shortcut.{stack,properties,channels}`, `document.history.toggle`, `document.history.resize` and
`document.tabs.overflow`.

## Tests

- `ShellLayoutTests`: the `XCTExpectFailure` block ("document inspector panels overflow their column") is **removed**.
  The document state now goes through the same strict column-content containment as library and RAW. The test
  passes strictly at all 24 size, state and appearance combinations.
- New `testDocumentInspectorEveryTabAndHistoryStateAtEverySize` covers 4 sizes × 3 tabs × History open/closed = 24
  windows, with 12 extra adjustment layers and 6 snapshots. It checks:
  - root and column containment;
  - no content control under the toolbar or outside the window;
  - no overlapping actionable siblings;
  - the inspector regions from `DocumentInspectorProbe` (tab bar, tab content, History header and body) are inside
    the window, below the toolbar, and do not overlap each other;
  - the tab content is at least the budget minimum, and the History body is at least its minimum;
  - the New Snapshot row is whole inside History, and the Layers or Channels footer is whole inside its tab;
  - Opacity and Fill are visible on Stack.
  Result: 24 of 24 windows checked, 0 failures.
- New `testManyDocumentTabsStayCapped`: the strip width stops growing after 4 documents (at 5–8 documents it stays
  within 8 pt of the 4-document width). In compact mode it is one tab plus the menu. Root containment holds at
  960 × 600 and 1280 × 800 with 8 documents.
- New `testInspectorTabShortcuts`: ⌃2, ⌃3 and ⌃1 go through `NSWindow.performKeyEquivalent` and select Properties,
  Channels and Stack.
- New `DocumentInspectorTabsTests` (pure): tabs and shortcuts, the budget sum (436 ≤ 548), History clamping, the
  300 pt slider rule, and the tab-strip window (1–10 documents, cap 3 and cap 1).
- `DocumentInspectorLayoutTests` (the width check for every layer kind at 288, 296, 320 and 380 pt) and
  `ThemeLintTests` pass.

Final `swift test --jobs 2` run:

```
Test Case '-[TesseraCoreTests.ShellLayoutTests testDocumentInspectorEveryTabAndHistoryStateAtEverySize]' passed (36.662 seconds).
Test Case '-[TesseraCoreTests.ShellLayoutTests testInspectorTabShortcuts]' passed (2.080 seconds).
Test Case '-[TesseraCoreTests.ShellLayoutTests testManyDocumentTabsStayCapped]' passed (3.039 seconds).
Test Case '-[TesseraCoreTests.ShellLayoutTests testShellContainedAtEverySizeStateAndAppearance]' passed (37.043 seconds).
	 Executed 377 tests, with 1 test skipped and 6 failures (0 unexpected) in 161.966 (161.994) seconds
✔ Test run with 5 tests in 2 suites passed after 0.045 seconds.
```

The 6 failures are the 4 `DocumentAdjustmentJSONTests` cases (`testAuto`, `testColorLookup`, `testMatchColor`,
`testFixtureCoversEveryKindAndRoundTrips`). They are outside this package: neither the tests nor the code they exercise changed here, and I did not re-run them on the base. The cause is that the engine JSON from M5-32
(`highlight_clip`, `shadow_clip`, `dither`, `source_filename`, `color_intensity` …) has keys that the Swift adjustment
models do not round-trip yet. They are unrelated to layout, so this package does not touch them. The run contains no
`XCTExpectFailure`. `swift build` succeeded, and `xcodebuild -scheme Tessera -configuration Debug …` ended with
`** BUILD SUCCEEDED **`.

### Layout matrix (document window, dark; `evidence/after/inspector-document-<size>-<tab>-history-<open|closed>.png`)

| Size | Stack open / closed | Properties open / closed | Channels open / closed |
|---|---|---|---|
| 960 × 600 | pass / pass | pass / pass | pass / pass |
| 1280 × 800 | pass / pass | pass / pass | pass / pass |
| 1440 × 900 | pass / pass | pass / pass | pass / pass |
| 1728 × 1117 | pass / pass | pass / pass | pass / pass |

The main shell matrix (library, RAW and document states × 4 sizes × light/dark) passes 24 of 24 with the document
column checked strictly.

## Before / after screenshots (the real `ContentView` in the background harness, own window only)

| Size | Before | After |
|---|---|---|
| 960 × 600 | `evidence/before/document-960x600-{dark,light}.png`: "Color" and "Layers" headers overlap, the Layers footer touches "Channels", History is clipped off the bottom, the tools palette runs under the toolbar (Move hidden) | `evidence/after/document-960x600-{dark,light}.png` |
| 1280 × 800 | `evidence/before/document-1280x800-{dark,light}.png`: History and Snapshots clipped | `evidence/after/document-1280x800-{dark,light}.png` |
| 1440 × 900 | `evidence/before/document-1440x900-{dark,light}.png` | `evidence/after/document-1440x900-{dark,light}.png` |
| 1728 × 1117 | `evidence/before/document-1728x1117-{dark,light}.png` | `evidence/after/document-1728x1117-{dark,light}.png` |

Also in `evidence/after/`: the 24 inspector-matrix captures and `tabs-8-documents-{960x600,1280x800}.png`. In
`evidence/selftests/`: document steps 1–4 (the tabs switched with ⌃), Gaussian Blur / Levels sheets
(`filter-1.png`, `filter-4.png`), Neural Filters sheets (`retouch-10…18.png`) and the vector 1440 pt Properties tab
(`vector-27-379-inspector-1440-styles.png`).

## Self-tests

All were launched with `tools/orchestrate/wp/B5-16/run-selftests.sh`: `open -g -n … --nonactivating --app-dir
<scratch> --folder <copy of fixtures/raw>`. Only the run's own window was captured (`screencapture -x -o -l`). Only
its own PID was quit. The frontmost app (`lsappinfo front` → `ASN:0x0-0xbb6bb6`) stayed the same throughout, and no
watchers were left running. Logs are in `evidence/selftests/*-selftest.log`.

```
document-selftest: done, 0 failure(s)     (incl. check B5-16 ⌃2 shows Properties / ⌃3 Channels / ⌃1 Stack ok)
tools-selftest: done, 0 failure(s)
filter-selftest: done, 0 failure(s)
retouch-selftest: done, 0 failure(s)
styles-selftest: done, 0 failure(s)
channels-selftest: done, 0 failure(s)
text-selftest: done, 0 failure(s)
vector-selftest: done, 0 failure(s)
```

This branch has no `--transform-selftest`. That self-test belongs to B5-12, which is not merged into this base, so
it is not run here.

## Deviations and notes

- **Background runs.** In `DocumentSelfTest`, `FilterSelfTest`, `ToolsSelfTest`, `ChannelsSelfTest` and
  `StylesSelfTest`, `mark()` used to raise the window to the floating level and call `orderFrontRegardless()`. The
  Layer Style / Global Light panels were made key. Under `--nonactivating` (`BackgroundRun.active`) none of this
  happens now: the panels are ordered to the back instead. Document and filter steps also print `window-id` for
  own-window captures.
- **Window fallback.** While another instance of the app was running (another worktree, same bundle id), the SwiftUI
  `Window` scene opened **no window** in a `--nonactivating` launch (`NSApp.windows == []`), so the document
  self-test had no viewport. In that case only, `BackgroundRun.ensureWindow` hosts `ContentView.root` in its own
  window after 5 s. The window is ordered to the back and never made key. It is logged as
  `background-run: no scene window; …`. The document, tools and retouch runs used this window, which settles at the
  960 × 652 minimum; the later runs had the normal scene window.
- The document, tools, filter and styles runs add `--new-document`, so document mode exists before Edit in Layers
  (the pattern B5-09b documented for `open -g`). `DocumentSelfTest` also waits up to 180 s for the viewport to
  attach.
- `VectorSelfTest`, `TextSelfTest` and `ChannelsSelfTest` select the tab they look into (Properties for shape and
  text controls, Stack for the outline, Channels for the channel steps).
- `DocumentInspectorProbe` (off unless a test enables it) records region frames through `onGeometryChange`. SwiftUI
  builds no accessibility tree in the test process, which is prohibited from activating, so the probe is how the
  harness checks SwiftUI regions.
- No change to `Shell/**` or the shared components. `SheetScaffold` keeps its content unscrolled, so the scrolling
  body is added per document sheet. No NEEDS.md was required.
