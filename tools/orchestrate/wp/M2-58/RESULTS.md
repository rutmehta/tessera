# M2-58 results

RESULT: PARTIAL — resumed correctness validation passes after the current main merge and a test-fixture correction. The full Swift Auto Upright setter meets P10 timing limits in three recorded runs, and controlled Sony ARW before/after captures show exact pixel parity at exposure 0 and +1. P11 presentation/performance and P01 actual input-to-present acceptance remain unverified. The historical failures below are preserved; the final sections supersede their current status.

## Scope

Read `tools/orchestrate/audits/perf/REPORT.md` in full and M2-53 RESULTS. The relevant audit acceptance definitions are REPORT.md:187,196-198. Implementation work is confined to the M2-58 worktree and allowlist; no Document, jobs, or previews implementation was edited. The historical checkpoints below preceded recovery commits; the September 27 sections record subsequent recovery, integration and evidence commits on `wp/M2-58`. The controlled parity diagnostic temporarily installed its identical untracked test in main as well, then removed only that owned file after verifying its hash. No foreground activation occurred, and this worker never merged or pushed main.

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


## September 27 resumption on current main

Recovered work was preserved before integration: `/Volumes/betterSSD/tessera-cache/recovery/M2-58-20260927-102859/` contains a binary tracked patch, an archive of all 52 untracked source/evidence files, and a SHA-256 manifest. No M2-58 worker was active. Commit `45c69a2` preserves the recovered implementation/evidence; merge `fbacd9e` brought main `c0d4535` into `wp/M2-58`. Main was never modified or pushed. Excluded Document/jobs/previews source was only inherited from main, never edited by this package.

### Remaining detail race corrected

A direct mask edit or AI raster completion could invalidate an in-flight detail crop before the later unidentified viewport callback informed Swift. The host settings revision alone could not detect it.

- A lock-free engine `detail_revision()` combines the render-request revision (advanced even when a request coalesces without a new viewport generation) with the existing AI mask-raster revision. A rendered `DetailPreview` carries the captured revision.
- The main-actor detail completion checks that token immediately before delivery. A stale result is rejected and admits one replacement, while the existing interaction/session/visibility and single-flight checks remain active.
- The scheduler regression failed with two assertions against the old behavior (`engine-detail-red.log`) and passed all eight standalone value tests after correction (`engine-detail-green.log`). All nine scheduler tests, including AppModel notification ordering, passed in the actual app test target.
- Rust tests exercise coalesced settings with unchanged viewport generation and real AI `set_ai(Ready)` publication. The latter holds the session, AI map, and resampling locks while another thread reads the revision with a bounded timeout; it observes the changed token before any new viewport generation. This tests publication/invalidation, not neural inference quality.
- The Swift RAW test renders a detail crop, performs an actual direct linear-mask mutation, and observes the token change before a callback is needed. Generated Swift/header bindings were regenerated from the final merged Rust source and inspected for `DetailPreview.revision` and `detailRevision()`.

### Validation and first-attempt fixture failure

