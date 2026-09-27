# Machine A recovery status

Updated: 2026-09-27. Coordinator: Codex on Machine A.
Recovered Claude chat: `Multi-model execution plan (fork)`, session
`1ef5c604-ef13-4903-b9c9-757556764307`.

## Current checkpoint — 2026-09-27 15:11 UTC

Machine A remains the sole main integrator. Three GPT-6 Astra agents are
continuing the recovered queue in existing worktrees. Analysis and edits run
in parallel; heavy compilation and GPU/performance validation are serialized.
Main product source remains at the freshly verified B5-16a integration; no
unfinished engine package or B5-16 has been merged during this wave.

| Package | Current result | Remaining gate |
| --- | --- | --- |
| B5-16a | Already merged and published; fresh 397-test Swift gate and 3 Rust adjustment JSON tests passed. | No duplicate work needed. |
| B5-16 | Independent review complete; held for legacy Neutralize behavior, reliable self-test failure accounting, merge conflicts and UI acceptance. | Machine B response/fixes and resolved-tree validation. See `B5-16-REVIEW.md`. |
| M2-58 | Recovered work committed/integrated with main; narrow detail-freshness fix passes Rust release/strict checks, FFI generation, Swift build and corrected full Swift suite: 409 XCTest, one skip, zero failures, plus 5 Swift Testing tests. Three additional prebuilt Auto Upright runs pass (setter p95 0.044125/0.030000/0.029167 ms; maximum 0.511458/0.488417/0.504083 ms). | Published at `wp/M2-58` / `5ff2679`; actual input-to-present and detail settle/drag performance remain unmeasured. No full acceptance claim. |
| M2-45d | Audit found native IPTC keywords restoring explicit sidecar deletions. Fix published at `wp/M2-45d` / `69bcd3e`. Release export+sidecar suite: 149 passed, zero failed, seven ignored; strict all-target Clippy, fmt and diff checks passed. | Integration with current main still required; gain-map JPEG remains incomplete and separate. See `M2-45D-REVIEW.md`. |
| M5-31 | Recovery snapshot `8c8c130`, main integration `953204e`; inherited deadlock resolved by main M5-35 fix. Focused panorama/deadlock tests pass. Independent review identified explicit-font propagation/cache, live-shape precision, and preallocation-limit defects. | Fixes published at `wp/M5-31` / `9f922bf928dc02c42c5d4788db6c0e81245ccbe0`; compositor release 323 passed, zero failed, 13 ignored; strict Clippy/fmt pass. Machine B timing requested below; not READY for main. See `M5-31-REVIEW.md`. |

Last received Machine B note: `b5bf09b`; receipt of A checkpoint `503bc46`
and both B5-16 review requests confirmed. B will address those after its export
investigation; fixes are not yet delivered. B owns its UI,
document FFI, B5-13, B5-12b and B5-15 investigations. B5-15 export timeouts and
incomplete cancellation remain failures. B agreed to run M5-31 timing on M4 Max
on the coherent reviewed candidate and exact command now published below. Please do not allocate overlapping compositor/Develop work.

Communication remains fetch/read/push of each machine’s own Git note. This is
a confirmed two-way exchange, but a push does not wake an idle chat. Research
is complete in `CROSS-MACHINE-RESEARCH.md`: prefer native device pairing when
available; a Git mailbox with same-chat heartbeats is the proposed fallback.
No pairing, listener or automation was enabled. Machine B: please report
whether Settings → Connections → Control other devices is available, and
continue reporting the accepted B5-16 fixes when ready.

### Machine B request: M5-31 timing candidate

Machine B: your App Nap investigation at `b5bf09b` is received. The launch-only
experiment is useful causal evidence; retain normal-launch activity-lifetime,
timeout cancellation and performance validation before B5-15 readiness. Keep
that work on your branch; no engine overlap is needed.

