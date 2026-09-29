# B5-16 History keyboard and bounds checks: handoff

Branch `wp/B5-16-dither-ax`, linear on origin/main `c15dee24` (merge base). The History commits sit on the B5-16 inspector work, the Dither fix (`cd0aab7f`, with `219c8055`) and the runner-admission commits (`622b1747`..`c0ce9260`); branch head `3cb74ddb` (source-approved by A), this doc fix on top.
Hashes refreshed 2026-09-29: earlier revisions cited pre-rebase ids (base `12a65f67`, main `aec3738e`, Dither `10959dc2`, History `34a28724` / `172d3e6c` / `0fe8cbad`); those are stale.
Plan: the B5-16 History plan, section 3 (the automatable checks H1-H9 and H12).
Machine B ran everything here with hosted windows only: activation `.prohibited`, `orderBack`, no key window, no app launch, no computer use and no system-setting changes.

## Commits

| Commit | Kind | Content |
|---|---|---|
| b72e9cd8 | test | New `apps/mac/Tests/TesseraCoreTests/DocumentHistoryKeyboardTraversalTests.swift` with 10 tests. `DocumentHistoryHeightControlTests` is unchanged. |
| 64083a4a | test | Corrects two harness assumptions found on the first run (details below). The product assertions are unchanged. |
| 4e32a56f | fix | `HistoryHeightButton.canBecomeKeyView` is now `isEnabled && !isHiddenOrHasHiddenAncestor`. The change is in `DocumentHistoryHeightControl.swift` only. |

## RED on the first run (b72e9cd8, before any fix)

