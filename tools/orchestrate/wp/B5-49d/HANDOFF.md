# B5-49d — Tab destinations checked by identity through SwiftUI proxy stops

Branch `wp/B5-49`, on top of `1bf23f3c`. Machine A remains the merger. Swift only: no Rust, `Cargo.lock` or `board.json` change.

## Machine A's failure

`DocumentPanelTabTraversalTests.testTabFromRowEyeLeavesTheLayersListWhenTheInspectorJoinsAnExistingWindow`, both pinned variants, on a machine whose real setting is Full Keyboard Access (FKA) on:

- pinned on: `[document.layers.outline, NSButton, document.layers.row.0.visibility, KeyViewProxy, KeyViewProxy]`
- pinned off: `[document.layers.outline, KeyViewProxy, KeyViewProxy, KeyViewProxy, KeyViewProxy]`

The test allowed four presses from the Layers list to History −. SwiftUI gives every focusable SwiftUI control its own `SwiftUI.KeyViewProxy` view, and those follow the real setting whatever the pin says. On A, more than four proxy stops sit between the list and −. The app was not at fault: the trail shows Tab leaving the row correctly.

## Option taken: (a)

`KeyViewWalk` (`apps/mac/Tests/TesseraCoreTests/KeyViewWalk.swift`) presses Tab until the target is first responder, compared by object identity.

- **Bound.** `budget` counts only controlled stops, the target included. Controlled stops are buttons (pinned), text, lists and the app's key-owning views. Uncontrolled stops — SwiftUI proxies, and AppKit controls of other classes that follow the real setting — are allowed in between, up to an explicit `uncontrolledLimit` of 64.
- **Termination.** A controlled stop seen twice, the start included, ends the walk at once with `.repeated`. Uncontrolled stops are not repeat-checked, because the trail cannot tell whether consecutive `KeyViewProxy` entries are one view holding the keyboard while focus moves inside SwiftUI. Every press is one or the other, so the walk ends within `budget + uncontrolledLimit + 1` presses.
- **Identifying a proxy.** The walk tries three things in order:
  1. the proxy's own accessibility identifier or label;
  2. otherwise, the element the hosting view vends at the proxy's centre (`accessibilityHitTest`);
  3. otherwise, the frame of the control it stands for, in host points.

  In the background test host SwiftUI builds no accessibility tree, so the frame is what names it, e.g. `KeyViewProxy(824,602 15×14)`. "Reaches −" never depends on naming a proxy: − is a native `HistoryHeightButton` and is compared by identity.

### Simulating proxy stops in process

`KeyboardAccessHarness.withSwiftUIProxies(focusable:)` replaces, in the test bundle only, `KeyViewProxy`'s own `acceptsFirstResponder` override. That method is the gate AppKit's key-view loop asks. `true` puts the proxies into the loop, as on a machine whose real setting is on; `false` takes them out, as on one where it is off. Nothing else is touched.

The row-exit test, and the new L2 test, now run six arrangements: each pinned mode × proxies as the machine has them, forced in, and forced out.

Measured on this FKA-off machine (list → −):

| Arrangement | Trail |
| --- | --- |
| pinned on, proxies in | `outline → NSButton → row.0.visibility → KeyViewProxy(824,602 15×14) → decrease` — the shape of A's trail |
| pinned off, proxies in | `outline → KeyViewProxy(824,602 15×14) → decrease` |
| proxies as the machine has them / out | no proxy stop |

With proxies forced in, the test also requires at least one uncontrolled stop, so the simulated path is really exercised.

## Item-by-item

