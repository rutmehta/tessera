# M2-56 → Machine B: document-mode layout causes (apps/mac/Sources/Tessera/Document/**)

M2-56 did not edit `Document/**`. The shell now contains every column (`Shell/ShellLayout.swift`,
`containedColumn()`): the document inspector can no longer push the split view under the toolbar
or past the right edge, and the options bar / Properties heading sit below the toolbar at every
tested size. What is left is **inside** the inspector column: its stacked minimum heights are taller
than the column, so the bottom (History, Snapshots, and at 960 × 600 part of Channels) is clipped at
the window's bottom edge instead of scrolling. `ShellLayoutTests` records this as a known,
non-strict expected failure (`document inspector panels overflow their column`); when it stops
failing, delete the `XCTExpectFailure` block in `Tests/TesseraCoreTests/ShellLayoutTests.swift` so
the check becomes strict.

Line numbers are at `f5a016f` (wp/M2-56 base).

| # | File:line | Cause | What to change |
|---|---|---|---|
| H1 (D01) | `Document/DocumentView.swift:98–121` (`DocumentInspector`) | One non-scrolling `VStack`: Properties `ScrollView` min 192 (`:106`), Layers min 256 (`:113`), hairline, Channels (`:117`), History (`:120`) all stacked; the sum (≥ 675 pt collapsed, ≈ 1000 pt with Channels open) exceeds a 548–748 pt column. `layoutPriority` cannot reconcile minimums. | Make the column's total minimum ≤ 548 pt (960 × 600 content minus toolbar): e.g. a vertical split (resizable Properties / Layers / Channels+History panes with small minimums), or one outer `ScrollView` for the lower panels, or tabbed Channels/History. Properties heading and Layers list must stay reachable at 960 × 600. |
| H2 (D01) | `Document/DocumentHistoryPanel.swift:26` | History list fixed at `Theme.Height.row * 6` (144 pt) plus Snapshots rows and the New Snapshot row (`:29–53`) outside any scroller; snapshot rows are uncapped. | Put Snapshots inside the same scroller as the history list, or give the panel a max height and scroll; do not fix the list height. |
| H3 (D01) | `Document/Channels/ChannelsPanel.swift:28` | List height `rowHeight * min(rows.count, 8)` (up to 256 pt + footer 28) when expanded. | Cap lower (4 rows) or let it take a share of a resizable pane. |
| H4 (D02) | `Document/LayersPanel.swift:20–31` | Opacity and Fill `DocSlider`s side by side (each needs its title + readout); at the 264 pt panel interior they fit only with M2-56's ValueSlider truncation. | Nothing required now; if the readouts grow (e.g. "100.0 %"), stack them or use one row per slider. |
| H5 (D02) | `Document/LayersPanel.swift:32–48` | Lock label + 4 fixed 20 pt icons + a fixed 96 pt Filter field in one row. | Let the Filter field shrink (`minWidth` 48, `idealWidth` 96) or drop it into an overflow at narrow widths. |
| H6 (visible in captures) | `Document/DocumentView.swift:98–108` | The Properties `ScrollView`'s `minHeight` (192) clips the "Color" section header so the "Layers" header draws over it (see `evidence/after-document-1280x800-dark.png`, "Color/Layers" and the "FX" row touching "Channels"). | Part of H1: give Properties/Tool sections their own pane or let the LayersPanel footer and Channels header not share the boundary. |
| H7 (R02/L3) | `Document/DocumentView.swift:223–257` (`DocumentStatusBar`) | Zoom, tool, selection, render readout and "n open" are `.fixedSize()`; only the canvas text truncates. | Same pattern as the library `StatusBar` in `Shell/ContentView.swift` (M2-56): `ViewThatFits` full / compact rows, the message at ideal width 0 so it never decides the variant. |
| H8 (R03/L3) | `Document/DocumentView.swift:144–169` (`DocumentTabs`) | The whole tab strip is `.fixedSize()` (`:166`); tab titles are capped but the tab count is not, so many tabs widen the toolbar item without limit. | Cap the strip (e.g. `maxWidth` = 3 tabs) and move the rest into an overflow menu; drop `fixedSize()` on the strip. |
| H9 (L3) | `Document/DocumentView.swift:197` | Tab close button hidden with `opacity(0)` keeps its slot (intended, so the title does not move). | Keep, but document the choice; everything else in the shell now removes hidden parts. |
| H10 (R11) | `Document/DocumentView.swift:22`, `:81` (`ZoomHUD`) | The hidden zoom HUD is `opacity(0)` and still takes 28 pt + 16 pt padding at the bottom of the canvas stack. | Show it as an `.overlay(alignment: .bottom)` of the viewport so it takes no layout height. |
| H11 (R5) | `Document/Tools/ToolsPalette.swift:13–33` | The tools palette is a non-scrolling vertical stack (~560 pt). It fits the 548 pt canvas at 960 × 600 only barely and not with progress strips or the tether panel open. | Wrap it in a vertical `ScrollView` (indicators hidden) or split it into two columns below a threshold height. |
| H12 (R04/R05) | `Document/Filters/FilterSheets.swift:14–52`, `Document/Retouch/RetouchViews.swift:133–249` | Fixed-height sheets without a body scroller (audit L4). | Header/footer outside a `ScrollView` body; sheet minimum height rather than a fixed one. |

Machine B's other findings, for reference: the toolbar drawing over the options bar / Properties
heading, clicks on Character ▸ Style reaching Auto Edit, and the sidebar under the traffic lights
after a resize were all caused by the oversized document column centring the split view upwards;
M2-56 fixes them at the shell (see REPORT.md). Stale status hint and ⌘Return after a handle drag
(B5-10 verify) are not layout and are untouched.
