# Tessera task board — Machine A coordinator

## Codex performance lanes — 2026-10-01

Phase-2 assignment: `ebae08bb`. Claude A retains all main merges; B owns app-side P16/P20/B5-33. LR-0 source inventory is published at `5bce045e` on `codex/lr-0-inventory`, awaiting coordinator review; LR implementation remains gated by that review. LR-2 `SOURCE-PLAN.md` is committed on the style branch and records the new brief `1a6f01e5` grayscale/PV2010 gaps; Adobe presence does not prove active counters, and the raw-map contract landed through main `0c6ab1c7`; source reconciliation is preserved at `8d61adce` on the style branch. Integrated KEY_MAP is201 (58 passthrough), superseding LR-0's199/56 snapshot; dated LR-0 addendum at `7f0590b7` independently reconciles the counts and status amendments; coordinator matrix approval remains pending. PERF-2/3 source design is also committed on the style branch. No peer receipt for LR status/result has been verified.

| Lane | Owner | State | Verified evidence / next action |
| --- | --- | --- | --- |
| PERF-1 style export | Codex A / Astra | Observed RED `ad4b7165`; candidate `b64e5e00` source-reviewed; benchmark `b193aacf` authored | Focused Release run compiled: four analytic/context tests passed, two expected source-count failures (6 vs1 raster, 2 vs1 live). Live pixel oracle passed; later style-count assertions were not reached. Evidence/hashes in `PERF-1/RED-RESULT.md` at `e32e6320`. Candidate implements frame-local bounded style reuse with budget/lifecycle/parity tests; independent budget and routing reviews found no source blocker (preserved376833f0). Candidate compilation/tests UNRUN; exploratory benchmark authored with explicit scope/high-water caveats, also UNRUN. Frozen preimplementation blur/artifact validation and final performance evidence remain pending. No GREEN or performance result. Batch19 completed; batch21 integrated; explicit runtime handoff remains unverified. |
| PERF-4 Gaussian | Codex A / Luna | Candidate `04e481cd`; review-ready, not merged | Focused parity/cancel, full Release filters suite, strict Clippy and fmt pass. Paired 24 MP r12 convolve median ratio 2.2352x (1938.054/867.070 ms). Public apply measured 2180.016→1086.382 ms = 2.0067x, but baseline/final load differs and margin is negligible; not a robust/general whole-apply claim. Exact trials, logs, exits and hashes are in `tools/orchestrate/wp/PERF-4/RESULT.md` and `/Volumes/betterSSD/tessera-validation/perf4/`. |
| PERF-2/5 Camera Raw and memory | Codex A / Luna source review | Source design committed on style branch | Multiple full-frame conversion buffers identified; report RSS is not live-heap allocation. Coordinate one camera_raw.rs owner; counting-allocator oracles required. |
| ENG-1 texture/clarity conditioning | Codex A / independent reviewers | Tests `88d5b4ef`, review/diagnostics `c682f621` on `codex/eng-1-conditioning`; UNRUN | Three CPU regression groups author sign crossing, almost-no-op identity and fixed eligible adjacent-ULP sensitivity with selected 1e-4 scene-linear bound, distinct from encoded 0.01 acceptance. Independent source review preserved; bounded witness/neighbor diagnostics added without assertion changes. No formula/product/shader change. Both GPU paths, sharpen chain, goldens and 24MP acceptance remain. |
| PERF-3 vector drag | Codex A | Source design committed on style branch | Preserve pixel parity and dashed-stroke fallback; no runtime result. |

Accepted phase-2 mailbox publication `aed5dbb5-33ee-40ba-bbf5-c348cac218af`; peer receipt unverified. No compiler/benchmark may overlap another A build or GPU test. Test/source work may proceed independently. B5-prof timings were measured under load on a different host and are diagnostic, not this host's baseline. No Swift or GUI jobs launched by Codex for PERF-4.

### External Lightroom accessibility integration — 2026-10-01 10:56 UTC heartbeat

Origin main `eca703ba` integrates B5-36 Lightroom report/fidelity accessibility. External batch23 evidence on that exact revision records Swift gate0, binding drift0, strict0; raw Swift log has891 XCTest cases/3 skipped/0 failures plus5 Swift Testing passes. These are independently read external results, not Codex-run or installed-app visual acceptance. Batch22 `e32e864d` write-guard candidate separately records Rust/Clippy/fmt/Swift/strict0 and binding drift0, but is not an ancestor of observed origin/main; do not claim it integrated. No compiler/test process observed at this snapshot, which is not an exclusive handoff. No mailbox message/peer receipt; six historical in-progress entries remain reconciled, not retried. Current-source LR addendum stays a frozen2164d370 observation; new main diff is app/test/handoff only. Codex runtime and approval gates remain unchanged.

### LR-0 current-source reconciliation — 2026-10-01 10:51 UTC heartbeat

Source-only addendum `7f0590b7` on `codex/perf-1-styles` preserves the original `5bce045e` matrix and records current main `2164d370`:201 unique mappings,143 CrsKey declarations,58 passthrough names,zero missing CrsKey. Astra and parent independently recounted source. Only UprightFourSegmentsCount/UprightTransformCount were added; ExtendedToneCurveName2012 is explicitly handled outside KEY_MAP with four curve names. Tagged lexical retention and identity-aware grouped diagnostics are reconciled, without claiming rendered translation or approval. No relevant dependency diff since0c6ab1c7. Required coordinator approval and Adobe semantic/representation gates remain open. External Swift PID5361 remains active; no Codex runtime launch. Mailbox zero pending,75 receipts,six historical in-progress,16 expired,zero invalid; no peer receipt or duplicate wakeup.

### External integration reconciliation — 2026-10-01 09:36 UTC heartbeat

Main2164d370 records batches20/21 integrated after OCR fixture corrections through04b22a93. Independently read external `batch21-04b22a93`: Swift gate0, binding drift0,889 XCTest cases/3 skipped/0 failures plus5 Swift Testing passes (main handoff says879; raw log says889). Prior3e39eb06 four-failure result remains preserved and is not relabeled passing. No compiler/test process observed in this snapshot, but no exclusive handoff or mailbox receipt arrived. New MACHINE-A.md still says Codex lanes not started, so the previously reported coordinator discrepancy persists. Codex feature candidates and evidence remain unmerged, validation held pending handoff.

### External Swift gate failure — 2026-10-01 09:06 UTC heartbeat

Batch21-3e39eb06 Swift gate exited1: summary reports888 XCTest cases,3 skipped,4 failures and names `MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth`; separate Swift Testing5 cases passed. Rust/Clippy/fmt0 and binding drift0 do not override this failure. Existing coordinator's strict Swift build remains active (PID38029 under parent11928); no completed strict result or overall acceptance verified. Main remains0c6ab1c7. Codex does not duplicate app/GUI repair or compiler work; mailbox has no new request/handoff.

### Retained-source dependency landed — 2026-10-01 08:40 UTC heartbeat

Main advanced to0c6ab1c7, integrating B5-29c/29d and retained-source parity/resume corrections. External batch20-0c6ab1c7 exits now show Rust/Clippy/fmt/Swift0 and binding drift0; earlier failed attempts remain above/below as historical evidence. Reused LR-0 Astra worker completed read-only comparison, preserved8d61adce: LR-2 plan now uses tagged `.properties` lexical Lua strings, handles descriptor variants and explicitly guards retention eligibility when promoting CrsKey entries. No implementation approval is inferred; Adobe conversion math remains unresolved. External batch21 parent11928/cargo11932 owns compiler/runtime, evidence batch21-3e39eb06; no Codex runtime launch. Mailbox zero pending and no verified runtime handoff. Claude A coordinator discrepancy remains unresolved.

### External retry reconciliation — 2026-10-01 08:15 UTC heartbeat

Batch20 `7501d410` Rust gate exited101: `ffi_retains_develop_sources_and_publishes_oversized_cells` failed at lrcat_streaming_parity.rs187 (Null versus expected string151388160). Clippy/fmt exited0; this does not qualify the failed Rust gate. Existing Claude coordinator now runs `batch20-0c6ab1c7`, parent94293/cargo94297; no outcome verified. Retained-source integration remains unaccepted from this evidence, Codex LR implementation remains gated, and no duplicate repair/runtime is launched. Mailbox and coordinator discrepancy are unchanged.

### External gate failure and retry — 2026-10-01 08:05 UTC heartbeat

Batch20 `d56218a6` failed: Rust101/Clippy101/fmt1/Swift gate2; binding drift0 does not make this passing. Rust log identifies an unclosed delimiter in `crates/tessera-ffi/tests/lrcat.rs` (function starting537/println615); Swift preflight reports missing fixtures/raw. The existing Claude A coordinator already owns a retry at `7501d410`, parent86203/cargo86209, evidence `batch20-7501d410`; no retry outcome verified. Codex does not duplicate its repair or runtime. Main555e4999 and the outstanding coordination blocker remain unchanged.

### Runtime hold — 2026-10-01 07:55 UTC heartbeat

External batch20 parent76994/cargo77000 is running Release tests for import-lrcat/tessera-cli/tessera-ffi, with evidence under `/Volumes/betterSSD/tessera-validation/batch20-d56218a6`. No completed gate exit was verified. Main remains555e4999; mailbox has no new request or peer receipt. Previously reported coordinator discrepancy persists; Codex runtime remains held, without duplicate launches or wakeups.

### Coordination discrepancy — 2026-10-01 07:30 UTC heartbeat

Main is `555e4999`. External batch19 records Rust/Clippy/fmt/Swift/strict exit0 and binding drift0; no compiler/test process was observed in this snapshot. This is not an exclusive runtime reservation. New main `MACHINE-A.md` says Codex has not started and is awaiting Rut, which conflicts with the published LR-0 inventory, PERF-4 validated candidate, PERF-1 source candidate and ENG-1 authored tests above. No peer receipt for the existing runtime request or these handoffs is verified. User attention is needed to reconcile the Claude A coordinator's view and secure an explicit runtime handoff; restarting from an empty process snapshot already caused the documented batch18 race. No duplicate session, wakeup or acceptance receipt was sent; all feature work and evidence remain preserved. Mailbox remains zero pending,75 receipts,six historical in-progress,16 expired,zero invalid.

### Runtime reconciliation — 2026-10-01 07:05 UTC heartbeat

Main advanced to `b8b6145e` (B5-34 stack-wide Camera Raw zoomed-out detail policy). External batch18 evidence `batch18-b8b6145e/exits.txt` records Rust/Clippy/fmt/Swift gate exit0 and binding drift0. These are observed external results, not Codex-run acceptance. External batch19 now owns the runtime: parent41901/cargo41905, `index` and `tessera-ffi` Release tests in `/Volumes/betterSSD/tessera-worktrees/batch19`, evidence directory `batch19-b033c8b5`; downstream Clippy/fmt/Swift/strict stages are not yet verified. Codex candidate tests and benchmarks remain UNRUN and require explicit coordinator handoff. Existing slot request `ec6a6548-1ecb-430d-a9fe-512d9b2d0efd` remains published without peer receipt; no duplicate request or wakeup. Mailbox unchanged: zero pending,75 receipts,six historical in-progress,16 expired,zero invalid. No historical retry or receipt ACK.

### Observed RED and runtime race — 2026-10-01 06:34 UTC heartbeat

Batch17 exited with Rust/Clippy/fmt0. After a clean process scan, Codex focused PERF-1 run started06:35:56Z; external batch18 started06:35:59Z, causing compilation overlap. Codex run completed exit101 (four passes/two intended RED failures) before an identity-guarded cancellation attempt; no process was signalled. No timing inference, no competing retry. Runtime-start publication `e0628ea5-75fb-4e47-90e1-10215853fd58` was not a verified reservation. Result publication `460114d2-a65f-4340-85cf-cfc231eba002` preserves this caveat. Explicit coordinator handoff is needed before another run; existing slot request remains outstanding. ENG-1 isolated managed worktree moved/repaired onto betterSSD at `/Volumes/betterSSD/tessera-worktrees/codex-eng-1-conditioning`; source-only tests published88d5b4ef. PERF-1 source implementation and independent ENG-1 test review are active, no second runtime.

### Runtime reconciliation — 2026-10-01 06:23 UTC heartbeat

Main is `b1af2436` after external batch16/B5-35 merge. External evidence `batch16-746576c1/exits.txt` records Rust/Clippy/fmt/Swift exit0 and binding drift0; Codex did not execute those gates. Before Codex could run, external batch17 parent72750/cargo72801 began integrating B5-32b tests/docs (`a68be567`), so the runtime hold continues. Request `ec6a6548-1ecb-430d-a9fe-512d9b2d0efd` asks the existing coordinator for the next free compiler/GPU slot; published with Git-note fallback, receipt/wakeup unverified, no duplicate enqueue. Style tests through `fc680629` remain UNRUN. `88551d67` reconciles pending B5-32b diagnostic coverage: reuse its stage/all-pixel reports, do not confuse ignored diagnostics or permissive bounds with ENG-1 0.01 acceptance. Mailbox has 0 pending, 75 receipts, 6 historical in-progress, 16 expired, 0 invalid; no historical retry or receipt ACK.

### Source checkpoint — 2026-10-01 06:12 UTC heartbeat

Main advanced to `b07993a1` (ENG-1 brief only). Reused workers published same-pass style namespace fixture `11c318d3` and ENG-1 source/independent-review/validation plans `392009d4` on the style branch. ENG-1 acceptance publication `9391d593-839a-4d71-b5be-0c47fef12b43` succeeded; peer receipt unverified. No runtime claim: external batch16 parent 18601 progressed into Swift compilation and still owns the lane. Mailbox remains 0 pending, 6 historical in-progress, 16 expired, 0 invalid; no receipt ACK or retry. Native B snapshot is unchanged historical notLoaded and does not establish current peer activity. Next after pipeline release: execute authored PERF-1 tests; ENG-1 product work requires its own branch and observed RED evidence.

### Source checkpoint — 2026-10-01 06:03 UTC heartbeat

Style branch `073819fc` preserves independent fixture review and live-scene routing findings. LR-2 Adobe-primary research at `e21a7871` documents UI behavior but leaves exact serialized schema and legacy conversion formulas unresolved; no guessed runtime oracle or product implementation. External batch16 parent PID 18601 remains active with Cargo/rustc descendants, so PERF-1 execution stays held. Next action after the complete external pipeline exits: run authored tests and preserve observed RED/analytic results before cache implementation. Main remains `202e7150`; Codex made no main merge. Mailbox reconciliation: 0 pending messages, 75 receipts, 6 historical in-progress, 16 expired, 0 invalid; historical work is not retried and no receipt is ACKed. No new peer receipt or wakeup is claimed.

## Coordinator reconciliation — 2026-09-30 06:37 UTC

