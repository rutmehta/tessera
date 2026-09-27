# M2-57 — thumbnail admission and viewport scheduling

RESULT: FAIL — implementation and focused regressions pass, but the required full gate does not. Latest verification observes variable RAW fallback timing (an isolated pass and a combined-gate failure), a document smart-filter assertion failure in another combined run, and six document adjustment JSON failures in the separately executed full Swift suite. Swift 6.2.4 execution is also unverified (installed compiler is 6.3.3).

## Scope

Read `tools/orchestrate/audits/perf/REPORT.md` in full and `tools/orchestrate/wp/M2-53/RESULTS.md` before implementation. This addresses audit hotspot 8 (`REPORT.md:106`) and P08/P09 (`REPORT.md:194-195`). No document or develop Rust source edits, commits, pushes, application launches, or foreground UI operations. All Cargo invocations retained `/Volumes/betterSSD/tessera-cache/target/M2-57`; no checkout `target/` was used. There is no `HERMES_KANBAN_TASK` in this runner, so no board lifecycle transition is possible.

## Implementation

### P08

- `apps/mac/Sources/TesseraCore/ThumbnailLoader.swift:124` keys shared flights by existing image identity and preview tier. Grid and filmstrip subscribers share one decode/result while retaining independent cancellation.
- `ThumbnailLoader.swift:181-210` admits a bounded newest-request backlog. Thumbnail pending work is at most twice the sum of registered grid/filmstrip viewport capacities, plus four bounded active flights. Legacy callers without registered viewports use an explicitly declared 32-cell viewport. Loupe pending work is capped at two, shares the aggregate budget when thumbnails are visible, and retains two slots for its one-photo viewport when the retained grid is hidden. No per-subscriber detached tasks or unbounded OperationQueue backlog remain.
- `ThumbnailLoader.swift:301-350` immediately detaches cancelled subscribers, removes abandoned pending flights, and keeps cancelled active work counted until it actually returns. A replacement for the same image/tier cannot overlap that active generation. Preview-tier work is admitted before the thumbnail backlog; per-tier queue priority and FIFO ties are honored.
- `ThumbnailLoader.swift:266-270,353-377` preserves result identity through main delivery and late subscription. A completed result stays available for the bounded flight lifetime even if another image evicts it from the LRU before worker cleanup. Invalidation is checked again between reentrant callbacks.
- `ThumbnailBrowser.swift` registers geometric viewport capacity, refreshes it during layout/visibility changes, detaches cells at `didEndDisplaying`, resumes them on display, and releases its registration on teardown. The opacity-hidden retained grid contributes zero thumbnail capacity and cancels its visible-cell subscribers. `ThumbnailCell.swift` retains represented-identity checks.
- Updated the existing 100-cell warm configuration benchmark to register its actual 100-cell viewport. Previously it requested 100 images through the implicit 32-cell viewport and waited for intentionally discarded work; its two 60-second waits were a real test integration failure, fixed without changing assertions.

### P09

- `crates/jobs/src/lib.rs:69-85` adds opt-in `with_interactive_reservation`. Worker zero accepts only UI/Viewport work; the other workers remain shared. `crates/tessera-ffi/src/lib.rs:312` actually opts the engine's three-worker pool into this policy.
- `crates/jobs/src/lib.rs:280-326` bounds background starvation: every ninth admission on each shared worker serves the oldest queued background job when one exists. This is a dispatch-count bound for finite/cooperative jobs, not preemption or a real-time guarantee. Default and single-worker constructor behavior are preserved.
- Submission and reprioritization wake all eligible workers. Jobs and cancellation destructors run outside scheduler locks. Tests cover three blocked previews, promoted work, fairness, and 4,000 concurrent randomized submissions/cancellations/reprioritizations with terminal completion checks.

### RAW stage cancellation

