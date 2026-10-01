# B5-39 — load-resistant Swift layout probes

Branch: `wp/B5-39`, base `b1af2436` (origin/main at allocation). Local commits only.

## Harness design

All synchronization lives in `apps/mac/Tests/TesseraCoreTests/LayoutProbeHarness.swift`.
It flushes AppKit layout and display without implicit animation, pumps the run loop in
at most 5 ms slices, and compares exact snapshots of native descendant identity, converted
frames, bounds, visibility, and `DocumentInspectorProbe.frames`. Native `needsLayout` and
`needsUpdateConstraints` flags must be clear. Hosted views must have matching consecutive
reads and 50 ms of quiet across display ticks. Polling is bounded at two seconds; failure
reports pending view classes and the last measurements. A baseline is sampled before the
first run-loop spin, so a busy callback cannot consume the budget before a comparison is
possible. Synchronous AppKit work/run-loop callbacks cannot be preempted by this deadline.

SwiftUI has no public pending-layout query. Its geometry callbacks plus AppKit flags and
stable observations provide the available synchronization evidence. This does not claim to
prove that an arbitrary future async model operation has completed.

Async MainActor tests use `settleAsync`, which yields the actor in <=5 ms sleeps and uses
exactly the same `Probe` state machine. A nested synchronous run loop cannot execute all
queued MainActor jobs. Intrinsic `sizeThatFits` probes use the same bounded comparison loop
on returned sizes: they need two equal reads, but no display-tick wait or native-window
flags because these hosts are deliberately unattached and have no window layout.

The hosting-root transaction deliberately sets `animation = nil` and `disablesAnimations =
true`; AppKit flushes run with duration zero and implicit animation disabled. The shared
`LayoutProbeHarness.window` factory also sets `NSWindow.animationBehavior = .none` before
any ordering. All 32 direct test-window construction sites use this factory, including
native key/focus, text, transform, vector, recovery, workspace restoration, and sheet-parent
fixtures. This prevents window lifecycle animations from accumulating blocked dispatch workers
in a locked console. These tests
check final geometry, not animation. Shell windows establish their requested bounds before
being ordered behind other windows, avoiding initial callbacks from the hosting controller's
preferred-size geometry. No geometry tolerances or existing layout assertions were loosened.

The console was locked during verification (`CGSSessionScreenIsLocked = 1`, screen-capture
access granted). WindowServer refused Masks' `screencapture` with "could not create image
from window" even after stable layout. The Masks OCR path now caches the actual hosted
AppKit content into a bitmap; Vision still checks the identical full-label assertions at
both widths and appearances. This renders actual views, not text synthesized from the model.
Optional whole-window evidence retains the original `screencapture` path.

## Consumers

- `ShellHarness.window` / `settle`: ShellLayoutTests and all callers, including Agent Review.
- `DocumentInspectorLayoutTests`: every layer-kind and width sizing query.
- `MasksPanelLayoutTests`: actual populated inspector before containment and OCR.
- `DocumentHistoryHeightControlTests`: actual inspector creation/recreation.
- `DocumentHistoryKeyboardTraversalTests`: hosted creation, resize, preference, and tab changes.
- `AgentReviewLayoutTests`: synthetic busy presentation, plus shared shell hosting.
- `PeopleLayoutTests`: hosted containment/header probe.
- `DocumentInspectorActionButtonTests` and `DocumentDitherCheckboxTests`: async native adapters.
- `LayoutContractTests` and Shell's document-tab width probes: intrinsic sizing.
- `LayoutProbeHarnessTests`: delayed frame mutation/dirty layout in a real background window,
  and a deliberately never-stable measurement that must produce an expected timeout failure.

## Before-fix evidence and development runs

Historical evidence is not a same-machine statistical baseline:

- `origin/wp/B5-34:tools/orchestrate/wp/B5-34/EVIDENCE.txt`: two completed full gates failed
  (one inspector layout, one shell containment); a third gate was stopped after failures.
  A focused inspector rerun failed with four geometry violations; an isolated-preference
  focused rerun passed. The third gate also recorded History control lookup and Masks capture
  failures. Thus 2/2 completed historical gates failed, plus one incomplete failed attempt.
- Merged `tools/orchestrate/wp/B5-35/evidence/swift-final-failure.log`: one inspector failure
  with a stale historyBody at x=900 against a header at x=1440.
- This branch's first complete development run (`focused-preflight.log`): 54 tests, six
  assertion failures, including one stale inspector overlap and one empty shell-control
  collection. This was an intermediate implementation, not an untouched baseline.
- `focused-initial.log` and `sizing-diagnostic.log` were deliberately stopped after 31 and
  44 timeout assertion lines respectively: they incorrectly required native layout flags
  to clear on unwindowed sizing hosts. They are not completed test runs.
