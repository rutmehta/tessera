# B5-35 — deterministic thumbnail mip reuse regression

Status: implementation and Rust verification complete; acceptance BLOCKED by the full Swift gate.

Reviewer: Machine A. Branch: `wp/B5-35`. Local commits only; no push or merge.

## Problem and root cause

`composite_thumbnails_reuse_mips_across_edits` used `warm * 4 < cold`, including
resident drag ticks, as its proof of mip reuse. CPU scheduling, GPU queueing and
readback under shared-machine load can change that ratio without changing cache
behavior. A timing ratio cannot distinguish a cache regression from contention.

## Fix

Replace the ratio with an opt-in, instance-local mip-cache probe. The probe is
disabled by default and exposed through Rust-only, hidden test-support methods;
it is not exported through UniFFI. It counts CPU mip cache hits/completed builds
and resident retained-node/content-addressed hits/newly queued mip builds.
It does not change cache keys, retention, invalidation, eviction or rendering.

The regression establishes a baseline before the cold thumbnail, requires cold
mip construction, then requires positive hit deltas and zero rebuild deltas after
an opacity edit. Each resident drag tick has the same assertions. The existing
CPU drag exception remains: scratch documents have different CPU cache keys,
so reuse across those keys is not an existing CPU guarantee. Both paths retain
a loose absolute 30-second bound per warm thumbnail/tick. The edited thumbnail
must still have a different surface ID.

The first test commit intentionally fails compilation because the diagnostic
hook does not exist yet (five E0599 errors). The implementation follows in the
fix commit. A separate runtime negative control verifies the actual regression.

## Validation and numbers

Ten consecutive release runs passed under a simultaneous fresh-target
`cargo build --locked --release -p compositor`. The load target was
`$HOME/.cache/tessera-target/B5-35-load`, separate from the prescribed
`$HOME/.cache/tessera-target/B5-35` test target. The Cargo build PID was alive
both before and after every run; the build completed successfully in 1m 27s.
No `-j` override was used. All later builds/gates run serially.

| Loaded run | Cold ms | Edited ms | Drag min–max ms |
|---|---:|---:|---:|
| 1 | 241.151 | 0.974 | 0.330–0.576 |
| 2 | 122.981 | 1.074 | 0.366–0.724 |
| 3 | 273.575 | 0.822 | 0.375–0.693 |
| 4 | 303.455 | 1.138 | 0.433–1.144 |
| 5 | 117.200 | 1.511 | 0.826–3.534 |
| 6 | 120.121 | 0.941 | 0.622–0.846 |
| 7 | 250.777 | 1.217 | 0.712–1.314 |
| 8 | 117.814 | 1.093 | 0.420–1.247 |
| 9 | 138.556 | 1.212 | 0.806–1.998 |
| 10 | 114.445 | 1.158 | 0.587–2.904 |

Initial fixed release run: cold 398.123 ms, edited 33.462 ms; five drag ticks
5.934–31.222 ms. Counters `(hits, rebuilds)` were `(0, 0)` before rendering,
`(0, 350)` cold, `(44, 350)` after the edit, and `(264, 350)` after five ticks.
Each edit/tick added 44 mip hits and zero builds. These are observations, not
performance-ratio requirements. The historical timing flake was not reproduced
in this run.

### Runtime negative control (local only)

Temporarily added `self.layers.clear(); self.mips.clear();` at the start of
`ResidentRenderer::render`, discarding retained raster-node and content-addressed
mip lookup state before each thumbnail. The unchanged regression **failed** at
`edited thumbnail must hit cached mips`:

- Cold: `(hits, rebuilds) = (0, 350)`, 516.208 ms.
- After opacity edit with reuse disabled: `(0, 700)`, 245.069 ms.
- The edit rebuilt all 350 mips and had zero hits.
- Fixed implementation: `(44, 350)` after the edit in every loaded run.

The fault was restored from a byte-for-byte source backup, and `cmp` verified
restoration. The fault is not part of any commit. The restored regression passed.

### Gates

- `cargo test --locked --release -p compositor`: PASS, 329 passed / 0 failed /
  11 ignored across 57 result groups (including doc-tests).