| Item | Status | How / test |
| --- | --- | --- |
| Fix the failing assertion | Done (option a) | `rowEyeTraversal`, via `KeyViewWalk` |
| Unit-test the walk with A's two trails | Done | `KeyViewWalkTests`: `testMachineATrailWithFullKeyboardAccessPinnedOnReachesDecreaseThroughProxies` and `…PinnedOff…` (2, 4, 9 and 40 proxy stops), plus one proxy held for six presses, endless proxies bounded, closed loop, proxies hiding a loop, over budget, target counted, consumed key, description. RED at `8fc9c540`: 28 assertion failures with the old four-press bound, including A's exact trails; green at `af23afbf`. |
| Simulate proxy stops in process | Done | `withSwiftUIProxies`; `testForcedProxiesJoinAndLeaveTheKeyViewLoopAndRestore` and `testASwiftUIProxyIsNamedByTheControlItStandsFor` use a real hosted SwiftUI proxy |
| Audit the same assumption elsewhere | Done | See Audit |
| L1: stop the walk as soon as a view repeats | Done | `LayersOutlineView.tabFromRowControl` keeps a visited set instead of a 512-step cap; no other behaviour change; covered by the existing row-exit tests |
| L2: Shift-Tab from History − back into the list | Done | `testShiftTabFromHistoryDecreaseReturnsToTheLayersList`, all six arrangements |
| L3: swizzled FKA getter safe off the main thread | Done | The pinned state lives in an `OSAllocatedUnfairLock`; the exchanged `isFullKeyboardAccessEnabled` reads no main-actor state; the `canBecomeKeyView` override answers from the pin only on the main thread, via `MainActor.assumeIsolated`, and otherwise calls AppKit's own implementation. `testPinnedAccessorAnswersOffTheMainThread` (KVC from a detached thread). |
| L4: normalise SwiftUI `$[0-9a-f]+` | Done | `normalizeAddresses` covers `0x…` and `$…`, now in the result tables as well as the traces; `testTraceAddressNormalizationIsDeterministic` |

## Audit: fixed Tab counts to an identified control

| Place | Pattern | Action |
| --- | --- | --- |
| `testTabFromRowEyeLeaves…` list → − | 4 presses, every stop counted | **Failed on A.** Identity walk, 4 controlled stops |
| same test, eye → − → + → ↺; eye ⇧Tab → list | 60 presses in total; 4 presses | Identity walks; the ⇧Tab walk must also stay inside the row with no uncontrolled stop |
| `testTabFromLayersEyeButtonReachesHistoryHeightButtons` | 60 presses in total | Identity walks (− in 4 controlled stops, then + and ↺ in 1 each) |
| `testTabFromLayersOutlineReachesHistoryHeightButtons` | 60 presses in total | Identity walk, 4 controlled stops |
| `DocumentHistoryKeyboardTraversalTests.testHostedInspectorKeyViewLoopReachesAllEnabledHistoryButtons` | 40 presses for the whole inspector loop, SwiftUI stops included | + and ↺ in 1 each; back to − within 40 controlled stops, with uncontrolled stops limited separately and repeat detection |
| Checklist 5 (on) | list → eye in exactly 1 Tab; eye ⇧Tab → list in exactly 1 | By identity (a group row has a disclosure first); row-internal stops only |
| Checklist 6a | 60 presses in total | Identity walks; the note still reports the press count |
| Checklist 16b (on) | 12 presses Name → Load 3D LUT → Dither | Identity walks; Dither must not come before Load |
| Checklist 6b; History `testTabTraversesNativeHeightButtonsInOrderSkippingReadout`, `testTabSkipsDisabledButtonsAtEachClamp` | Single presses | Unchanged: adjacent buttons inside one AppKit control (a standalone AppKit window in the History suite), where no SwiftUI stop can intervene |
| Checklist 17b | One hop | Unchanged: it already reports N/A when a hop starts on a proxy |
| Checklist 18 / 19a | Up to 60 presses until the panels return | Unchanged: no identified destination, progress is checked on every press, and the inspector (with its proxies) is hidden |
| Key safety, key focus, key routing, focus trace suites | No Tab walks | Nothing to change |

Only the first row failed on A. The three 60-press and one 40-press bounds had the same assumption with more headroom.

These are the only bounds in the change that grow, and Machine A asked for each one in its finding. The 40 and 60 limits now apply to controlled stops instead of all stops, and the separate uncontrolled limit of 64 is added. Repeat detection is new and stricter: a native stop seen twice fails the walk immediately.

## Checklist artifacts

