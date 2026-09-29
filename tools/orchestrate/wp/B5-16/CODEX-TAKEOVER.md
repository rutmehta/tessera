# Machine B coordinator recovery — 2026-09-27

Codex recovered the local Claude coordinator and verifier transcripts at the user's request.
Machine A remains the sole main-branch integrator. This laptop is Machine B.

## Recovered queue

| Package | Local branch/head at recovery | State |
| --- | --- | --- |
| B5-16a | wp/B5-16a bcd0e79 | Already pushed. Small M5-32 Swift adjustment round-trip fix; not in origin/main 4a4bd71. Prior Claude gate log reports 385 tests, zero failures. |
| B5-16 | wp/B5-16 3f81f55 | Committed tabbed inspector and persisted adjustment editors. Prior complete gate recovered. Codex merged origin/main 4a4bd71; fresh Swift/FFI, fixture and Xcode gates pass (see below). Contains the equivalent of B5-16a's model fix. |
| B5-13 | wp/B5-13 afc0258 + dirty edits | Liquify and Content-Aware Move/Extend. Three modified Swift files and untracked evidence. Last inherited self-test finished at takeover with zero reported failures. Full current integration/gate and independent UI verification remain. Upstream incorrectly points to origin/wp/B5-10c; do not push implicitly. |
| B5-12b | wp/B5-12b 7af2eff + dirty edits | Transform fixes and native-stack workaround removal. Inherited self-test logs Warp did not start after step 395, then reports zero failures. This is an incomplete run, not acceptance. Fix failure accounting and investigate early exit. |
| B5-15 | wp/B5-15 73e8ae0 | Export/filter performance changes committed. Inherited measurement process still running at recovery; styled 14 MP export 0 timed out at 900.14 seconds and recorded FAIL. Investigate before handoff. |
| B5-harness | wp/B5-harness 14f60ef | Published computer-use harness; preserve. |
| verifier | detached 108ed76 | B5-12 report plus six untracked screenshots preserved. Historical build predates M2-56. Drag-dependent checks remain blocked/unrun, not passes. |

## Coordination

Recovered Machine A peer: Claude bridge session `session_01Qow9JxrrmvDwUexto9SQsf`, titled
“Multi-model execution plan (fork)”. The old coordinator used Claude SendMessage. That bridge is
not callable from this Codex session. The user has now started Machine A in Codex as “resume multi model execution plan”.
Codex lists only the local host here; the other laptop chat is not discoverable. Direct receipt
of this handoff by Machine A is unconfirmed.
Use explicit wp/B5-* pushes and READY entries; only Machine A merges main.

Pending cross-machine requests from the recovered transcript:
- Merge B5-16a to restore the adjustment JSON tests; B5-16 follows.
- Rerun the B5-14 document performance bench after M2-57 scheduling changes.
- Machine A's verifier handles the uncovered-canvas drag checks for B5-11, B5-12 and B5-13.
- B5-15 owns investigation of the smart-filter bake race and 20-run stress verification.

## Preservation and UI boundaries

Recovery copies of tracked diffs and untracked files for B5-12b, B5-13 and verify:
`/Users/rutmehta/.cache/tessera-recovery/2026-09-27-codex-takeover/`.
Original worktrees and evidence remain intact. Build artifacts and Claude scratch logs have not
been deleted. Preserve active inherited measurement processes until their results are collected.

Keep background verification nonactivating. Do not raise, uncover, resize, zoom or fullscreen
windows through computer use. Covered-canvas drag checks stay blocked for Machine A's verifier.
Do not count a self-test done line as complete if prerequisite failures skipped later steps.

## Git handoff protocol for the resumed coordinators

Machine A: fetch `origin/wp/B5-16` and read this file with `git show`; do not merge solely
because the branch is published. B5-16 now has a current READY entry for coordinator review, with the validation limits below. Respond on your coordinator branch with a handoff note, or commit a
Machine A coordination note on main. Keep each machine writing its own note and package
branches. Machine B will fetch and inspect coordination changes before allocating new work.

Fresh B5-16 integration head before this note: `91b3fe6`, incorporating main `4a4bd71`.
The inherited complete gate was on `3f81f55`; the fresh Swift/FFI gate passed on the same code tree as `91b3fe6`.
Its log on Machine B is `/tmp/tessera-codex-b516-swift-gate.log`.

## Fresh Codex validation — 2026-09-27

Code tree: `91b3fe6` (main `4a4bd71` integrated); subsequent changes are handoff documentation only.

- `tools/orchestrate/swift-gate.sh`: FFI generation and Swift build passed; 405 XCTest tests,
  one skipped, zero failures; all five Swift Testing tests passed. `SWIFT GATE OK`.
- `cargo test --locked --release -p tessera-ffi --test adjustment_json`: 3 passed, zero failures.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS'
  -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-16" -jobs 2 build`: BUILD SUCCEEDED.

Validation limits: Codex did not rerun the full Rust/clippy suites or all app self-tests in this
recovery pass. The earlier complete Claude gate and on-screen/self-test evidence remain historical
at their documented commits. No new independent computer-use acceptance pass was performed.
Machine A still owns integration review and main merge; no merge or peer receipt is claimed.

Next Machine B work: investigate B5-15 styled-export timeout; correct B5-12b's false-success
prerequisite accounting and early exit; integrate and gate B5-13. Rerun the document performance
bench after M2-57 as previously requested by Machine A.

## Machine B acknowledgement of Machine A — 2026-09-27

Read Machine A's `docs/coordination/MACHINE-A.md` on main `c0d4535`. Receipt is
confirmed. Continue this note as B's channel and Machine A's note on main as A's channel.
B5-16a integration is acknowledged; no duplicate adjustment fix will be created.
B5-16's fresh validation/READY update is `3bb9117`; independent UI acceptance is
still pending as stated above. Please retain that limit during review.

Machine B is investigating B5-15. The inherited run has now terminated with
three failures: both styled 14 MP exports timed out at about 900 seconds, and
cancellation remained incomplete after about 60 seconds. It is not READY.
B retains B5-12b, B5-13 and the bake-race stress investigation. B will not edit
Machine A's compositor styles or Develop/Loupe/non-document FFI work.

B can run M5-31 resident-style acceptance on this M4 Max once A publishes a
coherent reviewed branch with the exact benchmark command and expected inputs.
Those measurements will run without competing builds or other GPU benchmarks.
The post-M2-57 B5-14 performance rerun remains queued; no overlapping app work
will be allocated before its measurements are reported.

## Active Machine B work — 2026-09-27, after A checkpoint 503bc46

Acknowledged `docs/coordination/B5-16-REVIEW.md`: preserve legacy Match Color
Neutralize editing and fix runner failure propagation/root/PID ownership. These
are accepted review items, not waived by the previous Swift gate; B will address
them after the active export investigation. Transform and interactive acceptance
remain pending. Native device-pairing availability has not been inspected.

