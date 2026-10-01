# B5-29d: streaming Lightroom import over the FFI

Branch `wp/B5-29d`, based on `wp/B5-29c` at `324566da`. Local commits only.

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

Both `inspect_lrcat` and `Engine::open_lrcat` consume `import_each`. Full imported
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
The bundle retains the same JSON values, including all source rows and unknown
recipe members; JSON object member order is not significant.

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

## Before / after

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

## Gates

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

## Local commits

B5-29c follow-up: `87670d44` RED crash test, `b0eb7ab1` durability/refactor,
`5027d39b` durability documentation. These three can be cherry-picked as a
group without the B5-29d FFI changes.

B5-29d: `44f23d74` RED allocation test and measurement driver, `7ce2ea74`
streaming FFI and parity coverage, `7c05895c` measurement preflight guard.
All carry the requested co-author trailer. No push, board edit or Cargo.lock
change.