- **Step 16a.** The reopened rename is now taken from the row as it is after the first rename ends, and must have a field editor before its Tab is sent. In one regeneration of about twelve, that Tab had gone to the Layers list while the step still reported PASS.
- **Hosted window.** It is released after each variant.
- **Counts.** Unchanged: FKA on 21 PASS / 7 N/A, FKA off 19 PASS / 9 N/A, no FAIL. Eight consecutive regenerations with the command below were byte-identical for all four files.

Correction to B5-49c. B5-49c said a regeneration of the on variant alone was byte-identical. That held for `RESULTS.md` only. Generated alone, a variant's trace differs in two records (steps 18 and 19a, first responder = the window): their `semantics` field records which background window AppKit reports as accessibility-focused. That depends on the order of the process's window list (the hidden text-input window created in step 16a sorts ahead of a later window), not on the keyboard. The tables are unaffected. The tracked files must be regenerated together:

```sh
cd apps/mac
TESSERA_REGENERATE_KEYBOARD_RESULTS=1 swift test -c release -Xswiftc -enable-testing --filter DocumentKeyboardChecklistTests/testCombinedKeyboardChecklist
```

## What remains unverifiable off Machine A

- **Real proxy behaviour.** Forcing the proxies in yields one proxy stop between the list and −, while A shows four or more. SwiftUI's own focus engine here still believes the setting is off and moves past the other proxies. The walk is proven on A's actual trails in `KeyViewWalkTests`; the real count only bounds it between 4 and roughly 57, since A passed the old 60-press test.
- **The forced-out cells on A** (an ON machine made to look like OFF) have never run on a real-ON machine.
- **L2 Shift-Tab back through real proxies on A** is new and unobserved there. It is a separate test, so a failure would be isolated and named.
- **Proxy names on A.** If SwiftUI vends an accessibility element on A, proxies will be named by it; here only the frame fallback was exercised.
- **The non-button AppKit classification** (`isControlled` returning false for sliders and pop-ups) was never met in a walk here.

## Gates

At `f9ca68bb` (only this document follows).

```sh
export PATH="$HOME/.cargo/bin:$PATH" CARGO_BUILD_JOBS=5 RAYON_NUM_THREADS=5
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-49"
unset TESSERA_REGENERATE_KEYBOARD_RESULTS TESSERA_TEST_SYSTEM_FKA
(cd apps/mac && ./build-ffi.sh) && tools/orchestrate/swift-gate.sh   # twice, status captured before and after each
```

| Run | XCTest | Swift Testing | Result | Status before / after |
| --- | --- | --- | --- | --- |
| Gate 1 | 966 executed, 3 skipped, 0 failures | 5 passed | SWIFT GATE OK | 0 / 0 bytes |
| Gate 2 | 966 executed, 3 skipped, 0 failures | 5 passed | SWIFT GATE OK | 0 / 0 bytes |
| Full suite, `TESSERA_TEST_SYSTEM_FKA=1` | 966 executed, 3 skipped, 0 failures | 5 passed | exit 0 | — |
| Full suite, `TESSERA_TEST_SYSTEM_FKA=0` | 966 executed, 3 skipped, 0 failures | 5 passed | exit 0 | — |
| Strict release build (`-strict-concurrency=complete -warnings-as-errors`) | — | — | exit 0, no warnings | tree clean afterwards |

No `cargo clean`: no Rust crate was touched. Nothing was restored, staged or committed between the gates.

## Commits

- `8fc9c540` test: replay Machine A's trails through the old bound (RED)
- `af23afbf` test: bound walks by controlled stops (green), L4
- `a6cd0ae8` fix: L1 repeat stop in the row-exit walk
- `867adc0d` test: SwiftUI proxy control, thread-safe pin (L3)
- `cd281635` test: identity walks across the keyboard suites, L2
- `da96c08c` test: checklist 16a hardening, table normalisation, window release. The title says the artifacts became independent of process history; that holds for the tables only. Generated alone, a variant's trace still differs in two records (see Checklist artifacts).
- `f9ca68bb` docs: regenerated results

## Scope

No GUI launch, activation, key or main window, system setting change, `defaults write`, or access to the prohibited library locations.