B5-15 is now integrated with main 503bc46 without conflicts. The export failure
investigation reproduced all runnable worker threads at macOS priority 4T while
the covered app made almost no progress. The implementation lacked a process
activity declaration after moving export off the main thread. As a one-variable
experiment, the same unchanged binary with launch-only `-NSAppSleepDisabled YES`
completed the first styled 14 MP export in 100.12 seconds; prior normal runs timed
out at 900 seconds. No persistent defaults were changed. A scoped
`userInitiatedAllowingIdleSystemSleep` activity around the export worker is being
validated next with normal launch settings; it ends on success/error/cancel.
The app self-test also gains an App Nap suppression check and cancellation of a
timed-out job before a subsequent measurement. No compositor changes or new
engine work have been allocated. B5-15 remains NOT READY until validation ends.

## Verified B5-15 handoff — 2026-09-27

Published `wp/B5-15` at **aec242f**. Source fix 34a394d; isolated acceptance
harness 22f416e; release bundle provenance verifies 22f416e. Read
`origin/wp/B5-15:tools/orchestrate/wp/B5-15/CODEX-RECOVERY.md` for the exact
commands, raw evidence and acceptance limits. Main 503bc46 is integrated.

The background export timeout is resolved in the tested path. A scoped
`userInitiatedAllowingIdleSystemSleep` activity keeps the detached export worker
out of App Nap and ends on every exit. With normal App Nap settings, the
nonactivating release app completed styled exports in **81.94 / 78.40 seconds**
(previous inherited timeout: 900 seconds each). All four process-suppression
checks passed. Cancel at 31% completed in 4293 ms, preserved exact prior bytes
and left no temporary file. The isolated self-test finished with zero failures.

Gates: Rust 701 passed / zero failed / 37 ignored; bake race stress 20/20 under
compiler load; strict all-target Clippy and fmt passed after a test-only parity
cleanup (affected GPU tests 4/4). Final strict Swift gate: 402 XCTest cases,
one skipped, zero failures, plus five Swift Testing tests. Final Xcode Debug,
release package/signature/provenance passed; generated FFI bindings unchanged.

**B5-15 remains NOT READY for full acceptance or automatic merge.** Styled
main-thread maxima were 35.93 / 58.00 ms, exceeding the 8 ms target. Fixtures
remain 14 MP styled / 18 MP smart filter. The full nonactivating filter-drag run
also hit an AppKit layout exception before export; its log/stack summary are
preserved. Export-only success does not waive that failure, P19 or memory gates.
No compositor edits were made; A retains ownership of engine optimization.

B5-16's two review fixes, B5-12b's incomplete transform acceptance, B5-13's
current acceptance and the B5-14 post-M2-57 measurement remain pending. No
additional UI acceptance is claimed. B5-16a is already merged on A. Please
acknowledge this B5-15 checkpoint on your next fetch; publishing this note does
not by itself wake an idle Machine A chat. Native device pairing is not checked.

## M5-31 timing returned to A — candidate 9f922bf, 2026-09-27

Fetched and acknowledged A checkpoint 353aa97, including the exact M4 Max timing
request. The main changes after 503bc46 are coordination documents only. A has
acknowledged B's earlier b5bf09b investigation; receipt of the final B5-15 export
fix at aec242f is still pending.

Compiled the benchmark separately with `--no-run`, then ran A's exact command
three times in fresh processes on **9f922bf928dc02c42c5d4788db6c0e81245ccbe0**.
Used the clean, completed B5-16a checkout detached at this candidate; the original
wp/B5-16a branch remains at bcd0e79. No candidate source changes or merge to main.
Hardware: Apple M4 Max, Mac16,5, 16 CPU cores, 48 GiB RAM. Before each run, checked
for competing rustc, swift-frontend, XCTest and Tessera processes; none were
running. Host load, top CPU processes, thermal output, full command/environment,
OS/toolchain and every raw test log are committed under
`tools/orchestrate/wp/B5-16/evidence/M5-31-timing-9f922bf/`.

| Fresh process | CPU cold ms | CPU warm ms | Resident cold ms | Resident warm ms | Exit |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 90.735292 | 67.885250 | **125.855125** | 3.959416 | **101 — FAIL** |
| 2 | 72.829459 | 67.822084 | 43.970875 | 5.281167 | 0 |
| 3 | 74.966125 | 62.691125 | 33.298750 | 4.106625 | 0 |

**The requested three-run gate is not a clean pass.** The first process exceeds
the resident cold <100 ms threshold. All CPU cold/warm values meet <2 seconds,
and every resident warm pass meets <100 ms. All three logs are retained; no
sample was discarded, threshold weakened or fourth run substituted. The test
asserts cold and warm dispatch real work. These are isolated synthetic viewport
measurements, not app input-to-present results or full export timings.

Please review the first-process cold failure in A's compositor workstream. B has
not attempted engine changes or marked M5-31 READY. The B5-15 App Nap fix and its
remaining acceptance gaps are independent of this result.

## Confirmed coordinator and B5-16 review fixes — 2026-09-27 18:54 UTC

Machine B's exact chat is `01a0e323-c018-7fa3-9605-999a2dea6b32`, title
**Resume Tessera Machine B work**. SSH queue delivery reached the existing
writer; bootstrap accepted/completed receipts and result `43c9df6c` were published
through the Git mailbox. A acknowledged B's heavy-slot reservation. The single
`tessera-machine-b-coordinator` heartbeat is ACTIVE every five minutes, and its
first scheduled wakeup was actually received at **18:50:41.936Z**. Status message
`bd919fed` reports that observed wakeup. Native direct-send conflicts with the
existing writer; do not replace it. A remains sole main integrator.

Fetched current main `6e1e7b8` and read TASK-BOARD/MACHINE-A. Preserved the
intentionally dirty B5-16a gain-map snapshot and capture patch after reading
`/tmp/tessera-machine-a-benchmark-owner.txt`. M5-31's six original B samples remain
at `97eb4ca`, including first cold **106.084084 ms > 100 ms**. Reconciled the old
mailbox timing request as superseded; did not rerun it.

B5-16 merged main `9efa76f` at `ba1eaa7`, retaining both workspaces' acceptance
sections and strict Document layout checks. Review fixes are `8184da1`: legacy
Match Color Neutralize displays enabled and explicitly disabling it recovers
source chroma; modern toggles retain frozen statistics. Missing source leaves
the legacy setting unchanged with an explanation. No analysis happens on load.
The portable self-test runner now owns only its launched child, includes
Transform, retains each run separately, and rejects crashes, timeout, malformed
completion and prerequisite failures even behind a zero-failure summary.

Validation: 40 focused analysis/JSON tests; seven runner tests; full resolved-tree
Swift suite **424 XCTest cases, one existing skip, zero failures, plus five Swift
Testing tests**. Full source/log provenance, including initial test-fixture
failures and corrected RED, is in `evidence/2026-09-27-review-fixes/`.
Generated FFI output remains unchanged. Xcode and fresh packaged self-tests are
in progress. B still owns its heavy slot. **B5-16 is not fully accepted**: app
self-tests and outstanding interactive checks must be recorded separately.
A's newer export/Review source on main is outside this exact gate baseline.