Executed the exact required chained gate in `gate-resume.log` with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-58`, `MACOSX_DEPLOYMENT_TARGET=15.0`, and `RUST_TEST_THREADS=1` for Rust release tests. Rust release tests, strict clippy, formatting, workspace check, FFI regeneration/build, and Swift debug build all passed. Both new Rust regression names appear in that log as passing.

The chain then exited 1 in release Swift: 409 XCTest tests, one existing skip, exactly one unexpected failure. My newly added direct-mask assertion initially used the JPEG test's tiny RGB fixture; detail rendering rejected that source with `RGB sources have no camera metadata` before reaching the mask assertion. The existing JPEG test behavior was restored, and the identical meaningful token assertion was moved onto the designated real RAW fixture. No production workaround or weakened threshold was introduced. All prior DocumentAdjustmentJSONTests failures are resolved by inherited main; the fixture round-trip and engine schema tests passed.

After that test-only correction:

- Focused release Swift: **15 tests, zero failures** (`swift-corrected-focused.log`).
- Full `swift test --package-path apps/mac -c release -Xswiftc -enable-testing`: **exit 0, 409 XCTest tests, one existing skip, zero failures, plus five Swift Testing tests passed** (`swift-resume-full.log`, `swift-resume-full-exit.json`). Existing expected ShellLayout behavior and compiler/native-library warnings remain unchanged.
- Rust source hashes match the gate snapshot, so the completed Rust checks remain evidence for the final source. This is a completed stage-by-stage gate after a Swift fixture correction, not a claim that the original chained command exited zero.
- Python bench discovery: **27 passed** (`python-resume.log`); the separate P10 submission-boundary check also passed. Path verification found no exclusions/allowlist violations (`scope-resume.json`).
- `git diff --check` still identifies UniFFI-generated trailing whitespace (`diff-resume-final-check.log`); no clean-whitespace claim. The actual generated files were retained rather than hand-editing their output.
- Swift is **6.3.3**. Swift 6.2.4 was not executed. Artifact/archive/binding hashes are retained in `validated-artifacts.json`, with source freeze hashes/timestamps in `gate-resume-source.json`.

### P10 full Swift setter measurement

The new main-actor RAW test measures the complete synchronous `DevelopController.apply` call, including Swift patch merge, JSON encoding, and FFI. It attaches a 1280×900 planned viewport on the Sony ARW, sends 101 exposure edits with Auto Upright enabled (100 interactive, final noninteractive), waits for final causal input 101, and verifies the histogram generation. It retains the requested **p95 <2 ms and max <8 ms assertions**. It does not install a display-link flush callback, so each measured call reaches the synchronous setter directly. Coalescing may skip obsolete worker frames; these are not 101 completed Upright renders.

After the full suite, three fresh test processes ran from the same release binary using `--skip-build`; no concurrent heavy work was started by the coordinated Machine A lanes. The host remained shared. Exact commands, all load averages and results are in `swift-upright-runs.json` and the per-run logs. Each run passed all timing, final-input, histogram and mask-token assertions.

| Run | Median setter ms | p95 setter ms | Maximum setter ms | 1-min load before / after |
|---|---:|---:|---:|---:|
| 1 | 0.021875 | 0.044125 | 0.511458 | 2.87 / 2.88 |
| 2 | 0.021583 | 0.030000 | 0.488417 | 2.88 / 2.90 |
| 3 | 0.022583 | 0.029167 | 0.504083 | 2.90 / 3.55 |

Median of the three run medians: **0.021875 ms**. Worst p95: **0.044125 ms**; worst maximum: **0.511458 ms**. Driver/filesystem caches were not purged. There is no controlled pre-change Auto-Upright Swift-setter baseline, so this is threshold evidence, not a claimed speedup. It does not measure all main-thread occupancy.

### Acceptance still open

No new app was launched or window activated/raised during this resumption. The retained earlier background traces still have no actual presentation timestamps. P01 input-to-present p50/p95 therefore remain unavailable; GPU completion or callback latency is not substituted. P11's <=200 ms actual final-detail presentation and <=10% open-versus-closed drag-p95 comparison remain unmeasured. A controlled before/after Auto-Upright pixel-parity comparison is also not established by the passing correctness suites. The full M2-58 performance acceptance is consequently **PARTIAL**, despite passing final correctness/build stages and observed P10 setter thresholds.


## Controlled P10 settled-output parity

The parent granted a serialized heavy slot after M5-31 finished. An identical temporary Rust integration harness was installed exclusively in the existing main and M2-58 checkouts, compiled against their separate release targets, and removed only after its SHA-256 matched the installed source. No tracked product source was edited, no additional worktree was created, and no UI was launched or activated.

- Before product source: `c0d45355f7ff97254f5f847bc8bdc5063be1fd1e`; observed main HEAD `503bc4635961963cda6098bbaa6f588d9e7a18bf` differed only outside the checked Rust product paths.
- After product source: `5ff267933be108494d78eab55181288258253e16`.
- Cargo manifest/lockfile, `.cargo` and all tracked crate source were checked against each pin before and after; the runner retained complete source hashes. Both unchanged checks passed, and both owned temporary-test paths were absent after cleanup.
- Both builds used `cargo test --locked --release -p tessera-ffi --test m2_58_upright_parity_capture --no-run --message-format=json`, `MACOSX_DEPLOYMENT_TARGET=15.0` and the existing external `main` / `M2-58` targets. No Swift or binding regeneration was needed.

The first attempt stopped at a harness-only compile error: an incorrect `is_empty()` assertion on the unit return from `set_settings` (E0599). No captures ran; cleanup and both source checks passed. That evidence remains in `parity-capture/`. Removing that assertion retained `unwrap()` success and did not change any equality criteria. The corrected identical harness compiled for both revisions, and all four capture processes exited zero. `parity-capture-retry/report.json` contains exact commands, executable/harness/fixture/source hashes, process durations, load averages and comparisons.

Each process used a fresh engine, support directory and copied Sony ARW with no inherited sidecars. Settings were native process revision 2, Auto Upright, exposure 0 or +1, noninteractive final rendering, a 1280×900 requested viewport and RGBA8 output. The initial frame settled before the update. The newer final callback synchronously copied every valid pixel row while producer serialization protected the surface; row padding was excluded. Histograms were read within the callback and required matching frame generation. Filesystem and driver caches were not purged.

| Auto Upright exposure | Valid frame | Level | Compared bytes per frame | Changed bytes | Metadata differences |
|---|---|---:|---:|---:|---:|
| 0 | 2460×1638 RGBA8 | 1 | 16,117,920 | 0 | 0 |
| +1 | 2460×1638 RGBA8 | 1 | 16,117,920 | 0 | 0 |

Exposure 0 bytes hash: `110ae9e8e7235eec70f99d45fa75caf8a2c9600efcb8b2e6644fe662dd81e63e`.
Exposure +1 bytes hash: `82e4f850e1397b73df995e139a8569d9ca9405586e5357b1dc86e3d2f1b5ad29`.
Both sides also matched full RGB/luminance histograms, normalized settings, process version, valid/display/surface dimensions, final level and reported `Metal (Apple M4)` backend. The four one-minute load samples before execution were 7.9253, 7.3706, 7.3706 and 7.3408; process durations are diagnostic capture durations, not performance comparisons.

Full raw pixel blobs were preserved outside Git under `/Volumes/betterSSD/tessera-cache/evidence/M2-58-parity-20260927-151311/`. Every copied blob was SHA-256 verified before removing only its owned in-checkout copy; `parity-capture-retry/pixel-artifacts.json` records all exact locations, sizes and hashes. Reports, complete histogram metadata, build/run logs and the reusable diagnostic are committed.

This establishes **bounded P10 settled-output pixel parity for these two controlled fixture cases**, alongside the earlier full Swift setter threshold evidence. Baseline `FrameInfo` lacks actual-residency metadata; both capture records explicitly leave residency null. Requested GPU and reported Metal do not prove the per-frame rendering route. No all-image parity, actual P01 presentation latency, or P11 <=200 ms / <=10% performance result is inferred. Overall acceptance remains **PARTIAL**.