- Initial `cargo test --locked --release -p tessera-ffi`: FAIL in the unrelated
  `develop::export_batch_does_not_starve_slider_drag` adaptive-level assertion:
  20/120 frames at L2 versus a required 90%; p90 render time was 15.2 ms.
  The original output is preserved in `evidence/ffi-initial-failure.log`.
  No code in that path was changed. The isolated retry passed with 114/120
  frames at L2 and 9.1 ms p90 render time. The complete suite then passed with
  `--test-threads=1`: **535 passed / 0 failed / 28 ignored**, 49 result groups.
  Existing ignored tests were not enabled; no additional tests were skipped.
- `cargo clippy --locked --all-targets -p compositor -- -D warnings`: PASS.
- `cargo clippy --locked --all-targets -p tessera-ffi -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- FFI build: PASS (exit 0), regenerated bindings unchanged; arm64 archive built.
- Initial full Swift gate: FAIL. 861 XCTest cases, 3 skipped, four assertions in
  `DocumentSaveSheetProbeTests.testClosingCapturedParentDuringRealFolderChooserDrainsNativeSheet`;
  five Swift Testing tests passed. The gate correctly rejected XCTest's
  `(0 unexpected)` summary. Failure output is preserved in
  `evidence/swift-initial-failure.log` and `evidence/swift-gate-initial.log`.
  This native folder chooser test is outside the changed mip/thumbnail paths.
  An unchanged isolated retry passed (6.544 seconds; exit 0), recorded in
  `evidence/swift-isolated-retry.log`.
- Second full Swift gate after a fresh successful FFI build: FAIL. The gate named
  `MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth`
  and six `SmartPreviewThumbnailTests` cases (see `evidence/swift-gate-retry.log`
  for the complete names and a malformed/interleaved FFI diagnostic). All 11
  tests in those two suites then passed unchanged in isolation (6.879 seconds,
  exit 0; `evidence/swift-second-isolated-retry.log`). This does not establish a
  definitive root cause for either full-suite failure. Shared-machine load was
  extreme during these runs (observed one-minute load averages up to 826).
- Final complete Swift gate: FAIL (exit 1), 861 XCTest cases, 3 skipped, 1 failure;
  all five Swift Testing tests passed. The remaining failure is
  `ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`
  at `apps/mac/Tests/TesseraCoreTests/ShellLayoutTests.swift:194`:
  `document-1728x1117-stack-history-open historyBody overlaps historyHeader`.
  Reported frames: `{{900, 412}, {288, 192}} / {{1440, 1085}, {288, 32}}`.
  Full suite duration: 424.039 seconds. See `evidence/swift-gate-final.log`
  and `evidence/swift-final-failure.log`.

**Required `SWIFT GATE OK` was not achieved.** No Swift source or test was changed,
no test was suppressed, and no gate was weakened. Machine A must resolve or
re-verify the unrelated Swift gate failures before accepting/integrating this
package. Three full attempts and passing targeted retries are reported honestly;
the underlying cause of the Swift failures remains unverified.

## Reproduction

Run from this worktree with:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-35"
cargo test --locked --release -p tessera-ffi --test document_viewport \
  composite_thumbnails_reuse_mips_across_edits -- --exact --nocapture
```

`evidence/loaded-runner.sh` records the exact ten-run/load procedure used, including
checks that the separate Cargo build remains alive throughout every run. Its load
target was fresh for this verification; use another fresh directory if replaying
so an already-cached build cannot finish before the loop.

The successful complete FFI retry was:

```sh
cargo test --locked --release --no-fail-fast -p tessera-ffi -- --test-threads=1
```

All original failures and retries remain explicit; the serial FFI pass does not
claim that the default concurrent suite was clean on its first attempt.

## Commits and scope

- `5ccc48fe` — `test(B5-35): assert thumbnail mip reuse with cache diagnostics`
- `153fec6c` — `fix(B5-35): expose opt-in mip cache reuse diagnostics for tests`
- The following `docs(B5-35):` commit contains this handoff and evidence.

Base at start: `68264c74c365d8e7a4c33ddaecebf934f9305b08`, matching local
`origin/main`. All changes stay on `wp/B5-35`. `board.json` and `Cargo.lock`
are unchanged. No GUI app launch, screen capture, push or merge was performed.
Machine A owns review/integration.