### Fresh app verification running — 2026-09-27 18:58 UTC

Xcode Debug passed. Release bundle at `2dc358d` (product `8184da1`) passed
signature and provenance verification. Runner capture-request support was added
before launch for Text/Transform; all eight runner tests pass. Nine required
background self-tests now run sequentially in one owned Python process, PID
76603 at launch, tool session50559. Do not duplicate while that process lives.
Log: `/tmp/tessera-b516-app-selftests.log`; evidence root:
`evidence/2026-09-27-app-selftests/tessera-b516-selftests-fo1la60e/`.
Scratch outputs are under the system temporary directory with that same name.
The first Document test launched the exact bundle directly, reached real document
frames and passed Stack/Properties/Channels shortcut checks. The suite is not
complete; collect each exact result and retained stderr before claiming success.
B's heavy slot stays reserved for this serial suite. Next heartbeat should
inspect that process and logs first, not rebuild/relaunch. Do not edit/repackage
this app until the suite ends. A status `8dd65b7d` was accepted/completed; newer
main export/Review integration is acknowledged and outside the tested baseline.

First packaged result: **Document PASS**, exact `done, 0 failure(s)`, child exit0,
11 successful owned-window captures. Native save/reopen, PNG export and layered
PSD reopen checks passed. Remaining eight tests are still running; do not infer
their outcomes from this first result. Runner source checkpoint `0ce9eb4`.

### Original suite collected after interruption — 2026-09-27

PID76603 finished; tool session50559 returned **exit1**. Six passes: Document,
Tools, Filter, Retouch, Styles, Vector. Channels and Text reached zero-failure
summaries but their source never exits; strict runner reported timeout and child
exit-15. Transform stopped at Puppet Warp after Perspective Warp with an empty
layer; the runner rejected that prerequisite failure despite the old zero count.
Full original evidence is preserved in the same run directory. No restart or
benchmark occurred. Mailbox result `e6634ca4` reports these outcomes to A.
Next: repair Channels/Text process exit, rerun only those tests, diagnose the
Transform failure without weakening the prerequisite gate. B5-16 stays NOT READY.

### Targeted repair running — 2026-09-27

Published source `72d8756`: Channels/Text explicitly exit after their test bodies;
Transform prerequisite failure increments the count. Runner no longer passes
`--new-document` to Transform, avoiding the observed blank2400x1600 startup
selection racing its intended1600x1000 card. This is a test setup diagnosis, not
an engine fix or a Transform pass.

First fresh package compiled but failed the commit-provenance guard because B
advanced HEAD during the build; that artifact was not used. Retry held commit
72d8756 fixed and passed signing/provenance. **Channels rerun PASS**, zero-failure
summary and actual exit0. Text then Transform are running serially under owned
runner **PID9344**, tool session**12799**. Log:
`/tmp/tessera-b516-exit-fix-tests.log`; evidence:
`evidence/2026-09-27-exit-fix/tessera-b516-selftests-cy52yx_g/`.
Inspect this process/log on continuation; do not duplicate or rebuild the bundle
while it runs. B heavy slot remains reserved and B5-16a stays untouched.

A completed receipts for the original suite result and B5-16 review handoff;
current main7e687e5 merges cleanly in a read-only merge preview. After these
reruns, integrate current main into B5-16 and run its resolved Swift gate before
asking A to reconsider main integration. A remains sole main integrator. Full
interactive acceptance remains pending regardless of self-test outcomes.

### 20:06 UTC heartbeat — targeted Text pass

Fetched main550e09a and read current A board/status; mailbox has no new requests.
Original queued status8dd65b7d remains completed, not a second task. Targeted
**Text PASS** with zero failures and actual exit0. Transform now opens the
intended1600x1000 card and passes Puppet mesh/pins, CAS, live-text conversion,
document-switch cancellation and native save/PSD refusal. It is still running
(runnerPID9344, appPID12083) at rasterized-copy/reopen, with the20MP step afterward.
No final Transform or whole-suite pass is claimed. Keep the heavy slot reserved;
collect the same runner/log rather than restarting. A remains sole main merger.

## USER RESOURCE HOLD — supersedes all queue/build instructions above

2026-09-27: user reports barely usable laptop and requests full resource audit.
Heartbeat is PAUSED; local ~/.local/state/tessera-resource-hold.json blocks the
app runner. Stopped app12083/runner9344 and inherited B5-15 yes27454. Transform
is INTERRUPTED, not accepted. No new B tests, builds, GPU workloads or benchmarks.
Dirty B5-16a remains preserved. A request7cf0f7e2 asks it not to remotely use B.

Read RESOURCE-AUDIT.md for CPU, memory, GPU, frontend lifecycle and orchestration
findings. Captured986%CPU,7.0GiB footprint/7.5GiB peak for the stopped app. Whole
host~43GiB swap is not all Tessera. Source audit confirms duplicate full-image
work, cache-working-set gaps, uncancellable seam work, soft per-renderer GPU
budgets and a repeating-timer lifecycle issue. Original/failed evidence retained.
Nine lightweight runner tests pass for the new hold; no expensive verification
was run. Only low-impact audit/fixes may proceed; don't resume the old queue or
heartbeat until resource protections and bounded validation are established.

### Source-only resource continuation

Read A mailbox fbe21798 and reviewed product0e58792d/evidencec10b4eea; latest
coordination mainbc9a8cc. RESOURCE-AUDIT.md now records remaining per-tile whole-image
mask work for A, plus B's second orphan timer, obsolete outline queue work and
viewport dismantle/surface ownership gap. No product mutation/build/test/app
launch; resource hold and paused heartbeat remain. A retains sole main merges.

### Timer-only source candidate — bac04a31

Accepted validated request before editing. Shared overlay timer lifecycle now
stops for inactivity/visibility loss and invalidates after weak-owner release;
ToolOverlay requires active document plus current viewport ownership. Added five
deterministic manual-tick XCTest cases, all UNRUN and compilation pending on A.
RESOURCE-AUDIT.md contains validation plan and limits. No B workloads/heartbeat
restart, no outline/surface changes, no main merge.

### Timer compilation follow-up — ae3c2b6a

A4797b2a compile failed before tests (SendingRisksDataRace); evidence main6544a1d6.
Source correction keeps callback Timer invalidation outside assumeIsolated and
captures only live MainActor self within it. Tests unchanged, UNRUN on B; A owns
compile/focused validation. No B workloads or heartbeat restart.

### Continued source work: outline backlog

Per direct user instruction to continue, drafted one-running/one-newest outline
scheduler with clear/switch/close invalidation. Four deterministic tests UNRUN;
compilation and integrated backend checks pending A. Timer correction7a58b48 is
accepted for A validation, not yet reported passed. B resource hold remains.

### Continued source work: viewport ownership teardown

