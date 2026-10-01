# B5-29d: streaming Lightroom import over the FFI

Branch `wp/B5-29d`, rebased onto `wp/B5-29c` at `041456c9`. Local commits only.

## Current-base acceptance (2026-10-01)

Measured the supplied read-only catalog copy after this worktree's builds and
tests finished, with all roots relocated to absent private temporary folders.
Inspect: 21,656 images, 21,615 edited, 0 virtual copies.
Apply: 21,656 images, 0 imported, 21,656 missing originals,
0 virtual copies. Counts are recorded here only; no filenames or catalog
contents are included. This is a catalog/bundle benchmark, not accessible-original
sidecar/indexing throughput. The machine is shared, so external contention is
not controlled.

| Measurement | Rebased result |
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

### Current gates

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

## Rebase and local commits

New base: `041456c98a9362b22c7026b01747ad9bf732e263`.

Dropped all three old B5-29c durability commits:

- `87670d44`: old pre-publication visibility test. Its absent-destination assertion
  conflicts with 29c's explicit empty reservation contract. The base already tests
  failed/no-op publication cleanup, empty-reservation recovery and preservation
  of nonempty destinations; no unique compatible coverage was lost.
- `b0eb7ab1`: superseded by 29c's plain fsync with EINTR retry, directory syncs,
  and publication/recovery implementation.
- `5027d39b`: documentation for the superseded durability implementation.

The only rebase conflict was `apps/tessera-cli/src/import.rs` while replaying
`87670d44`; resolved by dropping that commit, retaining 29c verbatim. The other
two superseded commits were explicitly dropped. All four 29d commits replayed:

| Original | Rebased | Purpose |
| --- | --- | --- |
| `44f23d74` | `42dd56cd` | allocation test and measurement driver |
| `7ce2ea74` | `96b38c00` | streaming FFI and parity coverage |
| `7c05895c` | `4f76452d` | missing-originals measurement guard |
| `1f2be5dc` | `ed8a29ce` | handoff documentation |

All commits are local only and carry the requested co-author trailer. No push,
board edit or Cargo.lock change.
