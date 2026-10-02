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