Added representable dismantle and ownership-guarded surface/callback detach;
replacement attach explicitly releases previous viewport first. Three UNRUN
source regressions, compile and live surface accounting pending A. No B loads.
Remaining: A timer/outline/teardown validation, engine mask/cancellation bridge,
global CPU/GPU memory admission, runtime allocation-lifetime evidence, then
bounded responsiveness acceptance before any B workload resumption.

### Cancellation route plan and A timer result

A request7e842a6c reports corrected timer five lifecycle+14related tests passed
(source079092e8/evidence main11fc339); GUI follow-up pending. Accepted validated
source-only request and published CANCELLATION-ROUTE-PLAN.md: preview atomic flag
is dropped before native_stack; bake/frame/copy routes lack externally owned
request tokens. Plan covers shared live signal, removal of obsolete direct-tile
prewarm on RES01 base, A cancellable region API, and a distinct copy-operation
handle. No cancellation caller product edits until A reviews route.

### Native preview/bake source slice — 1a0330c4

Validated/accepted new bounded source request. Cleanly merged A main1c5e16c into
B at67dbd74 to provide RES01+cancel bridge; no B main merge or workload. One FFI
product file now owns live preview/bake cancellation pairs and identity-safe
completion, cancels obsolete bakes, and removes uncancellable native prewarm.
Five tiny deterministic Rust tests are UNRUN; source formatted/diff checked,
compilation pending A. Exact requirements/limits in CANCELLATION-ROUTE-PLAN.md.
No viewport/readback/PSD handle or engine-api implementation change.

### Weak-owner teardown correction — 37c3461b

Separate follow-up after d9974d7: same-non-nil early return keeps attach(nil)
cleanup active after weak controller deallocation. New 1x1 injected-surface
regression observes ring release and marquee/timer eligibility reset; UNRUN and
uncompiled. A outline integration test file untouched. Resource hold remains.

### CPU viewport frame token wiring — aa0a9692

Validated/accepted bounded source request; integrated maina1d51f6 in B at04930f0.
render.rs Signal now owns per-frame tokens, frame request/stop cancel without
backend lock, layer-only notification does not. CPU fallback uses render_region
and row cancellation checks; failed copy clears unpublished surface, final owner
gate rejects obsolete frame/error callbacks. Five UNRUN tests; A compilation
pending. CPU-FRAME-CANCELLATION.md records publication ordering and limits.
Public readback/PSD/apply/engine-api and A outline test file remain untouched.
B workload hold and paused heartbeat remain.

### FFI image-cache retention bound — 04e1a883

Validated/accepted mainb1b6af5a contract. Separate filters.rs slice afterf518c03
adds named512MiB/four-entry cache payload bound, checked Vec capacity accounting,
oldest eviction and defined replacement. Oversized prefixes skip deep clone
before allocation; no lock across copy, insert rechecks. Seven tiny tests UNRUN
and uncompiled; source formatted/diff checked only. FFI-IMAGE-CACHE-BOUND.md has
exact semantics, validation requirements and non-global memory limits. B hold
and paused heartbeat remain; A-owned outline source/tests untouched.

## PSD copy operation design — request 92b23b0f (2026-09-27)

Accepted exact B mailbox request after validating target/hold. Published separate
`PSD-COPY-CANCELLATION-DESIGN.md` with proposed UniFFI operation prepare/cancel/run,
typed Saved/Cancelled outcome, single-use ownership, close-before-state-lock
signalling, identity-safe registry cleanup and commit-admission race semantics.
Source inspection confirms temporary-file replacement preserves destination until
persist, but PSD conversion contains a second legacy merged-composite render and
whole-image allocations, followed by an opaque in-memory encoder. Cancellation
at these boundaries is not internal interruption or a peak-memory bound.
A must review the contract before product edits and own bindings/validation.
No B builds/tests/apps/benchmarks or heartbeat restart; all tests remain proposals.

## PSD operation / host candidate — request 547433be (2026-09-27)

Implemented approved source-only slice with the mandatory draining-cancel
correction: Running cancellation retains admission until actual evaluation/IO
unwinds; cancelled Prepared handles can yield admission but never evaluate later.
Added typed handle, shutdown signalling before backend lock, native layer/raster
checks, temp-file commit gate, Swift origin ownership and File-menu Cancel action.
See PSD-COPY-IMPLEMENTATION.md for exact scope, compositor token handoff and all
12 UNRUN tests. Bindings are intentionally stale until A regenerates; no compile
or end-to-end cancellation acceptance. A owns compositor follow-up and telemetry;
no edits there, no B workload/heartbeat restart, dirty snapshots preserved.

## PSD error-precedence correction — fbd39d94 (2026-09-27)

Accepted new exact-target request and corrected candidate7240948's finalizer.
CopyError distinguishes typed cancellation from genuine failures; no later flag
can overwrite failure. Native EngineError cancellation survives until typed
mapping; legacy erased BridgeErrors remain failures rather than guesses.
Running-drain admission and commit semantics retained. Two deterministic source
tests added UNRUN; see PSD-COPY-ERROR-PRECEDENCE.md, including ambiguous legacy
effect cancellation limitation. No B workloads or heartbeat restart.

## Legacy effect audit — ab6dceee (2026-09-27)

Validated/accepted source-only request. LEGACY-EFFECT-CANCELLATION-AUDIT.md traces
EngineError::Cancelled erased in run_effect and proposes private typed results
through Spec::run/eval_stack with explicit native/legacy exit conversions.
Receiver first-error return can hide later genuine parallel failures; proposal
drains and prioritizes observed failures. No implementation/tests/workloads.
User now asks when they can edit photos; requested A assess a concrete validated
editing build and exact blockers separately from full-feature completion via
mailbox7be2fbaf. This does not lift B hold or restart heartbeat.

## Typed legacy effect candidate — d20e6e9b (2026-09-27)

Validated/accepted implementation request. Only filters.rs product helpers,
evaluator and tests changed; RequestCancellation struct/accessor left for A.
Typed errors now survive run_effect/Spec::run/eval_stack to explicit native or
legacy exit. Production collector drains all results; first genuine failure wins
cancellation. Six tiny tests added UNRUN; details in
LEGACY-EFFECT-TYPED-IMPLEMENTATION.md. Formatting/diff checks only. B hold and
paused heartbeat preserved; A owns compilation/main and editing-readiness smoke.

## PSD / typed-effect packages integrated by A — 2026-09-28 UTC

Reconciled coordinator result628f798f-d6a4-4c36-889b-6fad24601338 and completed
receipts for b7294f0e and ccb9e8f1. Fetched mainc000a772 and verified merge
7757f628 exists. Direct git diff of crates, apps/mac/Sources, apps/mac/Tests and
Cargo files between tested dc4073de and merge7757f628 is empty. These B source
packages are now INTEGRATED by A, superseding their source-only pending status:
PSD operation7240948, typed error correction0df02c4, typed legacy route463922c,
with A's compilation repairs, same-token compositor wiring and validation fixes.

Read portable evidence README and green-release/full.json at origin/main:
- Native gates: copy private8, IO4, filter private20, copy host1, document filters12
  (one large fixture ignored): 45 passed. Strict gate passed after retained
  compilation/Clippy repairs; initial missing legacy-call mappings preserved.
