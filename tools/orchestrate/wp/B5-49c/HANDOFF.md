# B5-49c — keyboard checklist independent of the machine's Full Keyboard Access setting

Branch `wp/B5-49`, on top of `61e4b8a4`. Machine A remains the merger. Swift only: no Rust, `Cargo.lock` or `board.json` change. Supersedes the affected claims of [B5-49](../B5-49/HANDOFF.md) and [B5-49b](../B5-49b/HANDOFF.md); in particular `focus-hosted.jsonl` is replaced by one trace per mode.

## What Machine A's failure was

Two things, and only the first was a test problem.

1. The checklist asserted whatever the machine's own Full Keyboard Access (FKA) setting produced. Machine B has it off, Machine A on (`AppleKeyboardUIMode` = 2).
2. With FKA on, the app had a real keyboard trap. Document mode is entered after the window exists, and AppKit then leaves each Layers row with a key-view loop closed on itself. Tab from a row's eye button found no next key view: the keyboard stayed on the eye, or cycled list → row → list, and never reached History. With FKA off nobody can Tab to the eye, which is why B never saw it.

Pinning FKA on in process on this FKA-off machine reproduced A's two messages verbatim (`Step 5: Eye Tab did not move`, `Step 6a: Wrong History order: []`) before the fix.

## Mechanism

| Layer | What reads the setting | How the test controls it |
| --- | --- | --- |
| App | `KeyboardAccessPolicy.isEnabled` (new, `apps/mac/Sources/Tessera/App/KeyboardAccessPolicy.swift`). It is the app's only read: the focus trace's `fullKeyboardAccess` field. Default is `NSApplication.shared.isFullKeyboardAccessEnabled`. Tab routing and key safety in `KeyRouter` never read it. | `KeyboardAccessPolicy.override` |
| AppKit, public accessor | `NSApplication.isFullKeyboardAccessEnabled` | Test-bundle method exchange in `KeyboardAccessHarness` |
| AppKit, button key-view membership | `NSButton.canBecomeKeyView`. AppKit answers this from private cached state, not through the public accessor. | Test-bundle override in `KeyboardAccessHarness` |
| AppKit other control classes, SwiftUI focus proxies | Private state fed by the real system setting | Not controllable in process. Steps that need them are N/A with the reason. |

`KeyboardAccessHarness.withMode(true|false)` pins all three controllable layers for a scope and restores the previous value. Nothing writes a defaults domain; `defaults write` is never run; `/usr/bin/defaults read` is used only to print the generating machine's value into the result header.

The fix for the trap is in `LayersOutlineView.keyDown` (`apps/mac/Sources/Tessera/Document/LayersOutline.swift`): the unhandled Tab from a row control climbs the responder chain to the outline, which moves to the row's next key view, or leaves the list (forward to the first key view after it, Shift back to the list itself). It reads no keyboard-access setting.

## Item-by-item

| Task item | Status | How / test |
| --- | --- | --- |
| 1. One injectable policy for the app's own decisions | Done | `KeyboardAccessPolicy`; `InspectorFocusTrace.routeEvent` reads it. Grep finds no other app read of `isFullKeyboardAccessEnabled` or `AppleKeyboardUIMode`. `HistoryHeightButton.canBecomeKeyView` is a deliberate mode-independent override and is unchanged. |
| 1. Control AppKit inside the hosted window without touching preferences | Done for buttons and the public accessor; not possible for other control classes and SwiftUI proxies (see Limits) | `KeyboardAccessHarness` |
| 2. Checklist as two pinned variants, both passing on any machine | Done | `testCombinedKeyboardChecklistFKAOn`, `testCombinedKeyboardChecklistFKAOff`. Each first asserts that the policy, the accessor and the row eye's `canBecomeKeyView` follow the pin, then asserts the mode's rows: 6a PASS in both, 5 and 16b PASS (on) / N/A (off), 17b N/A (off). |
| 2. Proof of independence | Done as far as one machine allows | See Evidence |
| 3. Re-evaluate the 10 N/A steps | Done | See Tables |
| 4. No display-size assumptions | Done | `KeyboardTestWindow` does not constrain its frame to the screen; setup requires the hosted content to be exactly 1100×848 pt. All coordinates are in points relative to view bounds; no assertion reads a screen or a backing scale. |
| 5. Audit B5-21 / B5-25 suites | Done, one gap found and closed | See Audit |
| Regenerate tracked results, separate ON/OFF tables, opt-in only | Done | `tools/orchestrate/wp/B5-49/RESULTS.md` (+ `GUI-RESULTS.md` alias), `focus-hosted-fka-on.jsonl`, `focus-hosted-fka-off.jsonl`. `testEachVariantReplacesOnlyItsOwnResultTable`, `testResultArtifactsRequireExplicitOptIn`. |
| Clean tree after two consecutive gates | Done | See Gates |

