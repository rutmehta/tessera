# Tessera task board — Machine A coordinator

Updated: 2026-09-27 21:03 UTC

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
| RES-02 | Cancellation inside expensive transform kernels | A Sol implementation in codex/transform-cancellation; B retains Document/FFI integration | Final transform gate66/0 after preserved RED2; product610ca001/evidenced946b460 pushed. Strict check and compositor bridge next; no end-to-end cancellation claim. No B run. |
| RES-03 | Repeated full-image filter-mask blending | A resource Sol; B report0926ae38 accepted | Count tiny multi-tile masked work/allocation and preserve separate masks sharing unmasked cache key. Existing hotspot remains outside RES01; source analysis next. |
| OPS-01 | Persistent autonomous coordinators | A root + B coordinator | A coordinator remains active. B paused its heartbeat and load tests following the user’s report of severe responsiveness/resource pressure; no B rebuild, benchmark, runner or heartbeat restart without B/user direction. B audits low-impact resource evidence. A local work stays separate. |

## Ready / next

A compiler slot: Luna final popover-dismissal focused rebuild/GUI; then transform strict/bridge tests. Resource final checks8focused/60library (Metal exercised,1ignored benchmark)/48integration/strict pass. Root merged/pushed RES01 atd2ac1226. No B builds or heartbeat restart.

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
Review+Masks is merged and pushed at main **b2757c7**, byte-identical product/tests
to fe0ff9e. Full455XCTest/one existing skip/zero failures plus5SwiftTesting passed.
Evidence: tools/orchestrate/wp/UX-02a/combined-integration-20260927/.
Luna's isolated a54044f GUI checks remain separately scoped: Library/Edit/back,
selected-fixture AutoEdit, Review/Edit/back, draft focus and single-entry accept.
Multiple betterSSD builds/catalogs are authorized; serialize desktop control.
Loupe9f62d87 focused4/0 passed, but actual GUI found Escape on Display info also switches Loupe to Grid. Root reproduced; Luna repairs keyboard ownership before merge. The isolated candidate remains available as RED evidence.

Gain-map CI supplemental and CLI/FFI/MCP targeted checks passed after preserved
fixture repairs. Original ImageIO4pass/1fail remains unresolved. Evidence-only
c716460 is pushed on codex/gainmap-restoration; dirty product source remains intact.

RES-01 bounded investigation9a28ab84 completed with source/evidence above; this
closes the requested investigation, not the resource incident. New result
fbe21798 was published and SSH queue01a0e4a8-4970-7301-90c4-bd96dd3adb83 accepted;
B completed receipt confirmed with source review673e2a7 and result0926ae38. A accepted residual mask work. Source-only frontend timer requestbac04a31 queued as01a0e4ad-008a-7f31-a8ec-f53e64afee8a; new request receipt still pending. No B workload authorized.
Loupe keyboard correction passed14 focused tests, but GUI Escape left popover open. Local /tmp disposable app-support let same binary start; external-volume mkdirat hang preserved separately. Explicit popover onExitCommand fix is now under test. Both GUI failures and earlier binaries preserved; no Loupe merge yet.

B’s original coordinator received queued messages and completed the Git bootstrap.
Its previously observed scheduled heartbeat is now paused by B. B paused its heartbeat and load tests for the user-requested resource audit;
A must not launch B builds/benchmarks or resume that coordinator loop. The dirty
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
| UX-02 | Navigable Review and resume persistence | A root; Review workspace merged b2757c7 | Ownership/save barriers and mutation exclusion DONE:23regressions included in full436+5 gate. UX02a source4fcb802 passed43focused tests; 34captures include real ready pixels; recovered2-test status/ready gate passed. Checkpoint a54044f and isolated hands-on checks passed; fe0ff9e full455+5 passed and merged mainb2757c7. UX02b relaunch persistence still pending. |
| UX-03 | Selective batch editing and source/target clarity | A; after UX-01/02 contract | Preserve source/target snapshots and settings selection; define undo/review semantics, verify no unseen batch action from a single-key shortcut. |
| UX-04 | Visual/accessibility refinement | A b516_review implementation; B agreed split | Masks slice041778b passed populated 288/380-point light/dark checks and ThemeLint (2/2), with root visual review; merged mainb2757c7 after full455+5 gate. Loupe9f62d87 focused4/0 passed; GUI Escape leaked to Grid, now held for fix/retest. No Document/global-theme edits. |
| UX-05 | True live-RAW/layer continuity | A engine+B document contract | Separate substantial dependency: current transition is rendered copy. Define graph/persistence/version contract and release scenario before making live-raw claims. |

## Machine B / integration dependencies

Last peer note read: `673e2a7`. B's resource hold supersedes its older queue.
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
| B5-15 | Export App Nap fix / performance | B published `aec242f`; export-only improvement does not clear main-thread maxima35.93/58.00ms vs8ms, AppKit layout exception, P19 or memory gates. B owns app fixes; A engine optimization only with explicit evidence/split. |
| B5-12b | Transform acceptance | Interrupted for resource hold; not accepted. No rerun. |
| B5-13 | Liquify / move / extend | B owns preserved work and current acceptance. |
| B5-14 | Post-M2-57 performance measurements | B queued; no inferred pass. |
| UI-VERIFY | Isolated app interaction and uncovered-canvas checks | User authorizes activating isolated betterSSD builds with disposable catalogs. Luna owns shared desktop control for Loupe after its gate; actual presentation/performance checks remain separate from functional GUI checks. |

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