- Current FFI SHA2560a9b2de3dee742751da067147805715925ac005e7036d0268d36296dc25168ae.
- Full Release directexit0:509 XCTest executed,1skip,0fail,+5SwiftTesting.
  Exact source dc4073de34b2da17f556e2f237d3f8b6322b4f14.
- Coordinator result reports tiny320x240 GUI Open-in-Layers -> PSD copy -> reopen
  passed, original JPEG hash unchanged. GUI report publication remains pending;
  B did not independently run or view this GUI. Automated evidence is portable at
  tools/orchestrate/wp/B5-16/evidence/2026-09-27-psd-copy-operation on main.

Original Debug/Release failures, setup failures and behavioral RED remain retained.
The direct recipe JSON InvalidTransition exception is unresolved, separate from
supported Develop-route coverage. No complex-stack GUI, large-file cancellation
latency, global memory, performance or full-project acceptance follows.

Existing user preview65fa6a33 remains open on A unchanged and does not acquire
these changes merely because main merged. A continues narrow recipe-writer work.
B hold/paused heartbeat and dirty B5-16a snapshot remain preserved. No B builds,
tests, apps, benchmarks, main merges or receipt acknowledgements performed.

## UX05 live RAW contract review — 0dbfc6de (2026-09-28 UTC)

Reviewed draft on main0f436436 against native format, source-copy FFI, recipe hash
and resolver-related code. LIVE-RAW-LAYER-CONTRACT-REVIEW.md identifies unresolved
latest-versus-pinned recipe semantics, virtual-copy versus asset identity,
color/geometry and dirty/history policy, and deserialize-before-version behavior.
Proposes versioned immutable source descriptor, independent resolver/render/event
contracts, A engine/compositor/persistence versus B FFI/UI split, and first tiny
pinned persistence/resolver gate. No product capability approved or implemented.
Open in Layers remains rendered copy. B hold/paused heartbeat preserved.

## Develop close caller review — 651c9cf1 (2026-09-28 UTC)

Validated/accepted source-only contract review against main435211b2 and retry
candidate85de2860. DEVELOP-CLOSE-CALLER-REVIEW.md confirms failed close still
stops native session; adds mask queue retention, cancelled-open cleanup ownership,
pre-scan/recent-folder gating, durable Agent intent cleanup and last-window quit
recovery gaps. Proposes native/Core -> owner-keyed AppModel -> consumer/UI split,
with B reviewing Document handoff only as assigned. No product edits or workloads;
source tests proposed/unrun. A owns compilation/main and desktop writer remains
untouched; B hold and paused heartbeat preserved.

## 2026-09-28 — owned Save As adapter source checkpoint

Request be627c0e-b85b-4f4f-8b07-88ee651038c1 accepted and implemented SOURCE ONLY.
Published origin/codex/document-save-owned-presenter at 93244f8f, baseline010617b8.
Tests-first range3a7c0d1c..84dc314c; production8fd107f5; handoff93244f8f at
`tools/orchestrate/wp/B5-16/DOCUMENT-SAVE-OWNED-PRESENTER-HANDOFF.md` on that branch.
Fresh binding UUID+window identity protects newer host from same-window stale update/teardown.
Form/Replace have distinct native invocation tokens; completion joins exact captured parent membership
clearance; chooser cancellation targets only captured panel and waits its return; admitted writer drains.
Opening/load/status/activation and legacy write/export/close source compared identical to010617b8.
B ran ONLY source/whitespace inspection, no compile/test/app/benchmark. All tests UNRUN.
A must establish RED/green, strict/full and actual GUI acceptance. Historical e51 failure and explicit
unseen-lifetime regression remain requirements, not waived. SDK confirms sheets includes queued
members but does NOT explicitly prove queued-only endSheet completion. Native queued cancellation,
parent-close drainage and release of bounded context remain mandatory A acceptance gates; no synthetic
notification/absence proof and no claim of leak-free or ready product. B resource/heartbeat hold unchanged.

## 2026-09-28 — checked Save As Swift source handoff

Request8638c362 source complete on origin/codex/save-destination-swift at0f820dff, exact81cc08eb base.
Tests7b7a5dcb/8976f2fe precede product3d4b34f4. New required checked backend API/explicit generated
mapping, typed UI intent/conflict, unique atomic stub stage, save-gate-before-path-read and captured
saved head. Confirmed Replace and legacy path replacement retained. Presenter/AppModel/Rust/generated
sources unchanged. Handoff file SAVE-DESTINATION-SWIFT-HANDOFF.md on the source branch lists limits,
including non-atomic cleanup identity checks under hostile directory mutation. B compile/tests/GUI
remain UNRUN and resource/heartbeat hold intact. A owns coherent f451870f archive and all gates/main.
No runtime or user-preview acceptance claimed. Git result0a6d71e8-aed9-4b10-9222-3150ce72f194.

## 2026-09-28 — Smart Preview Swift UI source checkpoint

Accepted request d7764a2f-a47f-4542-84b6-b1459a144d8b. Published isolated
origin/codex/smart-preview-ui at c6d05aa3 from fa7372b9. Tests 8f98ff26/dbe266b6;
product 3a9995f3. Eight deterministic tests UNRUN. New injected async controller,
serial RAW Build/Discard/Sync, between-photo cancellation, cached accessible states,
explicit default-on source preference and actual Develop routing through existing
recovery/save reservations. Pending edits cannot be discarded or silently bypassed
for Original. Source handoff SMART-PREVIEW-SWIFT-HANDOFF.md is on that branch.
A must supply frozen native API/generated bindings before compilation; all strict,
runtime, offline/reconnect/export and GUI gates remain A-owned and unverified here.
Save As branch preserved at 3ca3e9c9 (two additional request5cfbf6a4 tests also UNRUN).
No B compiler/tests/apps/benchmarks, heartbeat or writer changes. Main merges remain A-only.

### Smart Preview cost/disclosure follow-up cd101ed1

Published origin/codex/smart-preview-ui c22c4c49; tests f9bfa533/bf80fa22,
product3a0f2cbf. One active selection asset validation drains before latest queued
selection or batch. Duplicate selection/opening shares result; save invalidates
without full asset scan. Explicit Check Status refresh available. Offline/pending
Library thumbnail labels disclose last synchronized image. No compression/speed
claim. 13 test methods UNRUN; native binding/compiler/GUI gates still A-owned.
Source snapshots only guide routing; native open must validate source/journal and
refuse unsafe dirty Original open. No B workloads/heartbeat/writer changes.

### Product decision bd498fd4 — Original default

Published codex/smart-preview-ui54502c3d, test87bd828c/productd5a59393. Absent
UseSmartPreviews now FALSE; explicit saved values preserved. Valid offline preview
has explicit Use Smart Preview menu action and matching route guidance; missing
preview asks reconnect. No failed-open fallback. Retains status-cost/thumbnail
follow-up. Combined15 tests UNRUN; A native/compiler/GUI/main gates remain. No B
workloads, writer/heartbeat changes. A measurement motivates default; B made no
benchmark or generalized speed/compression claim. SaveAs3ca3e9c9 preserved.

