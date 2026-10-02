# B5-48 — Confirm-time Export Flat snapshots

Branch `wp/B5-48`, local only, based on `fa04e5fc` (origin/main including ENG-2 + ENG-2b).
Implementation complete; **Rust validation remains non-green**. Both Swift gates
pass. The default Rust aggregate failed three tests; isolated serial retries
passed Develop and viewport, while Liquify still exceeds its unchanged limit.
The `docs(B5-48):` commit containing this handoff is the final local lane tip.

## Changes and semantics

- Restore synchronous main-actor snapshot acquisition when `startExportFlat` handles
  the user's confirmation. Native snapshot acquisition runs on main; rendering,
  encoding, and writing remain on the export worker. Edits committed after confirm
  are excluded. The captured native job retains its snapshot across document close.
- **Deliberately invert B5-47's assertion that the snapshot must never run on main.**
  ENG-2b's short publication lock removes the old render/edit lock dependency.
  Replace B5-40's prepare-to-worker edit-inclusion test with a deterministic workspace
  test that pauses the worker, commits an edit after confirm, then checks exported
  pixels against the pre-edit image and proves the edited image differs.
- Force both phase and numeric progress AX notifications when a row changes task
  identity, even when both tasks display Preparing 0 %. Test the first publication
  for a new row and reused row, as well as phase-only and duplicate updates.
- Poisoned shutdown sets mutable and published `closed`, clears surfaces, and marks
  the last valid publication closed without publishing the partial edit that panicked.
  Test poison retention, closure, immutable snapshot identity, rejected new exports,
  and repeated shutdown.
- Frame completion uses a constrained guard exposing immutable state and a single
  `advance_ring` operation. It intentionally has no `DerefMut`: reader-visible writes
  at the call site require the normal publishing guard. The actual rendered-frame
  regression verifies cursor advancement, unchanged reader data/publication identity,
  and unchanged IOSurface retention.

## Tests first

- `365c49da` — `test(B5-48):` regression tests and a deterministic worker-start seam.
- Native RED: 1 passed, 1 failed. Poisoned shutdown leaves `closed` false.
- Native focused GREEN after implementation: 2 passed, zero failures.
- Swift RED: 2 tests, 4 expected assertion failures. No snapshot exists when
  confirm returns; exported pixels include the later edit; both same-value row
  rebindings miss the two notifications.
- `8d7fd05b` — `fix(B5-48):` implementation and final tested source.
- Swift focused GREEN: 17 Export Flat/HUD tests passed, zero failures.
- Evidence: `evidence/{rust-red,rust-green,swift-red,swift-green}.log`.

## Snapshot cost and validation

SwiftPM release XCTest, final source, actual `DocumentWorkspace.startExportFlat`
path. The measured `export_flat_snapshot` span brackets `beginExportFlat` and includes
trace-recording overhead. All final-source samples acquired the snapshot on main.
This is a shared host, not a controlled system-wide benchmark.
No B5-48 build/test workload ran concurrently with the isolated measurements.

| Fixture/run | Confirm snapshot |
|---|---:|
| 32 × 24 fill, ordinary 17-test suite | 5.458 µs |
| 32 × 24 fill, opt-in launch 1 | 8.125 µs |
| 32 × 24 fill, opt-in launch 2 | 7.000 µs |
| 32 × 24 fill, opt-in launch 3 | 7.500 µs |
| 5212 × 3468 developed RAW + Gaussian smart filter radius 8 | 24.292 µs |

All three isolated tiny-fixture launches used `TESSERA_FILTER_PERF=1`, passed,
and exercised the generous **20 ms** confirm-snapshot bound. Their median is
7.500 µs. The 18 MP run passed with the performance flag unset and zero dropped
trace events; its deterministic main-thread snapshot assertion is always enabled.
All four isolated measurement launches are retained; the focused-suite
measurement is listed separately from the three opt-in tiny-fixture launches.
See `evidence/snapshot-cost.json`, `snapshot-*.log`, and `18mp.log`. Full 18 MP trace is local at `/tmp/B5-48-18mp-trace.json`.

These instrumented call timings do **not** close the existing whole-main-thread
<8 ms target or establish a universal bound for every document. Snapshot timing
assertions require `TESSERA_FILTER_PERF`; no Liquify/frame-delivery limits changed.

- Clippy, all `tessera-ffi` targets, `-D warnings`: passed (exit 0).
- `cargo fmt --all -- --check`: passed (exit 0).
- Full FFI aggregate: **exit 101; 572 passed, 3 failed, 31 ignored** across
  51 result blocks including doc-tests. Failures:
  - Develop `export_batch_does_not_starve_slider_drag`: **0/120 at L2**
    (all at L3; required 108/120). Render p90 6.3 ms, maximum 12.4 ms.
  - Liquify `brush_latency_on_a_20_megapixel_layer`: **p95 492.7 ms**
    versus 250 ms; median 293.0 ms, max 1393.7 ms.
  - Viewport `frames_for_a_replaced_ring_are_dropped`: no `r.dropped`
    record. Its preceding check that all delivered frames belong to the new
    surface ring passed; the other five viewport tests passed.
  Compact complete result blocks and all failure diagnostics are in
  `evidence/rust-full.log`; full local log `/tmp/B5-48-rust-full.log`.