This Codex coordinator resumed read-only reconciliation after its workers hit usage limits. Main is now `74f76332`; `MACHINE-A.md` records a replacement Claude Machine A coordinator and subsequent integrations. Its newer handoff supersedes stale rows/runtime text below: opaque CFA is integrated (`30b298c8`), Dither is integrated (`c9efa690`), and WB attribution G12 is complete with caveats; the failed decision-cache product remains unmerged. The newer B5-21/B5-25 GUI acceptance remains pending per the latest handoff. These are reconciled repository records, not newly executed acceptance checks.

Fetched origin and read B's `origin/wp/B5-16` takeover; that historical file still ends at the Dither source handoff. Mailbox poll: 10 pending messages all target the exact A chat UUID, six older accepted receipts, six expired messages, zero invalid messages. Pending results and accepted receipts overlap the replacement coordinator's work; no duplicate acceptance, execution or completion receipts were issued. No compiler/test runner was observed; existing user app processes were preserved. Ownership must be reconciled with the replacement coordinator before this chat launches workers, GUI tests or merges. No queue wakeup was sent.

<!-- CURRENT-QUEUE:START -->
## Current execution board

Updated 2026-09-29 07:30 UTC. “Done” applies only to the stated scope; historical failures remain below and in the linked evidence directories.

| Task | Owner | State | Verified outcome and next action |
| --- | --- | --- | --- |
| Smart Preview assets, journals, offline editing and sync | A | Done | Main `89b78881`; desktop controls, cached Library and proxy thumbnails integrated. Original files remain unchanged. |
| Smart Preview desktop workflow | A | Done for the tested local profile | Actual offline edit → separate-process reopen → reconnect/sync passed. Full 4920×3276 Original export and two-proxy Compare passed. External-volume profile startup and broader camera/UI coverage remain unqualified. |
| Smart Preview Metal rendering | A | Done for the qualified route | Main `8e100b80`; automatic CPU/Metal selection, exact CPU fallback for mapped geometry, HDR recipe preservation. 13 Engine processes, 312 frames, 63 comparable pixel pairs and 15 resource-release cycles passed. Original remains the default editing source. No general speedup over Original is claimed. |
| Checked Save As | B source / A integration | Done | Main `3bad015a`; native, 703-test Swift suite, strict build and actual Cancel/new-name/Replace/reopen workflows passed. Late-arrival races have native/Swift coverage, not a claimed GUI reproduction. Saved-HUD wording remains a noted clarity limitation. |
| Tabbed inspector and persisted Auto/LUT controls | B source / A GUI worker | Unmerged; remaining actual GUI acceptance | Current `d0191c41`:54 focused,3 layout/theme,strict and full741 XCTest passes/one skip plus5 SwiftTesting;155 mandatory checks and8,396 immutable inputs independently verified. Actual LUT keyboard/actions/light-dark compact layout passed. Dither-origin Tab FAILED: hides panels; exact evidence preserved and narrow B correction next. History/doc-tab/persistence interactions remain. |
| History accessibility | B source / A GUI worker | Automated qualification verified; actual keyboard pending | Current `d0191c41` includes prior History fixes and passes full suite. Earlier observed LUT Tab interception is corrected and bounded actual LUT workflow passed. History real keyboard traversal, bounds/pointer/clamp/restart remain distinct pending checks; no full VoiceOver claim. |
| Historical runner admission | B source / A | Reviewed; integration pending | Candidate `1972f603`: 12 exact-source Python tests passed for Transform arguments and resource-hold admission. This does not establish an actual Transform app workflow. Integrate with the qualified inspector package. |
| Bounded RAW capture ownership | A | Done for the component | Main `1b073b0c`; 42 capture contracts, full 60-test suite, strict checks and fixture preservation verified. Holds an identified captured byte stream; does not promise an atomic filesystem snapshot. |
| Closed captured-CFA decoder | A | Done for the component | Main `95bfadcf`; 11 focused tests, two explicit qualification tests covering Sony/Fuji/Nikon/Canon/DNG, full 71 ordinary tests and strict checks passed. Returns owned samples/metadata after stage cleanup. No recipe/render/document integration or decoder-memory bound is claimed. |
| Restricted RAW render admission | A | Private foundation qualified and integrated | Exact `c595dad2` source: 20 focused tests, full 116 passed/two ignored, strict/format passed; independent 8,971-input verification. Missing-fixture failure preserved; intended Leica DNG retry qualified. No pixel renderer, ICC/environment authority, memory reservation or public integration. |
| Private RAW normalization seam | A | Qualified and integrated | Exact `2d7c00fa`: shared private normalization and legacy delegation; 21 focused, full 92/two ignored, strict/format, five unchanged fixture hashes and independent 9,141-input verification. No public conversion API, new renderer or global memory-bound claim. Next opaque closed continuation has protocol RED/API evidence; implementation remains pending. |
| Opaque captured-CFA owner | A codec / independent review | Done for the component | Main `30b298c8` integrates exact `d908374f`: compile, 5 protocol GREEN, API positive + 8 intended negatives, full raw-decode 98 passed/4 ignored, actual Sony/Fuji/Nikon/Canon/DNG internal and external family runs, strict and fmt all passed with source frozen; independent read-only verification PASS. Evidence `tools/orchestrate/wp/UX-05/evidence/2026-09-29/owned-cfa-d908374f-green/`. First 02 launch failure (lost execute mode) preserved as harness setup failure. No rendering/public conversion/memory-bound claim. |
| Smart Preview reopen baseline | A | Measured and independently verified | Harness `3614e21b`: 24 opens, four stable cohorts, 12 pixel comparisons and resource release passed. Auto reopen-to-final-callback medians: 646 ms SDR / 589 ms EDR; CPU: 167 / 166 ms, at the same 820×546 level. Evidence on main `b9ccf6fe`. These are startup measurements on one fixture/host. |
| Bounded calibration-decision cache | A FFI / independent reviewers | Performance qualification failed; unmerged | Functional/native/frame/resource gates passed. All 22 timing processes completed, but WB edits regressed from ~5 ms to ~14.5–14.7 ms in SDR/EDR. Original thresholds and failure preserved; no rerun or product merge. Pure exact-key diagnostic `a9b90750` passed19 tests/strict/fmt/default graph with independent verification. Live wiring proposal needs storage/lifetime clarification; no real-frame diagnosis, performance rerun or remedy yet. Live attribution diagnostic: design rev7 independently approved; Stage A transport GREEN `a95f8ca2` independently verified; Stage B running. |
| Machine B coordination | A / B existing desktop writer | Active | New requests use the verified SSH queue plus durable Git mailbox. A validates targets and publishes receipts; a queue ID alone is not peer receipt. B owns its source branches; A alone merges main. Lifetime correction `47307c56` completed for diagnostic repair only; 48 tests/strict and actual trace passed that scope. Routing request `5b27b9df` has verified peer receipt; B source `527410c7` composed as A `d0191c41`; bounded actual LUT keyboard/action/appearance checks passed. Full inspector gates remain pending. Earlier failures remain preserved. |

**Current runtime owner:** None at this checkpoint. Dither `b766abf6` focused gate failed three detached AX assertions in one case (59passes); B exposed-element/native-control investigation next, layout/strict/GUI unrun. RAW `d908374f` compiled but GREEN launch hit runner copy-mode PermissionError before test execution; versioned setup correction underway, all subsequent gates unrun. WB guard scaffold is source-only; original performance failure remains unmerged.

**Preserved failures:** inspector `1d361fa3` had unsupported direct AX test assumptions and test-order leakage; `cd0b850d` had an unsized fixture; `7732a03e` had the theme-token violation. Each failure and correction is retained. The earlier GUI's brief profileless relaunch has unmeasured default-profile effects; no claim of zero user-state changes is made. Owned test apps are closed.

**Queue reconciliation:** M2-45d was already integrated through `5e5593c0`; do not repeat the merge. M5-31 remains unaccepted after its preserved 106.084 ms result exceeded the 100 ms gate. No unchanged benchmark rerun or threshold waiver is scheduled.
<!-- CURRENT-QUEUE:END -->

## Actual GUI offline editing and reconnect verified — 2026-09-28 19:18 UTC

The isolated final08 app now passes the observed local-profile workflow: build preview; ordinary quit; hold only copied original folder; new process opens cached read-only Library with visible thumbnail; explicitly edit proxy to exposure+1.00/temperature5700K; save; quit/relaunch offline with the edited rendering/settings preserved; refuse dirty discard; quit/restore copied original; Sync succeeds1/1; Original editor retains both settings. Owned app is closed. Root independently verified all47 final owned-file hashes; original/source RAW bytes remain unchanged. Native CUA AX/screenshots were observed in the agent tool transcript; no screenshot files are fabricated.

Evidence: `tools/orchestrate/wp/SMART-PREVIEWS/evidence/2026-09-28/gui-c1f9d4e0/GUI-LOCAL-CHECKPOINT.md`. The initial external-volume profile startup failure remains separate evidence; local-profile success does not qualify that configuration. GUI full export, offline export refusal and Compare remain deferred. Functional desktop process-restart acceptance is now supported for this single copied Sony fixture; cross-camera, multi-photo and performance claims remain open.

Runtime lane returned to FFI for diagnostic07 and subsequent GPU fidelity qualification. GPU remains unaccepted/unmerged, Originaldefault unchanged. In parallel, checked Save As source review is complete in `SAVE-DESTINATION-INTEGRATION-REVIEW.md`: no new Swift blocker, but native75a1b9a6/test2a22145b dependency is not yet on main; future integration must regenerate combined bindings rather than overwrite Smart Preview APIs with old81cc08eb output. No new Save As runtime acceptance.

## Active qualification follow-up — 2026-09-28 19:12 UTC

The accepted desktop implementation remains main `89b78881` with full evidence `62e4ee5b`. Nine B Smart Preview result receipts are completed. Additional Git status `16770a81` is published; no new peer receipt or wakeup is claimed.

GPU candidate remains feature-only and unaccepted. First Metal test01 and diagnostic06 each report8 pass/2 fail. Conditional identity-upright mapping removes an unnecessary coordinate conversion but does not fully resolve parity; Compact stage isolation points to upstream geometry rather than the display transform. FFI is preparing test-only exact-coordinate/sampler diagnostics with independent codec review; tolerances are unchanged. Original stays default.

Isolated final08 GUI package and provenance are preserved in `d099e109`. First launch reached neither an observable UI nor Smart Preview interaction: native CUA timed out and owned-process sample waited inside macOS preferences file access. No permission cause is asserted without observation. The reviewer owns a single bounded local-profile retry using copied data; FFI has released runtime execution and performs source-only work during it. Other apps and user photos remain preserved. No packaged GUI or separate-process desktop acceptance yet.

## Smart Preview desktop implementation integrated — 2026-09-28 19:00 UTC

Main `89b78881` integrates feature `c1f9d4e0`: Compact camera-linear previews, explicit proxy editing, offline cached Library, local edits/reopen/reconnect synchronization, and saved-edit proxy thumbnails with bounded retry. Original remains the default editing source and full-quality export uses the original. Main product/native/Cargo files exactly match the tested feature; newer coordination records and evidence are preserved.

| Work | Owner | Verified state | Next action |
| --- | --- | --- | --- |
| Native local thumbnails | A codec; independent review | `4cf91289`:187 FFI tests,28 previews tests passed;3 existing performance tests ignored; strict/fmt pass. Root verified7016 immutable Git inputs. Native evidence `cae2004d`. | Monitor with UI workload; no speed claim. |
| Desktop integration and B routing/retry | A integration; B source | Full Swift08:689 XCTest cases,1 skipped,0 failures;5 Swift Testing cases pass. Focused67 and strict optimized product pass. Root verified7018 Git inputs plus4 generated/archive artifacts, unchanged across gates. | Nine completed B receipts published; evidence `62e4ee5b` pushed. |
| Actual Sony proxy workflow | A codec | Real Swift/native offline thumbnail and edit/save/new-Engine offline reopen/reconnect sync pass individually and in full suite. Preserved original hashes checked by tests. | Packaged-app visible GUI and separate-process restart remain separate acceptance work. |
| Proxy GPU acceleration | A FFI next; independent review | Candidate and harness applied in feature only; first Apple M4 Metal suite8 passed/2 pixel-parity failures. Candidate remains unaccepted and unmerged. | FFI owns sole compiler/GPU lane; codec independently reviews geometry discrepancy. Fix parity before actual Engine latency qualification; Original remains default. |

The existing small-window Document layout expected failure and one existing skipped test are retained; this is not a claim that all historical GUI issues are resolved. Generated UniFFI output has three whitespace-only lines flagged by plain diff-check; coherent generated output is preserved and handwritten diff-check passes. Previous thumbnail failure14, native strict/fmt failures and initial full Swift failure remain durable evidence. No packaged-app GUI, physical input-to-present, cross-camera fidelity, or accelerated performance acceptance is implied.

B remains source-only with its existing writer and Document/SaveAs ownership. Latest compact native snapshot confirms completed retry source handoff `d40354e4`; `notLoaded` does not negate the peer result/receipt. Git status message `c2b22d9b` published the preceding validation checkpoint; publication is not peer receipt. A remains sole main integrator. Other UI/UX and engine board work is preserved below.

## Swift offline workflow passed; thumbnail implementation in native testing — 2026-09-28 18:38 UTC

Feature checkpoint `4b5f9c70` passes the actual Swift cached Library → proxy IOSurface edit/save → offline reopen → reconnect/synchronize workflow (run13). Original photo and hidden sidecar hashes are unchanged. This creates new Engine objects, not a separate-process restart or a packaged-app GUI acceptance. Root verified 7,014 immutable Git inputs and four generated/archive artifacts for runs13/15/16. Portable evidence is committed in `d6ac99fc`, under `swift-4b5f9c70`.

The desktop Auto Edit regression also passes (run10, two tests). Document transform tests pass (run15, 19 tests), and strict optimized app product compilation passes (run16). Main now integrates B's bounded Document warning/shortcut fixes and synchronous checkbox wrapper through `4c07e5eb`; all five affected product/test files match the tested feature checkpoint. Completed receipts were published for B results `874d59b4` and `87e9de81`. The initial optimized Swift IR crash09 and hash-map alias oracle failures11/12 are retained. The final relative-name test helper fixes only the oracle; no source hash checks were weakened.

The separate offline thumbnail test14 remains FAILED after waiting60 seconds. A codec worker now owns the sole compiler lane for the source-reviewed native proxy thumbnail implementation and 13 authored tests. B thumbnail source `51e0cfb6` has clean independent review, seven tests still UNRUN, and awaits the native binding. B accepted follow-up `dfb80e63` for bounded retry: ordinary scrolling can cancel Swift flights while old native jobs still occupy the eight-job limit. No false pending response or Original fallback is permitted. B's existing writer and branch ownership remain preserved.