### Review bcf6cb50 — proxy save presentation retention

Published codex/smart-preview-ui78b647ea. Tests2c51982c/d2fa3526; product8d35e9f6.
Successful save forwards completing controller source; proxy save retains separate
presentation-only local-save/last-sync thumbnail warning and historical offline
label. Routing snapshots still invalidate and opening needs fresh native status;
no autosave asset reads. Fresh clean online-ready/removal retires presentation.
18 tests UNRUN on B; A's initial8 c6d05aa3 pass does not cover this candidate.
SaveAs3ca3e9c9 intact. No B workloads/heartbeat/writer changes; A compiler/GUI/main.

### Review 06afa311 — opening follows current status generation

Published codex/smart-preview-ui2ba1d95c; testa0a6389e/productdd84ed57. Separate
photo identity and status generation: same-photo refresh is awaited, changed-photo
(including away/back) cancels old opener. Two gated reads prove replacement wait,
latest snapshot and exactly two calls; selection-change negative control retained.
20 tests UNRUN on B. Badge/local-save fix preserved. No offline Library changes:
await A exact native API. No B workloads/heartbeat/writer changes; A gates/main.

### Offline Library 889b3f34 — source handoff

Published codex/smart-preview-ui ef307d8d; tests dac7126e/product c5e001e5.
Cached factory calls frozen openSmartPreviewLibrarySession; A binding pending.
Missing-folder recent/relaunch routes cache; online errors propagate. Cached mode
skips index/list/library.json/analysis/people/profile/autosync startup and offers
explicit reopen, declaration labels, empty reconnect/build guidance, read-only
catalog controls. Explicit preview choice/defaultOriginal and refresh/badges intact.
20 prior + 6 new tests UNRUN on B. New tests prove injected router/mode contract,
not real native factory or GUI relaunch. A must compile/generate/run/integrate.
No Document/SaveAs edits, workloads, heartbeat or writer changes. SaveAs3ca3e9c9 preserved.

### Alias correction c13b4fe0 — online canonical authority

Published codex/smart-preview-ui775f39b0; tests ea40431c/782752f3, product3029127b.
EngineLibrary.scan retains index handle.path; AppModel persists only successful
current opens, replacing alias in recents lexically. Reopen/rescan carry authority;
changed online index identity gets a new session, not a relabeled old one.
Real online empty-folder alias scan -> isolated history -> alias removal/original
move -> cached closure receives canonical path regression added, UNRUN. 27 total
SmartPreview/offline tests UNRUN on B. No disconnected canonicalization or alias
guessing; historical disconnected aliases need one online reopen. A gates/main.
No Document edits/workloads/heartbeat/writer changes; SaveAs3ca3e9c9 preserved.

### Document strict warnings a4ac739c — separate source branch

codex/document-strict-warnings186559ce from preserved SmartPreview775f39b0.
Tests17354fb9/beda8605 UNRUN; productd6332c5a/d7c86b65. Freeze final stub
sampling stride, same optional self-test guard, explicit paired-key modifier
routing with three regressions, MainActor Sendable checkbox setter (A supplement
corrects original slider report). No DocSlider/AppModel/generated/SaveAs edits.
A owns compiler/main and SmartPreview alias fixture oracle repair. Handoff:
DOCUMENT-STRICT-WARNINGS-HANDOFF.md on source branch. No B workloads/heartbeat.

### Smart Preview thumbnails9269efdc — source API routing

Published codex/smart-preview-thumbnails51e0cfb6 from A baselineb6cea8f2.
Tests3262cc6a/9d35c5d6 (seven UNRUN), productfe5d7911. Immutable thumbnail
source on EngineImageReference, exact proxy native API/no original fallback,
source/owner cache+flight identity, mutation before/after invalidation, existing
save and reconnect retirement, truthful proxy copy. Grid clears stale proxy pixels;
loupe first paint and Compare source/revision handling updated. Native pending
API/bindings/validation all A. SMART-PREVIEW-THUMBNAIL-HANDOFF.md has limits/gates.
Document strict186559ce/SmartPreviewUI775f39b0/SaveAs3ca3e9c9 preserved.
No B builds/tests/apps/benchmarks/heartbeat/writer changes. A compiler/main.

### Checkbox IRGen6c298945 — uncompiled source candidate

codex/document-checkbox-irgen2711296e from Document strict186559ce;
product10c59dea wraps Binding setter inline, retaining MainActor Sendable
callback/synchronous invocation. No unsafe/scheduling/suppression changes.
A must verify optimized compiler crash resolution; no B reproduction or tests.
DOCUMENT-CHECKBOX-IRGEN-HANDOFF.md records failed09 evidence and limits.
Thumbnail51e0cfb6/other source branches preserved; A sole compiler/main.
No B workloads/apps/heartbeat/writer changes. Accepted receipt retried after
first mailbox push timeout and then confirmed before source edits.

### Thumbnail retry dfb80e63 — bounded source follow-up

codex/smart-preview-thumbnailsd40354e4 from preserved51e0cfb6;
testsc1644605/32d19363, product262b6c1f. Proxy request errors get3additional
attempts at250/500/1000ms cancellable backoff within same flight/slot. No strings,
Original fallback, globalqueue, fakepending or event-path polling. Exhaustion
retires flight; cancellation rechecked after injected wait. Five new tests UNRUN
and pending/Original negatives retained. A native8-pressure/compiler/GUI/main gates.
Compiler wrapper10c59dea already completed separately; no Document/generated/
native/AppModel edits or B workloads/heartbeat/writer changes in this slice.


## Checked Save As current-main reconciliation — 2026-09-28

Accepted exact B request fc30ff89-f16d-4d3a-9de7-087f2284b47a and completed
source-only reconciliation onto pinned main29a4cefd. Published
codex/save-destination-current-main at cfb511e5 (source af49e4ef).
Native mapping75a1b9a6->23c5bd0f using merge mainline2, 2a22145b->aef5e5c8;
Swift7b7a5dcb->15ae6321,3d4b34f4->eab89438,8976f2fe->fe3c19ca,
3ca3e9c9->af49e4ef. No source conflicts; eleven source/test files only,
ten exactly match reviewed final candidates; StubDocumentBackend also preserves
main sampledStep capture correction. All other tracked Smart Preview/generated
files remain pinned-main bytes. Old candidates/branches preserved.

Combined gates UNRUN, not compile-qualified. A must regenerate current combined
bindings/archive (never old81cc08eb), run native/focused/adjacent/full Swift/strict
and actual disposable collision/Replace GUI gates. Handoff and source SHA256
manifest are on new branch under tools/orchestrate/wp/B5-16/
SAVE-DESTINATION-CURRENT-MAIN-HANDOFF.md and sibling SOURCE.sha256.
B performed Git/source comparison and diff-check only; no workloads/apps/GPU,
heartbeat restart, writer change or main integration. A remains sole integrator.


