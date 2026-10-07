# B5-49e — independent review corrections

Branch `wp/B5-49`, based on `565db01e`. Reviewed the complete B5-49d independent ruling. Changes are confined to Swift test utilities and tests; no production, Rust, lockfile, board, golden, or checklist artifact change.

## Finding → code → test

| Finding | Code / resulting contract | Test / evidence | Status |
| --- | --- | --- | --- |
| BLOCKER: reaching − did not establish the first History entry | `KeyViewWalk.reachesFirst` requires arrival by object identity and no earlier + or ↺ anywhere in that walk's trail. Applied in both eye-entry tests, both outline-entry paths, and checklist 6a; the later + and ↺ walks retain their one-controlled-stop budgets. | `testWrongInitialHistoryEntryIsRejectedEvenWithInterveningProxies`: each wrong initial button, with 0, 1, 9, or 40 intervening proxies; ordered positive controls at every count. Existing hosted panel/checklist tests exercise the same helper. | Fixed |
| SHOULD-FIX: reverse traversal accepted another row | `staysInSelectedRow` requires every stop to be controlled and either the outline or a descendant of the selected row. `rowEyeTraversal` passes `fixture.row`. L2 deliberately retains its broader `inList` predicate. | `testSelectedRowWalkRejectsOtherRowsAndUncontrolledStops`: selected disclosure accepted, another row's disclosure rejected; hosted row-eye traversal runs both pinned modes with natural, forced-in, and forced-out proxy arrangements. | Fixed |
| SHOULD-FIX: checklist 5 accepted external uncontrolled stops | Forward and reverse walks both validate the whole trail using the selected-row predicate. Forward validation starts at the outline, so the first press cannot escape validation. | Same scripted row test rejects external proxies and even uncontrolled stops within the selected row in both directions; both hosted checklist variants pass. The forward budget of two is exactly the old initial press plus one controlled stop, not an increased allowance. | Fixed |
| SHOULD-FIX: proxy integration test assumed frame naming | Controlled detached-view test asserts exact fallback formatting. Separate semantic test checks label naming and identifier precedence. Hosted SwiftUI integration accepts the exact frame name or a semantic name from the proxy / supported accessibility hit-test path, retaining identity, uncontrolled classification, frame bounds, and identifier assertions. | `testProxyNameFallbackFormatWithoutAccessibilityOrWindow`, `testProxyNamePrefersIdentifierThenLabel`, `testASwiftUIProxyIsNamedByTheControlItStandsFor`. | Fixed |
| NIT: uncontrolled target could bypass limit | Target acceptance now checks both controlled and uncontrolled limits. Existing budgets and the 64-stop uncontrolled limit are unchanged. | `testUncontrolledTargetMustFitTheLimit`: target at uncontrolled stops 63 and 64 succeeds; target at 65 fails with `uncontrolledLimit`. Existing controlled-target budget test remains. | Fixed |

## RED → GREEN

- `78fb648d` — RED: 19 `KeyViewWalkTests` executed, 10 assertion failures across 3 tests, exit 1. Eight wrong-first-History cases, one other-row case, and the uncontrolled target at 65 failed for the intended reasons. Existing destination-only and broad-list contracts were factored into helpers to expose those false passes before tightening them.
- `b87c2811` — fix: targeted run of `KeyViewWalkTests`, `DocumentPanelTabTraversalTests`, and `DocumentKeyboardChecklistTests`: 41 executed, 0 failures, exit 0 (19 + 17 + 5).
- Naming-format and semantic tests also ran in RED; those correct existing naming paths passed. The hosted fallback-only assertion was corrected as explicitly requested by the review.

## Final gates

All final gates run sequentially at `b87c2811`. Regeneration is disabled. Standard lane build environment: Cargo build jobs 5, Rayon threads 5, lane-isolated Cargo target directory. No exclusions or golden updates.

| Gate / attempt | XCTest executed / passed / skipped / failures | Swift Testing | Exit / result | Clean tree before / after |
| --- | --- | --- | --- | --- |
| FFI regeneration, attempt 1 | — | — | 0; no bindings drift | 0 / 0 status bytes |
| Swift gate 1, attempt 1 | 971 / 968 / 3 / 0 | 5 passed | 0; SWIFT GATE OK | 0 / 0 status bytes |
| Swift gate 2, attempt 1 | 971 / 968 / 3 / 0 | 5 passed | 0; SWIFT GATE OK | 0 / 0 status bytes |
| Full suite FKA=1, attempt 1 | 971 / 968 / 3 / 0 | 5 passed | 0 | 0 / 0 status bytes |
| Full suite FKA=0, attempt 1 | 971 / 968 / 3 / 0 | 5 passed | 0 | 0 / 0 status bytes |
| Strict release build, attempt 1 | — | — | 0; no warnings or errors | 0 / 0 status bytes |

No failed final-gate attempts or timing retries. XCTest-reported test durations: gate 1 451.042 s, gate 2 443.273 s, FKA=1 436.937 s, FKA=0 308.726 s. The intentional RED run and the targeted GREEN run above are the only other test invocations.

The three existing opt-in skips are `LibraryTests.testTwentyThousandEngineLibraryMeasurement` (generated 20,000-file measurement) and `SmartPreviewNativeWorkflowTests.testActualCachedThumbnailAfterOriginalDisconnect` / `testActualSwiftBridgeOfflineLibraryRenderSaveReopenAndReconnect` (external RAW fixture not supplied). No new skips were added.

Each full Swift suite uses `swift test -c release -Xswiftc -enable-testing`. The strict build uses `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors`.

No Rust crate changed, so Rust workspace gates and `cargo clean` are inapplicable under this lane's explicit Swift-only instructions. FFI regeneration still runs independently and within each Swift gate.

## Scope and limitations

No foreground GUI launch, focus activation, system setting change, or prohibited library access. Added fixtures are synthetic. Logs remain outside the repository; this handoff records only sanitized results. No bindings drift, assertion weakening to obtain green, relaxed bound, command-level test exclusion, or golden re-pin.

The environment-mode suites and forced proxy arrangements do not change the machine's real macOS Full Keyboard Access setting. Execution on the merge machine with real FKA ON remains an integration check there; scripted trails validate interleaving independently of local system state, within the unchanged termination bounds.
