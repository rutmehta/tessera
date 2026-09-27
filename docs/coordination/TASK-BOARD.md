# Tessera task board — Machine A coordinator

Updated: 2026-09-27 17:56 UTC

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
| ENG-31 | M5-31 source reuse and cold timing | A root; published wp/M5-31 `441da3e` | Local326pass/0fail/14ignored plus6fresh timing samples andstrictchecks pass. Await B repeat on exact candidate via mailbox4820486e-d71c-4fb1-a88f-6919f34d0eaa; prior B125.855125ms failure remains recorded. Main integration pending. |
| COM-01 | Establish usable authenticated Machine A ↔ B communication | A `cross_machine_research` +root | Native UI inspection blocked; only local app host exposed. Bonjour MacBook SSH refused, B identity unverified. Mailbox integrated067261e,9tests pass; A heartbeat polls it. Handshake1c60b6fd-49d0-43e5-8f6d-81d0766f272f published on codex/coordination-a0c6479d; B receipt/heartbeat unconfirmed. Root polls/bootstrap; agent advancing EXP-45 while waiting. Avoid a second agent server owning active desktop chat. Implement durable deduped Git fallback where needed. No exposed unauthenticated listeners. |
| OPS-01 | Persistent autonomous coordinator | Root; current chat | Heartbeat `tessera-machine-a-coordinator` ACTIVE every5minutes, created2026-09-27. Check running work before duplicate launches. Read/advance board; stay quiet unless meaningful outcome/blocker. Local execution requires awake host/app and available service limits. |

## Ready / next

UX-02 safety prerequisite is now assigned to `b516_review`: isolate review queue
owner/engine/image identities across folder switches and asynchronous completions.
Scope is AgentController and focused tests on a separate main-based worktree;
new Review navigation/resume persistence remains planned in UX-02-PLAN.md.
M5-31 candidate is validated locally and published; B repeat is pending. INT-45 license check also passed (existing unused-license
allowance warnings only); reproducible gated script is
`/tmp/tessera-export-integration-gate.sh`, not yet executed.


Latest checkpoint: UX-01 merged/pushed main4925677. UI agent advances UX02a
navigation while the ownership agent fixes four reproduced cross-library hazards.
Ownership RED:4tests/6expected assertion failures, no setup/unexpected failures;
old Accept trained the newly selected folder, old Revert changed the prior recipe,
late Accept overwrote new-folder status, and old-run completion updated/presented
foreign queue state. Root INT-45 build gate now owns heavy slot (session25483).
Gain-map source is ready and waits for live-output/host validation. B mailbox
still absent; A message head eed5afdb, benchmark repeat request4820486e published.
Root prepared INT-45 in the managed worktree
`/Users/rutmehta/.codex/worktrees/export-integration/tessera`, branch
`codex/export-integration`: main d5366d7 plus a clean **uncommitted merge** of
wp/M2-45d69bcd3e. Candidate tree9fe7a245e2bc070e65fb3f4c0a4301f233ee473d;
formatting and source whitespace checks pass. Preserve the merge state. Full
five-package Rust/strict checks, licenses, workspace and FFI/Swift validation
wait for the build slot; no main merge yet.

EXP-45 diagnostic progress: independently sourced Skia ISO fixtures and the
preserved Tessera prototype now yield actual SDR/HDR pixels in a small ImageIO
probe. The agent is isolating decode options and verifying reconstruction before
restoration in a separate managed branch. Root authorized narrow restoration with
the original tolerance preserved; whole-frame edge error remains separately
reported. This is diagnostic evidence, not completed export interoperability or
a shipped feature.

| ID | Work | Owner / branch | Entry condition / acceptance |
| --- | --- | --- | --- |
| DEV-58 | Complete remaining Develop performance acceptance | A; `wp/M2-58` `196005e` product `5ff2679` | P01 actual input-to-present and P11 detail settle<=200ms /drag p95 regression<=10% remain unverified. Read M2-58-PRESENTATION-PLAN.md: actual Loupe scanout requires a non-occluded surface and detail CALayer publication lacks a display-time oracle. Keep acceptance pending; do not substitute callbacks or transaction completion. Existing correctness/parity evidence preserved. |
| EXP-45 | Gain-map JPEG reference interoperability and implementation | A `cross_machine_research`; `wp/M2-45d` `69bcd3e` | Establish independently valid ISO HDR reference and actual decoded pixel control before changing encoder; distinguish Apple gain-map fixtures from ISO. Finish only with required interoperable output evidence. |
| INT-45 | Review/integrate validated DNG/PQ-HLG/native metadata slice | A root; `wp/M2-45d` | Current main merge-tree clean. Determine explicit partial-slice readiness, run resolved-tree gates; do not call absent gain-map complete. |
| UX-02 | Navigable Review and resume persistence | A `develop_resume` UX02a from main4925677; `b516_review` ownership prerequisite | Navigation-only implementation active; no relaunch persistence claim. Four ownership RED tests fail6expected assertions; source fix underway. APIs coordinated between agents. Resume-index phase follows captured-owner safety and navigation. |
| UX-03 | Selective batch editing and source/target clarity | A; after UX-01/02 contract | Preserve source/target snapshots and settings selection; define undo/review semantics, verify no unseen batch action from a single-key shortcut. |
| UX-04 | Visual/accessibility refinement | A+B agreed file split | Precision Graphite proposal: neutral evaluation surround, readable density, stable chrome, real light/dark and contrast/focus checks. Reuse recovered prototype; do not mistake mockups for app QA. |
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