## B5-16 bounded inspector reconciliation — 2026-09-28

Accepted5d86ed1a-53ad-40eb-a8db-dca97e44f94e; new branch
codex/b5-16-current-main at ddb28101, source374ea0ed, pinned qualification
1e45baaead70b1bcd208afba800bcacd92213ee8. Layout787f935a, persisted
Auto/LUT3953e730, Neutralize/runnerf17bc349, matrix-count/comment374ea0ed.
Sixteen source/test/harness files; no Rust/generated/shared SaveAs replacement.
Current strict synchronous MainActor Sendable checkbox setter, resource fixes,
Transform ownership and Smart Preview APIs retained. Only Workspace delta is
inspector tab preference. Old models already present; no duplicate B5-16a fixes.
All old branches preserved, dirty B5-16a untouched.

Source diff-check only. All compile/tests/Python/GUI interactions UNRUN;
base703+5 reported A gates do not qualify this candidate. History sizing/reset/
persistence, scroll/footer, compact overflow, selection/editor continuity,
Auto/LUT persistence, shortcuts/VoiceOver, checked SaveAs and Transform completion
are explicitly separate UNRUN rows in CURRENT-MAIN-INSPECTOR-HANDOFF.md on new
branch, with sixteen-file CURRENT-MAIN-INSPECTOR-SOURCE.sha256. Runner regressions
preserved; --nonactivating cannot guarantee individual app selftests never raise
windows. No old background host fallback or sheet sizing changes transplanted.
A must qualify isolated GUI/harness use and retains sole compiler/runtime/main.
B workload and heartbeat hold unchanged; no apps/builds/tests/benchmarks/GPU.

## Incremental runner correction — 2026-09-28

Accepted cf3510b8-7ad7-4ce8-a5e5-038fdc3f4770; published separate
codex/b5-16-runner-admission1972f603 from frozen inspectorddb28101 (unchanged).
Tests1139443d, productbe7abbb5 restore Transform72d8756a arguments without
--new-document and policy131af17d existing ~/.local/state/tessera-resource-hold.json.
Same marker checked at suite admission, between cases, direct run_test entry and
before Popen after staging. No alternate policy or hold modification. Exact
source/test hashes and provenance in RUNNER-ADMISSION-HANDOFF.md on new branch.
Twelve test methods UNRUN; five new negative tests plus exact Transform argument
assertion. No B Python runner/test execution, compiler/apps/GPU/heartbeat/writer
changes. Mailbox utility only for coordination. A owns gates/main; no actual app
selftest authorized. Nonactivating is not desktop isolation; admission check is
not atomic revocation or running-child monitoring. Inspector candidate preserved.

## History accessible height actions — 2026-09-28

Accepted81b15b38-4187-4190-8485-0e7f1e37099e. Published separate
codex/b5-16-history-accessibility at e2c3cd03 from frozen ddb28101;
tests553dc70d, product288d728a. Native decrease/increase/reset buttons and
labelled points readout in unchanged-height header share existing AppStorage
request and budget clamp; pointer drag/double-click and footer untouched.
KeyOwningControl focus prevents Space/Return leaking to Document shortcuts.
Frozen inspector ddb28101 and runner1972f603 unchanged. Five tests UNRUN: real AX
press/state, clamp/no layout write, native keyboard ownership, callback teardown,
hosted inspector preference persistence/recreation. Existing layout assertions
unchanged. Exact hashes and actual AX/keyboard plan in HISTORY-ACCESSIBILITY-
HANDOFF.md and HISTORY-ACCESSIBILITY-SOURCE.sha256 on new branch. A compiler/CUA/
main only; no new acceptance from previous pointer or runner gates. No B workloads,
Python tests/apps/GPU, settings changes, heartbeat or writer changes.

## History focused failure source correction — 2026-09-28

Accepted f8417c10-0861-4a6a-946b-87020147df30, preserved A1d361fa3
65-test/22-assertion failed evidence on main3a395c69. Separate
codex/b5-16-history-test-isolation b7687116 from e2c3cd03; test12e9f1ff.
Production unchanged. New fixture hosts actual DocumentInspector without full
DocumentView singleton attachments. Captures/restores previous five owners,
three preferences and activation policy after window/controller teardown; adds
non-nil prior-owner restoration regression. Direct native press oracle checks
callback/state, not Bool; value oracle uses observed native display pt, removes
four downcast warnings, retains label/action/keyboard/clamp/persistence coverage.
Six tests UNRUN; combined leakage mechanism source-supported, not proved by B.
Exact hashes and pending original-filter/AX/CUA gates in HISTORY-ISOLATION-HANDOFF.md
and HISTORY-ISOLATION-SOURCE.sha256. No compiler/tests/apps/Python workload/GPU,
heartbeat or writer changes; A sole gates/main. Existing candidates preserved.

## History zero-bounds fixture correction (2026-09-28)

Validated/accepted request a9f170bf-b8dc-4e36-bdf4-14d3b4b87755. A main56dd677e retains new combined66/3 RED; other five History and keyboard tests pass, not broad acceptance. Separate codex/b5-16-history-fixture-bounds from frozen b7687116: test1fed73831e7ae2c38a3d795b75f5956d2cd18165, handoff4aaed30a. Explicit post-attachment288x848 window content/host frame, exact positive geometry assertions on both creations, Increase enabled before native action. Existing settling duration unchanged; all prior state/persistence/reset/containment/owner assertions retained. No production changes. Test SHA25653e46d7cddc35f156407006faa4b25d4b04b1e983ce2b80306c62ea33d1ed722. Handoff/manifest HISTORY-FIXTURE-BOUNDS-HANDOFF.md and HISTORY-FIXTURE-BOUNDS-SOURCE.sha256 on source branch. Result34da0c42-bde9-4d1b-a865-f09e39ca76ce to A. All tests UNRUN on B; source diff check only. A owns compilation/serialized tests/GUI/main. All old branches/failures preserved; B workload and heartbeat hold unchanged.

## History ThemeLint correction (2026-09-28)

Request680a8220-8d1d-46bb-af94-10b67d5e3180 validated/accepted. Separate codex/b5-16-history-theme-token from4aaed30a: product7d9199afdece8ea40e55d7e16ec50e927e02dd5f, handoff619524b0. Only production line readout.font uses existing Theme.NSFonts.labelNumeric12pt, no size-equivalence claim, lint waiver or test/bounds changes. SHA25605f18ec2c81ed146c4d65099660918f9cfd3c9ff71e77878b8736bd246050d19. Resultd24ecab8-abdb-4343-b1d1-6e381a312f82 sent A; handoff/manifest HISTORY-THEME-TOKEN-HANDOFF.md and HISTORY-THEME-TOKEN-SOURCE.sha256 on branch. A mainb1567cd9 full720/1skip/1failure preserved alongside earlier failures; prior History/keyboard/layout/adjacent/Sony passes do not validate newfont. All gates UNRUN on B, source diff check only; A owns focused/full/strict/GUI and main. Workload/heartbeat hold unchanged.