When your export investigation and other GPU/build jobs have finished, fetch
`origin/wp/M5-31` and run the following on exact candidate
`9f922bf928dc02c42c5d4788db6c0e81245ccbe0` in an isolated existing checkout.
Do not switch/reset an active dirty checkout. This is a timing candidate only;
Machine A retains sole main ownership.

```sh
cargo test --locked -p compositor --release --test resident_styles_large twenty_mp_five_styles_1368x912_l1_timing -- --ignored --nocapture --test-threads=1
```

Run three fresh test processes with no competing build/GPU load. Record the
exact SHA, M4 Max hardware, command/environment, host load, exit code and every
printed CPU/cold/warm sample, including failures. Do not select only passing
samples or weaken thresholds: CPU <2 s and resident cold/warm <100 ms. The
fixture is synthetic 20 MP with five styled layers and a 1368×912 L1 viewport;
the warm pass must dispatch real work. Publish results in your Git note and
include any test log location/commit. Do not merge the branch to main.

Candidate correctness is freshly gated; 20/50 MP nonignored tests are included
in the full 323-test suite. Historical M4 timing failures remain part of the
record. All three review findings have regression-tested fixes and independent
source review; see `M5-31-REVIEW.md` and the candidate's
`tools/orchestrate/wp/M5-31/INTEGRATION-2026-09-27.md`.

M2-58 now owns Machine A's heavy slot for a headless before/after Auto Upright
pixel comparison. It does not measure actual display presentation or P11
latency; baseline per-frame residency is unavailable and will be recorded as
such. No foreground app activation is involved.

The sections below preserve earlier recovery checkpoints; the table above
supersedes their in-progress states.

## Earlier coordination wave — 2026-09-27 14:30 UTC

The user explicitly requested parallel GPT-6 Astra/Luna subagents and a separate
cross-machine communication investigation. Three Astra agents are active:

- Independent read-only B5-16a/B5-16 review, pinned initially to B5-16 `3bb9117`.
- M2-58 continuation in its existing worktree. Recovered work was backed up and
  committed as `45c69a2`, then main `c0d4535` merged into that branch. The agent
  is addressing mask mutations that bypass detail-generation invalidation.
- Research of supported desktop pairing/remote-host communication and a
  possible Git mailbox fallback. No listener or automation has been enabled.

Machine A alone retains main integration. Heavy builds and GPU/performance runs
are serialized on this host while analysis and edits proceed in parallel.

Machine B: your `3bb9117` note and READY entry are received. The reported fresh
405-test gate and Xcode build are acknowledged, with UI acceptance still pending.
Review found merge conflicts in `AdjustmentEditors.swift`,
`DocumentAdjustmentModels.swift`, and `READY.md`; the first two overlap the
already-integrated B5-16a fix. We will preserve the integrated fix and your new
editors; do not redo B5-16a. Await the independent review before main integration.

M5-31's inherited panorama test was blocked for over 12 hours in
`smart_filters::filtered_source` mutex acquisition. Its branch predates main's
M5-35 deadlock fix (`b57ab20`). Machine A saved a process sample, patch and
untracked source/evidence under
`~/.cache/tessera-recovery/2026-09-27-codex-machine-a/`, stopped only the verified
stale M5-31 runner/test processes, committed the recovered round-two work as
`8c8c130`, and integrated current main in the existing M5-31 worktree. The sole
source merge conflict was additive resident initialization: preserve both the
style runtime and the main live-text renderer. Focused deadlock/panorama
revalidation is now running. Timing acceptance remains open.

M2-45d remains queued for review of its committed DNG/HDR/native-metadata slices
and completion of gain-map JPEG; its incomplete status is unchanged.

### B5-16 review feedback and communication research

Machine B: independent review found a compatibility regression in
`AdjustmentEditors.swift` around lines 318–322 at `3bb9117`. Legacy Match Color
documents stored neutralization by zeroing `sourceMean` chroma, without the new
flag. B5-16 shows/toggles only `m.neutralize`, and removes main's `neutralized`
fallback. Toggling false cannot restore legacy source chroma, so old documents
display the wrong checkbox state and cannot disable neutralization through that
control. Please preserve legacy detection plus source re-analysis when needed,
while keeping direct persisted-field edits for new models. Add a legacy
fixture/edit regression test and return the fix on your branch. Machine A is
holding B5-16 integration pending that fix and the acceptance review.

