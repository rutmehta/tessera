# M2-56 report — layout foundations

Branch `wp/M2-56` (base `f5a016f`). Evidence base: `tools/orchestrate/audits/layout/REPORT.md` (Astra) and
Machine B's `origin/verify/B5-10:tools/orchestrate/wp/B5-10/verify/VERIFY-REPORT.md` (not merged).

## How it was reproduced (background-safe)

`Tests/TesseraCoreTests/ShellLayoutHarness.swift` hosts the real `ContentView.root` (the app's own root)
in a titled, full-size-content, unified-toolbar `NSWindow` like the app's. The test process's activation
policy is `.prohibited`, the window is `orderBack`'d and never made key, and captures use
`screencapture -l <window> -x -o` followed by `sips -Z 1400`. Every window asserts `NSApp.isActive == false`.
No test calls `makeKeyAndOrderFront` or `NSApp.activate`: `PeopleLayoutTests`, the one existing caller,
now uses the harness and `orderBack`. The same harness made the before and after captures. Before is the
base sources plus only `ContentView.root`; after is this branch.

States: 24 generated JPEGs on the engine (library), `fixtures/raw` on the engine (RAW, 5 items), and the
stub layered document (6 sample layers) over a 40-item stub library. Sizes: 960 × 600 (the declared
minimum content size; outer 960 × 652), then 1280 × 800, 1440 × 900 and 1728 × 1117 outer, each in light
and dark. That makes 24 windows.

Checks per window (`ShellLayoutTests`):
1. Root containment: no split view, split item or hosting view lies outside the content view
   (`ShellLayoutAudit.containmentViolations`, in the app target so other code can reuse it).
2. The window keeps its requested size, clamped only to the declared minimum.
3. Toolbar safe area: no content control starts under the toolbar, and none intersects a toolbar item
   or the window buttons.
4. No two actionable controls overlap.

SwiftUI builds no accessibility tree until an assistive client queries the process. A test process with
prohibited activation cannot be queried (AX returns `kAXErrorNotImplemented`), so checks 3 and 4 use the
window's AppKit controls instead: pop-ups, text fields, sliders, checkboxes, ValueSliders, outlines,
toolbar items and window buttons. The containment checks cover drawn-only SwiftUI buttons.

**Before (base sources): 168 failures** (`evidence/before-layout-failures.txt`). Examples:
- At 960 wide, in every state, the split view was 1204–1235 pt wide at x = −122…−137. The sidebar sat off
  the left edge and the inspector ran past the right edge.
- Document at 960 × 600: the split view was 869 pt tall at y = −82. The Properties Name field (y = 2) sat
  under the toolbar's overflow button, the sidebar sat under the traffic lights, and the Fill slider ran
  past the right edge. This is exactly Machine B's symptom (`evidence/before-document-960x600-dark.png`).
- Document at 1280 × 800: the split view was 869 pt tall in an 800 pt window
  (`before-document-1280x800-dark.png`).

The before run used the first revision of the harness, which differs from the final one in two ways:
- It also counted overlay scrollers overlapping content; the final harness ignores those.
- At 960 it asked AppKit for a 600 pt outer height, which AppKit grows off-screen.

**After: 0 shell failures** in all 24 windows. One known, non-strict expected failure remains and shows in
the test log: the document inspector's own scroll views extend below the window's bottom edge, because
its stacked panels are taller than the column. That is handed off to Machine B (see below).

## Findings

| Finding | Status | What changed / evidence |
|---|---|---|
| **D01** Document Properties / header intrudes into the toolbar at 1280 × 800 | **Fixed (shell part); inspector interior handed off** | Every column is now a `containedColumn()`. `ContainedColumn` in `Shell/ShellLayout.swift` reports the size it is offered, places its content top-leading and clips it. The window root is a `GeometryReader` (`ContentView.root`). A column's minimum can no longer raise the split view's height or centre it at a negative origin. Properties now starts below the toolbar at every size (`after-document-1280x800-dark.png`, `after-document-960x600-dark.png`). The inspector's bottom (History, Snapshots) is now clipped instead of its top. The stacked minimums themselves belong to Machine B (DOCUMENT-HANDOFF.md H1–H3, H6). |
| **D02** Fill readout and right-hand layer actions clipped at the right edge | **Fixed (shell); row contents handed off** | `ShellBudget` defines the yield order. (1) The sidebar collapses below 1188 pt: detail 384 + sidebar 228 + inspector counted twice on macOS 26 (measured 1204 = 228 + 384 + 2 × 296). The person's choice returns when it fits again. This goes through `DocumentWorkspace.columnVisibility`, because `NavigationSplitView` ignored an override made only in the binding. (2) The inspector's ideal and maximum width drop to what fits (`ShellBudget.inspectorFit`, ≥ 288). (3) The filmstrip hides when the canvas would be under 320 pt. The detail column has the 384 pt minimum in every mode. No control is past the right edge at any tested size. The LayersPanel rows are H4/H5. |
| **D03** The Highlights label and −100 readout run together at 80 pt | **Fixed** | `ValueSlider.textRects` draws the value whole at the trailing edge. The title goes in the remaining rect, minus an 8 pt gap, and truncates at the tail (`draw(with:options:)`). Tested at 80, 82.67, 110 and 110.67 pt with −100, +100, 0 and +2.50. |
| **D04** Compare caption and decision badge overlap by 12 pt | **Fixed** | `ComparePaneView.captionPartition` splits the usable span once, after removing both insets and an 8 pt gap. The badge gets its measured width up to 40 %; the file name gets the rest. The pane relayouts on configure. Tested at widths 120–900 with badge widths 0–400. |
| **D05** A themed menu can paint wider than a narrow row | **Fixed** | `ThemeMenuStyle` no longer uses two-axis `fixedSize()`. The menu is `hugCompressible()`: the new `HugCompressible` layout in `App/Components.swift` takes the ideal width when it fits and the offered width otherwise. The chevron stays fixed; only vertical `fixedSize` remains. `MenuPicker` no longer clips titles by character count: the title truncates in the middle at the measured width. In Soft Proof the menu gives way, never "Other…", and the help text carries the full profile name. Tests: a long ICC name at 120, 160 and 200 pt stays within the proposal; a short title still hugs; `SoftProofPanel` fits the 264 pt minimum interior. |
| **D06** An oversized keyword chip escapes FlowRow | **Fixed** | `FlowRow.frames` is a pure, tested function that measures and places with the same proposal. An item wider than the row is offered the row's width, and the first item wraps like any other. `KeywordChip` and the suggestion chips truncate their text in the middle, with the full keyword in help; their remove buttons stay fixed. Tests: the pure frames, plus hosted chip rows at 264 and 272 pt. |
| **D07** Colour-wheel puck 4 pt outside the view at full saturation | **Fixed** | The disc is inset by the puck's painted envelope: radius 5 + stroke 1 = 6 pt. The mouse mapping is unchanged relative to the disc. The only visual change is a disc 8 pt smaller in diameter. Tested at every 15° on 72 × 72, 80 × 72 and 264 × 144. |
| **D08** Dragging slider ring 0.5 pt outside | **Fixed** | `ValueSlider.ringPath` insets the ring path by half its stroke (1 or 2 pt). Tested at the minimum-value thumb. |
| **D09** Point-curve end knot 0.75 pt outside | **Fixed** | `CurveEditorView.knotPath` insets an outlined knot's path by half its 1.5 pt stroke, so the painted knot stays within the 4 pt plot inset. Tested. |
| **D10** Hidden sidebar badge keeps a 16 pt slot | **Fixed** | In `SidebarCell`, the badge width and its gaps (and the detail gap) go to 0 when that part is hidden. Test: an unbadged 200 pt row gives at least 20 pt back to the title and detail. |
| Machine B: the toolbar draws over the options bar and the "Properties" heading at 1280–1512 | **Fixed** | Same cause as D01: the split view was centred upwards. Now the options bar and Properties sit below the toolbar at 1280, 1440 and 1728, and at the 960 minimum. The harness asserts that no content control is under the toolbar. |
| Machine B: clicks on Character ▸ Style reach Auto Edit | **Fixed** | Check 3 (no content control intersects a toolbar item) passes at every size. Before the fix, the Properties field sat under the toolbar overflow button. Two further changes mean fewer items overflow: below 1280 pt the toolbar's text buttons show icons only (`toolbarCompact`), and the Grid-only thumbnail slider is now removed in other modes instead of set to `opacity(0)`. |
| Machine B: after a resize the sidebar slides under the traffic lights | **Fixed** | The harness resizes each window with `setFrame` (three passes) and checks that the sidebar outline is inside the window and clear of the window buttons. Before the fix this failed at 960 (x = −114…−129) and in document mode (under the close and zoom widgets). |
| Machine B: a fresh `--app-dir` instance lists other instances' folders | **Fixed** | New `TesseraCore/AppDefaultsIsolation.swift`, installed first thing in `TesseraApp.init`. With `--app-dir` or `TESSERA_APP_DIR`, `UserDefaults.standard` becomes an absolute-path suite stored in `<app-dir>/Preferences.plist`. That store holds the folder registry (last and recent folders), basket target, panel states, window frames and every `@AppStorage`. It neither reads nor writes the shared per-user domain (verified: a key in the app domain is not visible through it), but still reads the global domain (locale). Without an explicit app dir nothing changes. `AppModel.swift` was not edited (outside the allowed paths). Test: isolation in both directions, plus flag and environment resolution. Not verified with a launched app, because launching activates the app (`TesseraApp.swift:108`) and the user is at this Mac. |
| L3 shell compact behaviour | **Fixed (shell); document bars handed off** | Status bar: `ViewThatFits` picks the full row or a compact one (position, decision, K/R counts, basket count), with the full strings in help. The message has an ideal width of 0, so it never decides the variant. Filter bar: only the action group switches to icons, so the rule field keeps its identity and focus. Tether session fields shrink to 120/124 pt instead of a fixed 200/240 (R01). Toolbar labels go compact, and the thumbnail-size slot is removed outside Grid. The document status bar and tabs are H7/H8. |
| L5 geometry polish | **Fixed** | See D04 and D07–D10 above. |
| R01 Tether session row | **Fixed** | Flexible field widths (above). Not captured with a live session. |
| R02 Status bar long strings | **Fixed (library)**; document handed off (H7) | Test: `StatusBar` with a long message fits 384, 480 and 640 pt at 24 pt height. |
| R03 Many document tabs | **Handed off** (H8) | The code is in `Document/**`. |
| R04–R06 sheet envelopes (L4); R07–R10, R12 | **Not in scope** | Outside M2-56's scope. L4 sheets are a separate package, and the filter and neural sheets are in `Document/**` (listed as H12). |
| R11 Document palette / hidden ZoomHUD | **Handed off** (H10, H11) | The code is in `Document/**`. |
| `PROBE_HIDE` debug switch left in `ContentView` by B5-10 | **Removed** | It was an environment-driven layout switch in the shell. |

## Tests

New:
- `LayoutContractTests` (14 tests): ValueSlider rects, ring and drawing at 80 pt; FlowRow pure and hosted;
  MenuPicker and Soft Proof widths; Compare partition; wheel puck; curve knot; sidebar slot; status bar;
  the `ShellBudget` yield order; `--app-dir` defaults isolation.
- `ShellLayoutTests`: 1 test covering the 24 windows.
- `ShellLayoutHarness`: support code.

Changed: `PeopleLayoutTests` is now background-safe, fails when the outline is missing, and adds the
containment check. B5-10's `DocumentInspectorLayoutTests` still passes unchanged.

Gate (`./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing`): see the end of
this file. ThemeLint is green. `build-ffi.sh` left the generated bindings unchanged, so there is nothing
to commit there.

## Files

- Shell: `Shell/ContentView.swift` (root, contained columns, yield order, compact status bar and toolbar);
  `Shell/ShellLayout.swift` (new: `ContainedColumn`, `ShellBudget`, `ShellLayoutAudit`, `toolbarCompact`).
- Shared:
  - `App/Components.swift`: `HugCompressible`, menu style, MenuPicker.
  - `App/TesseraApp.swift`: only the root and the defaults isolation.
  - Inspector: `ValueSlider.swift`, `LibraryPanels.swift`, `UnderstandingPanels.swift`, `ColorWheelView.swift`,
    `CurveEditorView.swift`.
  - Other views: `Compare/CompareView.swift`, `Sidebar/SidebarView.swift`, `Loupe/SoftProof.swift`,
    `Library/FilterBar.swift`, `Tether/TetherPanel.swift`.
  - `TesseraCore/AppDefaultsIsolation.swift` (new) and `DESIGN.md` §3.5.
- Evidence: `evidence/before-*.png` and `evidence/after-*.png` (same harness, ≤ 1400 px), plus
  `evidence/before-layout-failures.txt`.
- Handoff: `DOCUMENT-HANDOFF.md`.

## Gate result

`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-56 ./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing`,
run from `apps/mac`: **exit 0**. **318 XCTest tests, 0 failures** (plus 5 Swift Testing tests, all passing).
The run includes the 15 new tests (`LayoutContractTests` 14, `ShellLayoutTests` 1 over 24 windows),
the updated `PeopleLayoutTests`, `DocumentInspectorLayoutTests` and `ThemeLintTests`. The one expected failure
(the document inspector's interior, non-strict) is logged, not counted as a failure. `git status` shows no
change to the generated bindings (`Sources/TesseraFFI`, `Sources/CTesseraFFI`) after `build-ffi.sh`.
