# B5-29d: streaming Lightroom import over the FFI

Branch `wp/B5-29d`; review started at `a4b2c72a`, comprising `wp/B5-29c`
base `3eeeaefc` plus five replayed 29d commits. This review adds commits on top;
no rebase, push, board edit, or Cargo.lock change.

## Review fixes and gates (2026-10-01)

The retained-source parity assertions now require `shape = "lua-values"` and
read `UprightVersion`, `RetouchAreas`, and `ExtendedToneCurvePV2012` under
`properties`, matching 29c round three (`6be9003b`).

Non-blocking review items:

- **(a) Done:** publish missing retained side files on every apply, including
  resume with an existing plan and a later session with new oversized cells.
  Existing identical cells are reused; conflicting bytes at the same relative
  filename fail explicitly without overwriting the earlier bundle. Regression
  coverage repairs a deleted cell, publishes a new session's cell, and checks
  conflict rejection and original-byte preservation.
- **(b) Done:** a per-photo spool read failure checkpoints completed sidecar
  writes before returning the error. A fault-injection unit test truncates the
  spool after the first record has loaded, verifies one durable `done` entry,
  and verifies that a fresh session resumes it. A simultaneous state-write
  failure still returns an error; this does not add crash/power-loss durability
  between the existing 50-photo checkpoints.
- **(c) Deferred:** inspect still uses omission/prefix descriptors, whereas open
  externalizes cells and counts the retained bytes. Oversized-cell disk estimates
  and reason strings can therefore differ. Swift calls open and receives that
  path's estimate/reasons. Aligning the two needs a shared accounting/reporting
  contract without making inspect retain the full oversized data.
- **(d) Deferred:** no aggregate disk cap for retained side files. Ordinary I/O
  failures are reported; successful externalization can use unrestricted disk
  space. A cap needs an explicit budget and an observable retention-failure policy.
- **(e) Deferred:** sidecar recovery references remain bundle-relative `large/...`
  paths without identifying their owning bundle. Bundle identity/relocation
  semantics need an explicit representation before changing serialized recipes.

Serial Rust gate results on source commit `2a97e529`, using
`PATH="$HOME/.cargo/bin:$PATH"` and
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-29d`:

| Command | Result |
| --- | --- |
| `cargo test --release -p import-lrcat -p tessera-cli -p tessera-ffi --test lrcat --test lrcat_streaming_parity` | PASS: 7 + 2 tests |
| Same package set with `--lib lrcat` | PASS: 1 importer + 5 FFI tests |
| `cargo test --release -p tessera-ffi` | PASS: 541 passed, 28 ignored, 0 failed; includes the allocation gate (116.93 s) and parity tests |
| `cargo test --release -p import-lrcat` | PASS: 62 passed, 0 failed, 0 ignored; includes scale, golden and retention coverage |
| `cargo test --release -p tessera-cli --bin tessera import::tests` | PASS: 3 passed |
| `cargo test --release -p tessera-cli --test import_models` | PASS: 5 passed |
| `cargo clippy --release --all-targets -p import-lrcat -p tessera-cli -p tessera-ffi -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |

The 28 ignored tests remain the suite's default ignored benchmark/exclusive
fixture tests; none were newly ignored. The first development attempt found
missing `Arc` clones in test setup, then a regression-test lookup used the
uncanonicalized fixture path. Both test issues were corrected before the passing
runs above. The final full FFI run covers the corrected source and tests.

Logs are local at `/tmp/B5-29d-review-*.log` (integration, lib, ffi, importer,
cli-unit, cli-models, clippy, fmt). All commands exited 0. Upstream LibRaw C/C++
build scripts emit deprecation warnings; Rust clippy with warnings denied passes.
No Swift gate, FFI binding regeneration, or real-copy performance run was repeated
in this Rust-only review.

The real catalog (including the provided copy) was not opened during this review.
Performance and Swift results below are historical, not current-base acceptance.

