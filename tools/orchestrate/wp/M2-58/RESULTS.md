# M2-58 results

RESULT: FAIL — latest exact gate fails in DocumentAdjustmentJSONTests; the identified-settings detail-settle race is fixed, but end-to-end presentation and P11 performance acceptance remain unverified.

## Scope

Read `tools/orchestrate/audits/perf/REPORT.md` in full and M2-53 RESULTS. The relevant audit acceptance definitions are REPORT.md:187,196-198. Work is confined to the M2-58 worktree and allowlist; no Document, jobs, or previews implementation was edited. No commits or foreground activation.

## Implementation

- P10: residency/Upright preflight moved from synchronous `Shared::render` to `DevelopJob::run`, behind the render serial lock but outside the session state lock. Image, renderer, settings, output, and adaptive-level state are immutable snapshots. Cancellation and generation are checked before preparation and again before surface acquisition/publication. The level/adaptation policy and pixel operators are unchanged.
- P11: `DetailPreviewSchedule` defers exact L0 work during settings gestures, admits one job, invalidates stale completions and session/visibility changes, and schedules directly after the final setter. Basic, generic panel, history-group amount, and mask slider/gradient routes notify the scheduler. Work already running is not preempted.
- P12: callback producer replaces a single pending mailbox value; only the first offer schedules a main drain. Lower generations cannot replace newer ones, even after draining. Each callback carries its matching histogram, eliminating the synchronous `getHistogram()` before `onFrame` (including when histogram UI is hidden). Histogram computation itself remains part of rendering.
- Surface ownership: synchronous callback acquires an IOSurface use-count lease. Mailbox replacement releases abandoned leases. Controller current display, loupe frame, and Metal completion retain required leases. Producers skip leased slots, with cancellation-aware worker backpressure if all are leased. Frame metadata alone holds no lease. The Swift allocator upgrades a one-slot request to at least two slots.
- P01: new identified settings API carries host input identity through coalescing. Callback exposes worker-entry timestamp using the same `CACurrentMediaTime` clock as Swift and actual resident/fallback outcome, not session backend. Unidentified history/other renders do not inherit a slider's identity. Loupe presentation carries identity/residency unchanged. The timing summary joins real input/dequeue/callback/present records, deduplicates first presentation per input, and leaves missing presentation percentiles null.

## Verification notes