In parallel, the FFI worker prepares only a temporary source harness for future actual Engine/IOSurface GPU qualification. No GPU candidate has been applied or run; Original remains default. Full current Swift suite, thumbnail runtime, visible GUI and acceleration acceptance remain open. Main merges remain A-only; other UI/UX and engine work is preserved.

## Native batch repair integrated; desktop qualification continues — 2026-09-28 18:13 UTC

Main8c962911 integrates bounded native repair1a958076; evidence65317e03 preserves all attempts22–29. Final focused3, related index/agent/Console161 (3 ignored), FFI175, strict all-target Clippy and actual Sony offline/edit/reconnect/full original JPEG workflow passed. Root verified19 scoped source hashes per final run against immutable Git candidate and before/after equality. This repairs batch handling without weakening generic Original write guards; no full Swift or feature acceptance is implied.

FFI worker retains sole compiler lane to regenerate matching archive after B Document strict source cherries and the independently reviewed actual Swift/native workflow test. Its recipe-history setup and asynchronous thumbnail polling review findings are fixed in the source candidate; runtime remains pending. Native thumbnail candidate is source-only under review, with explicit local-only loading, bounded scheduling and stale identity checks. Symlink and transient failure behavior are being reviewed before application. GPU proposal remains unrun/unapplied.

B request9269efdc has an actual accepted Git receipt and matching peer commentary; queue01a0e92f was processed, not merely accepted. B implements cached-library thumbnail source routing and invalidation on a separate branch from published A b6cea8f2. B Document/SaveAs ownership and source-only workload hold remain. A alone owns main merges; unfinished UI/UX and engine queue remains preserved.

## Offline thumbnail gap and active regression qualification — 2026-09-28 18:04 UTC

A continues Smart Preview acceptance in the feature checkout. Mixed Auto Edit regression now passes the three focused native tests in run25: two valid plus one missing target for independent/coherent batches, including a reappeared invalid sidecar, and dirty/active editor no-write guards. Earlier runs22/23/24 remain failed evidence. The fix limits both catalog resync and Agent perception to the admitted file; broader index/agent/Console and FFI suites are running with the sole compiler lane. No native repair merge or full desktop acceptance yet.

Source inspection found a separate offline thumbnail gap: ordinary embeddedPreview stats the missing original before cache lookup, so warming the cache does not solve disconnected Library thumbnails. A worker prepares an explicit validated local proxy thumbnail API and an actual Swift/native offline workflow test; these are source-only and unrun. The native API is smartPreviewThumbnail(imageId:maxPx:), preserving pending/PreviewReady semantics and rejecting invalid local assets without Original fallback. Local-only validation must avoid the current Develop loader's Original freshness I/O. Independent review found an invalid recipe-history setup in the draft workflow test; correction is required before running it.

B owns the bounded Swift source-role, thumbnail routing, invalidation and disclosure follow-up: request9269efdc-91e7-4c49-bb4c-8e2e1db89bae published through Git, SSH queue01a0e92f-3606-77d1-881a-3bd32a531068 accepted. Peer receipt is not yet confirmed. B Document strict source186559ce has clean source review and remains queued for A integration/testing; previous request processing is confirmed by B result874d59b4. Original remains default. GPU proposal remains unrun/unapplied. Other UI/UX and engine board work is preserved; A alone merges main.

## Full desktop gate failed; batch regression under repair — 2026-09-28 17:45 UTC

The first full Swift run at feature567f5e92 plus generated bindings/import fix exited1:672 XCTest cases,1 skipped,10 assertion failures across3 tests;5 Swift Testing cases passed. Do not treat this as accepted full-app integration. Native archive/binding regeneration passed270.5s and generated delta was reviewed.

Confirmed product regression in native OriginalWriteReservation integration: Auto Edit with one missing photo aborts whole batch before per-image results. Existing AgentReviewLayout expects two successes plus one retained failure; queue is incorrectly empty. A FFI author prepares a partition-safe guard fix, reserving all IDs/checking dirty state while retaining missing entries as non-writing failures and preserving batch indices. Independent reviewer confirms cause; this blocks full product acceptance despite earlier bounded native passes.

Other failures: OfflineLibraryRouting test expected Foundation-normalized /var instead of native canonical /private/var (7 assertions); A uses independent POSIX realpath only while online for expected fixture, preserving product canonical authority. WorkspaceReadyPhoto expected prefeature label without Original suffix (1 assertion); A updates expectedlabel while retaining actualRAW readiness/layout/mask checks. Missing FolderHandle import compilation failure02 retained and repaired. After focused28 verification/checkpoint, native guard repair owns next compilerlane; final full/strict deferred until repairs integrated.

B has new source requesta4ac739c for preexisting Document strict diagnostics, SSH queue01a0e91a-11cb-7671-ab32-464ff07da425 accepted; clarified checkbox Binding setter (not slider) in statusde6dcb0f/queue01a0e91c-e7b9-7d61-ad99-d514ca9520a9, peer receipt pending. No warning suppression or weakened test thresholds. GPU consolidated proposal remains source-approved but unrun/unapplied. Main merges A-only, older work preserved.


## Compact builds accepted; full desktop integration running — 2026-09-28 17:31 UTC

Main integrates codec8b654835 and native builder8c5fd747. New previews use Compact2048; existing Detail/v1 assets remain readable with no automatic migration/discard, and Original remains default editing source. Sony Compact1640×1092 is7,319,246 bytes versus16,646,144 original (56.03% smaller). Codec error bounds passed, actual preserved v1 asset retained exact historical edited hash and exact upgraded samples. Full CPU+image-core257 tests/5 ignored passed; native builder172 unit tests plus real Compact offline/reconnect/conflict/full4920×3276JPEG workflow passed, strict/fmt0. Root independently verified6938 Git input hashes for each candidate's final gates. Evidence compact-8b654835 and ffi-compact-8c5fd747. No matched-output or interactive speed claim.

B canonical-path correction775f39b0 received, result8b47428c accepted, clean independent source review. Requestc13b4fe0 peer receipt verified, queue01a0e903 processed; no duplicate dispatch. Older alias-only history already disconnected requires online reopen once. B UI775 integrated only on feature codex/smart-previews for matching FFI generation and full Swift gates;27 source tests still await full-package execution (earlier exact2ba isolated20 pass separately). Codec agent owns sole compilerlane. GPU/coarse/FFI proposals source-reviewed but UNRUN/unapplied; never call them accelerated product behavior. Protected GUI permission blocker remains; no app lifecycle action or bypass. Other queue preserved.


## Offline Library native acceptance — 2026-09-28 17:16 UTC

Integrated94557873/35e378b4 catalog-backed offline Library API. Full cull+FFI Release gate444 passed/15 ignored at35e; only real-workflow test changed in945 to use canonical catalog folder captured online. Corrected actualSony workflow passed plus strict/fmt at945. Root verified6938 Git file hashes for fullgate and finalruns08–10, production identical across test correction; original fixture unchanged. Evidence offline-library-94557873 preserves failed canonical alias tests06/07 and initial lint failure03.

Isolated exact B2ba1d95c controller/model20 tests passed onSwift6 Release (13.19s build+test), sourcefreeze unchanged. This excludes AppModel/nativebinding/fullapp and six new offline-routing tests in B ef307d8d. B ef307d8d result42a031cd accepted for A review. Review confirmed online alias retained instead of canonical index handle.path; new requestc13b4fe0 published, SSH queue01a0e903-d122-7fd2-b86d-5ea06fadf13f accepted, peer receipt pending. Do not claim offline app reopening accepted until that correction and integrated gates pass.

Next compiler lane: Compact tier component and actual size/legacy-v1/fidelity checks, Original/Detail defaults unchanged pending results. GPU proposals remain unapplied; adaptive coarse GPU interaction still needs qualification. Protected GUI blocker and other unfinished board work preserved. Latest status publication does not prove peer receipt.


## Native Smart Preview editing integrated — 2026-09-28 17:04 UTC

Native candidate e1eca7ba (including81a6cc53) is merged after independent review and exact fifteen-file Git hash verification for final runs18–21. Final171 FFI unit tests passed, real Sony workflow passed, strict all-target Clippy and formatting passed. Workflow verifies clean and dirty offline restarts, exact captured recipe/history/unknown fields, local save, reconnect synchronization, conflict retention, byte-identical Original copy and4920×3276 edited JPEG export. Source fixture unchanged. Portable evidence: tools/orchestrate/wp/SMART-PREVIEWS/evidence/2026-09-28/ffi-e1eca7ba. Earlier64 related integration passes belong to the earlier source checkpoint; all failed attempts remain preserved.

This accepts the native Engine path, not the full app. Offline Library patch now has clean source review after fixing fresh-session people filtering; codec agent owns the sole A compiler lane for its native tests. B accepted request889b3f34 and is implementing the cached Library Swift path in its existing writer. B UI2ba1d95c refresh/autosave corrections have clean source review;20 tests remain UNRUN at that head. A native bindings, current Swift tests and GUI acceptance remain outstanding. GPU and Compact patches remain source-only. Original stays default: measured CPU proxy edits are slower than original Metal. Other engine/UI tasks and protected GUI blocker remain preserved.


## Public offline engine workflow verified; app reopening remains open — 2026-09-28 16:46 UTC

Feature worktree only, NOT main integrated: focused native22/22 passed, realSony public Engine build/render/offline two-restart save/reconnect/sync/conflict test passed. Positive rendered JPEG after sync is4920x3276 and brighter/changed versus baseline; separate Original copy is byte-identical. Source fixture unchanged. Per-run input hashes and direct exits retained in /Volumes/betterSSD/tessera-validation/smart-previews/ffi. Initial compile/harness failures preserved. FullFFI unit run07 is165pass/6fail from existing expected Original-conflict wording; compatibility wording restored and08retry underway. No fullgate acceptance yet; scoped applied-source review active.

Root discovered app-level offline relaunch gap: EngineLibrary.scan requires originalfolder/indexing and cull admission skipsmissingfiles. DirectEngine workflow does not waive this. A source worker now prepares explicit catalog-backed validated-preview session; B must receive exactcontract before implementingSwiftreopen. GPU L0 and Compact tier patches are source-only/unmerged. Originaldefault remains because proxyCPU warmedit384.7ms versus originalMetal33.07ms.

B UI54502c3d results484a7979/7b88e668 acceptedtarget, tests15UNRUN atthathead. Earlier exactc6d05aa3 isolated8controller tests passed. Autosavewarning requestbcf6cb50 queued01a0e8e0, same-photo refresh/open race request06afa311 queued01a0e8e8; peerreceipt pending forlatest. Existing writer preserved, no duplicate sessions. Next: fullnative/review, offlineLibraryAPI, coherentbindings+BUI and actualendtoendapp acceptance.

## Current gate — 2026-09-28 16:33 UTC

Exact Bc6d05aa3 controller +8 model tests passed in isolated Swift6 Release package, sourcefreeze identical; evidence swift-model-c6d05aa3. This excludes livebindings/AppModel/app/GUI and laterBfollowups. NativeFFI corrected patch now applied only in featureworktree; sole compilerlane assigned to FFIagent. GPUproxy L0 patch preparation is source-only, not enabled or benchmarked. Originaldefault decision unchanged.

## Measured Smart Preview default decision — 2026-09-28 16:32 UTC

Matched Sony2460x1638 encoded-display test at6c057641,2 fresh renderers per route/18 rows: originalCPU cold1101/warm794.5/edited818.2ms; proxyCPU385.8/380.9/384.7ms; originalMetal resident tiles including readback332.6/10.05/33.07ms. Proxy improves CPU route but is11.6x slower than originalMetal for warm edits. Keep Original default; explicit/offline Smart Preview choice remains. No appzero-copy or input-to-present claim. Root checked raw rows, source/harness freeze, direct0 and unchanged original. Evidence: matched-sony-6c057641. Initial wrong resident entry-point failure retained.

B delivered sourcec6d05aa3; exact-target resultb7e144bc accepted pendingnative/bindings/tests. Follow-upcd101ed1 accepted (status coalescing/stale thumbnails). New product-decision requestbd498fd4 published and SSHqueue01a0e8db accepted; peer receipt still pending. Compact2048 source patch prepared but UNRUN/unmerged. FFI has four reviewed corrections in preparation, then actual offline Engine workflow test. No fullfeature acceptance.

## Active Smart Preview tasks — 2026-09-28 16:30 UTC

| Task | Owner | Verified state | Next action |
| --- | --- | --- | --- |
| Persistent codec + journal | A | Integrated main2db82c7f; evidence46469b08. Codec159pass/2ignored; final journal13pass; earlier fullFFI156 separately scoped. | Retain original identity and exact recipe safety through callers. |
| Image-core render routing | A | Integrated merge before6c20bed5, exact6c057641. Full94pass/2ignored, focused7pass, strict/fmt0;6935 frozen Git blobs independently verified; review clean. | Measure matched-output original/proxy CPU and explicit original Metal route. |
| Offline editor + reconnect | A FFI implementer/reviewer | Source patch NOT integrated. Review found four issues: XMP directory durability, post-ack intent cleanup, unknown nested array preservation, remaining writer admission. | Fix and rereview, then actual offline-session/reconnect/export tests. |
| Compact proxy tier | A independent source worker | Source preparation only. Existing2560 tier Sony proxy16.63MB≈original16.65MB; no useful saving. | Explicit2048 tier, compatible version validation; measure size/fidelity, no silent precision change. |
| Library/Develop controls | B existing desktop writer | Original requestd7764a2f accepted; follow-upcd101ed1 queued01a0e8d5 and peer commentary confirms processing, receipt pending. Tests UNRUN. | B source handoff; A native bindings/tests. Expensive status reads off UI thread; offline thumbnails visibly stale. |
| Complete product acceptance | A | NOT ACCEPTED. No interactive speedup, offline end-to-end or GUI acceptance. | Finish above, original hashes, full-quality original export and failure paths; resolve existing protected macOS dialog only through allowed user action. |


## Smart Preview codec and journal accepted — 2026-09-28 16:26 UTC

Main 2db82c7f integrates reviewed codec61e4f4f5 and journal/admission1fd63563. Codec full Release159 passed/2ignored, strict/fmt0; root verified6933 immutable source hashes. Journal final helper13 Release passed/strict/fmt0; earlier full156 FFI belongs3cff7441. Portable evidence is under tools/orchestrate/wp/SMART-PREVIEWS/evidence/2026-09-28/storage-1fd63563 and codec-61e4f4f5, with preserved failed attempts.

Real Sony fixture produced2460x1638 F16 camera-linear proxy,16632169 bytes versus16646144-byte original: only0.084% smaller. Generation529ms, encoding172ms, reopening45.5ms; custom WB/exposure renders from decoded proxy, max sample error0.00012207. Original SHA256 unchanged. Useful storage savings and comparative interactive speedup remain UNPROVEN. This is not a completed offline editing feature.