## Tables

Full rows are in [RESULTS.md](../B5-49/RESULTS.md).

| Variant | PASS | N/A | FAIL | TEARDOWN | Key events |
| --- | --- | --- | --- | --- | --- |
| FKA on (pinned) | 21 | 7 | 0 | 1 | 118 |
| FKA off (pinned) | 19 | 9 | 0 | 1 | 113 |

The ten former N/A steps:

| Step | FKA on | FKA off | Reason for what remains N/A |
| --- | --- | --- | --- |
| 1b | N/A | N/A | Native ⌘N menu dispatch and the visible sheet need the application command scene. Not an FKA matter. |
| 5 | **PASS** — list Tab → selected row's eye → Tab moves on; eye Shift-Tab → list | N/A | Off: the eye is not a key view, so there is no eye ring. The off behaviour is still asserted (Tab unhandled, skips the eye, leaves the list, panels stay). |
| 6a | **PASS** — eye → − → + → ↺, Shift-Tab → + | **PASS** — list → − → + → ↺ | None. |
| 8 | N/A | N/A | The titlebar toggle and SwiftUI toolbar exist only in the application window; NSToolbar and SwiftUI proxies follow the real setting and need a key window. |
| 11 | N/A | N/A | As 8, and the sidebar toggle is a nil-targeted action through the key window's responder chain. |
| 16b | **PASS** — Properties Name → Load 3D LUT → Dither | N/A | Off: Load 3D LUT and Dither are not key views. The off behaviour is asserted (Name Tab skips both). |
| 17b | N/A on a machine whose real setting is off; PASS or FAIL on one where it is on | N/A | The checklist's destination, the Color header, is a SwiftUI focus proxy. Pinned on with the real setting off it stays out of the loop, so the hop cannot be reproduced; the native part is still asserted (Dither Tab unhandled and moves, Shift-Tab returns to Dither). |
| 19b | N/A | N/A | macOS full-screen Space transition needs an application window and could take focus. |
| 21b | N/A | N/A | Toolbar click and visible sheet need the application scene. |
| 22b | N/A | N/A | Application Quit / save prompts need the running app lifecycle. |

So a manual FKA-on pass on Machine A is reduced to 1b, 8, 11, 19b, 21b, 22b, plus 17b only if the automated row reports N/A there.

## Evidence that the result no longer depends on the system setting

This machine: `defaults read -g AppleKeyboardUIMode` → does not exist; real AppKit FKA false.

| Run | Result |
| --- | --- |
| Before the fix, FKA pinned on, this FKA-off machine | Steps 5 and 6a fail with Machine A's exact messages. The pin reproduces the other machine's behaviour. |
| Both variants, no simulation (the gate) | Pass |
| Keyboard suites with the *system* simulated on, `TESSERA_TEST_SYSTEM_FKA=1` (67 tests: checklist, panel Tab traversal, History traversal, button key safety, focus trace, key focus, key routing) | 67 passed, 0 failures |
| Same, simulated off, `TESSERA_TEST_SYSTEM_FKA=0` | 67 passed, 0 failures |
| Whole suite, system simulated on | 951 executed, 3 skipped, 0 failures; 5 Swift Testing passed |
| Whole suite, system simulated off | 951 executed, 3 skipped, 0 failures; 5 Swift Testing passed |
| `xcrun xctest -AppleKeyboardUIMode 2 -XCTest <four keyboard suites> TesseraPackageTests.xctest` | 42 passed, 0 failures |
| Same with `-AppleKeyboardUIMode 0` | 42 passed, 0 failures |

What the argument-domain override does, measured with a standalone AppKit probe on macOS 26: `-AppleKeyboardUIMode 2` does reach the process (`UserDefaults.standard` returns 2), but AppKit ignores it — `NSApp.isFullKeyboardAccessEnabled`, `NSButton`, `NSSlider` and `NSPopUpButton` `canBecomeKeyView` all stay false. It is therefore not a way to make AppKit behave as the other machine. The harness reads the same argument (or `TESSERA_TEST_SYSTEM_FKA`) and applies it through the pinned accessors, so the two rows above exercise the harness's simulation of the other machine, not a native AppKit switch.

## Limits, stated plainly