## Historical acceptance on 041456c9 (2026-10-01)

Measured the supplied read-only catalog copy after this worktree's builds and
tests finished, with all roots relocated to absent private temporary folders.
Inspect: 21,656 images, 21,615 edited, 0 virtual copies.
Apply: 21,656 images, 0 imported, 21,656 missing originals,
0 virtual copies. Counts are recorded here only; no filenames or catalog
contents are included. This is a catalog/bundle benchmark, not accessible-original
sidecar/indexing throughput. The machine is shared, so external contention is
not controlled.

| Measurement | Historical 041456c9 result |
| --- | ---: |
| Inspect | 11.265 s |
| Inspect process wall, including cleanup | 11.88 s |
| Inspect maximum RSS | 755,695,616 bytes |
| Inspect peak footprint | 624,886,792 bytes |
| Open | 10.443 s |
| Apply after open, including missing-originals preflight | 0.582 s |
| Open + apply total | 11.026 s |
| Open + apply process wall, including cleanup | 11.08 s |
| Open + apply maximum RSS | 698,531,840 bytes |
| Open + apply peak footprint | 549,471,192 bytes |

All targets met: inspect < 20 s, open + apply < 60 s, both RSS and footprint
< 1 GB. Full SHA-256 equality was checked before and after; prefix
**`eb60e744dbec2547` remains unchanged**. No original catalog or photo was modified.

### Historical gates on 041456c9