- `crates/previews/src/raw.rs:23-43,146-167,261-293` provides cancellable variants while preserving existing public entry points. Checks separate source inspection/read, RAW/embedded decode, rendering, fit/orientation, per-level resize, JPEG encode, atomic writes, and alias publication, including Linear DNG.
- Full is the last pyramid level published. Cancelled incomplete pyramids cannot masquerade as complete hits; already completed levels remain valid and retries can finish.
- `crates/tessera-ffi/src/preview.rs:204-231,251-311` passes the job context checks through and guarantees a terminal state/event when a queued job is discarded or an active job is cancelled. The completion guard disarms before callbacks, so reentrant replacement requests are not failed by the old job's destructor.
- Cancellation remains cooperative between stages, not an interruption inside native decode/render. This does not add a new public FFI subscriber-cancellation API or cancel independent Develop opens.

## Before/after evidence

These are deterministic correctness/admission harnesses, not scrolling FPS or input-to-display measurements. M2-53's actual `PerformanceTrace` begin/end spans are used for the synthetic reversal harness. No application was launched; if a future app-level run is needed it must use `open -g` with `--nonactivating`.

| Boundary | Before | After |
|---|---|---|
| Overlapping same-key subscribers | Original implementation produced distinct CGImage identities; regression failed | Both subscribers receive the same decoded CGImage; regression passes |
| 20,000 requests without yielding delivery actor | Unbounded intermediate implementation retained 20,000 subscribers (original source also had no bound) | 68 retained: 64 pending plus four active for declared 32-cell viewport |
| 20k forward + reverse, 24-cell viewport | No equivalent original measured timing | Five release repeats: peak 24 pending, four active, 24 subscribers; exactly the final 24 identities delivered on every run |
| Cancel and immediately resubscribe same key | Regression exposed two overlapping flights | One active flight for that key; replacement waits for old cleanup |
| Mixed thumbnail/loupe pending budget, two-cell viewport | Intermediate implementation retained six pending | Four pending maximum; loupe displaces old thumbnail backlog |
| Loupe admission behind one active + one pending thumbnail | Completion order `[0,1,2]` | `[0,2,1]`, loupe admitted next |
| Late subscription after cache eviction but before flight cleanup | Delivered-flight subscriber lost its callback | Receives retained result; deterministic gated regression passes |
| Hidden retained grid capacity | 40 pending slots in test | Zero thumbnail pending slots |
| Interactive admission while three slow previews are submitted | Original scheduler timed out at 100 ms with all three preview workers occupied | 20/20 release repeats below 5 ms; median **0.0131245 ms**, maximum **4.166042 ms**; third preview cannot occupy reserved worker |
| Background job behind queued UI stream | Original priority-only order completed all 32 UI jobs first | Background completion within eight preceding UI admissions on shared worker |

The five release reversal spans have median **1086.831208 ms** for the entire 40,000-request synthetic scenario including cancellation, bookkeeping, final decoding and delivery. This intentionally holds the delivery actor during synthetic traversal; it is NOT a main-thread-task percentile or an interactive app latency claim. No comparable original end-to-end timing exists, so no timing speedup is claimed.

Measurement provenance: `scheduler-measurements.json` contains every actual release sample and simultaneous host load; one-minute load ranged roughly 31–46 during those runs. `swift-measurements.json` contains all five release trace samples, counts, and load (roughly 13.6–15.0 one-minute load). Other builds were present throughout; earlier load was over 40. No p95 claim is made from these small contended samples. `measure_scheduler.py` and `measure_swift.py` reproduce the harnesses. Swift measurements use optimized release Swift linked against the rebuilt release Rust archive, not an installed app bundle.

RED logs retained: `swift-singleflight-red.log`, `swift-unbounded-red.log`, `swift-overlap-red.log`, `swift-review-red.log`, `swift-late-red.log`, `swift-viewport-red.log`. Scheduler RED outcomes and tests are detailed in `scheduler.md`. No timing threshold was weakened and no failing test was newly ignored.

## Verification and failures

