# Tessera task board — Machine A coordinator

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