Image-core route6c057641 has initial focused6/6 Release passed; new codec reopen regression and final full gate pending, sole A compiler lane. Independent reviewer audits FFI integration while another worker prepares remaining writer guards. B Smart Preview UI requestd7764a2f remains accepted; actual peer commentary reports batch reservation and source switching work, tests UNRUN. Save As source results0a6d71e8/7d8f8c4c remain accepted/pending runtime, not duplicate tasks. Next: validated image-core, FFI offline save/reconnect/export gates, generated bindings and B UI integration.

## Smart Preview persistence and UI ownership — 2026-09-28 16:15 UTC

Export/Adobe compile repair is integrated mainfa7372b9, exact088cc261;11 export tests passed/1 existing ignored,33 Adobe tests passed, strict/fmt/direct0. Root independently rehashed1102 inputs; clean review and23 portable evidence payloads preserved. This resolves the three exhaustive-match failure; original failure remains retained.

Journal/admission candidate3cff7441 passed13 store +6 admission tests and full156 FFI library Release tests. Review then required retry-safe directory publication and first-create symlink support: correction1fd63563 passed13 focused Release tests +strict/fmt. The156 full-suite result belongs to3cff7441, not a repeat on1fd. Final scoped review pending; no offline-editor acceptance yet. Codec now owns sole compiler lane for bounded compressed persistent camera-linear format and existing Sony ARW fixture validation; image-core route patch is source-reviewed but still UNRUN. FFI offline integration patch is being prepared separately; no active compiler inputs changed by that preparation.

B accepted new Swift Smart Preview UI requestd7764a2f at GitB7f0bc4c. SSH queue01a0e8c7-a96e-7813-8927-f9d78c4cefb1 reached the existing writer, verified by peer commentary and accepted Git receipt. B owns bounded Library/Develop Swift source on codex/smart-preview-ui; A retains Rust/tests/main. Frozen methods: build_smart_preview, smart_preview_info, discard_smart_preview, synchronize_smart_preview, open_smart_preview_develop_session. B Save As3ca3e9c9 remains preserved/pending A runtime gates. No duplicate queue or writer reset. No full-quality proxy export, speedup or complete-feature claim.

## Export integration gate failed — 2026-09-28

The first broader FFI compile after the accepted CPU-only gate failed: three export exhaustive RenderSource matches did not handle CameraLinear. CPU component tests remain147pass/1ignored, but this is NOT a passing application integration. Failure preserved at /Volumes/betterSSD/tessera-validation/smart-previews/storage/smart-preview-store.log; no concurrent tracked-source change. Original engine implementer owns immediate export admission repair and explicit proxy/full-quality-export refusal tests, plus Adobe-route refusal. Storage tests paused to serialize compilation; codec and image-core work are source-only preparations. Do not treat main's CPU component merge as build-ready application acceptance until repair is validated.

## Smart Preview CPU component accepted — 2026-09-28 15:55 UTC

Integrated in-memory component 2ab94fd9, exact product a3ab5034/f06ddb2b. Final Release pipeline-cpu gate:147 passed,0 failed,1 existing ignored; strict all-target Clippy and targeted formatting exit0. Root rehashed436 frozen inputs; independent Astra review clean. Portable evidence: tools/orchestrate/wp/SMART-PREVIEWS/evidence/2026-09-28/engine-a3ab5034 (37 payloads plus manifest). Missing-API, HDR-fixture and lint failures retained. This is NOT complete editable Smart Previews: codec, image-core route, offline journal/admission integration, Library UI and end-to-end acceptance remain.

Luna now has the sole compiler lane for local journal+stable ImageId admission tests; initial source review found missing constructor initialization and directory-sync gaps, being fixed before acceptance. Astra prepares bounded compressed camera-linear codec source; separate Astra prepares image-core routing patch without changing active compiler inputs. B returned test-only3ca3e9c9 for request5cfbf6a4; result7d8f8c4c accepted after target validation, tests still UNRUN. Queue01a0e8b2-65a8-7d22-99a4-734c0c98a167 processing is confirmed by native completed peer turn and Git result. No duplicate dispatch. A alone merges main.

## Smart Preview implementation and B handoff — 2026-09-28 15:43 UTC

User explicitly authorized implementation. Clean managed checkout export-integration is reused as codex/smart-previews from main7f98ab79. Astra implements the camera-linear CPU boundary with sole compiler lane; Luna independently prepares local journal storage source without builds; a second Astra reviews B Save As source. Plan: docs/superpowers/plans/2026-09-28-smart-previews.md. End-to-end Smart Previews are NOT implemented/accepted yet. Original photos remain untouched; existing Sony ARW fixture suffices to start, optional camera/sample-folder question is not blocking.

B source result0a6d71e8 and status426c3a04 target this chat UUID and have accepted receipts. origin/codex/save-destination-swift0f820dff contains tests7b7a5dcb/8976f2fe and product3d4b34f4 on81cc08eb; all B runtime UNRUN. Takeover now92a7cb4e. B confirms queue01a0e8a7-e244-72d0-8145-4e331243a383 reached its existing writer and that the tests-only checkpoint had already advanced. This supersedes the earlier interruption/usage blocker; no duplicate resume. Source result remains pending A review/tests and main merge. B retains Document ownership.

## Peer interruption reconciled and proxy source boundary checked — 2026-09-28 15:30 UTC

Fetch/takeover unchangedcd8445349; mailboxBcbad4852 still has accepted request8638c362, with no result/new incoming or in-progress A work. Native compact snapshot now exposes B's actual latest turn01a0e898-9700-7622-b781-08019f6c758d: failed with usage-limit error after source work. This conclusion comes from the turn error and saved commands, not its separate notLoaded host status. A single recent-turn read confirms B created codex/save-destination-swift at81cc08eb and committed tests-only7b7a5dcb in /Users/rutmehta/Developer/lightroom/.worktrees/B5-16. Three test files,257 insertions/13 deletions; explicitly UNRUN. No completed product or published handoff is claimed. Preserve B's branch/reservation and accepted receipt; do not ACK, resend or launch a duplicate writer.

Root advanced Smart Preview source analysis without unavailable agents: existing DNG export renders the recipe, LinearRaw reopening selects RGB, and Develop updates recipe.source_kind from that route. This is not a safe automatic replacement for an original RAW editing source. SMART-PREVIEW-STATUS.md now records the pre-edit generation, original recipe identity, route/control fidelity and export gating requirements. No proxy implementation or runtime. A's next independent reviews remain blocked by observed usage limits; completed descriptor source/evidence unchanged. Heavy lane idle; no app/process changes.


## Follow-on agents limited; B accepted destination work — 2026-09-28 15:25 UTC

Descriptor implementation is complete and pushed mainbed7ff3f, with98 passing engine-api tests/strict/fmt and independent reviews. Completed-status Git message2c518614-f222-4e20-84ca-fdcb58ef25f1 is published; no receipt of that status is claimed.

The next private-capture source-plan agent and Smart Preview feasibility agent both failed with an explicit Codex usage-limit response (reported retry Oct4,2026 3:54PM; timezone unspecified). Neither requested report exists. They are NOT running and no next implementation is accepted. Preserve completed work/evidence; resume these small source tasks once an agent can run, without blindly launching duplicate workers. Root completed only the Smart Preview source inventory in SMART-PREVIEW-STATUS.md: editable lightweight/offline proxies remain NOT IMPLEMENTED; JPEG display caches and reduced-level rendering are separate.

New mailbox headBcbad4852 contains accepted receipt for existing destination request8638c362-1aef-4fda-9117-c5af79e50e92. B explicitly reports beginning bounded source-only Swift implementation on81cc08eb, tests-first/product separate, no builds/tests/apps/heartbeat. This is verified peer handling via Git, not proof of a new SSH queue delivery or completed product. No receipt ACK or duplicate request was sent. Takeover branch remainscd8445349; incoming messages/in_progress/invalid/expired are empty. A retains sole main merges; B source ownership and workload hold remain.


## Pinned descriptor accepted; Smart Preview status clarified — 2026-09-28 15:21 UTC

A integrated exact source083018a9 after task review and fresh whole-branch Astra review cleared it. Full engine-api Release passed98 named tests (63unit+35integration, including20 descriptor tests),0fail/ignored,0doc tests. Strict Clippy and targeted formatting direct0. Root verified6809 tracked inputs and all five changed Git blobs; final aggregateabbedcb6e4b022a3b4d2b173bfb3772ddf69ed7f63377b296a56822b4bc38be2. Root verified114 portable payloads/219806bytes at tools/orchestrate/wp/UX-05/evidence/2026-09-28/pinned-raw-descriptor-083018a9/. Earlier failures and incomplete draft provenance remain explicit. Final merged crates/Cargo/.cargo content is byte-identical to tested candidate; intervening main changes were coordination/plan docs only.

Descriptor-only scope is DONE: private immutable declaration, exact recipe bytes, current-settings hash, duplicate/unknown/overflow rejection, locator-independent input identity, additive contract1.7.0. It does not verify actual asset bytes or enable source capture, decoder/render admission, a graph node, FFI/UI or Smart Previews. Current settings are authoritative and history is opaque; future writable consumers need separate history validation.

User asked about lightweight RAW editing copies. Source audit confirms Smart Previews are specified but NOT implemented: docs/05-catalog-storage-and-import.md section2.2 proposes2560px lossy DNG; crates/previews implements JPEG display pyramids, and develop::open_develop_session still opens indexed original via RawImage::open. No Smart Preview generation/offline-editor fallback was found in source or fetched branch history. Add this as an explicit unfinished product task, distinct from cached display images and reduced-level rendering. It is not delivered by the descriptor.

Luna is source-planning the next bounded private-capture primitive only; no implementation/runtime claimed. A remains sole main merger. B is unreachable by current SSH hostname, latest native snapshot unavailable, no new mailbox receipt. Preserve B destination request/reservations and source-only hold. Protected desktop dialog remains pending; no app/capture action.


## Descriptor focused gate and review — 2026-09-28 15:13 UTC

Candidate12b354fd passes20 focused tests/direct0. Astra reviewed the exact correction and cleared five findings plus three maintainability notes. Root independently rehashed6809 tracked files and all five changed Git blobs; before/after aggregate33d6a78cb2625b47b81151d013cabfef52345bffd642f451555207cbc2af25b8 is identical. Earlier draft compile/behavioral failures remain preserved; their initial untracked-source provenance was incomplete and they are not final gates.

Luna now owns the sole compiler lane for full engine-api Release tests, formatting and strict Clippy. These broader gates and whole-branch review remain pending. Additive public contract version is1.7.0 per crate policy; existing recipe schema, process semantics and golden hashes remain unchanged. No asset verification, source capture, actual decoder/render, app or B-owned edits. B communication remains publication-only while hostname resolution fails; existing request/reservations remain preserved.


## Pinned RAW descriptor implementation — 2026-09-28 14:55 UTC

A is implementing the descriptor-only engine-api boundary on codex/pinned-raw-descriptor from main5f33e174. Luna owns implementation and the sole narrow compiler lane; Astra independently reviews parsing/default/identity edges. No GUI/capture/decoder/FFI or B-owned edits. Plan: docs/superpowers/plans/2026-09-28-pinned-raw-descriptor.md.

The chosen contract treats current settings as the pinned snapshot and preserves history as opaque exact bytes; it does not validate writable history or return a writable Recipe. Recursive duplicate decoded keys and unknown current-settings members reject. Requested-input identity includes exact payload bytes, asset declaration, owner, route and suffix, but excludes locator hints. Descriptor success is neither asset verification nor render admission. All new implementation gates are pending.

Fetch/read of B takeover is unchanged; mailbox has no new messages, invalid/expired entries or in-progress work. Native peer snapshot is unavailable, so no peer progress/receipt/wakeup is inferred. Existing destination request remains reserved without duplicate queue dispatch. Protected-dialog question remains pending; no app action. A alone merges main.


## Current execution queue — 2026-09-28 14:43 UTC