- Isolated Develop retry: **exit 0; 1 passed**, **120/120 at L2**;
  render p90 3.6 ms, maximum 21.1 ms. `evidence/develop-serial.log`.
- Isolated Liquify retry: **exit 101; 1 failed**, **p95 343.2 ms** versus
  the unchanged 250 ms limit; median 259.5 ms, maximum 390.6 ms.
  `evidence/liquify-serial.log`. The aggregate and this retry remain non-green;
  no host-load explanation is established by these observations.
- Both retries used `--exact --nocapture --test-threads=1`, ran one at a time
  after the aggregate, and had no other B5-48 build/test workload active.
- Required `apps/mac/build-ffi.sh` followed by `tools/orchestrate/swift-gate.sh`:
  **exit 0, SWIFT GATE OK**. **915 XCTest tests, 3 skipped, zero failures**;
  **5 Swift Testing tests passed**. The existing skips are unchanged.
  `evidence/swift-gate.log`; full wrapper log `/tmp/B5-48-swift-gate.log`.
  The test harness uses `.prohibited` activation policy.
- Isolated viewport retry: **exit 0; 1 passed**, unchanged
  `frames_for_a_replaced_ring_are_dropped`, `--exact --nocapture --test-threads=1`.
  It ran alone after the Swift gates. `evidence/viewport-serial.log`.
  The aggregate failure is retained; no assertion or sleep duration changed.
- Strict release product build with `-strict-concurrency=complete` and
  `-warnings-as-errors`: **exit 0**, `Build of product 'Tessera' complete!`
  (207.95 s). `evidence/strict-release.log`.
- `git diff --check`: passed. Generated bindings are unchanged. Rust changes
  are confined to `tessera-ffi`; no dependency, Cargo.lock, or board.json edits.

No GUI foreground launch, installation, push, merge, Cargo.lock or board.json edits.

## Reproduction

From the worktree root:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-48"
export CARGO_BUILD_JOBS=3
cargo test --locked -p tessera-ffi --no-fail-fast
cargo clippy --locked -p tessera-ffi --all-targets -- -D warnings
cargo fmt --all -- --check
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