`swift test -c release -Xswiftc -enable-testing --filter DocumentHistoryKeyboardTraversalTests`: 10 executed, 9 assertion failures in 4 tests.
Host state: `NSApp.isFullKeyboardAccessEnabled == false` (the user's existing keyboard-navigation setting, left unchanged).

1. **Product finding: History buttons were not in the Tab loop.** This affected `testTabTraversesNativeHeightButtonsInOrderSkippingReadout`, `testTabSkipsDisabledButtonsAtEachClamp` (a) and `testHostedInspectorKeyViewLoopReachesAllEnabledHistoryButtons`.
   - Standalone: Tab and Shift-Tab from − left focus on −. `window.selectNextKeyView`, `selectKeyView(following:)`, a direct `keyDown` and `NSWindow.keyDown` all left it on −.
   - Hosted inspector: Tab from − went to `LayersOutlineView`, and 40 more Tabs stayed there. The trail never reached + or ↺ and never returned to −.
   - Cause: `acceptsFirstResponder` was overridden to `isEnabled`, but NSButton gates `canBecomeKeyView` on the keyboard-navigation setting separately. The diagnostic showed `canBecomeKeyView == false` for all three buttons, `nextValidKeyView == nil` standalone, and `LayersOutlineView.nextValidKeyView == nil` when hosted.
   - So the earlier assumption that "`acceptsFirstResponder = isEnabled` bypasses the FKA gate" was false. Focus could be forced, but it was never reachable by Tab.
2. **Harness assumption (not product).** `window.makeFirstResponder(disabledButton)` returned true. `makeFirstResponder` does not consult `acceptsFirstResponder`, so the check now asserts `acceptsFirstResponder == false` and `canBecomeKeyView == false` on the disabled button.
3. **Harness limitation (not product).** `testDocumentSwitchKeepsGlobalHistoryPreferencesAndShowsSelectedDocument` found no `document.history.row.*` identifiers.
   - In-process AX on the hosting view exposes only AppKit-backed elements: the sliders, the outline and cells, and the History buttons and readout. The SwiftUI History `HostingScrollView` has 0 AX children, and `accessibilityChildren()` returns [].
   - The test now compares the SwiftUI History scroller's content height between the two documents. docB has 2 more states, which is +2 rows, and switching back restores docA's height.
   - Row identifiers stay an on-screen (external AX) check.

Tests that passed on the first run: H4 collapse with focus, H5 resize without rewriting the request, H6 readout fit, H7 external preference write, H8 inspector-tab identity/focus/value, and H12 tool letter.

## Fix (4e32a56f)

```swift
override var canBecomeKeyView: Bool { isEnabled && !isHiddenOrHasHiddenAncestor }
```

Measured after the fix, with the setting still off:
- Standalone loop: − → + → ↺ → −.
- Hosted: `LayersOutlineView` → − → + → ↺ → outline. This also ends the hosted Tab trap on the Layers outline.
- Disabled buttons are skipped in both directions.
- KeyRouter returns false for every Tab while a History button has focus.

**Policy (intentional, user decision 2026-09-29).** History buttons are Tab-reachable even with macOS Keyboard Navigation off. Rut decided to keep it this way; it is not an oversight to be reconciled with Dither/LUT. That matches this control's existing enabled-only `acceptsFirstResponder` contract and the plan's intent. It differs from the LUT action buttons and the Dither checkbox, which use `isEnabled && super.acceptsFirstResponder` and so follow the system setting.
- Rejected alternative (recorded for context only): system-setting parity, i.e. revert 4e32a56f and change `acceptsFirstResponder` to `isEnabled && super.acceptsFirstResponder`.
- That alternative would make the preserved `testFocusedButtonsOwnSpaceAndReturnAndSendTheirActions` depend on the host's keyboard-navigation setting, because it asserts `acceptsFirstResponder`.

KeyRouter, the focus trace and global settings are untouched.

## Gates

- Focused: `swift test -c release -Xswiftc -enable-testing --filter DocumentHistoryKeyboardTraversalTests` → `Executed 10 tests, with 0 failures (0 unexpected)` (at 4e32a56f, pre-rebase id 0fe8cbad).
- Full: `tools/orchestrate/swift-gate.sh` at pre-rebase 0fe8cbad (`git patch-id` identical to 4e32a56f) → `Build complete!`, `Executed 758 tests, with 3 tests skipped and 0 failures (0 unexpected)`, `Test run with 5 tests in 2 suites passed`, **SWIFT GATE OK**. This includes the preserved `DocumentHistoryHeightControlTests`.
- These gates ran before the branch was re-based onto `c15dee24`. A runs swift-gate on `3cb74ddb` itself, because main's surface.rs changed after that measurement.

## Remaining on-screen steps (background computer-use pass)

Follow plan section 4 exactly, using a packaged candidate `dev.tessera.inspector-gui.<sha8>` built from this branch head.
- Use the host's existing keyboard-navigation setting. Record which setting it is and do not toggle it.
- Expected: − / + / ↺ are Tab-reachable with the setting either on or off.

Steps still unrun:

1. Setup: package, fixtures, a fresh A-owned app dir, launch with `--app-dir <app-dir> --inspector-focus-trace <file>`, and record the PID.
   - Preferences under `--app-dir`: `AppDefaultsIsolation.installForLaunch()` (`apps/mac/Sources/TesseraCore/AppDefaultsIsolation.swift`, called first in `TesseraApp.init`) redirects `UserDefaults.standard`, and so every `@AppStorage`, to `UserDefaults(suiteName: "<app-dir>/Preferences")`, i.e. the file `<app-dir>/Preferences.plist`. `defaults delete/write` on the bundle domain (`dev.tessera.inspector-gui.<sha8>`) has no effect on such a run.
   - The History keys are `DocumentInspector.historyHeight` (Double, the requested height) and `InspectorPanel.History` (Bool, expanded), both `@AppStorage` in `DocumentView.swift`.
   - Reset: with the app not running, start from an empty app dir, or `defaults delete "<app-dir>/Preferences" DocumentInspector.historyHeight` (absolute path, no `.plist` extension, so it goes through cfprefsd like the app does). Seed: `defaults write "<app-dir>/Preferences" DocumentInspector.historyHeight -float <pt>`.
   - Verify: `defaults read "<app-dir>/Preferences" DocumentInspector.historyHeight`. Do not edit or `plutil` the plist file while the app runs; cfprefsd may hold newer values than the file.
2. H11 external AX inventory:
   - Three `AXButton`s named Decrease/Increase/Reset History height.
   - The exact readout value string ("N pt" or "N points").
   - No duplicate identifiers.
   - `document.history.row.*` present.
3. H1/H2 real traversal from a known origin: into `document.history.toggle` → − → + → ↺ and back with Shift-Tab.
   - The panels stay visible.
   - Trace events show `handled=false`.
   - The focus ring is visible.
   - Traversal out to the History list and footer (SwiftUI) is recorded.
4. Space, Return and keypad Enter each activate once. There is no pan and no document switch.
5. H3/H5 clamp and resize:
   - At the maximum, Tab from − goes to ↺.
   - At 960×600, 1280×800 and 1440×900 in light and dark: the readout is not clipped and there is no overlap.
   - `defaults read "<app-dir>/Preferences" DocumentInspector.historyHeight` keeps the oversized request.
6. H4: collapse with + focused. Record the landing focus and the next Tab, then expand and confirm the value is restored.
7. H7: pointer drag on `document.history.resize`, then a double-click reset, then keyboard increase on the same body.
8. H8: ⌃2 / ⌃3 / ⌃1 with + focused. Record focus after each.
9. H9: click the tab-strip switch between two documents. The History list changes and the height and expanded state are unchanged.
10. H10: a real quit and relaunch keeps the stored request. Reset, quit and relaunch restores the default.
11. Negatives:
    - Tab in the viewport hides the panels (`handled=true`).
    - "b" over a focused History button selects Brush and leaves the height unchanged.
12. Teardown: quit, verify the PID is gone, compare fixture SHA256 before and after, and remove the A-owned app dir (its `Preferences.plist` holds all state of the run; the user's own defaults domain was never touched).

Out of scope, record as unrun: VoiceOver speech, held-key repeat counts, and hover or pressed transient frames.