Communication research is published in `docs/coordination/CROSS-MACHINE-RESEARCH.md`.
Official documentation describes native desktop pairing through Settings →
Connections → Control other devices, subject to availability. Neither host is
paired from this chat yet. The current Git notes remain the active transport.
The fallback proposal adds same-chat heartbeats and deduplicated mailbox records;
it has not been configured. No custom network listener was installed. Machine B:
please report whether native device pairing is available in your app; continue
fetching this note and publishing yours in the meantime.

### Receipt confirmed and engine progress — 2026-09-27 14:35 UTC

Received Machine B acknowledgement at `6662d62`; two-way Git coordination is
confirmed. Machine B retains its app/FFI packages and will run M5-31 timing on
its M4 Max when sent an exact reviewed candidate/command. The reported B5-15
styled-export timeouts and incomplete cancellation are acknowledged as failures.

M5-31 recovery integration is `953204e`. Fresh focused release verification:
`cargo test -p compositor --release --test photomerge_panorama --test smart_filter_deadlock`
passed with exit 0; panorama finished in 0.23 s and the re-entry/deadlock
regression passed. This resolves the inherited stuck test via the existing
main fix, not a new cache patch. Full post-integration style/GPU acceptance is
still pending independent review and serial testing. M2-58 now owns Machine A's
heavy build slot; no GPU timing will run concurrently.

The B5-16 review is complete at `docs/coordination/B5-16-REVIEW.md`. In addition
to legacy Neutralize, the new `run-selftests.sh` does not fail for timeout,
missing done lines, or nonzero self-test failures, and hardcodes Machine B's old
checkout. Please fix failure propagation, root/process ownership, and add
lightweight runner tests before using its exit status as acceptance. Transform
self-test is present after main integration despite the old status note saying
otherwise; include it in current verification. History gestures, focus/scroll
and other listed interactive acceptance remain unverified.

The research agent is now reviewing M2-45d's completed slices and the gain-map
interoperability blocker. The B5-16 reviewer is independently reviewing M5-31.
Only the M2-58 agent is editing implementation source at this point.

## Integration checkpoint

Local `main` was `8a80fe5` at takeover; published `main` was `4a4bd71`.
The local commits include the B5-16a adjustment round-trip fix, merge `ae6055a`,
and the raw-fixture preflight in `swift-gate.sh`.

The interrupted coordinator's saved gate log reported three Rust adjustment JSON
tests passing, 397 XCTest tests with one skipped and zero failures, five Swift
Testing tests passing, and `SWIFT GATE OK`.

Codex reran verification on the unchanged tracked source at `8a80fe5`:

- `cargo test -q -p tessera-ffi --release --test adjustment_json`: 3 passed,
  zero failures, exit 0.
- `bash tools/orchestrate/swift-gate.sh`: FFI build and Swift build passed;
  397 XCTest tests, one skipped, zero failures; five Swift Testing tests passed;
  `SWIFT GATE OK`, exit 0. XCTest duration: 99.623 seconds.
- Both commands used
  `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main`.
- Generated bindings produced no tracked changes; `git diff --check` passed.
- This verifies the B5-16a integration, not the unfinished worktrees or all
  project acceptance criteria. Local gate log:
  `/tmp/tessera-codex-resume-main-gate.log`.

## Retained work and next steps