- Required exact gate was run repeatedly by the parent agent. Latest invocation is `gate-last.log`, exit **101** at `previews::tests::raw_without_jpeg_is_rendered`, `crates/previews/src/lib.rs:337`: **7.20483925 s**, assertion `<3.0 s`. Earlier runs were 5.777299 s, 10.108530917 s, and 5.18217225 s; an isolated four-Rayon-thread attempt was 12.674306833 s. Serialized Rust tests also did not clear the timing threshold. Source/pixel assertions passed, but host contention is not a proven exclusive cause, so this remains a real failed gate.
- Latest gate's jobs suite passes (20 scheduler tests plus one pressure test, existing benchmark ignored). Previews unit suite: 21 pass, one timing failure, three existing ignored measurements. Cancellation regressions pass.
- Separately executed full `cargo test -p tessera-ffi --release`: **270 passed**, exit **0**, including RAW completion tests and edited-preview integration (`ffi-tests.log`). This does not retroactively make the combined gate pass.
- Ran the remaining Rust static gate independently: clippy for all three packages/all targets with `-D warnings`, `cargo fmt --check`, and `cargo check --workspace` passed. Existing vendor LibRaw deprecation diagnostics remain build-script warnings.
- `./build-ffi.sh` and `swift build` completed successfully, followed by the full release Swift suite (`swift-gate.log`). That run executed 385 XCTest tests, one skipped, with eight failed assertions in five cases. Six assertions are the unchanged document adjustment JSON mismatch cases (Auto, Color Lookup, Match Color, and fixture round-trip). The other two were the warm-100-cell viewport registration issue fixed above. Five Swift Testing tests passed.
- After final changes, debug and release focused verification each pass **23 tests**: 11 thumbnail queue tests, one viewport test, 10 preview event/cache/invalidation tests, and the repaired 100-cell configuration test. Logs: `swift-final-focused.log`, `swift-final-release.log`. Release reruns used `-Xswiftc -enable-testing`. All five repeated reversal harnesses pass.
- Installed compiler reports Apple Swift **6.3.3**, not 6.2.4. New geometry arithmetic preserves CGFloat operands and explicit integer conversion, but execution with 6.2.4 cannot be claimed.
- Programmatic tracked/untracked path validation found zero allowlist violations and no excluded document/develop edits. `git diff --check` passes. No commit or push.

## Remaining acceptance

The package is not green until the required full gate passes unchanged, the document JSON mismatches are resolved by their owning workstream, and Swift 6.2.4 compatibility is executed if that exact toolchain is required. Real-app 20k RAW scrolling/reversal and foreground-preservation measurements were not performed; the measured bounds here are the production loader/scheduler under synthetic headless harnesses, not a substituted app benchmark.

## Independent retry verification

Re-read the entire audit and M2-53 results, inspected the current loader, scheduler admission, and RAW cancellation path. Preserved the existing implementation and test thresholds rather than treating the LibRaw compiler warnings as the failure. No new application source changes were made in this retry.

- Ran the exact requested combined gate, retaining the external Cargo target: exit **101**, recorded in `gate-retry.log`. Jobs passed 20 scheduler tests and one pressure test. Previews passed 21 unit tests, with three existing ignored measurements and one failure at `crates/previews/src/lib.rs:337`.
- The RAW fallback took **4.051791416 seconds** in that gate. An isolated run also failed at **4.642428542 seconds** (`retry-raw.log`). The two observed durations have median **4.347109979 seconds**, not a controlled performance baseline. Preflight load averages were **20.29 / 20.13 / 27.94**; concurrent builds remained present. The image mean/stddev assertions passed. Host contention is not established as the sole cause, and the `<3.0` assertion was neither weakened nor bypassed.
- Executed the rest of the gate separately (`retry-remaining-gate.log`). Clippy with `-D warnings`, formatting, workspace check, FFI generation/archive build, and Swift debug build passed. The full release Swift suite ran **386 XCTest tests**, with one skipped and **six failed assertions**, all in `DocumentAdjustmentJSONTests`. Auto expects missing `highlight_clip`/`shadow_clip` fields, Color Lookup expects missing `dither`/`source_filename` fields, and Match Color expects missing `neutralize`; fixture round-trips repeat those discrepancies. Thumbnail queue, viewport, preview-event, and library responsiveness tests passed. This separate run does not make the combined gate pass.
- A separate full FFI test invocation was interrupted by the tool's **420-second timeout** (`retry-ffi.log`) during document transform tests. It is partial evidence, not a new full-suite pass; the earlier completed FFI result above remains labeled as the earlier attempt.
- Installed Swift is still **6.3.3**, so no Swift 6.2.4 execution claim. `git diff --check` passed, and programmatic validation of all tracked/untracked paths found **zero allowlist or exclusion violations**. No application launch, foreground activation, commit, or push was performed.