- Executed the complete required chained gate with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-58` and `MACOSX_DEPLOYMENT_TARGET=15.0`. Rust release tests, clippy with warnings denied, formatting, workspace check, FFI build and Swift build passed. The final release Swift test command exited 1: 382 XCTest tests, one skipped, six assertion failures, all in `DocumentAdjustmentJSONTests`. Five additional Swift Testing tests passed. Full output: `gate-verified.log`.
- The six assertions concern missing round-trip keys for Color Lookup (`dither`, `source_filename`), Match Color (`neutralize`), and Auto (`highlight_clip`, `shadow_clip`). The fixture-only test reproduces three assertions without an engine call (`document-baseline-failure.log`). Its test file, JSON fixture and `DocumentAdjustmentModels.swift` are unchanged from HEAD. No unrelated Document implementation or test expectations were changed to hide these failures.
- Focused release Swift run passed all 11 tests across Develop, detail scheduling and mailbox suites (`swift-focused.log`). Scope verification found no disallowed paths (`scope-verification.json`).

- New boundary regression first failed on synchronous `.can_render_resident`; passes after moving it to the worker.
- Detail scheduling subtask reported five isolated XCTest checks passed and successful Tessera target compilation. Full verification below supersedes isolated checks.
- Standalone Swift 6 mailbox check actually executed: replacement, late-generation rejection, and IOSurface use-count lifetime checks passed.
- Python discovery: 27 tests passed, including causal identity joins and unavailable-presentation handling.
- Read-only concurrency review found one-slot ring deadlock and stale identity leakage into reset/history; both were corrected before final verification.
- Initial gate failed on one test `LevelSink` initializer missing new fields; fixed. An intermediate gate was stopped when review corrections made its binary obsolete, not counted as a pass. A packaging attempt was also deliberately stopped before source corrections.
- Toolchain available here is Swift 6.3.3, not 6.2.4. No executed Swift 6.2.4 claim.
- Merge hotspot: generated `apps/mac/Sources/TesseraFFI/TesseraFFI.swift` and `apps/mac/Sources/CTesseraFFI/CTesseraFFI.h` overlap other work packages. Regenerate from the merged Rust sources rather than choosing one sibling's generated binding wholesale.

## Before / after measurements

Historical before is the retained real M2-53 `drag-recheck/trace.json`, recalculated with the current summarizer into `historical-before.json`. It is NOT a controlled same-load A/B baseline: M2-53 reports concurrent builds and load above 100; current load samples have also been recorded. That trace did not enable Auto Upright or Detail.

| Boundary | Historical before median / p95 / max (ms) | M2-58 after |
|---|---|---|
| Main FFI settings span | 0.889875 / 1.225208 / 1.287166 | 0.300125 / 0.345042 / 0.405875 |
| Callback-drain span | 1.072333 / 1.653792 / 2.022041 | 0.345375 / 0.420125 / 5.878958 |
| Input-to-actual-present p50 / p95 | null / null | null / null; zero valid presentation timestamps |
| Auto Upright setter | not measured | 0.023750 median / 0.044250 p95 / 0.655958 max; 101 calls |
| Detail settle latency / open vs closed drag p95 | not measured | not yet measured |

Background app measurements must use `open -g` and `--nonactivating`, with foreground identity checks and verified release provenance. Occluded-window presentation callbacks can legitimately have no presented timestamp. That is an unavailable measurement, never a performance pass.

The Upright row is the executed Rust FFI setter benchmark (`upright-bench.log`), not the full Swift main span. It uses the Sony ARW, a 1280×900 planned view, Auto Upright, 101 back-to-back identified exposure updates, and verifies that the final identified frame eventually arrives with matching histogram. This deliberately exercises coalescing while analysis can be in flight, not 101 completed cache misses. Load samples around the command (including compilation) were 52.14 / 40.42 / 35.93 before and 26.40 / 39.81 / 39.45 after. Other builds remained active. The final-frame correctness/identity check passed; this alone does not establish the full P10 Swift main-span or pixel-parity acceptance.

Three subsequent executions of the exact release integration-test binary from the final gate are retained in `upright-runs.json` and `upright-run-{1,2,3}.log`. Each had 101 setters and successfully received input 101. Run medians were 0.011542, 0.022750, 0.023417 ms; median of those medians is 0.022750 ms. Respective p95 values were 0.012000, 0.031709, 0.037125 ms; worst observed maximum was 3.077458 ms. Per-run one-minute load averages were 30.28, 33.47, 35.34, with concurrent builds. These are setter/coalescing measurements, not completed-frame throughput.

## Actual background app run

`drag-after/{run,trace,summary,foreground}.json` records a fresh provenance-verified release launched through `open -g -n` with `--nonactivating`. The runner exited 2 for incomplete P01, not a pass. Foreground identity remained unchanged, the real grid mounted, self-test completed, no trace records dropped, and frame logging remained off.

- 121 input events, 121 callbacks and job-dequeue records, 120 causally joined callbacks. All 121 callbacks reported actual resident output.
- 122 drawable callbacks reported unavailable presentation times; zero actual presented timestamps. Input-to-present percentiles correctly remain null. No completion-time substitute was used.
- Main FFI and callback spans are in the table. Whole-main-thread occupancy is not measured. Engine sink p95 was 3.532750 ms, not input-to-display.
- Load before/after the app run: 36.13 / 34.14 / 36.07 and 39.65 / 34.98 / 36.33 (`app-load.txt`). These before/after timing rows are observations under different shared-host loads, NOT a controlled speedup claim.
- Pinned release base commit and source/archive/binding digests are retained in `drag-after/summary.json`. Independent provenance verification passed after the run.

## Remaining acceptance

Actual presentation timing remains unavailable under the required background-only conditions. P11's <=200 ms ordinary settle and <=10% open/closed drag-p95 comparison have not been measured. The main-span app test used ordinary exposure, not Auto Upright; the Auto Upright benchmark measured the Rust setter separately. Swift 6.2.4 is not installed here. Therefore passing builds/tests do not establish a full M2-58 acceptance PASS.

## Independent retry verification

Re-read the complete audit and M2-53 results, inspected the existing implementation, and preserved it without additional application-source changes. This retry does not claim to resolve the remaining acceptance items.

- Ran the exact required chained gate myself with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-58`: exit 1 (`gate-retry.log`). Rust release tests, strict clippy, formatting, workspace check, FFI generation/build, and Swift build passed. Release XCTest executed 382 tests, one skipped, with the same six assertion failures in `DocumentAdjustmentJSONTests`; the five Swift Testing tests passed. The six detail-scheduling, three Develop, and two mailbox tests all passed in this run.
- Reproduced the three fixture-only JSON round-trip assertions without calling the engine using `swift test --package-path apps/mac -c release --skip-build --filter DocumentAdjustmentJSONTests.testFixtureCoversEveryKindAndRoundTrips`: exit 1 (`document-fixture-retry.log`). The affected model, fixture, and test remain unchanged from HEAD. This is schema drift in the separate Document adjustment path, not evidence of a Develop regression. No test was weakened to conceal it.
- Python discovery passed 27 bench tests and the separate P10 boundary test. Toolchain is Swift 6.3.3; Swift 6.2.4 compatibility is still not execution-verified.
- The existing release bundle passed source/archive/binding provenance verification before a fresh background run. `drag-retry/{run,trace,summary,foreground}.json` records `open -g -n` with `--nonactivating`, unchanged foreground identity, 121 inputs, 121 job-dequeue records, zero dropped events, completed self-test, and zero valid presented generations. Runner exit 2 is an incomplete oracle, not a pass. Input-to-present p50/p95 remain null.
- Retry ordinary-exposure main FFI span: median **0.296833 ms**, p95 **0.323417 ms**, max **0.389417 ms**. Callback drain: median **0.323541 ms**, p95 **0.416792 ms**, max **0.814250 ms**. Compare only descriptively with the historical-before table, not as a controlled speedup. Load sampled before launch was **14.80 / 16.32 / 23.52** and after trace collection **53.47 / 34.09 / 29.43**; the required gate and other builds were active. This was not the Auto-Upright or open/closed-detail acceptance experiment.
- Programmatic allowlist validation found no disallowed modified/untracked paths. `git diff --check` reports whitespace from the UniFFI-generated additions; it is not reported as passing. No commit, push, or foreground activation was performed. `kanban_show()` could not identify a card because `HERMES_KANBAN_TASK` is unset in this runner.

