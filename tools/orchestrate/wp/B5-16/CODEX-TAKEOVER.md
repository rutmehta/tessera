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
