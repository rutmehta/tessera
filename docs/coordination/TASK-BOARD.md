# Tessera task board — Machine A coordinator

Updated: 2026-09-27 20:09 UTC

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
| ENG-31 | M5-31 source reuse and cold timing | A root; wp/M5-31 `97eb4ca`, product `441da3e` | A local326tests and6timings pass. Direct SSH six-sample repeat on B FAILED first original cold106.084084ms/100ms; remaining5pass. All evidence retained in branch; no main merge. `cross_machine_research` investigates residual cold cost, no threshold changes or replacement samples. B completed receipt confirmed; result7fc92eb5 supersedes benchmark request execution. |
| COM-01 | Establish usable authenticated Machine A ↔ B communication | A root + B coordinator | DONE verified SSH execution and existing-chat delivery via installed `codex queue`, peer reply, accepted/completed Git receipts. Exact B UUID01a0e323-c018-7fa3-9605-999a2dea6b32. Both heartbeat configurations active; B reports first scheduled wakeup at18:50:41Z, mailbox bd919fed. Preserve existing writers, use mailbox receipts and no duplicate enqueues. |
| OPS-01 | Persistent autonomous coordinators | A root + B coordinator | A tessera-machine-a-coordinator and B tessera-machine-b-coordinator confirmed ACTIVE every5minutes. Both check existing work and stay quiet unless meaningful outcomes. B reserves its heavy slot for B5-16. Local hosts/apps/service availability still apply. |

## Ready / next

Main fb7c604 now contains the verified export slice and Review ownership/save-order
safety. Final combined source b82fe333 passed436XCTest cases (one existing skip),
zero failures, plus5SwiftTesting. The missing-fixture Rust failure and obsolete
Swift DNG expectation failure are retained with their successful corrections.
Full provenance lives in tools/orchestrate/wp/INT-45/evidence/2026-09-27-integration/.

A resumed after a usage-limit interruption; no duplicate tests were launched.
UX02a's completed status/ready-preview gate passed two tests with zero failures,
and its source digest matched the retained manifest. Root inspected real saved
pixels at minimum width; 34 captures include ready states. Masks reproduced six real narrow-label failures plus one OCR punctuation mismatch.
The two-row correction passed focused Masks+ThemeLint checks (2/2) and root visual
inspection. Evidence checkpoint is being finalized; no main merge yet.
Review+Masks integration candidateb1d1a15 is prepared in the reused export-integration
checkout, retaining main export source and ownership evidence; full gate pending. A Luna
agent inspected the old baseline, then launched the isolated dev-test candidate.
Initial CUA binding failed; uniquely named executable variant bound successfully.
Luna verified Library/Edit/back, selected-fixture AutoEdit, Review/Edit/back,
draft text focus and single-entry acceptance. Full combined suite remains pending.
The user authorizes multiple isolated builds on betterSSD and computer-use tests;
keep catalogs separate and serialize UI control as well as heavy builds.

Sol gainmap_gate_review now owns A’s heavy slot for supplemental Core Image and
CLI/FFI/MCP checks, after reconciling that no prior process survived interruption.
The original ImageIO four-stop failure remains intact; no package pass claimed.

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
| EXP-45 | Gain-map JPEG reference interoperability and implementation | A gainmap_gate_review (Sol); codex/gainmap-restoration | A core gate FAILED:4passed/1failed; four-stop native peak7.9837623 vs16. Captured outputs preserved. ImageIO options tested unchanged; software CoreImage reconstructs the same four-stop file to16, ImageIO path still8. Supplemental native tests being prepared; original failed assertion retained, no tolerance changes. B frozen core1pass/4fail; independent ISO controls also yield no native HDR on B macOS26.1, documented as tested-host limitation, not encoder acceptance. Independent libjpeg reconstruction passes5patches. B snapshot/capture preserved; no new B runs while its slot is reserved. |
| INT-45 | Integrate validated DNG/PQ-HLG/native metadata slice | A root; main fb7c604 | DONE bounded slice. 564Rust/0fail/21ignored, strictchecks, FFI, final436XCTest/1existing skip/0fail +5SwiftTesting. DNG1.6/backward1.4 contract verified. Gain-map JPEG separate. |
| UX-02 | Navigable Review and resume persistence | A develop_resume; safety merged fb7c604 | Ownership/save barriers and mutation exclusion DONE:23regressions included in full436+5 gate. UX02a source4fcb802 passed43focused tests; 34captures include real ready pixels; recovered2-test status/ready gate passed. Source checkpoint and candidate packaging/integration next. UX02b relaunch persistence still pending. |
| UX-03 | Selective batch editing and source/target clarity | A; after UX-01/02 contract | Preserve source/target snapshots and settings selection; define undo/review semantics, verify no unseen batch action from a single-key shortcut. |
| UX-04 | Visual/accessibility refinement | A b516_review implementation; B agreed split | Audit complete. Implement independent two-row Masks header with full labels/accessibility; populated minimum-width visual acceptance pending. Loupe disclosure work deferred until UX02a coordinates ContentView extraction. No Document/global-theme edits; build slot pending. |
| UX-05 | True live-RAW/layer continuity | A engine+B document contract | Separate substantial dependency: current transition is rendered copy. Define graph/persistence/version contract and release scenario before making live-raw claims. |

## Machine B / integration dependencies

Last peer note read: `8e301f0`; latest published A status acknowledged by B:
`6e1e7b8` via mailbox bd919fed. B reports424XCTest/one skip/zero failures +5SwiftTesting, Xcode and package provenance passed. Runner reconciled: six packaged passes (Document/Tools/Filter/Retouch/Styles/Vector), three failures (Channels/Text timeout despite done0, Transform empty Puppet source). Fresh Channels now passes with exit0; Text/Transform reruns remain active. B found competing new-document startup in Transform harness and retains original failure. No final acceptance inferred. A acknowledges B5-15 checkpoint `aec242f` and failed M5-31 timing.

| ID | Work | State / next action |
| --- | --- | --- |
| B5-16 | Tabbed Document inspector / persisted editors | Review fixes published8184da1; strict packaged runner six passes/three failures retained. B repairs Channels/Text termination and investigates Transform. Hold main merge; new-main integration and interactive acceptance remain pending. |
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
