# M2-06 implementation and host validation

## Latest retry: presentation calibration corrected

The existing resident implementation was present at the start of this retry.
The new change fixes automatic backend selection: calibration now times the
actual IOSurface presentation path plus histogram on both backends, rather than
comparing GPU pixel readback against CPU tiles. The CPU branch uses the same
`develop::write_level` function as the live viewport. Temporary calibration
surfaces are owned and released, not leaked by the test helper.

A regression was run before the change and failed with 149688 pixel-readback
bytes instead of zero (`calibration-surface-red.log`). It then passed on Metal
with zero pixel readback, three histogram readbacks and three submissions
(`calibration-surface-green.log`). The final suite also exercises the CPU
calibration branch, zero-sized allocation rejection, retained surface access,
and release of the final owning reference.

The exact required test/clippy/fmt command was rerun personally after these
changes and exited 0: **92 passed, 0 failed, 7 ignored**. Evidence:
`calibration-final-validation.log`. Existing LibRaw C++ warnings remain.
`git diff --check` passed, all dirty/untracked paths are allowlisted, the
external `CARGO_TARGET_DIR` remained set, and no local `target/` exists.

The ignored develop benchmark was also rerun on Apple M4 using the NEF at L2:

| Backend | First frame | Tone median | Tone p90 | Tone max | WB median |
|---|---:|---:|---:|---:|---:|
| CPU | 256 ms | 10.2 ms | 11.0 ms | 12.1 ms | 77 ms |
| Metal | 481 ms | 4.1 ms | 4.3 ms | 4.8 ms | 7 ms |

Evidence: `calibration-develop-benchmark.log`. Both interactive latency targets
pass for the Bayer/basic-tone path. First-frame Metal remains slower.

**Overall work package remains incomplete**: X-Trans and extended M2 controls
still use hybrid barriers. This retry does not claim a universal resident
whole-chain implementation or completion of those missing paths.

## Result

The required Cargo gate and both interactive latency targets now pass on the
Apple M4 host. Three post-optimization runs measured GPU tone medians of
4.0 / 4.3 / 4.2 ms and WB medians of 8 / 8 / 8 ms on the NEF at level 2.
Full work-package coverage is still **not complete**: X-Trans and extended M2
image-level controls remain explicit hybrid fallbacks rather than a universal
resident single-submission path. The measured Bayer/basic-tone path is resident.

No commits were made. All repository changes are within the allowed paths.
Every Cargo command used the external target directory
`/Users/rutmehta/.cache/tessera-target/M2-06`.

## Implemented

- Additive, object-safe `StageOp::begin_resident` / `ResidentBatch` API and opaque
  resident handles in image-core. No engine-api changes.
- GPU `MemoKey` LRU with a configurable byte budget. Computed demosaic and WB
  outputs are packed f16. Decode source buffers intentionally preserve f32
  precision and count their full payload against the same budget.
- GPU halo gathering, crop/box resampling, output-level demosaic memoization,
  linear profile/WB matrices at preview resolution, and fused tone/display.
- One ordered compute pass and one explicit queue submission for a supported
  render, including all tiles and dirty stages. CPU consumers get one final
  staging map. Scratch buffers are reused inside the transaction; their live
  bytes are separate from persistent cache accounting.
- Direct RGBA8 IOSurface import via Metal
  `newTextureWithDescriptor:iosurface:plane:` and wgpu-hal `texture_from_raw`.
  Develop writes pixels directly, reading back only a 4096-byte GPU histogram.
  The surface-only API performs no readback at all.
- Surface presentation and histogram accumulation share one dispatch per tile,
  avoiding a second full traversal and its bind groups/parameters. Transfer
  diagnostics expose the last transaction's compute dispatch count. Tests check
  exact pixels, histogram counts, odd edge tiles, nonzero origins, untouched
  surface regions, histogram reset and repeated-frame determinism.
- Saved previews/loupes materialize the captured recipe on demand, instead of
  reading every displayed frame. Per-session writes are serialized; cancelled
  frames do not advance the presentation ring.
- Automatic CPU/GPU calibration on the first opened RAW. GPU is selected when
  both warm tone and WB edits are faster. Cold-fill time is reported separately.
  `TESSERA_RENDER_BACKEND=cpu|gpu` bypasses calibration.

## Required gate: PASS

Executed personally on the host after the final code changes:

```sh
cargo test -p pipeline-gpu -p image-core -p tessera-ffi --release && cargo clippy -p pipeline-gpu -p image-core -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check
```

Exit status 0. Release tests: **90 passed, 0 failed, 7 ignored**, across 28 suite
summaries including empty doctest suites. Clippy and formatting passed. Existing
LibRaw C++ build-script warnings are printed but are not Rust clippy failures.
Current evidence: `retry-final-validation.log`. The earlier
`final-validation.log` is retained as historical evidence.

Additional host commands also exited 0:

```sh
cargo test -p pipeline-gpu --release --lib single_pass_preserves_dispatch_and_clear_order -- --ignored --nocapture
cargo test -p pipeline-gpu --release --test resident_large -- --ignored --nocapture --test-threads=1
TESSERA_RENDER_BACKEND=gpu cargo test -p tessera-ffi --release --test develop -- --nocapture --test-threads=1
```

