# Tessera task board — Machine A coordinator

Updated: 2026-09-27 18:44 UTC

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
| ENG-31 | M5-31 source reuse and cold timing | A root; wp/M5-31 `97eb4ca`, product `441da3e` | A local326tests and6timings pass. Direct SSH six-sample repeat on B FAILED first original cold106.084084ms/100ms; remaining5pass. All evidence retained in branch; no main merge. `cross_machine_research` investigates residual cold cost, no threshold changes or replacement samples. B chat receipt still unconfirmed; result7fc92eb5 supersedes benchmark request execution. |
| COM-01 | Establish usable authenticated Machine A ↔ B communication | A root + B coordinator | DONE verified SSH execution and existing-chat delivery via installed `codex queue`, peer reply, accepted/completed Git receipts. Exact B UUID01a0e323-c018-7fa3-9605-999a2dea6b32. Both heartbeat configurations active; first scheduled B wakeup not yet observed. Preserve existing writers, use mailbox receipts and no duplicate enqueues. |
| OPS-01 | Persistent autonomous coordinators | A root + B coordinator | A tessera-machine-a-coordinator and B tessera-machine-b-coordinator confirmed ACTIVE every5minutes. Both check existing work and stay quiet unless meaningful outcomes. B reserves its heavy slot for B5-16. Local hosts/apps/service availability still apply. |

## Ready / next

Main fb7c604 now contains the verified export slice and Review ownership/save-order
safety. Final combined source b82fe333 passed436XCTest cases (one existing skip),
zero failures, plus5SwiftTesting. The missing-fixture Rust failure and obsolete
Swift DNG expectation failure are retained with their successful corrections.
Full provenance lives in tools/orchestrate/wp/INT-45/evidence/2026-09-27-integration/.

A’s heavy slot currently runs fresh gain-map core tests on the frozen13-file
snapshot. UI02a navigation, preview freshness and first-layered-copy save ordering
continue in source; focused behavioral/layout tests are next. A’s third agent
performs the bounded UX04 source/screenshot audit. Reuse safe managed worktrees.

B’s original coordinator received queued messages, completed the Git bootstrap,
and verified its single active heartbeat. Its B5-16 resolved-tree verification
owns B’s heavy slot; A must not launch additional B builds/benchmarks. The dirty
B5-16a gain-map snapshot/capture remains deliberately preserved. Native SSH-host
status may say notLoaded/interrupted despite actual desktop progress; use peer
receipts/status evidence. Never confuse queued/published messages with receipts.

M5-31 remains unaccepted after the direct B six-run first cold failure106.084084ms.
Passive diagnostic samples did not reproduce it or establish a cause. They remain
separate evidence; no threshold was weakened or failed sample replaced.

| ID | Work | Owner / branch | Entry condition / acceptance |
| --- | --- | --- | --- |
| DEV-58 | Complete remaining Develop performance acceptance | A; `wp/M2-58` `196005e` product `5ff2679` | P01 actual input-to-present and P11 detail settle<=200ms /drag p95 regression<=10% remain unverified. Read M2-58-PRESENTATION-PLAN.md: actual Loupe scanout requires a non-occluded surface and detail CALayer publication lacks a display-time oracle. Keep acceptance pending; do not substitute callbacks or transaction completion. Existing correctness/parity evidence preserved. |
| EXP-45 | Gain-map JPEG reference interoperability and implementation | A cross_machine_research; codex/gainmap-restoration | A fresh core gate RUNNING session89001. B frozen core1pass/4fail; independent ISO controls also yield no native HDR on B macOS26.1, documented as tested-host limitation, not encoder acceptance. Independent libjpeg reconstruction passes5patches. B snapshot/capture preserved; no new B runs while its slot is reserved. |
| INT-45 | Integrate validated DNG/PQ-HLG/native metadata slice | A root; main fb7c604 | DONE bounded slice. 564Rust/0fail/21ignored, strictchecks, FFI, final436XCTest/1existing skip/0fail +5SwiftTesting. DNG1.6/backward1.4 contract verified. Gain-map JPEG separate. |
| UX-02 | Navigable Review and resume persistence | A develop_resume; safety merged fb7c604 | Ownership/save barriers and mutation exclusion DONE:23regressions included in full436+5 gate. UX02a navigation/preview freshness/rendered-copy save ordering source in progress, focused gate next. UX02b relaunch persistence still pending. |
| UX-03 | Selective batch editing and source/target clarity | A; after UX-01/02 contract | Preserve source/target snapshots and settings selection; define undo/review semantics, verify no unseen batch action from a single-key shortcut. |
| UX-04 | Visual/accessibility refinement | A b516_review read-only audit; B agreed split | Audit actual narrow/readyRAW screenshots for dense overlay and Masks label wrapping; propose bounded A-only changes and acceptance. No Document/global-theme edits or heavy build. |
| UX-05 | True live-RAW/layer continuity | A engine+B document contract | Separate substantial dependency: current transition is rendered copy. Define graph/persistence/version contract and release scenario before making live-raw claims. |

## Machine B / integration dependencies

Last peer note read: `0232ff7`; latest published A status acknowledged by B:
`353aa97`. A acknowledges B5-15 checkpoint `aec242f` and failed M5-31 timing.

| ID | Work | State / next action |
| --- | --- | --- |
| B5-16 | Tabbed Document inspector / persisted editors | B accepted A's legacy Neutralize and runner failure-accounting findings. Fixes and interactive acceptance pending; hold merge. Resolve documented conflicts preserving B5-16a. |
| B5-15 | Export App Nap fix / performance | B published `aec242f`; export-only improvement does not clear main-thread maxima35.93/58.00ms vs8ms, AppKit layout exception, P19 or memory gates. B owns app fixes; A engine optimization only with explicit evidence/split. |
| B5-12b | Transform acceptance | Incomplete early-exit/failure accounting; B continues. |
| B5-13 | Liquify / move / extend | B owns preserved work and current acceptance. |
| B5-14 | Post-M2-57 performance measurements | B queued; no inferred pass. |
| UI-VERIFY | Uncovered-canvas drag checks | A verification responsibility; preserve nonactivating constraints and mark unexecuted checks as pending. |

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