| Task | Owner | Verified state | Next action |
| --- | --- | --- | --- |
| Editable Smart Previews | A engine design; B future editor UI | NOT IMPLEMENTED. JPEG caches and reduced-level rendering exist; editor still opens original. | Define/gate editable proxy format, build/discard storage, source/recipe linkage, offline editing, original reconnect/export and visible source status. No proxy feature acceptance. |
| Photo save recovery / saved-pixel admission | A | DONE bounded slice main2760ebeb; full599/1skip/0 +5 and actual recovery/reopen GUI | Global Quit/all-writer durability remains separate. |
| Recovery status ownership | A / Luna | DONE main291857e1; genuine RED then19 pass/direct0 | Preserve newer messages during retry. |
| Staged filters and person facets | A / Luna | DONE mainf27f96e4, evidenceb6608ee9; genuine6-failure RED,9 focused +44 adjacent/1skip/0 | Integrated source exactly matchesdd6;32 evidence payloads verified. No new GUI/full-suite claim. |
| PSD copy format preflight | A + B source | DONE main08e86f39;51 selected Rust tests +strict | No memory-budget/RSS claim. |
| Develop stale-writer protection | A / Resource Sol | DONE mainf4bc0c0b, Rust/Cargo exactly915c7f7b;25 tests +fmt/strict | Newer/unsupported owner fields fail closed;185 payloads and40 source hashes verified. No lease, all-writer CAS or conflict-resolution UI. |
| Recipe setter XMP parity | A / reviewer Sol | DONE main4ad017ab, exact Rust/Cargo3bd2e341. Genuine RED then19 combined tests +fmt/strict/direct0;15 frozen inputs verified. | Evidence535950bb/FFI737dcd09 published; current arm64 archive4a45, generated bindings unchanged. Earlier compile failure retained; normal preview unchanged. |
| Save As native ownership | B source; A integration/GUI | DONE main `11b31be6`. Product d695 passed 90 focused, full 642 XCTest/1 skip/0 failures +5 Swift Testing; actual isolated create/Replace/cancel/repeat/reopen passed. Test-only a927 passed actual parent close during native folder chooser. | Evidence cc9eb6f4, ede27821 and integration parent-modal manifest; historical RED and failed probes retained. Result d9b94a94 completed. No full 643 rerun, leak-proof or preview-refresh claim. |
| Save As destination safety | A native; B Swift source reserved | Native 75a1b9a6 passed 11 selected tests + fmt/strict; test-only 2a22145b pins actual directory conflict and passes focused/fmt/strict. Generated checkpoint 81cc08eb built arm64 archive f451870f, input hashes verified. | B request8638c362 now has accepted Git receipt at Bcbad4852: source-only Swift work on81cc08eb begun. Earlier SSH attempt failed/noqueueID; do not resend accepted work. Await B source result, preserve ownership. No full product/UI acceptance. |
| ISO gain-map interoperability control | A / Luna experiment; Astra review | Phases1–9 preserved; split/A ImageIO~8 vs reference/CI~16 remains unresolved. Original acceptance4pass/1fail. | Phase10 attempt03 completed9 native processes/runner0: uniform target8 responds16→8, A/split remain8 even at target16. Root/Astra verified18 float payloads; evidencef5893c0f integrated main7cf19811 after218 payload verification. Original failure unchanged. Proposed boundary-shift control rejected before execution: altered gain histogram and auxiliary encoder confound the intended inference. No new runtime selected; see EXP45-BOUNDARY-SHIFT-REJECTED.md. |
| Saved-recipe histogram without writer | A / Luna implementation; Astra/root review | DONE main22ff89b7 bounded prerequisite: exacta31d2518, two intended REDs, focused3 + adjacent7 tests, fmt/strict direct0. Evidence6be9d5d7 plus61-payload coordinator manifest. | Preserve no-writer/read-only path before any future lease. Stage C admission is validated separately; no UI/performance claim. |
| Stage C exclusive Develop admission | A / Luna implementation; Astra/root review | DONE mainf1f4abe2, bounded source59373066:137/137 FFI library tests, focused/recipe controls, fmt/strict pass. UI controls already main049bfe95,34 unique. Root verified472 evidence payloads and exact integrated source. | See DEVELOP-EXCLUSIVE-ADMISSION-REVIEW.md. Case aliases and external writers excluded; no rebuilt archive/GUI claim. |
| Recipe destination filename aliases | A design follow-up | Existing gap confirmed by disposable filesystem probe: case-variant sidecar names share inode but have different exact gate keys. No native fix/acceptance. | Design reviewed; retain exact-key scope for now. Persistent namespace reservation candidate and deterministic acceptance matrix are in DEVELOP-DESTINATION-ALIAS-DESIGN.md; see also DEVELOP-DESTINATION-ALIAS-LIMITATION.md. Scoped Stage C does not claim physical-file alias exclusivity. |
| Smart-filter mask thumbnail retention | B reserved Document FFI; A source audit | QUEUED contract follow-up: session map has no count/byte cap; native-to-Swift IOSurface ID lifetime must be preserved before eviction. No resource measurement or fix. | See resource-audit-20260927/MASK-THUMBNAIL-CACHE-FOLLOWUP.md. Wait for existing B reservation to complete/release; no duplicate workload or ownership takeover. |
| Develop presentation capability | A / Luna source readiness | Preserved wp/M2-58 at196005e; actual P01/P11 presentation acceptance remains unverified. Reconciled sourcec1c3b6cc passed139 FFI unit +3 integration tests, fmt/strict;6419 frozen inputs unchanged. Archive07d924df built/verified arm64; regenerated checkpointabe317c0. Broader three-crate Release passed579/0fail/29ignored; strict retry/fmt/workspace direct0,6419 inputs unchanged. Native evidence audited independently. | Tests-only adapter correctionf1d13c11 passed94focused and full661XCTest/1skip/0fail +5;6423 inputs unchanged. Originald53SIGSEGV/direct1 preserved. Signed exact-tested f1 package verified. Three visible attempts failed/direct1: external stdio creation/read denied, then /tmp relay launched67652 but startup stalled in mkdir with protected macOS dialog foreground; no trace or capability pass. Graceful termination requested but process remained alive. Await user handling of the exact permission dialog; no tool bypass or further runtime. Runner-only9423956c is preserved; Qualified interval source5e698e18 corrects two test-call argument orders after compile-only failures at9bf95af. Root read17 Python +27 focused Debug passes/direct0. Full Release now has durable child exit0:666 XCTest (665 passed,1skip,0fail) +5 Swift Testing. All6425 tracked inputs unchanged. Earlier two full runs have no verified child exit and remain non-additive; second wrapper failed on zsh read-only status. Portable evidence is main419c8e9b after root verified37 payloads/3,444,704bytes; no GUI or P01/P11 acceptance. Failed visible evidence is main75028319; compile-only capture helper evidence is mainfcfc64cc. Capture launcher is main3f78f2d9,7 stand-in tests/direct0 and13 evidence payloads verified; no real capture. Preserve normal preview. P11 requires evidence that detail appears; CALayer publication alone is insufficient. Read-only permission/clock preflight passed for its process only; no capture. Filtered capture helper remains source-only and cannot establish actual-visible P11 alone. |
| Live RAW / Layers continuity contract | A Luna source map + Astra engine audit; B future adapter review | Source review COMPLETE: rendered-copy behavior confirmed; pinned immutable snapshot supersedes automatic follow. Version preflight already DONE main372dbbcc/final26845eed. | Descriptor/capture proposal and independent review COMPLETE: see LIVE-RAW-PINNED-SOURCE-PROPOSAL.md and LIVE-RAW-CAPTURE-POLICY.md. Private captured-stream identity, raw schema/owner validation, stored decoder route and fail-closed geometry are specified. Synthetic CPU feasibility gate PASSED exactc3342cd3:1Release test/direct0;6791 inputs unchanged,1301 source blobs independently verified. Root verified13 evidence payloads/2,257,896bytes. Profile proposal is LIVE-RAW-CPU-PROFILE-PROPOSAL.md. Descriptor-only083018a9 now accepted with98 tests/strict/fmt; capture/admission, actual decoder/ICC/fingerprint and B adapter remain unimplemented/unaccepted; no app or real RAW run. |

Only A merges main. One heavy compiler/GPU/desktop lane on A; B remains source-only with its heartbeat paused. User preview57591 stays unchanged. No source-only result or failed fixture is a passing gate. Earlier sections below are retained history; this table supersedes their active-state wording.


## Parallel work and Save As design decision — 2026-09-28 07:05 UTC

- Luna owns sole compiler lane: staged-filter tests-only `eb15a6e0` on accepted status source291857e1, then narrow AppModel implementation if genuine RED. Resource explicitly released idle compiler while revising source; no overlapping build.
- Develop writer tests `546c19c1` reproduced stale flush returning success after newer settings (direct101); selection and legacy RGB controls passed. Initial fixtures78b failed for invalid history/Engine lifetime and are retained. Product28a hit compile error and source review found typed comparisons lose nested unknown JSON; unaccepted. Resource adds raw owner projection, fail-closed unknown handling and no-sidecar/selection equivalence; reviewer Sol checks source. No lease/batch Apply.
- Save As owned-presenter design9af7fb8b independently reviewed by root/Luna. Mandatory correction: distinct bridge binding generation so old teardown on the same NSWindow cannot invalidate the newer binding. Root checked Apple beginSheet/sheets contracts; no polling or parent-notification-as-identity shortcut. B implementation request `be627c0e-b85b-4f4f-8b07-88ee651038c1`, queue `01a0e6d4-d870-7190-bc68-d5b7331e6214` accepted; peer receipt pending at this publication. Source only; actual GUI/full acceptance remains required.
- Existing Save As candidate010617b8 retains five non-gap tests plus one explicit fully-unseen-lifetime RED, all UNRUN. No product merge or expected-failure waiver. Earlier actual Cancel/repeat failure remains evidence. Recovery-status fix291857e1 is already integrated, 19 tests/direct0 and23 payloads verified.

A alone merges main. B resource hold/paused heartbeat remains; user preview57591 is untouched.


## Save As failure isolated; parallel follow-up — 2026-09-28 06:49 UTC

- Recovery product is integrated on main `2760ebeb`; PSD preflight is integrated on `08e86f39` with 51 selected native tests and strict checks. These are completed integrations, not merely staged candidates.
- Refined diagnostic `e51ef7fd` passed 30 settlement + 3 probe tests/direct0 but reproduced actual Cancel → one repeat Save As failure. Root verified all ten frozen inputs; evidence is main `1ce68330`, with a portable 15-file manifest added here. Native end clears the parent while an unobserved attachment and live probe retain the old claim. No Save As product acceptance.
- B acknowledged/completed request `82960a88-0725-46a1-b556-3dc1f97a2c8c` after SSH queue `01a0e6c0-a858-73c0-80ed-453330756015`. Result `7f8c4d02-c680-42ea-a7d9-bc8f7d90e6d1` target/expiry validated and accepted. Tests843b0416/product3d08368e are source-only: observe matching captured sheet attachment at SwiftUI dismissal, retain native-end requirement. Reviewer Sol prepares trace-free integration onto current main; Resource Sol independently reviews. No B workloads.
- Luna owns the sole compiler lane for recovery status: actual tests-only RED76fd1a0c, final7135ea13 and adjacent tests next. Staged-filter nine-test REDf0155326 remains queued. Save As compiler/GUI waits for lane handoff.
- Isolated diagnostic app20736 quit. User preview57591 untouched. B hold/paused heartbeat remains; A alone owns main merges. Queue acceptance, peer completion and final product acceptance remain distinct.

## Engine RES05a accepted for integration — 2026-09-28 06:39 UTC

- Source `4f6b9751` passed51 selected Rust runtime tests (estimator4, PSD21, adjacent9, FFI preflight4, lifecycle8, IO4, host1), three formatting checks and three strict Clippy gates/direct0. Genuine pre-fix native-alpha RED31de failed the intended1×1 sentinel assertion/direct101. Root verified13 GREEN exits and all available source hashes;129 portable payloads verified/copied. Four Rust files staged for main exactly match tested candidate.
- PSD copy now checks unavoidable format/layout failures before rasterization using the same immutable snapshot. Guaranteed alpha is included for native copies; imported opaqueRGB is not rejected solely from a possible alpha plane. The pure payload estimate is not a memory cap, peakRSS bound, process permit, benchmark, or resource-incident closure. No GUI acceptance of this engine patch is claimed.
- Recovery product is already main `2760ebeb` with full599+5 and real recovery GUI evidence. Follow-up status fix source `7135ea13` (tests76fd/7135, product30c54) awaits serial RED/green; source review confirms it preserves a newer status published while retry is held. Staged-filter tests-only `f0155326` remains queued.
- Reviewer Sol owns sole heavy/desktop lane for refined Save As trace `e51ef7fd` now. It excludes new observable reads from instrumentation; original untraced failure remains unaccepted. B FFI result4abfa0c9 is ready for completion after this main integration; trace resulta0e0d785 remains accepted/pending. B hold/paused heartbeat and normal preview57591 unchanged. A alone merges main.

## Recovery accepted for integration — 2026-09-28 06:25 UTC

- Exact candidate `634b7e68` passed the behavioral preview RED/green, 68 adjacent tests, full599 XCTest/1skip/0fail plus5 Swift Testing, and isolated recovery GUI. Root verified unchanged source/FFI and signed package hash. Product Sources/Tests staged for main are byte-identical to this tested candidate; no other product paths differ.
- Real generated64×48 JPEG save obstruction retained the +1.19 editor, Retry Save/Keep Editing controls and window-close veto. Keep Editing canceled prior navigation; a fresh navigation stayed blocked. After moving only the fault directory, explicit Retry saved valid JSON1.19 and navigated to Library. Reopen showed +1.19; JPEG hash unchanged. Export settings preserved active editor; Layers installed a64×48 16-bit single-layer copy. Disposable app quit normally; user preview57591 untouched.
- Follow-up UX issue: previous blocked-navigation status text lingers after successful Retry. Owner A, ready for a guarded status-publication fix/test; core banner/navigation/persistence passed. Global multi-window Quit, crash/all-writer durability, actual export-start GUI, and B Save As remain outside this acceptance.
- Source product integration excludes unaccepted typed Save As and diagnostic traces. Recovery includes only tested Layers completion/status/activation APIs. Staged-filter tests-only branch `codex/staged-filter-recovery-red` at `f0155326` remains UNRUN; no product filter changes included.
- Resource Sol now owns sole heavy lane for RES05a Rust RED/green/adjacent/strict gates. Combined source `4f6b9751` contains reviewed A estimator and B pre-rasterization hook; still UNRUN until direct results. B result4abfa0c9 accepted and source reviewed. Refined Save As diagnostic `e51ef7fd` awaits next serial GUI slot. B source-only hold/paused heartbeat unchanged; A alone merges main.

## Validation advances — 2026-09-28 06:16 UTC

| Lane | Owner | Current evidence / next action |
| --- | --- | --- |
| Review/recovery | Luna sole heavy lane | Revised old-result regression now fails the intended missing replacement-preview outcome (one test/direct1). Fixed source `634b7e68` passes four Review/layout tests and combined 68 adjacent tests/direct0; root verified RED/green source and archive freezes. Full Release PASSED599 XCTest/1skip/0fail plus5 Swift Testing/direct0; root verified unchanged source/archive/head/status. Portable evidence main9e71b550,98 original payloads verified; relative manifest expanded to101 including verifier/manifests. Reviewer owns recovery GUI: controlled EISDIR retained editor/banner and vetoed close; explicit Retry/persistence still in progress. First non-discriminating pass preserved. |
| Save As diagnostic | Reviewer Sol; B source | Trace `cfdb8583` passes33/direct0 but actual Cancel/repeat **does not reproduce** untraced e8dd failure. Extra post-Cancel update/capture sets observed attachment before native end. Main `345d6e11` preserves15 verified payloads. No acceptance. B acknowledged refinement request35c7a99c; result a0e0d785 accepted. Removing new observable reads is frozen UNRUN as A `e51ef7fd`; next serial trace gate pending after recovery GUI. |
| Engine RES05a | Resource Sol; B narrow FFI hook | Pure estimator/preflight `142d1d6b` source-reviewed; four tiny test functions UNRUN. Reviewer caught native documents' mandatory alpha, now corrected with1x1 53-channel/sentinel test. No RSS cap/budget. B request `f32843f7-d4d5-4bb2-9889-c7b0f5095da5` asks pre-rasterization hook using same immutable snapshot; queue `01a0e6a8-0d20-7d40-808b-e4f6dc7ec488` and peer acceptance/completion verified. Result `4abfa0c9` accepted; B tests a5197dfb/product bcb1af3b source-reviewed and integrating onto A estimator branch. All Rust gates still UNRUN. |
| Staged filters | Resource preserved source; later A gate | Nine tests/source preserved on `codex/staged-filter-composition`; port only final tests/AppModel hunks onto accepted recovery. No divergent Document ancestry merge. |

B remains source-only with paused heartbeat. A alone merges main, one heavy/compiler/desktop lane, normal preview PID57591 untouched. Current main contains evidence/docs for these candidates, not unaccepted recovery/Save As/PSD product changes.

## Work in progress — 2026-09-28 06:07 UTC