RESULT: FAIL full gate fails RAW timing and full Swift document JSON regressions; exact Swift 6.2.4 execution unavailable.

## Latest gate and admission recheck

Re-read the full audit and M2-53 results and reviewed the existing scheduler, Swift loader/grid lifecycle, and RAW cancellation implementation. No application source was changed in this retry. The implementation and earlier before/after evidence above are preserved, not represented as newly authored changes.

- Executed the exact requested combined gate twice with the required external Cargo target. Both exited **101**. The first passed previews but failed `smart_filters_render_edit_and_round_trip_through_save` at `crates/tessera-ffi/tests/document_filters.rs:642` (`mask hides the noise`). This was captured in terminal process `proc_8159f31b803d`; its shell redirected only the Swift suffix, which was never reached. The second is fully captured in `recheck-full-gate.log` and failed the unchanged RAW `<3.0 s` assertion at **8.600194209 s**, after 20 scheduler tests and one pressure test passed. No thresholds were changed or bypassed.
- Before those gates, the isolated RAW test passed at **2.389264833 s** (`recheck-raw.log`). These differently loaded single runs are not a controlled before/after measurement. The variability does not establish the cause of the slow run or excuse its failed gate. Preflight load was **22.70 / 27.05 / 27.63**, and a later check was **54.67 / 35.33 / 30.42** while concurrent builds continued.
- Ran the rest independently in `recheck-remaining.log`: clippy with `-D warnings`, `cargo fmt --check`, `cargo check --workspace`, `./build-ffi.sh`, and `swift build` passed. Full release Swift tests completed: **386 XCTest tests, one skipped, six failed assertions**, all in `DocumentAdjustmentJSONTests`; five Swift Testing tests passed. The failures remain missing round-trip fields for Auto (`highlight_clip`, `shadow_clip`), Color Lookup (`dither`, `source_filename`), and Match Color (`neutralize`). These are document-model schema failures, not a reason to weaken the JSON oracle. No document source edits were made.
- All **11 thumbnail queue tests**, the viewport test, and **10 preview event tests** passed in that full Swift run. Repeated the release 20k forward/reversal trace harness five times: **24 pending, four active, 24 retained subscribers, exactly 24 final deliveries** every time. Median full synthetic `PerformanceTrace` duration: **645.842417 ms**. Actual samples and load (**10.83–11.72** one-minute) are in `recheck-reversal.json` and `recheck-reversal-*.log`. No original equivalent timing or app-frame speedup is claimed.
- Repeated the reserved-worker regression five times using the release test binary exercised by the gate: **five passes**, median interactive admission **0.016666 ms**, all below **5 ms**, at one-minute load **66.43**. Evidence: `recheck-scheduler.json` and `recheck-scheduler.log`. An earlier Cargo-driven repeat attempt timed out after 180 seconds while the FFI rebuild was running; it supplies no timing evidence. The successful direct-binary samples avoid Cargo build-lock waiting. No p95 is claimed.
- Swift reports **6.3.3**. Explicit CGFloat geometry remains intact, but Swift 6.2.4 execution is not verified. `git diff --check` and programmatic allowlist/exclusion checks passed. No foreground app launch, commit, push, or excluded source edit. `HERMES_KANBAN_TASK` is unset, so no task lifecycle transition is available.

RESULT: FAIL required gate has RAW timing and document test failures; Swift 6.2.4 execution remains unverified.