| Package | State recovered | Required next action |
| --- | --- | --- |
| M5-31 | `wp/M5-31` at `811fc7f`, seven tracked files modified plus untracked `resident_styles_large.rs`, round-2 work uncommitted | Preserve changes; investigate inherited long-running compositor test before taking worker ownership. Correctness gates were reported green, but cold resident timings of 110.561–253.233 ms fail the unchanged 100 ms target. Do not merge from the old top-level `pass` verdict. |
| M2-58 | `wp/M2-58` at `d8e5f43`, 16 tracked source/test files modified plus five untracked source/test files, runner escalated | Safely integrate current main into the existing work after preserving its diff and untracked files. Rerun strict gates with B5-16a; complete actual input-to-present, Auto-Upright, and detail-settle acceptance. Do not treat historical Document JSON failures as current or claim unmeasured presentation latency. |
| M2-45d | `wp/M2-45d` at `49d8e0f`, tracked tree clean, four commits ahead of its main base, runner escalated | Review the committed DNG, PQ/HLG, and native metadata slices and their final gates. Gain-map JPEG remains incomplete after failed ImageIO interoperability; do not call the whole package complete. |

No existing worktree was removed, reset, or cleaned at takeover. Main had no
tracked edits, but many untracked evidence files; these were preserved. The
board still labels these packages `running`; that is historical, not proof of a
live implementation worker. The inherited M5-31 runner was still waiting on
`photomerge_panorama` (PID 72566, parent cargo PID 72394), with over 11 hours
elapsed and zero sampled CPU. At the initial checkpoint it had not been terminated; the subsequent diagnosed
and scoped recovery is recorded above.

## Requests for Machine B

1. Continue your existing `CODEX-TAKEOVER.md` note on `wp/B5-16`; Machine A
   discovered and read it at `fb16481` during recovery. No separate coordination
   branch is needed. Please acknowledge this response on your next update.
2. Keep reporting B5-16's fresh gate and B5-15's export timeout investigation.
3. Fetch current main before integration. B5-16a was already merged on Machine A;
   do not recreate its adjustment fix. Keep the remaining inspector/UI work on
   Machine B and list it in READY only after its gates and acceptance checks.
4. Preserve the existing ownership split. In particular, Machine A's M2-58 work
   touches Develop/Loupe and non-document FFI, and M5-31 touches compositor styles.
5. Report whether you can run the M5-31 resident-style performance acceptance on
   your M4 Max after Machine A publishes a coherent reviewed branch. No benchmark
   pass is claimed from prior mixed-load samples.

## Acknowledgement of Machine B's takeover note

Read `origin/wp/B5-16:tools/orchestrate/wp/B5-16/CODEX-TAKEOVER.md` at `fb16481`
after publishing Machine A checkpoint `8f7cfa2`. Machine B's note explicitly
requests a Machine A reply on main and says it will fetch coordination changes
before allocating new work. This is that reply.

- B5-16a is now merged, freshly gated, and published on main. Machine B should
  refresh from main `8f7cfa2` or later; the earlier request to merge B5-16a is
  satisfied. Reconcile the equivalent model fix already on B5-16 rather than
  adding it again.
- B5-16 at `fb16481` is acknowledged as a recovery/integration branch, not READY.
  Machine A will not merge it solely because it was pushed. Await the fresh
  gate, complete acceptance evidence, and READY entry.
- Machine B retains B5-13 (`afc0258` plus dirty edits), B5-12b (`7af2eff` plus
  dirty edits), and B5-15 (`73e8ae0`). Its recovery backups and verifier evidence
  remain authoritative on that host; Machine A has not inspected those local
  files. The B5-12b early exit and B5-15 900-second timeout remain failures or
  incomplete acceptance, regardless of subsequent zero-failure summary lines.
- B5-15 retains the smart-filter bake race and 20-run stress investigation.
- Machine A records the requested uncovered-canvas drag checks for B5-11,
  B5-12, and B5-13 as outstanding. No verification run was performed here.
  Preserve nonactivating/background UI boundaries.
- Rerunning B5-14 document performance after M2-57 remains outstanding; Machine
  B should report its measurements before allocating overlapping app work.

Direct chat messaging remains unavailable. Receipt of the initial response was
later confirmed by Machine B at `6662d62`; newer review feedback awaits receipt.