From the worktree root after the Swift gate, isolated snapshot measurement
(before a strict build changes
SwiftPM's cached configuration):

```sh
TESSERA_FILTER_PERF=1 swift test --package-path apps/mac -c release \
  -Xswiftc -enable-testing --skip-build \
  --filter DocumentExportFlatTests/testWorkspaceSnapshotExcludesEditCommittedAfterConfirm
TESSERA_EXPORT_TEST_TRACE=/tmp/B5-48-18mp-trace.json \
  swift test --package-path apps/mac -c release -Xswiftc -enable-testing --skip-build \
  --filter DocumentExportFlatTests/testSmartFilterFixtureExportBoundsMainSpansAndCoalescesProgress
```

`CI`, `RAYON_NUM_THREADS`, and performance opt-ins were unset for default gates.
The three tiny-fixture measurement launches alone enabled `TESSERA_FILTER_PERF`.
All commits carry `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

Serial failure retries (unchanged source and bounds):

```sh
cargo test --locked -p tessera-ffi --test develop export_batch_does_not_starve_slider_drag -- --exact --nocapture --test-threads=1
cargo test --locked -p tessera-ffi --test document_liquify_ui brush_latency_on_a_20_megapixel_layer -- --exact --nocapture --test-threads=1
cargo test --locked -p tessera-ffi --test document_viewport frames_for_a_replaced_ring_are_dropped -- --exact --nocapture --test-threads=1
```

The first two retries preceded the Swift gates; the additional viewport retry ran
after the strict build. No B5-48 build or test overlapped any isolated retry. The
full suite's long RAW round-trip test was sampled for one second after the Develop
frame-delivery failure had already occurred; the sample showed lens CA estimation
during backend selection. It is diagnostic context, not an explanation of any
timing failure. That sample and complete raw command logs remain local in `/tmp`;
compact gate results and every failure diagnostic are committed in `evidence/`.


## B5-48b — Deterministic ring regression and export cleanup (2026-10-01)

Local follow-up on `wp/B5-48`, strictly on top of approved `47b17baa`; no rebase.

- Tests first: `14071dd5` adds the library regression and test-only hook, removes
  the sleep-based integration test, and converts the two deferred-export tests to
  the supported immediate snapshot API. The focused Rust regression passed before
  the cleanup commit; Swift tests were authored/committed before cleanup and run
  against the final implementation.
- `5c69ecc0` removes `prepareExportFlat`, `DocumentFlatExportPreparation`,
  `ExportSnapshotReservation`, and the reservation counter/release machinery.
  Close now closes the native session directly; captured export jobs remain
  independent. The existing backend closed flag remains in use.
- `present_frame` takes a one-shot, renderer-local `#[cfg(test)]` hook immediately
  after snapshotting and releasing the state lock. The test waits for its signal,
  detaches the old ring, attaches three new surfaces, releases the worker, waits
  for idle, and verifies a dropped record, nonempty delivery, no render errors,
  and that every delivered frame belongs to the new ring. It uses a tiny document
  and no sleeps. The receive timeout is only a 30-second hang guard. Disconnecting
  the release sender also unblocks the worker if the test unwinds.
- Stress: **200/200 passed, zero flakes**, 54.77 seconds, one fresh test process per
  iteration. Ran the built library test executable directly to avoid Cargo locks,
  alongside active release FFI compilation and full-suite Rust compilation.
  Captured concurrent rustc CPU samples are in `evidence/b5-48b/stress-load.log`.
  The stress runner has a 60-second process hang guard and verifies exactly one
  passed test on every iteration. Reproduce after compiling the library test:
  `python3 tools/orchestrate/wp/B5-48/evidence/b5-48b/stress.py` with the target
  directory below exported.
- Source search: `rg -n 'prepareExportFlat|DocumentFlatExportPreparation|ExportSnapshotReservation|exportReservations|releaseExportReservation' apps/mac`
  returned no matches (exit 1). Also no matches in repository source excluding
  historical handoffs/logs/JSON. Evidence: `evidence/b5-48b/unused-symbols.log`.

All gates use `PATH="$HOME/.cargo/bin:$PATH"`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-48`, `CARGO_BUILD_JOBS=3`.
No thresholds changed. Final gate results follow.

- Focused native regression: **exit 0; 1 passed**.
- Updated Swift export lifecycle tests: **exit 0; 2 passed, zero failures** on the
  final implementation (snapshot survives close; cancel preserves destination).
- `cargo clippy --locked -p tessera-ffi --all-targets -- -D warnings`: **exit 0**.
- `cargo fmt --all -- --check`: **exit 0**.
- `cd apps/mac && ./build-ffi.sh`: **exit 0**, bindings regenerated with no diff.
- `tools/orchestrate/swift-gate.sh`: **exit 0, SWIFT GATE OK**;
  **918 XCTest tests, 3 skipped, zero failures**, plus **5 Swift Testing tests passed**.
- `cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors`:
  **exit 0**, product build completed in 253.08 seconds.

- Full Rust aggregate (`cargo test --locked -p tessera-ffi --no-fail-fast`):
  **exit 101; 575 passed, 2 failed, 31 ignored**, 51 result blocks including
  doc-tests. The library's 230 active tests and all five viewport integration
  tests passed. Remaining failures:
  - Develop `export_batch_does_not_starve_slider_drag`: **0/120 at L2** (all at L3;
    required 108/120); render p90 6.4 ms, maximum 1397.2 ms.
  - Liquify `brush_latency_on_a_20_megapixel_layer`: **p95 319.4 ms** versus the
    unchanged 250 ms limit; median 232.0 ms, maximum 2041.4 ms.
  Complete result blocks and all failure diagnostics:
  `evidence/b5-48b/rust-full.log`; full log `/tmp/B5-48b-rust-full.log`.

- Isolated serial Develop retry: **exit 101; 1 failed**, **69/120 at L2** versus
  required 108/120; render p90 5.5 ms, maximum 34.7 ms. Evidence:
  `evidence/b5-48b/develop-serial.log`.
- Isolated serial Liquify retry: **exit 101; 1 failed**, **p95 391.7 ms** versus
  the unchanged 250 ms limit; median 272.9 ms, maximum 413.9 ms. Evidence:
  `evidence/b5-48b/liquify-serial.log`.
- Both retries ran with `--exact --nocapture --test-threads=1`, one at a time,
  after the full Rust suite and both Swift gates had finished. No other B5-48b
  build/test workload overlapped either retry. This is a shared host; these
  observations do not establish host load as the cause of either failure.
  `CI`, `RAYON_NUM_THREADS`, and `TESSERA_FILTER_PERF` were unset.

**B5-48b implementation and requested validation are complete; the Rust aggregate
and both performance retries remain non-green.** The ring regression passed its
focused run, all 200 stress iterations, and the full library suite. No performance
thresholds were changed or failures hidden. All static/Swift gates passed.

Serial reproduction (same exported environment as above):

```sh
cargo test --locked -p tessera-ffi --test develop export_batch_does_not_starve_slider_drag -- --exact --nocapture --test-threads=1
cargo test --locked -p tessera-ffi --test document_liquify_ui brush_latency_on_a_20_megapixel_layer -- --exact --nocapture --test-threads=1
```

`git diff --check` passed. Generated bindings, Cargo.lock, and board.json are
unchanged. No GUI foreground launch, installation, push, merge, or rebase.
The final local `docs(B5-48b):` commit contains this appendix and compact evidence;
full command logs remain at `/tmp/B5-48b-*.log`. All three follow-up commits end
with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