Passed serially with `PATH="$HOME/.cargo/bin:$PATH"` and
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-29d`:

- `cargo test --release -p import-lrcat -p tessera-cli -p tessera-ffi --test lrcat`
- Same package set with `--lib lrcat` (the filter selects four FFI unit tests;
  importer unit tests are exercised by the full importer command below).
- `cargo test --release -p import-lrcat`, including golden, retention,
  oversized-memory, real-catalog-shape and scale tests.
- `cargo test --release -p tessera-cli --bin tessera import::tests`
- `cargo test --release -p tessera-cli --test import_models`
- FFI `lrcat_streaming_parity` (two tests) and `lrcat_streaming` (allocation gate).
- `cargo clippy --release --all-targets -p import-lrcat -p tessera-cli -p tessera-ffi -- -D warnings`
- `cargo fmt --all -- --check`
- `apps/mac/build-ffi.sh`; no tracked Swift/C binding changes.
- Release measurement-driver build.

`tools/orchestrate/swift-gate.sh`: **FAILED**, exit 1. Swift compilation passed.
The full XCTest run reported three failures and then stopped making progress;
a sample showed XCTest waiting with AppKit animation threads blocked. After
confirming the exact runner belonged to this worktree, it was terminated with
SIGTERM. This was not a completed full-suite pass; no completed XCTest total
is claimed. The separate five Swift Testing tests passed.

- `MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth`:
  window capture returned no `CGImageSourceRef`. Both `IOConsoleLocked` and
  `CGSSessionScreenIsLocked` were verified true.
- `DocumentAdaptiveWideAngleTests.testOKSurfacesAFailedFinalTrace`: expected
  constraint-error string was nil in the full run; **passed on isolated rerun**.
- `ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`:
  history-panel bounds/overlap assertions failed in the full run;
  **passed on isolated rerun**.

The two unexpected failures were rerun together using release `swift test
--skip-build --filter` with their exact test names: two tests passed in 28.820 s.
No Swift source changes were made, and the full gate remains recorded as failed.

## Measurement protocol

`crates/tessera-ffi/examples/lrcat_measure.rs` calls the public FFI directly.
Run its `inspect` and `apply` modes separately under `/usr/bin/time -l`.
The apply mode includes `Engine::open_lrcat`, and reports open, apply, and total
wall time separately. It relocates every photo root to a nonexistent directory
under a private temporary directory. Thus it writes the import bundle and
library through the real FFI, but never writes sidecars beside real originals.
The driver now also checks the mapping preview and refuses apply unless every
master original is missing; this preflight is included in the after apply time.
This measures the catalog/bundle import path; it does not measure writing or
indexing 21k accessible originals. Synthetic fixture integration tests cover
sidecar writes, mapping, fidelity, cancellation, resume, and indexing.

The supplied read-only catalog copy was verified before measurement:
SHA-256 prefix `eb60e744dbec2547`. No source photo or original catalog is used.

## Implementation

Both `inspect_lrcat` and `Engine::open_lrcat` consume
`import_each_with_storage`. Inspect passes no storage and reports bounded
prefix/omission descriptors for oversized cells. Open owns a private staging
directory and externalizes oversized cells losslessly under `large/`. Apply
publishes and syncs these side files before publishing `import-plan.json`;
its disk estimate includes their bytes. The staging directory stays alive with
the import object for retries. Normal-size source rows remain in the spool. Full imported
records are serialized and dropped as they arrive. Inspect only counts their
serialized size; open also appends them to a buffered temporary bundle and
records byte offsets. A dedicated metadata type keeps only the fields needed
for summary, relocation, selection mapping, keywords and library merge.

Apply copies the spool to a same-directory temporary file, syncs it, and
atomically publishes `import-plan.json`. It reads one full record at a time
for sidecar writes. Fidelity sampling uses the same random-access records. Separate reopened file handles avoid sharing a seek
cursor between concurrent FFI calls. The temporary spool is owned by the import
object and removed when it is dropped. Open therefore needs temporary disk space
for one compact bundle; apply holds that spool and the persistent copy until
the import object is dropped. Source catalog changes after open cannot
change the pinned records. No whole-plan serialization or heavy recipe/history
retention remains in the app path.

The public FFI signatures and Swift import state machine are unchanged. The
Swift controller already calls `Engine.openLrcat` off the main actor. Summary
counts, grouped diagnostics, exact disk estimate, mapping, fidelity, progress,
cancel/resume, conflict protection and indexing retain their existing behavior.
The bundle retains the same JSON values for normal cells, including source rows
and unknown recipe members; oversized cells use the new base's lossless relative
references. JSON object member order is not significant. Degraded-image reports
stay individual in the FFI summary (one issue per image); ordinary repeated
warnings still aggregate. `recipe.unknown["lrcat_develop_source"]` retains the
base's original Adobe literals, including inactive settings and identity curves.
Malformed develop rows retain their complete text; oversized develop rows carry
the external recovery reference with `truncated=false` after open.

New integration coverage checks individual malformed/oversized diagnostics,
unconditional retention of UprightVersion, RetouchAreas and an identity extended
curve, byte-exact oversized-cell publication, pinned data after source mutation,
repeat apply, and retained files after the import object is dropped.

## RED evidence

The isolated counting-allocator test uses the B5-29c synthetic catalog generator
and the allocator from `import-lrcat/tests/scale.rs`. Before implementation,
inspect peak tracked heap was 289,801,483 bytes at 2,000 images and 2,800,036,234
bytes at 20,000 images. The scaling assertion failed as intended (98.765 s for
20,000-image inspect). The scale test measures open-plus-apply separately.
A separate 200-image parity test checks the generated summary, exact disk estimate, history counts
and complete parsed bundle equality against the serialized `import()` result.
SQLite allocations are outside the Rust allocator; the real-copy process measurement covers them.

The final scale run passed: 32,777,172 bytes at 2,000-image inspect,
62,778,283 bytes at 20,000-image inspect, and 63,705,450 bytes for open + apply.
The 20,000-image inspect took 48.449 s under severe shared-machine contention;
the synthetic test uses a loose 120 s timeout and strict allocation bounds.
All six existing FFI integration tests passed, as did the new parity test.

The first parity test compared parsed JSON to `serde_json::to_value` of native
structs. That is not a valid byte-serialization reference for `f32`: one curve
coordinate became `0.501960813999176` via promotion to `f64`, versus the correctly
serialized decimal `0.5019608`. Both sides now parse their serialized bytes.
A first-difference diagnostic avoids dumping the entire catalog on failure.

## Previous-base before / after (historical)

Real-copy counts: 21,656 images, 21,615 edited, 0 virtual copies. Apply reports
0 imported and 21,656 missing originals under the intentionally absent relocated
roots. Times below exclude temporary-directory cleanup unless marked process
wall. RSS and footprint are bytes, as reported by macOS `/usr/bin/time -l`.
Measurements ran on a shared machine; baseline inspect overlapped the RED test
and compilation, so wall-time speedups should not be treated as controlled
microbenchmarks.

| FFI measurement | Before | After |
| --- | ---: | ---: |
| Inspect | 97.831 s | 12.678 s |
| Inspect maximum RSS | 1,349,861,376 | 519,405,568 |
| Inspect peak footprint | 4,120,792,544 | 484,934,520 |
| Open | 68.621 s | 13.492 s |
| Apply after open | 14.177 s | 0.640 s |
| Open + apply total | 83.021 s | 14.132 s |
| Open + apply maximum RSS | 3,431,104,512 | 517,193,728 |
| Open + apply peak footprint | 4,621,815,648 | 527,909,800 |

Process wall times including cleanup: inspect **98.82 → 14.26 s**;
open + apply **86.65 → 14.42 s**. The after run was performed after this
worktree's build/test commands completed. Targets are met: inspect < 20 s,
apply (including open and cleanup) < 60 s, both RSS and footprint < 1 GB.
These are catalog/bundle measurements with intentionally missing relocated
originals, not a 21k-original sidecar/indexing benchmark.

The complete SHA-256 was checked for exact equality before and after the final
measurement; prefix **`eb60e744dbec2547`** remains unchanged. Counts match the
before run. No original catalog or source photo was modified.

## Previous-base gates (historical)

Passed:

- `cargo test --release -p import-lrcat -p tessera-cli -p tessera-ffi --test lrcat`
- Same package set with `--lib lrcat`
- `cargo test --release -p import-lrcat` (all targets, including golden and scale)
- `cargo test --release -p tessera-cli --bin tessera import::tests`
- `cargo test --release -p tessera-cli --test import_models`
- FFI `lrcat_streaming` and `lrcat_streaming_parity` integration tests
- `cargo clippy --release --all-targets -p import-lrcat -p tessera-cli -p tessera-ffi -- -D warnings`
- `cargo fmt --all -- --check`
- `apps/mac/build-ffi.sh`; regenerated Swift/C bindings are unchanged

`tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**. 861 XCTest tests,
3 skipped, 0 failures; 5 Swift Testing tests in 2 suites passed.
Builds and test commands run sequentially with the requested external Cargo
target directory. No GUI launch or screen capture.

## Base and local commits

Current 29c base: `3eeeaefc` (includes round-three source shapes at `6be9003b`).
Review starting tip: `a4b2c72a`. The five replayed commits actually present are:

| Current commit | Purpose |
| --- | --- |
| `851810a7` | allocation test and measurement driver |
| `6d589386` | streaming FFI and parity coverage |
| `0516abc0` | missing-originals measurement guard |
| `1afa1dcf` | handoff documentation |
| `a4b2c72a` | lossless source retention and publication |

The old `041456c9` base and earlier replay hashes are superseded. In particular,
the body of `a4b2c72a` describes the previous base; the ancestry above is the
current authority. No history was rewritten during this review.

Review source commit:

| Commit | Purpose |
| --- | --- |
| `2a97e529` | retained-source shape assertions; resume side-file publication and conflict protection; spool-error checkpoint and regression tests |

The following documentation commit records the verified gates and corrected base.
All new commits are local only and carry the requested co-author trailer.