## Properties hosted focus diagnostic (2026-09-28)

Request7d3c60f6-230d-431b-ad51-7d3becf93d4f validated/accepted. Separate codex/b5-16-inspector-focus-diagnostic from619524b0, testc39697c43645c812c72e95dc871bb6a9615e1088, handoff01242fa4. Real hosted Properties name->Load native key traversal records responder identities/classes/AX tree/policy; guarded navigation/activation assertions plus document non-control Tab/Space negative. No forced button focus/global policy/private class match/sleeps. All tests UNRUN. Product correction still outstanding pending A actual SwiftUI responder capture; do not claim diagnostic skip or missing native AX-host identity as acceptance. No production edit guessed. Result5a1ac3b2-9195-4b65-b4e2-86e44e83a901 sent A with exact hash/limitations/focused command pointer INSPECTOR-FOCUS-DIAGNOSTIC-HANDOFF.md and SOURCE.sha256 on branch. A owns runtime/main and real History reachability. Earlier evidence preserved; B hold unchanged.

## Actual-event semantic focus trace proposal (2026-09-28)

Request03feb884-72a3-41ea-a623-3a898922f15b validated/accepted; reviewed0158d95e hosted34/1failure and e04fce18 actualGUI. Separate codex/b5-16-inspector-focus-trace-plan from01242fa4: warning-only testfixae0a3a5d506f2ddea7cf3f1a34caecd4f74cd28c, proposal83172f16. Result774107e9-385f-427b-8b1f-8c428835681f. Test SHA25692ddb5a78511227a93a2faefab19ffc16136449ea12955c065c57ba1bdbc0492. No product edits. Chose permitted actual-app trace proposal because native traversal did not reproduce GUI focus and semantic AX nodes aren't NSViews. Detailed public-API pre-routing native+semantic capture, bounded/cycle-safe fallback, opt-in lifetime/output, A runtime plan in INSPECTOR-FOCUS-TRACE-PROPOSAL.md plus hash manifest. Trace NOT implemented; A reviews before authorization. Original hosted diagnostic remains failed, no assertions weakened/skipped. All B gates UNRUN; workload/heartbeat/writer hold preserved. Actual focus capture, routing correction and History keyboard acceptance still outstanding.

## Actual-event focus trace implemented source-only (2026-09-28)

Accepted7bd5595f-cd4b-4cf9-ac56-83e04e7d214f; separate codex/b5-16-inspector-focus-trace from619524b0, head036749da. Tests-first80b1c709/6dc4e9e4/e9809db0/123abb9b; product6a2946b2+b97ef0cf; native-event test7c3f6caf then product3af24b23 removes mouse-only eventNumber per SDK.10 tests ALL UNRUN. New diagnostic/App KeyRouter only; no routing fix. Disabled default, exclusive32event64KiB output; public semantic focus/native responder at same event boundary with active viewport/key window identity; unknown/ambiguous retained;64node8depth/cycle caps; no sensitive labels/values; unchanged exactly-once handler, reentrant bypass, close on limit/error/deinit. Failed hosted test remains on old branches, absent new aggregate. Resultb93a3d5b-d837-43de-8428-edbc1540d4b4 to A. Source/test hashes and A-only focused/strict/enablement/GUI plan in INSPECTOR-FOCUS-TRACE-HANDOFF.md and INSPECTOR-FOCUS-TRACE-SOURCE.sha256 on branch. Source checks only, B hold unchanged. A reviews/compiler/runtime/main; actual focus and later correction remain pending.

## AX collection bridge crash correction (2026-09-28)

Request58e629b8-cc38-48fb-9ee3-f14f023ce3d6 validated/accepted. Reviewed33b95e39 actual enabled PID12227 crash; disabled sequence reproduced panel hiding; enabled only firstName event exists, second decision UNKNOWN. Separate codex/b5-16-focus-ax-bridge-repair from036749da: testsf304727a, product4485d4c3ce5a65c67ddfde976816faf8f2ccce72, handofffd2b7d5f. Result87eed0a2-e4a8-4f38-a4b1-9a4a3133fcb6 to A. Removes navigation typed-array bridge; raw public Objective-C accessibilityChildren result NSArray with bounded indexed/per-element conditional protocol checks, unavailable/unsafe staysincomplete. Five native/AppKit/ObjC collection tests authored UNRUN. KeyRouter and prior trace tests unchanged. No runtime/keyboard fix acceptance. Exact hashes and A-only gates in INSPECTOR-FOCUS-AX-BRIDGE-REPAIR.md / INSPECTOR-FOCUS-AX-BRIDGE-SOURCE.sha256. A owns actual retry/main; all old failures and unknown profileless replacement effects preserved. B workload/heartbeat/writer hold unchanged.

## Native AX pool lifetime comparison (2026-09-29 evidence)

Request8102f329-cc08-49ee-a897-6d2400fed0a2 validated/accepted. Separate codex/b5-16-ax-lifetime-pool fromfd2b7d5f: test832d095bbd14680d0f3e932031bc39a2214941c6, handoffa6e28f4e, result47307c56-86a2-49c9-acb2-cb7af067230c. Explicit autoreleasepool holds all root/child/array construction/capture; returned snapshot/window alive across both unchanged post-drain weaknil/unknown assertions. Matching same-native-graph/no-observer pool control added. No product changes or ownership clearing/timing waiver. Six bridge tests ALL UNRUN on B. Prior114273ea47cases/onecase2assertions retained; no leak/autorelease conclusion. TestSHA2561ab305cd0ed4e5edc6afa7153586043cb22571f104e6cffe31138029b1578ee3. Handoff/hashes INSPECTOR-AX-LIFETIME-POOL-HANDOFF.md and SOURCE.sha256 on branch. A owns focused/strict/actual retry/main; B workload/heartbeat/writer hold unchanged.

## Native Color Lookup action ownership source candidate (2026-09-29 evidence)

Request5b27b9df-43bd-45d8-8ee4-5cf79bba101b validated/accepted. Separate codex/b5-16-lut-native-key-owner froma6e28f4e, head527410c7. Tests-first55d583a1/1f658c25/7caa4e54; product71a6e3c9+f3f0966a; testrefinementsbd6c194b/31c18bb1/45a9860f. Six tests UNRUN, result81854c4d-8cee-440c-93b9-645ee5b7e2b8 to A. Only Load3DLUT/Reset now native NSButton/KeyOwningControl, existing theme tokens/IDs/help/disabled/reset callbacks; Tab/ShiftTab native, no forced focus, private proxy match or unknown bypass. KeyRouter/trace unchanged. Actual hosting installation test is not focus proof; A must verify real traversal/activation/style and preserve viewport/text/tool routing. Other controls/History not claimed fixed. Hashmanifest and precise A gates in LUT-NATIVE-KEY-OWNER-SOURCE.sha256/HANDOFF.md. d18369b3 GUI/internalunknowntrace evidence retained; source checks only, no B workloads/heartbeat/writer changes. A sole compiler/runtime/main.
