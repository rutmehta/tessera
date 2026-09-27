# Tessera task board — Machine A coordinator

Updated: 2026-09-27 17:34 UTC

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
| UX-01 | Resume coherent Library / Photo Edit workspace and split inspector | A `develop_resume`; `codex/workspace-redesign`, `/Users/rutmehta/.codex/worktrees/workspace-redesign/tessera` | Concrete first-slice plan reviewed by coordinator; implementation assigned. Implement navigation/target clarity while preserving Library selection/filter/order/zoom and truthful rendered-copy Document boundary. Default provisional direction: Precision Graphite, retaining amber identity; historical palette approval is not claimed. Review changes, run targeted behavior/layout/accessibility checks and strict Swift gate. |
| ENG-31 | M5-31 first-process cold resident failure | A `b516_review`; existing `.worktrees/M5-31` | Diagnose B's exact `9f922bf` results: cold 125.855125 FAIL /43.970875/33.298750 ms; all warm pass. Stage profiling currently owns A’s heavy slot. Profile cause, fix with evidence, rerun unchanged <100 ms gate in fresh processes. Never discard failed samples. |
| COM-01 | Establish usable authenticated Machine A ↔ B communication | A `cross_machine_research` +root | Native UI inspection blocked; only local app host exposed. Bonjour MacBook SSH refused, B identity unverified. Mailbox integrated067261e,9tests pass; A heartbeat polls it. Handshake1c60b6fd-49d0-43e5-8f6d-81d0766f272f published on codex/coordination-a0c6479d; B receipt/heartbeat unconfirmed. Root polls/bootstrap; agent advancing EXP-45 while waiting. Avoid a second agent server owning active desktop chat. Implement durable deduped Git fallback where needed. No exposed unauthenticated listeners. |
| OPS-01 | Persistent autonomous coordinator | Root; current chat | Heartbeat `tessera-machine-a-coordinator` ACTIVE every5minutes, created2026-09-27. Check running work before duplicate launches. Read/advance board; stay quiet unless meaningful outcome/blocker. Local execution requires awake host/app and available service limits. |

## Ready / next

Current review checkpoint: UX-01 source and behavior tests are implemented but
have not compiled yet; review requires disarming all pointer tools on workspace
exit and verifying Library Loupe zoom restoration. ENG-31's new guard exposed
duplicate layer IDs in the inherited timing fixture. The proposed source reuse
must demonstrate benefit with valid distinct IDs before shipping; the original
cold failure remains open. EXP-45 is actively assigned, not waiting for B.

Build-slot handoff: UX-01 now owns the heavy slot; ENG-31 is preparing tests and
seed semantics source-only. Root prepared INT-45 in the managed worktree
`/Users/rutmehta/.codex/worktrees/export-integration/tessera`, branch
`codex/export-integration`: main d5366d7 plus a clean **uncommitted merge** of
wp/M2-45d69bcd3e. Candidate tree9fe7a245e2bc070e65fb3f4c0a4301f233ee473d;
formatting and source whitespace checks pass. Preserve the merge state. Full
five-package Rust/strict checks, licenses, workspace and FFI/Swift validation
wait for the build slot; no main merge yet.

EXP-45 diagnostic progress: independently sourced Skia ISO fixtures and the
preserved Tessera prototype now yield actual SDR/HDR pixels in a small ImageIO
probe. The agent is isolating decode options and verifying reconstruction before
proposing a production change. This is diagnostic evidence, not completed export
interoperability or a shipped feature.

| ID | Work | Owner / branch | Entry condition / acceptance |
| --- | --- | --- | --- |
| DEV-58 | Complete remaining Develop performance acceptance | A; `wp/M2-58` `196005e` product `5ff2679` | P01 actual input-to-present and P11 detail settle<=200ms /drag p95 regression<=10% remain unverified. Design nonactivating evidence path; report limitations honestly. Existing correctness/parity evidence preserved. |
| EXP-45 | Gain-map JPEG reference interoperability and implementation | A `cross_machine_research`; `wp/M2-45d` `69bcd3e` | Establish independently valid ISO HDR reference and actual decoded pixel control before changing encoder; distinguish Apple gain-map fixtures from ISO. Finish only with required interoperable output evidence. |
| INT-45 | Review/integrate validated DNG/PQ-HLG/native metadata slice | A root; `wp/M2-45d` | Current main merge-tree clean. Determine explicit partial-slice readiness, run resolved-tree gates; do not call absent gain-map complete. |
| UX-02 | Durable review destination and explicit action scope | A; after UX-01 | Existing modal review queue/order safety must be retained. Implement navigable review with named target/count, reversible actions and keyboard-safe scope; avoid implying unbuilt persistence. |
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