- **Document diagnostic / reviewer Sol owns the sole heavy lane:** B acknowledged request `95510c13` and returned opt-in trace `cd967702`, result `93e4084d` accepted after target/expiry validation. A source-reviewed and integrated it as `cfdb8583`; focused compile/tests then isolated Cancel → one repeat GUI trace are running/planned. No lifecycle correction or product acceptance. Prior e8dd GUI failure remains.
- **Review regression / Luna source:** first old-behavior run passed unexpectedly (one test), so it is preserved as non-discriminating evidence, not a successful RED. Reservation count alone did not prove evaluation or replacement delivery. Luna is refining the observation and actual replacement-render oracle; Resource added read-only waiter observation `d5ae4ef9`. Full597 preview failure remains open; no timeout increase.
- **Engine RES05a / Resource Sol source:** prepare pure checked PSD modeled payload estimate and unavoidable format preflight, with tiny tests, on a separate main-based branch. No compiler while Document owns lane; no FFI hook yet, no guessed process memory budget/RSS guarantee. Pessimistic alpha/channel weight must not become false rejection of valid opaque RGB output. Preserve staged-filter branch/tests for subsequent integration.

A alone merges main. B remains source-only; its heartbeat and workloads stay held. Normal preview PID57591 remains untouched. Published Git request, SSH queue acceptance, and B accepted receipt are now separately verified for this diagnostic request.

## Active coordinator lanes — 2026-09-28 06:00 UTC

| Work | Owner | Verified state and next action |
| --- | --- | --- |
| Document Save As cancellation | B diagnostic source; A reviewer GUI | `88de50ea` failed compilation (SDK has no didBeginSheet notification). Narrow A repair `e8ddabfa` passed 43 focused tests, but actual first Save As Cancel made three subsequent enabled Save As commands show no sheet. FAILED; no product merge. Portable evidence on main `ee8b46f0`, 20 payload hashes independently verified. Next: opt-in event tracing before another correction. |
| Review preview recovery | Resource Sol source; Luna tests and sole heavy lane | Opt-in gate drain wait `7f5c4c49` reviewed without source blocker. Held-flight regression `e042bac6` is unrun; Luna adds cancelled-waiter and failed-save controls before RED/green, unchanged layout and full suite. Prior full597 missing-preview failure remains unaccepted. |
| Layers handoff | B API; A recovery integration | `5a82058d` passed 58+7 unique focused tests; prior full597 failed only Review preview. c4ba RED and scoped passes published main `da6c5220` (44 payloads verified). Overall recovery acceptance awaits current gates and real GUI. |
| Staged filter composition | Resource Sol separate source; A later gate | Nine tests remain unrun. Transplant tests then explicit AppModel hunks onto accepted recovery; do not merge divergent branch/DocumentWorkspace ancestry. Preserve existing navigation cancellation helper and current Document APIs. |

B source-only diagnostic request `95510c13-77dc-4af0-8be4-aa4ebe0fa1f6` published; SSH queue `01a0e699-2282-7bf1-875c-d6ab85b63909` accepted, peer receipt not yet verified. Terminal/choosing results `5018e46b` and `6b02b287` now have failed receipts with actual GUI evidence. B resource hold and paused heartbeat remain. A alone merges main; one heavy lane. Isolated Document app is closed; normal preview PID57591 untouched. No source-only work or failed acceptance is counted as passing.

## Current blockers and active lanes — 2026-09-28 05:51 UTC

| Lane | Owner | Verified outcome / next action |
| --- | --- | --- |
| Recovery navigation / Layers | Luna tests; Resource Sol preview fix | `5a82058d` passed 58 + 7 focused checks with unchanged inputs, but full Release FAILED: 597 XCTest/1 skip/1 Review preview failure, plus five Swift Testing passed. Sole failure is AgentReviewLayoutTests line141 missing preview after33.161s. Prior Layers cancellation/status failures are fixed in scoped gates; no overall acceptance or recovery GUI yet. |
| Review preview transient admission | Resource Sol product; Luna deterministic test | Concrete source path: new Review preview receives `.blocked([])` while cancelled prior view retains read gate through actual native flight drain, then never reloads on drain alone. Consistent with observed timeout; deterministic held-flight regression pending. No timeout waiver, early native release, or automatic failed-save retry. Source-only while Document owns heavy lane. |
| Save As terminal ownership | Reviewer Sol; B source author | Frozen trace-free `88de50ea` includes choosing-sheet drain plus terminal probe leases/dismissal/clear-parent fallback. Independent review found no blocker; 43-test gate running, actual cancellation/queued-successor/Replace GUI follows if green. Prior 2a94 scoped full569+5/GUI passes remain on main `096c805c`; new candidate UNRUN until direct gate results. |
| Layers activation API | B source complete; A tests | Explicit activateDocument default true from B `e38caf11`, tests `153d7653`, A caller false in `5a82058d`. Real c4ba four-case RED reproduced status clobber; corrected scoped tests passed. Result `c73b8d12` accepted pending completed validation/evidence. |
| Staged filters / facets | Resource Sol separate branch | Original seven-test RED `e05a609b`, product+tests `df9fbfd7`; newer Layers/filter cancellation RED `c15b8ce8`, final source `0d584a71`. Nine tests await serial gates; not included in recovery candidate. Real seeded person IDs and preserved cleanup required. |

Main contains evidence/docs only for this wave. Earlier expanded recovery evidence is on `de30e940` (27 payloads verified; packaging correction documented). B accepted/completed source requests through Git mailbox and existing SSH queue; terminal-probe result `5018e46b` and choosing result `6b02b287` remain accepted pending runtime gates. A alone merges main. User preview PID57591 untouched; B source-only hold and paused heartbeat unchanged.

## Validation and review update — 2026-09-28 05:26 UTC

- Document `2a94a187` passed 32 focused tests and full Release: 569 XCTest, one skip, zero unexpected failures; five Swift Testing passed, direct exit 0. Root read the raw full result. The real same/distinct Save As matrix passed Cancel, Replace, and persisted-content reopening. Diagnostic trace was absent; isolated app closed.
- **Product merge held:** B final source review `84425738` found that an attached choosing sheet cancelled/superseded before submission can release successor admission on SwiftUI dismissal without native detachment. A independently confirmed the source gap; this specific GUI failure is not reproduced. Review result `8f8457ae` completed. Observed-detachment result `0f1f76fc` completed for its bounded passes, not whole-product acceptance.
- B source-only correction request `9ddfd017-dfe5-40ef-b357-7a76f258dea8` published; existing-chat SSH queue `01a0e679-e574-72e3-bd8e-083ad0e09675` accepted, peer receipt pending. Prior GUI failures and new passes stay preserved. No B workloads or heartbeat restart.
- Recovery Luna froze `f24534ca` with corrected fixture expectations, five added behavior cases, stable target/reuse fixes, and a bounded native-run hold for deterministic same-path rebind. The expanded **56-case focused gate is running**; full Release follows only if green. The dense-index case is a synthetic defensive state, not an end-to-end catalog reorder. Reviewer releases the heavy lane to Luna and packages Document evidence. A alone merges main; preview PID57591 unchanged.

## Current coordinator checkpoint — 2026-09-28 05:19 UTC

- Document candidate `2a94a187` has no trace instrumentation. All 32 focused tests passed with direct exit 0; root independently read the five raw logs/results. Reviewer owns the sole desktop/build lane. Same-file native Replace now appears; Cancel preserved bytes and confirmed Replace reopened the edited layer. Reviewer also completed distinct-target Cancel/Replace/reopen: Cancel preserved the destination, Replace changed only that file, and reopening showed the edited layer. Isolated app is closed. Full Release suite remains pending; no product integration yet.
- B final source review requested as `82acf317-b1a6-463a-8c9d-7b0a55c8f996`; SSH existing-chat queue `01a0e673-b6aa-7140-9202-5694edcd2a81` accepted. B accepted receipt verified in the mailbox; source review is in progress. B remains source-only with its workload hold and paused heartbeat.
- Recovery `ead7b7b0` retains its failed 51-case run (three assertions). Luna corrects contract expectations, adds People/smart-album/catalog-revision coverage, and integrates stable-target `12f5ed69` only. Resource fixes a concrete dense-index editor reuse bug: controller reuse must match stable image identity and library owner after catalog reorder. These source changes are not yet validated.
- Earlier full recovery run remains FAILED (581 XCTest, one skip, 20 assertions across seven cases; five Swift Testing passed). New focused/full gates and isolated recovery GUI are required. No failure is waived. Main product remains unchanged in this wave; A alone owns merges. Normal preview PID57591 remains untouched.

## Active validation board — 2026-09-28 05:12 UTC

| Work | Owner | State and next action |
| --- | --- | --- |
| Develop navigation, preview drain, output cancellation, window close | Luna; Resource Sol owns product fixes | `44037d65` passed all 41 focused tests, direct0; root read raw result and matched source freeze. Real RED `f8fefdf1` reproduced stale Layers status. Typed Save As is excluded from this candidate, preserving main Save As behavior. Full Release FAILED: 581 XCTest / 1 skip / 20 assertions across 7 cases; 5 Swift Testing passed. Evidence on main `0bcbcd0d`; root verified 17 payload hashes after correcting verifier path base, plus three unchanged freezes. Pending-open status and Agent reservation/reopen product defects are fixed in UNRUN source; legacy tests must await actual asynchronous settlement. Expanded `ead7b7b0` gate ran 51 cases / 3 failures: one outdated immediate-nil assertion and two held-gesture assertions incorrectly requiring exit from Photo Edit. Prior failures are preserved; corrections must retain workspace/identity behavior. Stable-target fix `12f5ed69` awaits regression tests. No full acceptance. |
| Document Save As lifecycle | A root/reviewer; B source author | `358d19d3` passed 24 tests but FAILED real same/distinct-file Replace: “Another sheet is still attached to the document window.” Portable evidence `e3186b62` is on main `046501aa`, 13 payload hashes verified. Isolated app closed. |
| Document native-detachment correction | B source complete; A validation queued | Native parent/sheet identity handoff `dbe55346` + lifetime cleanup `d91363a2`, probe refresh `9663c0e8`, tests `772896a9` + `58053e60` integrated only into isolated candidate `b3a47443`. Source review found no remaining blocker; 29 targeted tests PASSED with unchanged inputs, but real GUI FAILED again: same/distinct existing-file Save dismissed without Replace; repeated Save As could remain queued until document close/reopen. Files unchanged. Isolated app closed, evidence packaging in progress. Both result receipts now failed. Portable evidence is on main `107ddef1`, 15 hashes verified. B completed diagnostic request `93eabb45` (queue `01a0e656-588d-7d22-aaca-f6b3eb3a99c9`); result `48fb9f26` accepted, trace-only candidate `23a1947e` passed 22 tests, direct0, unchanged inputs. Diagnostic reproduced the failure: at didEndSheet the parent attachment is nil but old child sheetParent still points to parent, so the guard rejects the event. Trace on main `d89b0fd6`; diagnostic result `48fb9f26` completed with evidence. B accepted correction `b422178d` through queue `01a0e667-6f0d-7971-8e0b-20255a920465`, returned product `fbff1c64`/tests `cda44278`; result `0f1f76fc` accepted for 32-test/GUI validation. Trace instrumentation will be removed before final product gate. |
| Export settings UX | Resource Sol source complete | `f67dbc53` removes the no-read sheet-opening reservation; actual Start/watermark and Print reads stay gated. Behavioral regression passed in the interrupted `cf4f4114` run; combined acceptance remains pending. |
| Atomic Library gestures | Resource Sol source; Luna tests; reviewer independent | Fix held-save intent replacement across double-click selection/Loupe, Compare, People reveal, Tether focus/reveal and smart-album source/filter. Atomic source `d38f9ed8` is frozen, UNRUN; Luna adds held-close regressions. Agent release-order fixes `b29e202e`/`af9aca49` and AppModel close/open publication `5af7bac8`/`738422ad` are UNRUN and queued together. |
| Staged filter composition | A follow-up | Confirmed lower-exposure edge: multiple facet/filter updates while a retained recovery blocks Library navigation build from committed values and can lose earlier pending edits. Normal FilterBar is hidden in Develop. Requires staged-draft contract and deterministic multi-control test; not covered by atomic gestures or current acceptance. |
| Recovery GUI | Reviewer Sol | Bounded plan ready: generated tiny JPEG, reversible test-owned `.edits` obstruction, real failed-save banner, Retry/navigation and relaunch persistence. No recovery app launched; wait for focused gate and explicit desktop allocation. |

- Layers status bypass: DocumentWorkspace publishes status before A's typed callback. B accepted source-only request `ffea007b` through SSH queue `01a0e651-dd9c-7a83-ac8c-d0130ea8b985`; accepted peer receipt verified. B returned status API `8488a2fb` with five tests `1904bb29`; result `1bdbd38a` accepted for validation. Resource reviewed/integrated it and A opt-in `baf89e88`. Luna now owns compiler for real RED on `f8fefdf1`, then combined 41-case gate. Unaccepted typed Save As will be isolated out of the recovery product slice before full validation.

- A main `046501aa` publishes evidence/docs only for this wave; no AppModel or
  Document product merge. Original failed compile/GUI attempts remain preserved.
- B accepted/completed native-detachment request `46a76741` (SSH queue
  `01a0e63c-d85d-7be2-b995-e8d6e108512d`) and probe request `6c21ad7c`
  (queue `01a0e642-824a-7211-877e-3593ec91cd51`). Results `e67f3f26` and
  `f20d5d88` are accepted for A validation, not runtime acceptance. Earlier
  results `0802848a` and `0d9f2b07` retain failed receipts.
- Previous 13-case recovery evidence is on main `9cc2a0ea`; root verified all
  seven payload hashes plus exact frozen source. It does not cover newer changes.
- One compiler/GPU/desktop lane at a time. Normal preview PID57591 remains
  untouched. B workload hold/paused heartbeat continues. A alone merges main.
  Full AppModel, global Quit, cross-process writes and large-library acceptance
  remain outstanding; no missing or failed gate is counted as passing.

## Recovery tests passed; Document replacement remains blocked — 2026-09-28 04:06 UTC

- **Verified focused recovery gate:** exact `338e2878`, product `c4a86c05`,
  13 XCTest cases / zero failures / direct exit 0. Root read the raw log and
  direct-exit record. Nine state, two physical-alias/reservation and two existing
  AppModel admission cases passed. This is not full AppModel acceptance.
- **Document GUI failed:** exact `5ad1cbcc` passed 16 focused/adjacent tests,
  but existing-file Save As dismissed without a Replace confirmation for both
  same and distinct destinations. New-file save and Cancel passed. Evidence
  `81734bce` is on main `7978b98d`; original failures remain preserved. The
  isolated GUI app is closed; normal preview PID57591 remains untouched.
- **B correction under review:** peer completed request `031a10ea`; result
  `0802848a` supplied dismissal handshake `ae0cd8e4` and Shell adapter `c7f28b6c`.
  A accepted for review, then failed source acceptance: cancel before appearance
  can retain an unshown presentation ID indefinitely. Follow-up request
  `ca97f793-cf44-4810-b500-a02f7f501653` published; SSH queue
  `01a0e630-ea24-72a1-ac25-35c479df1f58` accepted. Peer receipt pending.