RESULT: FAIL required Swift gate has six Document JSON assertions; actual input-to-present and P11 performance acceptance remain unverified.

## Current retry: export reproduction and detail-settle correction

Re-read the complete audit and M2-53 results. Re-ran the reported failing export test with the focused test command, without changing its thresholds, level adaptation, export scheduling, or Rust implementation. The failure did not reproduce (`export-repro.log`): all 120 drag frames remained at L2; idle engine-sink median/p90/max were 3.0/4.2/6.6 ms, export-overlap engine-sink values were 2.4/3.3/19.2 ms, and set-to-frame values were 2.6/3.5/19.5 ms. Export completed all five images in 6.169662542 s. These are one run's diagnostics, NOT actual presentation latency or a controlled before/after improvement. A subsequent host snapshot reported load 11.53/21.24/25.88; other builds were active. The unchanged export test also passed in the full gate. The earlier failure is not claimed fixed: consecutive slow frames can legitimately trigger the existing L3 adaptation under contention.

Found and corrected a separate P11 race:

- Before: mouse-up started exact detail, then the same edit's viewport completion unconditionally advanced the detail request revision. This discarded a valid in-flight detail result and scheduled redundant L0 work.
- After: `DevelopController.settingsRevision` advances on settings application and reload. `DetailPreviewSchedule.observeSettings` deduplicates that identity both at admission and completion. Identified viewport callbacks no longer supersede matching settle work; an intervening settings edit still rejects old detail. Unidentified callbacks still invalidate, preserving updates from direct engine/AI-mask mutations.
- The new race test failed against the original unconditional invalidation behavior (`detail-race-red.log`) and passed after the change. Seven value-only scheduler tests passed in the standalone Swift package (`detail-race-green.log`), and all eight scheduler tests passed in the actual application test target. This is a scheduling correctness result, not a measured 200 ms settle result.
- Remaining limitation: direct engine mask changes do not advance the Swift settings revision. A pre-mutation detail job can publish before the later unidentified viewport callback invalidates it, as before this retry. Full mutation-identity coverage is not claimed.

Latest required gate, executed in full with the specified external Cargo target: exit 1 (`gate-current.log`). Rust release tests, strict clippy, formatting, workspace check, FFI regeneration/build, and Swift build passed. Release XCTest ran 384 tests, one skipped, with six assertion failures confined to `DocumentAdjustmentJSONTests`; five additional Swift Testing tests passed. The affected Document model and test are still unchanged from HEAD. No assertions were loosened or tests skipped to produce a pass.

Python discovery passed 27 tests plus the separate P10 boundary check (`python-current.log`). Programmatic path verification found no allowlist/exclusion violations (`scope-current.json`). Generated UniFFI additions retain generator-produced trailing whitespace; a clean `git diff --check` is not claimed. No commit, push, app launch, or foreground activation was performed in this retry. Swift remains 6.3.3; 6.2.4 was not executed. The existing before/after app measurements above precede this retry, and no new presentation percentiles are asserted.

The result remains incomplete: actual input-to-present timestamps under background-only conditions, Auto-Upright full Swift main-span/pixel-parity acceptance, and the P11 <=200 ms settle / <=10% open-versus-closed drag-p95 comparison are not established. `HERMES_KANBAN_TASK` is unset, so no board transition was available.

RESULT: FAIL required Swift gate has six Document JSON assertions; full P10/P11/P01 performance acceptance remains unverified.
