# Tessera task board — Machine A coordinator

Updated: 2026-09-27 22:08 UTC

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

A compiler slot: resource Sol is validating frozen viewport candidate `fb4d7df8`.
Luna's Review persistence candidate passed 47 focused tests after provenance and
ordinal corrections; evidence packaging and final source review are active. It is
not merged and GUI relaunch is unverified. Earlier failed gates remain preserved.
Engine Sol independently reviews B frame `f518c03c` and cache `755ac31e` in an
isolated A candidate; neither has compiled yet. These are next in the build queue.
Timer, outline, region, masked reuse and native preview/bake slices are merged as
listed below. Swift gates retain FFI archive `19f`, which predates current Rust
resource changes; a combined current-source application gate remains necessary.
B remains source-only, with no builds/tests/app launches or heartbeat restart.

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
| UX-02 | Navigable Review and resume persistence | A root; Review workspace merged b2757c7 | Ownership/save barriers and mutation exclusion DONE:23regressions included in full436+5 gate. UX02a source4fcb802 passed43focused tests; 34captures include real ready pixels; recovered2-test status/ready gate passed. Checkpoint a54044f and isolated hands-on checks passed; fe0ff9e full455+5 passed and merged mainb2757c7. UX02b plan reviewed: latest queue, canonical same-path library/app-support, no prompts or auto-replay, recipe-authoritative status, fresh owner/generation. Luna implementing; move/rename identity remains outside contract. |
| UX-03 | Selective batch editing and source/target clarity | A; after UX-01/02 contract | Preserve source/target snapshots and settings selection; define undo/review semantics, verify no unseen batch action from a single-key shortcut. |
| UX-04 | Visual/accessibility refinement | A b516_review implementation; B agreed split | Masks slice041778b passed populated 288/380-point light/dark checks and ThemeLint (2/2), with root visual review; merged mainb2757c7 after full455+5 gate. Loupe bounded disclosure/keyboard slice merged mainb18ab0bb after final14/0 and GUI: both popovers Escape→dismiss/stayLoupe, nextEscape→Grid, Right/D/X noleak, narrowlongname disclosure. RAW pointer/liveproof and syntheticCmdQ remain unverified. No Document/global-theme edits. |
| UX-05 | True live-RAW/layer continuity | A engine+B document contract | Separate substantial dependency: current transition is rendered copy. Define graph/persistence/version contract and release scenario before making live-raw claims. |

## Machine B / integration dependencies

Last peer note read: `755ac31e`. B's resource hold supersedes its older queue.
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
| RES-B-VIEWPORT | Ownership-safe viewport teardown | B63a06a2 source | Accepted9dc8db44; root review blocker: weak controller nil causes attach(nil) to skip local cleanup. Correctionb2f8d85 received/accepted4b930939; exact source review okay, four tiny ownership tests UNRUN pending A slot. |
| RES-B-CANCEL | Native preview/bake caller cancellation | B source-only request1a0330c4 | DONE main1fb7e983; exact testedcd07b435/7971a771 Rust/Cargo.5 private plus12 mixed-backend integration tests passed,1 ignored benchmark; strict FFI Clippy passed. Receiptb69c56ca completed. Frame/readback/PSD handles and large-image latency remain separate. |
| RES-B-FRAME | CPU frame request cancellation | B f518c03c; A engine Sol | Accepted resultb1b6b4dc. Five tests UNRUN; reviewing token lifecycle, callback acceptance and region/copy behavior before A gate. No GPU preemption or readback/PSD claim. |
| RES-B-CACHE | Byte-bound FFI image cache retention | B source-only request04e1a883 | B source755ac31e received and accepted resultc65a3f7e; seven tiny tests UNRUN. Independent A review and serialized validation pending. Named512MiB plus4entry cap, prefix-copy preflight. Not a process/GPU cap. |
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