Evidence: `retry-single-pass.log`, `retry-large.log`, and
`retry-develop-gpu.log`. These exercise >4096 ordered dispatches/clears, a full
36 MP NEF transaction, and real develop sessions on forced Metal respectively.
The normal gate also executes GPU CPU-parity/determinism, cache budget/LRU,
IOSurface roundtrip, histogram parity with zero pixel readback, cancellation,
raw-source precision, fused tone/display, and lazy-preview regressions.

## Current measured performance: both interactive targets met

The surface/histogram fusion was preceded by a failing dispatch-count regression:
`fused-surface-red.log` records three dispatches where the test requires two
(one clear plus one fused pixel pass). After the implementation,
`fused-surface-green.log` passes pixel/histogram parity and dispatch count.
The final required gate also includes the multiframe/offset/partial-tile test.

On the same Apple M4 / 7378x4924 NEF / 1845x1231 L2 output, three final-code
benchmark runs produced these measured milliseconds:

| Run | CPU first | GPU first | CPU tone | GPU tone | GPU tone p90 | GPU tone max | CPU WB | GPU WB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 281 | 450 | 11.9 | 4.0 | 4.1 | 4.7 | 88 | 8 |
| 2 | 270 | 454 | 12.1 | 4.3 | 4.5 | 4.8 | 107 | 8 |
| 3 | 256 | 405 | 11.3 | 4.2 | 4.3 | 4.8 | 86 | 8 |

Evidence: `retry-benchmark-fused.log`, `retry-benchmark-fused-2.log`, and
`retry-benchmark-fused-3.log`. The benchmark command below was executed once per
run. Tone columns are based on 40 edits; WB on six edits. First-frame GPU time is
still slower than CPU. These are wall-clock develop-frame times, not GPU-only
timestamp queries, and they include the histogram readback and surface completion.
The pre-fusion run in this retry measured 4.9 ms tone / 9 ms WB
(`retry-benchmark-before.log`); host contention means the timing difference alone
is not a controlled causal comparison. The eliminated dispatch is separately
covered by the regression test.

## Historical performance: previous attempt missed the tone target

Host: Apple M4. Fixture: `fixtures/raw/nikon-nef.NEF`, active 7378x4924,
level 2 output 1845x1231. This command was run, not estimated:

```sh
cargo test -p tessera-ffi --release --test develop bench_slider_latency -- --ignored --nocapture --test-threads=1
```

Previous tone/display-fused repeated runs (milliseconds; tone and WB columns are medians
printed by the benchmark):

| Run | CPU first | GPU first | CPU tone | GPU tone | CPU WB | GPU WB |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 547 | 844 | 16.9 | 10.0 | 184 | 14 |
| 2 | 424 | 485 | 16.2 | 7.0 | 159 | 15 |
| 3 | 497 | 496 | 16.2 | 6.6 | 178 | 17 |

Evidence: `develop-benchmark-repeat-{1,2,3}.log`. Host process inspection showed
substantial concurrent CPU activity and memory compression. These are nevertheless
the actual results; no <=5 ms claim is made and no other workload was terminated.
The initial fused run was also slower under contention (GPU tone 10.4 ms), kept
in `develop-benchmark-fused.log` rather than discarded.

Before tone/display fusion, a less-contended develop run measured GPU tone 5.4 ms,
WB 9 ms, first 347 ms vs CPU 10.4 / 76 / 249 ms (`develop-benchmark.log`). This
older run is contextual, not final-code acceptance evidence.

The separate resident CPU-readable-output benchmark was also exercised before
fusion: medians GPU first/tone/WB 335.500 / 6.625 / 10.388 ms vs CPU
227.539 / 9.047 / 74.338 ms (`resident-benchmark-final.log`). It includes a final
full-pixel map and is not the IOSurface presentation benchmark.

## Hardware bugs found and fixed

1. A wgpu command encoder is not necessarily one Metal command buffer. wgpu 30
   opens one HAL command buffer for every compute pass. Per-dispatch passes hit
   Metal's 4096 outstanding-buffer cap on the NEF and caused device loss at map.
   A logger/device-loss regression established this (`large-device-diagnostic-3.log`).
   Recording dispatches and compute clears, then replaying one compute pass,
   fixed the full-image regression. Queue-write batching alone did not fix it.
2. Converting raw CFA sources to f16 before reconstruction introduced a 3-code
   display difference after WB edits, exceeding the unchanged 2-code tolerance
   (`parity-diagnostic.log`). A failing exact-source regression demonstrated the
   precision loss (`raw-precision-red.log`). Decode sources now remain f32,
   computed stage memo outputs remain f16, and both cold/warm source bytes and
   cache accounting are tested. The original graph tolerance was not loosened
   to hide this regression.

## Remaining limits

- X-Trans and non-neutral extended M2 whole-image controls use the existing
  hybrid path and its CPU/GPU barriers.
- LRU accounting bounds retained memo payloads, not all in-flight command,
  bind-group, source and scratch allocations. Streaming and reuse reduce the
  cold-frame working set but do not establish a hard whole-process memory cap.
- Calibration uses one image and single edit samples and can be affected by
  host contention. It adds startup work on the first image and does not dynamically
  switch every subsequent image or unsupported control family.
- Tone-only <=5 ms is demonstrated for the measured NEF/basic-tone path, not
  guaranteed for arbitrary recipes, images or host contention.