- **Active owners:** Luna adds AppModel navigation regressions against frozen
  `61e453d7` and owns the single compiler slot. Resource Sol implements native
  preview-flight drain acknowledgment: subscriber cancellation is not native
  completion, so current Review/Print reservations are not yet proven safe.
  Reviewer Sol independently checks output lifetime and the window-close guard.
  Later source checkpoints remain UNRUN. No product merge from this wave.
- **Next:** finish these regressions, review B's revised presentation lifecycle,
  rerun bounded focused/full gates, then repeat isolated Save As and recovery GUI
  tests. A alone owns main merges. B stays source-only with its workload hold and
  paused heartbeat; published messages are not counted as peer receipt.





## Verified focused gates; GUI validation active — 2026-09-28 03:50 UTC

- **Coordinator component gate passed:** exact `6a03d66b`, nine tests/zero
  failures, direct exit0. Root read raw logs/exits and verified all 22 portable
  evidence files after correcting a self-referential checksum list. Evidence
  commits `bc57a859` + `456d1fde` are on main `144c911f`; this is evidence-only,
  not an AppModel product merge. Actor compile failure and fixture semaphore
  hang/sample remain preserved. FFI stays `8ab43f64`.
- **Document candidate focused gates passed:** exact `5ad1cbcc`, nine save +
  four Layers completion + three adjacent regressions, all zero failures/direct0.
  Root read result JSON and logs. B results `e9fe1c57` / `12d64cdd` remain accepted
  pending final evidence/GUI acceptance; no main product merge yet.
- Validator Sol owns the single desktop/GPU lane for a new isolated betterSSD
  package, real Save As Cancel/Esc/save/Replace Cancel/reopen checks. Existing
  preview PID57591 is protected. No overlapping build authorized during GUI.
- Resource Sol extends caller recovery, output lifetime and physical-source alias
  reservations. Coordinator `13523cbd` extends the tested component and is UNRUN.
  Luna owns new independent alias tests, then AppModel navigation tests after
  stable source freeze. All AppModel integration/quit/output claims remain pending.


## Focused validation and Document handoff — 2026-09-28 03:45 UTC

- Coordinator nine-test source accepted at `a19d7997`; first gate failed compile
  (exit1, no tests) on nested actor isolation. Repair `a139038d` compiled in
  test candidate `00a70950`, but the run stalled in the fixture: `performClose`
  removed the semaphore before `releaseGatedClose` could signal it. Luna preserves
  stack/log and repairs the fixture before retry. No runtime acceptance yet.
- B typed save candidate `b3b39c82`/handoff `60145d6c` has nine UNRUN tests;
  result `e9fe1c57` accepted for review. Independent source review found no
  blocker; actual SwiftUI dismissal/replacement remains a required GUI gate.
- B Layers completion candidate `9e140c5b`/tests `bfcf1689` has four UNRUN tests;
  result `12d64cdd` accepted for review. API status `675c8536` consumed with
  completed informational receipt. Independent source review accepted. Both
  requests have verified B receipts and results, not just SSH queue acceptance.
- Reviewer Sol now prepares exact Document validation head `5ad1cbcc` in reused
  workspace-redesign checkout, separate branch `codex/document-save-validation`.
  No concurrent build: Luna retains A compiler priority for fixture repair/retry.
- Resource Sol continues AppModel/Agent/Review/Output/Print and persistent recovery
  controls. Layers gate now transfers to B's actual-completion callback. Physical
  source aliases and navigation during active reservations still need completion.
  Preview subscription cancellation is not backend read drain; conservative
  natural-completion ownership is required. No AppModel acceptance or main merge.
- B workload hold and normal preview PID57591 remain unchanged.


## Active recovery implementation — 2026-09-28 03:31 UTC

- A main `f53c6092` adds B's reviewed Document preparation design `42ccdab8`;
  result `cf601ac9` has a completed source-plan receipt. No runtime acceptance.
- Resource Sol implements AppModel ownership and caller barriers in
  `codex/develop-app-recovery`. Coordinator checkpoints through `aca02792`
  repair unknown-ID false success and open-ticket ownership loss; review is
  ongoing. Caller migration is uncommitted and uncompiled. Root flagged Retry
  source identity, reentrant folder callbacks, reservation-before-close ordering,
  and Layers capture lifetime for correction before freeze.
- Luna owns eight dedicated component tests in `DevelopRecoveryStateTests.swift`;
  source review pending, all UNRUN. Earlier two AppModel admission tests remain
  behavioral RED. Reviewer Sol independently reviews coordinator and test source.
  A compiler is free until the reviewed source gate is allocated; no GUI lane.
- B receives source-only typed Document prompt/save settlement task
  `3b2a6fa3-81d3-4a25-8d4d-75e323afedce`, existing-chat SSH queue
  `01a0e610-36e8-73c1-a590-0ae110a6756d`. Peer accepted receipt verified; B is implementing source-only tests and API.
  No global Quit/draft adapter authorization.
  B workload hold and paused heartbeat remain unchanged.
- Coordinator source review now accepts `d49c12fc`: exact owner/controller
  handoff proof, bounded successful-close tombstones, cached failure on ordinary
  close and explicit `retryClose`. Luna finalizes focused tests; owns A compiler
  only after test source review, with logs/direct exits preserved.
- B Layers completion dependency requested separately as `7f1c2378`, queue
  `01a0e613-3b6c-7e60-8ca9-fb55fc1bc852`; queue accepted, peer receipt pending.
  A must hold its reservation through actual backend completion/drain.
- Normal preview PID57591 remains untouched. Main merges remain A-only.


## Current coordinator checkpoint — 2026-09-28 03:18 UTC

- **DONE native save retry** main `d4274a68`, tested `85de2860`: 21 Develop
  plus two adjacent tests, strict/fmt passed; repair reads current disk recipe.
- **DONE native close lifecycle and settings retention** main `01ac555e`,
  exact tested `40d3054e`, archive `8ab43f64`. Full Release 516 XCTest,
  one existing skip, zero failures, plus five Swift Testing tests; focused 41/0.
  Earlier failures remain preserved. This is not application recovery acceptance.
- **DONE coalesced mask retention** main `447772f5`, exact product `8c628e63`
  from B `639da049`. Safe baseline `36f5845d` failed two expected assertions;
  final 11 retention + 14 adjacent tests passed, direct exit 0. Root verified
  source/archive hashes and merged product equality. Portable evidence `bac0dcaa`
  under `tools/orchestrate/wp/UX-03/evidence/mask-retention-2026-09-28/`.
  No full-suite or GUI gate was run for this change. B result `737e0d32`
  has a completed receipt; initial source-review failure remains recorded.
- **DONE Core result-bearing close** main `435307e3`, exact tested `dbb740bb`,
  unchanged native archive `8ab43f64`. Focused13/0, adjacent50/0 including RAW,
  full Release540XCTest/1existing skip/0failures plus5SwiftTesting; direct exits0.
  Root verified seven input hashes, 55 portable evidence hashes and exact merged
  product tree equality. Failed host/native close retains pending edits/listeners
  and permits independent retry; all mutating entry points reject during close.
  Callback descendants keep their original attempt result to prevent auto retry.
  Evidence: `tools/orchestrate/wp/UX-03/evidence/develop-close-core-2026-09-28/`.
  Earlier harness compile failure, behavioral RED, NaN Foundation exception/crash,
  finite diagnostic control and missing-fixture failure are all preserved. JSON
  validity preflight fixes the exception path without discarding invalid pending
  edits. This is Core acceptance, not AppModel recovery or new GUI acceptance.
- **B recovery plan accepted**: revised `d4793913` integrated as main `26884965`.
  Shared save outcome is separate from caller cancellation; Quit late veto restores
  the prior editor, folder callbacks settle exactly once, and output admission
  lasts through actual capture/completion. First plan's ambiguous contracts have
  a failed receipt; revised result `3aa90667` has a completed source-plan receipt.
  Requests `3efd98f3` and `58434c0c` reached B through existing-chat SSH queues
  and received peer acknowledgements/results. No product/runtime claim follows.
- **AppModel admission RED confirmed** on `b928219d`, opener seam `58e1b39f`:
  two tests, two intended open-count2-versus1 failures, direct exit1. Both injected
  active-close/stale-open-cleanup failures were reached; no fixture error or hang.
  Root checked raw log. Luna packages evidence; compiler released. The tests prove
  normal reopening is wrongly admitted, not full retention or UI recovery.
- **Resource Sol implementing AppModel recovery** in `codex/develop-app-recovery`.
  Accepted API draft `b98dd299` is on main `3d5528fa`: observed recovery records,
  ownerless-session identity, scoped reservation release, strong open-ticket
  ownership independent of weak AppModel callbacks. Integrate seam/tests, then
  implement coherent registry/navigation/consumer failure barriers. Independent
  source review precedes next gate. Luna owns additional dedicated tests.
  No AppModel product acceptance yet; B Document adapter waits API freeze.
  Discard, output read reservations, complete quit restoration, and Stage C lease
  remain unaccepted. A alone owns main merges.
- **B status** `5ddb6abe` published through Git only, no new SSH wake/peer receipt
  claimed. B owns Document adapters after A API freeze; workload hold and paused
  heartbeat stay. Existing request/revision deliveries and peer results verified.
- Normal user preview PID57591 remains untouched on older65fa build; merging
  main does not update that package. No new performance acceptance claimed.

This checkpoint supersedes older in-progress ownership entries below.

Updated: 2026-09-28 00:58 UTC

User mandate: autonomously advance UI/UX redesign **and** the recovered engine
queue, using parallel GPT-6 Astra/Luna agents; coordinate Machine B and keep
Machine A as sole main integrator. Do useful work while other tasks wait.

## Operating rules

- Read this board and fresh peer status at each continuation. Verify processes,
  agent states, branches and dirty files before relaunching work.
- Root owns this board and `main`; agents report results and use isolated source
  areas. A owns Shell/shared/Library/Develop and engine work. B owns its active
  Document UI/document FFI packages. Agree any overlapping work through Git.
- One heavy build/GPU/performance slot on A. Parallel source analysis and edits
  are encouraged. Keep failures, all timing samples, source SHA and test evidence.
- Move a task to Done only when its actual acceptance is met. A green unit suite
  does not satisfy missing UI, performance, interoperability or integration gates.
- For each completion, choose the next ready task rather than ending at a summary.
  If blocked, state the dependency and work on another ready lane.
- Preserve existing worktrees/evidence. Never reset dirty work or force-push.
- Root publishes this board and MACHINE-A.md on main. B writes its own note at
  `origin/wp/B5-16:tools/orchestrate/wp/B5-16/CODEX-TAKEOVER.md`.

## Active

| ID | Work | Owner / location | Next action and acceptance |
| --- | --- | --- | --- |
| UX-01 | Library / Photo Edit workspace and split inspector | A root; main `4925677` | DONE first bounded workspace slice; sourcecommitc417229 merged/pushed. Full411+5 checkpoint, final28targeted/0,40layouts,1readyRAW passed; rootvisualreview accepted. Interactive/performance gates remain separate; existing overlay/wrapping polish goesUX04. |
| ENG-31 | M5-31 source reuse and cold timing | A root; wp/M5-31 `97eb4ca`, product `441da3e` | A local326tests and6timings pass. Direct SSH six-sample repeat on B FAILED first original cold106.084084ms/100ms; remaining5pass. All evidence retained in branch; no main merge. Cold diagnostics did not reproduce the outlier or establish a cause; A root retains the unresolved gate. No threshold changes or replacement samples. B completed receipt confirmed; result7fc92eb5 supersedes benchmark request execution. |
| COM-01 | Establish usable authenticated Machine A ↔ B communication | A root + B coordinator | DONE verified SSH execution and existing-chat delivery via installed `codex queue`, peer reply, accepted/completed Git receipts. Exact B UUID01a0e323-c018-7fa3-9605-999a2dea6b32. B scheduled wakeup was verified at18:50:41Z (bd919fed); B has since paused its heartbeat for the resource audit. Preserve existing writers, use mailbox receipts and no duplicate enqueues. |
| RES-01 | Repeated CPU smart-filter evaluation and pass retention | A root; main d2ac1226 | DONE bounded slice. Reviewed/tested e5bfbd5c merged with byte-identical crates/Cargo. 8focused,60library/1ignored (Metal exercised),48integration, strict pass. Full-mask work RES03, cancellation RES02 and global/GPU admission remain separate. B hold stays. |
| RES-02 | Cancellation inside expensive transform kernels | A Sol implementation in codex/transform-cancellation; B retains Document/FFI integration | Primitive DONE main505c4c29:66tests/strict; caller compositor bridge DONE main0d627023:31CPU tests/strict. No end-to-end app cancellation claim. B source plan056daf87 reviewed next; no B run. |
| RES-03 | Repeated full-image filter-mask blending | A resource Sol; B report0926ae38 accepted | DONE bounded slice main7d58ceff, exact420826ac Rust/Cargo bytes. Combined predecessor48CPU/strict; final partial-counter fix15focused/strict and behavioralRED0vs4096. Per-pass masked admission/reuse; digest scans remain per lookup. Not a global memory cap. |
| RES-04 | Cancellable CPU region API | A Sol; codex/cpu-region-cancellation | DONE maina1d51f6b exact8585478b crates/Cargo.19focused/strict pass. Region clipped at requested level; one pass across covered tiles. B source-only frame wiring requestaa0a9692 queued01a0e4d9; peer accepted, readback/PSD separate. |
| OPS-01 | Persistent autonomous coordinators | A root + B coordinator | A coordinator remains active. B paused its heartbeat and load tests following the user’s report of severe responsiveness/resource pressure; no B rebuild, benchmark, runner or heartbeat restart without B/user direction. B audits low-impact resource evidence. A local work stays separate. |

## Ready / next

**Fixed editing preview available on A.** Source main65fa6a33 with current FFI
2f241fe7 passed full504XCTest/1skip/0fail plus5SwiftTesting. Actual GUI tinyJPEG
export/reimport returns Unedited/Exposure+0.00; embedded and adjacent XMP omit
baked development. Copied16.2MP Sony RAW +0.10EV/5132K persists across relaunch.
Root checked logs/directexit, source/archive/package hashes, code signatures and
output metadata/image. No pixel-identity, broad cameras, PSD, large catalog or
performance acceptance. macOS15 compatibility unverified (dependency warning).

User preview is open empty at
`/Volumes/betterSSD/tessera-validation/editing-export-preview/Tessera-Editing-Preview-65fa6a33.app`
with separate `~/Library/Application Support/Tessera Editing Preview`, no test
arguments. Earlier preview remains unfixed historical evidence. Report/manifest
in the new package parent/evidence; portable82dc4902 evidence merged mainf09a66ff.
B result45d3595b published; SSHqueue01a0e53c-917e-76c2-b216-56ee3b2c0d3a accepted,
peer receipt confirmed by existing-chat reply at cursor14. Earlier update cursor13.