- `focused-diagnostic-final.log` is a compile failure fixed by marking the nested probe
  state MainActor-isolated; `focused-diagnostic-2.log` then ran nine tests with only the
  locked-console Masks capture failure. All four Shell tests passed in that run.
- `masks-bitmap.log`: Masks passed after using AppKit content capture.
- `pre-stress.log`: five tests, one timeout because the first run-loop spin consumed the
  budget before a baseline read. `pre-stress-2.log`: five tests, zero failures after taking
  the baseline before spinning.

## Final verification

Final source, including the nonanimated window factory, passed all five stress repetitions
with no retries or skipped tests in the selected classes:

| Run | Tests | Failures | XCTest duration |
| --- | ---: | ---: | ---: |
| 1 | 54 | 0 | 83.745 s |
| 2 | 54 | 0 | 87.342 s |
| 3 | 54 | 0 | 89.989 s |
| 4 | 54 | 0 | 85.564 s |
| 5 | 54 | 0 | 79.675 s |

Total: **270 test executions, zero failures; 0/5 failed stress invocations**. Every invocation
ran all 11 selected classes, including the two helper checks. See `evidence/stress-summary.log`
and `focused-1.log` through `focused-5.log`. The deliberate helper timeout is an XCTExpectFailure,
not an ignored failure or weakened geometry check.

The Rust workload ran from 07:13:52Z through 07:21:38Z; tests ran from 07:13:52Z through
07:21:02Z. Seven cargo invocations exited zero: the first was a one-second warm-cache check,
then six tessera-ffi release rebuilds sustained compilation through the batch. See
`cargo-stress-timeline.log`. Observed load averages during this final batch included
60.16 / 38.20 / 43.79 (`load-samples.log`).

The earlier five-run batch also passed 270 test executions with zero failures under a cold
Rust build followed by rebuilds (06:56:08Z–07:03:53Z). It preceded the window lifecycle change
and is retained separately in `evidence/pre-window-animation-fix/`, not substituted for the
final-version results above.

### Full gates (both retained)

1. **Failed and incomplete.** `DocumentAdaptiveWideAngleTests.testOKSurfacesAFailedFinalTrace`
   observed a nil error message instead of the expected constraint failure (untouched test).
   The run then stopped making log progress at 03:07:40 during SelfTestQuitTests. A process
   sample showed **81 NSAnimation blocking stacks** and the **dispatch-thread soft limit of
   80 reached in all 308 samples**. The owned XCTest process was terminated after several
   minutes without progress. The gate's printed "2 tests, 2 skipped" is its last completed
   suite summary, not a whole-run count. Evidence: `swift-gate-1.log`, `gate-1-partial.log`,
   and `gate-1-sample.txt`.
2. **Passed: `SWIFT GATE OK`.** The unmodified `tools/orchestrate/swift-gate.sh` completed:
   debug build 9.34 s; **866 XCTest tests, 3 skipped, 0 failures** in 170.479 s; **5 Swift
   Testing tests passed**. SelfTestQuitTests passed, including the timing test where the
   first attempt stopped making progress. Evidence: `swift-gate-2.log` and the complete
   retained `swift-gate-2-tests.log`. No environment overrides or excluded tests were used.

After routing all direct test NSWindow construction through the shared nonanimated factory,
`window-animation-preflight.log` passed 19 tests with zero failures. The never-stable helper
check uses only the queried measurements, independent of an unattached view's dirty flags.

## Reproduction and scope

First ran `export PATH="$HOME/.cargo/bin:$PATH"; cd apps/mac && ./build-ffi.sh` successfully.

`tools/orchestrate/wp/B5-39/stress.sh` runs five serial release-test invocations with one
parallel Rust release-build loop. Rust uses `--locked` and a separate target directory at
`~/.cache/tessera-target/B5-39-layout-stress`; between successful builds, only tessera-ffi's
artifacts in that dedicated target are cleaned to sustain compilation. The timeline records
build/test overlap. It does not claim to simulate eight to ten independent builds.

No product sources, Rust, Cargo.lock, or board.json edits. No app launch or foreground GUI
activation. The existing XCTest background windows remain nonactivating. FFI regeneration
produced no tracked source changes. No push or merge.

## Local commits and cleanup

- `87e0c6fd` — shared settling engine, animation-free fixture windows, unchanged geometry/OCR
  contracts, helper regressions, and repeatable stress runner.
- The accompanying `docs(B5-39):` commit contains this handoff and retained evidence.

After verification, `cargo clean` removed only the dedicated
`~/.cache/tessera-target/B5-39-layout-stress` build artifacts. The normal per-worktree FFI target
and Swift build remain available. See `evidence/stress-target-cleanup.log`.