- The on-machine case cannot be fully proven from an off machine. The pin controls buttons and the public accessor. SwiftUI focus proxies and AppKit's other control classes keep following the real setting. On Machine A, with the off variant pinned, those views remain key views; with the on variant pinned here, they stay out of the loop.
- The assertions are written to tolerate that: traversal steps count History buttons in order of discovery within a bounded number of Tabs and never require a particular intermediate stop. The off-variant rows 5 and 16b require only that the pinned buttons are skipped. Row 17b reports N/A instead of PASS when the real setting is off, and judges only hops that start on a native control.
- What supports the claim for Machine A: the simulated-on run reproduced A's failure before the fix, and the unpinned B5-21 suites that walk the same inspector passed on A with its real setting.
- The row-exit fix is verified in background-hosted windows only (never key, never activated), like the rest of this checklist. Behaviour in a real key window under FKA is expected to be the same code path but was not observed.

## Audit of the B5-21 / B5-25 suites

`DocumentPanelTabTraversalTests`, `DocumentHistoryKeyboardTraversalTests`, `DocumentPanelButtonKeySafetyTests`, `InspectorFocusTraceTests`, `KeyFocusTests`, `DocumentKeyRoutingTests`.

- None reads the system setting. They force the first responder and assert on app-owned controls whose key-view membership is mode-independent (`HistoryHeightButton`), on bounded Tab trails, or on `KeyRouter` results. They pass with the system simulated both ways and passed on Machine A.
- One hidden assumption: `testTabFromLayersEyeButtonReachesHistoryHeightButtons` hosts the inspector in a fresh window, where AppKit splices the rows into the window's loop. It never saw the closed row loop of the app's real arrangement. Added `testTabFromRowEyeLeavesTheLayersListWhenTheInspectorJoinsAnExistingWindow`, which enters document mode after the window exists and runs under both pinned modes. RED before the fix with FKA on (trail `eye → list → disclosure → eye`).
- `InspectorFocusTraceTests` builds its input with an explicit `fullKeyboardAccess` value and is unaffected.

## RED-first record

- `2f57cf15` (previous worker): checklist variants, pinned on fails.
- `fcfcd2d3`: per-mode assertions and the panel traversal regression. Run without the fix: checklist on-variant fails at steps 5 and 6a; the new traversal test fails with FKA on. 7 assertion failures in 21 tests.
- `b6c2feca`: the fix. Same 21 tests: 0 failures.

## Regeneration

```sh
cd apps/mac
TESSERA_REGENERATE_KEYBOARD_RESULTS=1 swift test -c release -Xswiftc -enable-testing --filter DocumentKeyboardChecklistTests/testCombinedKeyboardChecklist
```

Each variant rewrites only its own table between `<!-- FKA-ON:BEGIN/END -->` or `<!-- FKA-OFF:BEGIN/END -->` markers, so the file is the same whichever runs first or alone. Three consecutive regenerations, including one of the on variant alone, were byte-identical for all four files. No hexadecimal addresses; every trace record has `keyWindow=false`. The notes name the landing control and the generating machine's real setting, so a regeneration on Machine A may legitimately differ in those notes (and in 17b); routine gates leave the variable unset and write nothing.

## Gates

At `6b846a1b` (last code commit; only this document follows).

```sh
export PATH="$HOME/.cargo/bin:$PATH" CARGO_BUILD_JOBS=5 RAYON_NUM_THREADS=5
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-49"
unset TESSERA_REGENERATE_KEYBOARD_RESULTS TESSERA_TEST_SYSTEM_FKA
git status --porcelain
(cd apps/mac && ./build-ffi.sh) && tools/orchestrate/swift-gate.sh
git status --porcelain
```

| Run | XCTest | Swift Testing | Gate | Status before | Status after |
| --- | --- | --- | --- | --- | --- |
| 1 | 951 executed, 3 skipped, 0 failures | 5 passed | SWIFT GATE OK | 0 bytes | 0 bytes |
| 2 | 951 executed, 3 skipped, 0 failures | 5 passed | SWIFT GATE OK | 0 bytes | 0 bytes |

Nothing was restored, cleaned, staged or committed between the two runs.

Strict release build: passed, exit 0, no warnings, run after both gates; tree still clean.

```sh
cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

No `cargo clean` was run: no Rust crate was touched in this lane.

## Scope

No GUI launch, activation, key or main window, system setting change, `defaults write`, or access to the prohibited library locations. Fixtures are two generated JPEGs and synthetic documents.

## Noticed, not changed

After layers are inserted above an existing row, a Layers row control can keep its previous `document.layers.row.N.*` accessibility identifier until the row is reconfigured (observed once in diagnostics: the control identified as row 0 was in outline row 2). The checklist now requires the eye it uses for step 5 to be in row 0. The identifier refresh itself belongs to the B5-42 identifier work.