1. **PSD/Review bounded slice DONE main7757f628**, exact tested dc4073de
   product bytes; portable automated evidence8ac2e1ae and GUI6bef18a merged
   mainc000a772. Native45/strict, focused2/2, full509XCTest/1skip/0fail plus5
   SwiftTesting passed. Tiny320x240 JPEG Open-in-Layers→rasterizedPSD→reopen
   passed, sourceJPEG unchanged; root checked PSDheader/outputhash and both
   signedapp/executable/FFI hashes. Normal Library stub button hidden; explicit
   diagnostic launch shows it without loading20k. Release has no Debug menu,
   so separate Debug-menu visibility remains unchecked. No complex-stack GUI,
   largefile cancellation latency or performance claim. Prior failures retained.
2. **Two Engine writer gate DONE maina920c46c**, exact combinedcf161acb
   crates/Cargo bytes; portablecombinedevidence05522ce mergedmain0ee89e33.
   Sixfocused+3API+fmt/strict pass; rootverified76sourcehashes/directexits.
   Earlier1selectioncheckpassed, firstmissingmoduleRED and strictlintfailure
   retained. Rawrevision APIs are tested but not yet consumed by production CAS.
   Only set_selection/set_recipe_json diskRMW serialized, not otherwriters,
   staleJSONprotection or multi-filetransactions. BatchApply remains blocked.
   StageA RED confirmed false-success with missingXMP (f79d18d4); candidate1
   19Develop tests passed but review found concurrentflush outcome, gatedrepair,
   and savedpreview/notification gaps. Resource Sol corrects source-only.
   Luna temporarily owns compiler for two controllerRED cases bec2f630 using
   current0a9FFI; returnslot toResource afterward, no overlappingbuilds.
   Proposal staged/published maina59c8fd5. Luna source-plans StageB recoverable
   close/error-aware barriers; exclusivelease StageC remainsblocked. Current close
   currently drops saveerrors; retained sessions can mutate afterclose; histogram
   helper opens temporarysession; retry afterpartialcommit needs explicit state.
   No exclusivelease activation until failure-safeclose/read-onlypaths covered.
   DirectJSON diagnostic headless372f/context2457 both caught normalhistory
   BridgeError; originalphase/deinit remains unreproduced/unresolved, evidence
   main60a310ac. Diagnostic test code is not merged.
3. **Luna — desktop released; UX05 source contract:** validationapps closed; normal65fa userpreview
   untouched. Draft true liveRAW graph/persistence boundary and minimum next
   dependency without editing B-owned Document implementation. B review18cfb1c8
   accepted as design evidence; draftmain219bd62c picks pinned snapshot first.
   Format preflight DONE main372dbbcc, exactfinal26845eed production/testbytes:
   expectedfutureDecode RED preserved, v1control and final4roundtrip/strictfmt/
   clippy pass. Rootverifiedhashes/rawlogs and independentreview accepted.
   Nativeformat v1 remains, no RAWnode/schema/UI; desktop free. PSD fixture/report durable under tools/orchestrate/wp/UX-05-PSD-copy/
   evidence/2026-09-27. CUA screenshots inline only, no local export available.
4. **B — resource hold:** completed PSD receipts b7294f0e/ccb9e8f1 published.
   Result628f798f and SSHqueue01a0e571-f1e2-79d3-816b-b5871032994c published
   for source-package reconciliation. Existing B chat acknowledged receipt at cursor15 and is reconciling packages.
   No B workloads/heartbeat restart. A remains sole main merger.

Review persistence DONE main7adc4fa2 (494XCTest/1skip/0fail+5 and tinyGUI).
PSD primitive DONE mainebe7b043 (66tests/strict); current preview99ba predates it.
A alone merges main. PSD admission proposal is modeled work, not a memory cap.
All11 global test records are preserved evidence; original records left intact.

Current integrated product checkpoints:

- Review ownership/save barriers and export slice: fb7c604,436XCTest/one existing
  skip/zero failures plus5SwiftTesting; provenance INT-45.
- Review workspace + narrow Masks layout: b2757c7, exact fe0ff9e source,
  455XCTest/one existing skip/zero failures plus5SwiftTesting. Functional GUI
  covers Library/Edit/Review navigation, draft focus and single Accept. Multirow,
  relaunch and actual presentation performance are separate.
- Loupe disclosures/Escape: b18ab0bb,14focused/0 and actual JPEG GUI; both native
  popovers dismiss before workspace Escape, shortcut input does not leak.
  RAW pointer/liveproof untested; synthetic Command-Q inconclusive.
- Timer correction:261a585f,19focused/0 and tiny GUI hide/return/document switch/
  detach-return. No rate/CPU/memory claim. Original compile and copied-package
  Sparkle rpath failures preserved. The GUI retained older FFI archive19f.
- Mask reuse:7d58ceff, bounded per-pass masked payload and actual partial-cancel
  accounting. Final15focused/strict; predecessor48CPU/strict stays separately scoped.

Failed native gain-map reconstruction and original M5-31 cold timing remain
unaccepted. No thresholds, failed samples or original evidence were discarded.
B source-only hold supersedes all old benchmark requests or heartbeat startup
notes. Existing-chat SSH queues and peer Git receipts remain distinct states.

| ID | Work | Owner / branch | Entry condition / acceptance |
| --- | --- | --- | --- |
| DEV-58 | Complete remaining Develop performance acceptance | A; `wp/M2-58` `196005e` product `5ff2679` | P01 actual input-to-present and P11 detail settle<=200ms /drag p95 regression<=10% remain unverified. Read M2-58-PRESENTATION-PLAN.md: actual Loupe scanout requires a non-occluded surface and detail CALayer publication lacks a display-time oracle. Keep acceptance pending; do not substitute callbacks or transaction completion. Existing correctness/parity evidence preserved. |
| EXP-45 | Gain-map JPEG reference interoperability and implementation | A root; preserved codex/gainmap-restoration | A core gate FAILED:4passed/1failed; four-stop native peak7.9837623 vs16. Captured outputs preserved. ImageIO options tested unchanged; software CoreImage reconstructs the same four-stop file to16, ImageIO path still8. Supplemental software CoreImage and targetedCLI/FFI/MCP passed, evidencec716460; original failed assertion retained, no tolerance changes. B frozen core1pass/4fail; independent ISO controls also yield no native HDR on B macOS26.1, documented as tested-host limitation, not encoder acceptance. Independent libjpeg reconstruction passes5patches. B snapshot/capture preserved; no new B runs while its slot is reserved. |
| INT-45 | Integrate validated DNG/PQ-HLG/native metadata slice | A root; main fb7c604 | DONE bounded slice. 564Rust/0fail/21ignored, strictchecks, FFI, final436XCTest/1existing skip/0fail +5SwiftTesting. DNG1.6/backward1.4 contract verified. Gain-map JPEG separate. |
| UX-02 | Navigable Review and resume persistence | A root; Review workspace merged b2757c7 | Ownership/save barriers and mutation exclusion DONE:23regressions included in full436+5 gate. UX02a source4fcb802 passed43focused tests; 34captures include real ready pixels; recovered2-test status/ready gate passed. Checkpoint a54044f and isolated hands-on checks passed; fe0ff9e full455+5 passed and merged mainb2757c7. UX02b plan reviewed: latest queue, canonical same-path library/app-support, no prompts or auto-replay, recipe-authoritative status, fresh owner/generation. DONE bounded persistence slice main7adc4fa2:16cb6ce5 full494/1skip/0 plus5 and tiny JPEG GUI relaunch pass, evidence1b0f5283. Move/rename identity remains outside contract. |
| UX-03 | Selective batch editing and source/target clarity | A resource Sol; codex/batch-settings-draft | Pure immutable draft DONE main4f7813d2: 10focused+8adjacent passed; frozen source/targets, safe field groups and partial merge tests. Executor/UI blocked on complete recipe revision, shared writer exclusion and durable run/revert contract. No hidden batch shortcut. |
| UX-04 | Visual/accessibility refinement | A b516_review implementation; B agreed split | Masks slice041778b passed populated 288/380-point light/dark checks and ThemeLint (2/2), with root visual review; merged mainb2757c7 after full455+5 gate. Loupe bounded disclosure/keyboard slice merged mainb18ab0bb after final14/0 and GUI: both popovers Escape→dismiss/stayLoupe, nextEscape→Grid, Right/D/X noleak, narrowlongname disclosure. RAW pointer/liveproof and syntheticCmdQ remain unverified. No Document/global-theme edits. |
| UX-05 | True live-RAW/layer continuity | A engine+B document contract | Separate substantial dependency: current transition is rendered copy. Define graph/persistence/version contract and release scenario before making live-raw claims. |

## Machine B / integration dependencies

Last peer note read: `463922c`. B's resource hold supersedes its older queue.
Channels and Text targeted reruns passed with actual exit0; Transform was stopped
at rasterized PSD after986%CPU and7.0GiB footprint (7.5GiB peak). Host43GiBswap
is not all Tessera. B heartbeat and runner remain held. A accepted hold7cf0f7e2
and bounded A-only resource investigation9a28ab84. Source audit identifies
concurrent duplicate full-image calculations, omitted transient budgets and
uncancellable seam work; no blanket leak diagnosis. A owns compositor investigation;
B retains Document/frontend remediation. Main B5-16 integration stays held.

| ID | Work | State / next action |
| --- | --- | --- |
| B5-16 | Tabbed Document inspector / persisted editors | Original six-pass/three-fail evidence retained; repaired Channels/Text pass. Transform interrupted under resource hold. No new B loads; main integration and interactive acceptance remain pending. |
| RES-B-TIMERS | Two frontend overlay timer lifecycles | DONE bounded slice main261a585f/evidence1c5e16c1. Correction7a58b48 source079092e8 passed19focused/0 and observable tinyGUIflows. Original compile failure6544a1d6 preserved; result21c4f39e completed. No measured timer cadence/CPU/memory or full current-FFI app claim. |
| RES-B-OUTLINE | Bound pending selection outline work | B2429de2 source; A resource Sol candidate/tests | DONE main5fa0faea, exact tested398acb76/f2309eec Swift bytes.12 tests passed:3 blocked-fetch lifecycle,4 buffer,5 timers. Receipt674d7f3e completed. Already-running synchronous work remains uncancellable. |
| RES-B-VIEWPORT | Ownership-safe viewport teardown | B63a06a2 source | DONE main1c0f36b8, exact fb4d7df8 Swift bytes.16 focused plus27 adjacent passed; evidence c544be51. Original weak-nil blocker corrected b2f8d85. Both result receipts completed. Offscreen Metal/1x1 IOSurface; old FFI19f, no GUI/performance claim. |
| RES-B-CANCEL | Native preview/bake caller cancellation | B source-only request1a0330c4 | DONE main1fb7e983; exact testedcd07b435/7971a771 Rust/Cargo.5 private plus12 mixed-backend integration tests passed,1 ignored benchmark; strict FFI Clippy passed. Receiptb69c56ca completed. Frame/readback/PSD handles and large-image latency remain separate. |
| RES-B-FRAME | CPU frame request cancellation | B f518c03c; A engine Sol | Accepted resultb1b6b4dc. DONE main78492f7f after corrected544e8a13 passed33 combined tests/strict. Initial telemetry failure preserved; outcome accounting fixed and unchanged ring regression passes. Evidence8693b7b7. No GPU preemption or readback/PSD claim. |
| RES-B-CACHE | Byte-bound FFI image cache retention | B source-only request04e1a883 | DONE main78492f7f; seven tiny cache cases included in corrected33-test/strict gate. Evidence8693b7b7, resultc65a3f7e completed. Named512MiB plus4entry cap, prefix-copy preflight. Not a process/GPU cap. |
| RES-B-COPY | Cancellable rasterized PSD operation design | B request92b23b0f | Design8363c8a reviewed/completed74bb01e5. Operation/host source implementation authorized request547433be, queue01a0e4f1 transport accepted and formal peer accepted receipt confirmed. Running cancellation must hold admission until worker drains. A companion conversion plan a7762a05; no end-to-end cancellation or workload authorization. |
| B5-15 | Export App Nap fix / performance | B published `aec242f`; export-only improvement does not clear main-thread maxima35.93/58.00ms vs8ms, AppKit layout exception, P19 or memory gates. B owns app fixes; A engine optimization only with explicit evidence/split. |
| B5-12b | Transform acceptance | Interrupted for resource hold; not accepted. No rerun. |
| B5-13 | Liquify / move / extend | B owns preserved work and current acceptance. |
| B5-14 | Post-M2-57 performance measurements | B queued; no inferred pass. |
| UI-VERIFY | Isolated app interaction and uncovered-canvas checks | User authorizes activating isolated betterSSD builds with disposable catalogs. Desktop slot currently free; agents are on source/test lanes. Current user app and old isolated packages preserved. Actual presentation/performance checks remain separate from functional GUI checks. |

## Verified checkpoints (not whole-package acceptance)

| Work | Evidence |
| --- | --- |
| B5-16a main integration | Published; Rust adjustment JSON3/3, Swift397tests one skip0fail +5Swift Testing. |
| M2-58 correctness and bounded P10 | Branch196005e; Rust stages/strict checks pass, Swift409tests one skip0fail +5Swift Testing. Three setter runs meet2/8ms thresholds. Two Sony ARW Auto Upright A/B cases match16,117,920valid RGBA bytes each plus histograms/settings exactly. Baseline actual residency unavailable. |
| M2-45d keyword correctness | Branch69bcd3e; export+sidecar149passed0failed7ignored; strict all-target Clippy/fmt pass. Gain-map incomplete. |
| M5-31 correctness repair | Branch9f922bf; compositor323passed0failed13ignored; strict all-target Clippy/fmt pass. Fonts, live precision, smart-child level routing, allocation preflight fixed. Cold performance still fails. |
| Git coordination | Two-way acknowledgements confirmed. Git push alone does not wake idle peer. |

## Recovery references

- Current status: `docs/coordination/MACHINE-A.md`.
- Reviews: `B5-16-REVIEW.md`, `M5-31-REVIEW.md`, `M2-45D-REVIEW.md`.
- Communication findings: `CROSS-MACHINE-RESEARCH.md`.
- UX research: `tools/orchestrate/audits/ux/REPORT.md`; layout audit adjacent.
- Recovered prototype: `/private/tmp/claude-501/-Users-rutmehta-Developer-tessera/1ef5c604-ef13-4903-b9c9-757556764307/scratchpad/mock/tessera-redesign.html`.
  Preserve a durable source copy before depending on temporary storage.
