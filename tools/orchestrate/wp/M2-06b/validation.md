# M2-06b validation

RESULT: PASS

## Reconciliation

Reproduced both original `resident_model` panics with
`cargo test -p image-core --release resident_model -- --nocapture` before editing.
Revision-2 defaults activate Detail, so the old `has_m2_settings` routing selected
the host whole-image path despite a resident backend. The fix keeps default
sharpening/chroma NR enabled and retains the model's host-execution panic.

- Bayer recipes with Detail and basic tone now stay resident, including surface
  presentation. Extended tone/color/effects/geometry and X-Trans retain their
  existing hybrid fallback.
- Detail gathers immutable WB neighbours at the requested output level, using
  period-1 RGB edge clamping. Sensor reconstruction still uses level-0 CFA-phase
  clamping before the odd-origin crop and downsample.
- The existing GPU Detail decomposition/filter shaders execute within the same
  ordered transaction. Their pipelines are created once per backend. Detail
  strips its halo on-device and caches its interior before Tone. A tone edit
  reuses Detail; WB and Detail edits invalidate the appropriate chain key.
- The hybrid path explicitly disables Detail in its upstream WB-only recipe, so
  the default Detail pass is not applied twice.
- An expanded GPU regression exposed a 3-code error when disabling Detail after
  WB edits at L2. The extra output-demosaic checkpoint now retains f32 instead of
  introducing a second f16 rounding before WB. Sensor demosaic, WB and Detail
  memo outputs remain f16. Cache accounting uses actual GPU buffer bytes, and
  cold/warm behavior stays deterministic. No tolerance was increased.
- Zero-halo/disabled Detail still validates controls. A new regression failed
  before that check was added (`detail-validation-red.log`).

## Verification

Executed the exact requested chain personally, exit 0:

    cargo test --workspace --release && cargo clippy -p image-core -p pipeline-gpu -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check

Evidence: `final-validation.log`. Parsed Cargo summaries: 467 passed, 0 failed,
12 ignored. Existing vendored LibRaw C++ warnings remain and do not fail Clippy.
One earlier run failed the unrelated, stochastic
`ml-embed::hnsw_top_five_matches_exact_for_seeded_thousand_vectors` test
(`workspace-hnsw-flake.log`). The entire chain passed on retry without modifying
that crate or skipping the test.

Coverage includes unchanged display <=2-code and scene-linear <=0.005 model
parity, odd crop phase, default/edited/disabled Detail, cold/warm determinism,
L0/L1/L2 and tiny L12 GPU outputs, explicit matrix/Detail pixel counts and cache
invalidation, a cold partial preview requiring unrequested halo neighbours with
a zero-byte cache, one GPU submission/final readback, and zero warm-edit uploads.
Existing surface/histogram, cache-budget, GPU operator, WB revision-2 and golden
regressions pass in the workspace gate. `resident-green.log` also records the
expanded GPU resident suite passing separately.

## NEF L2 interactive benchmark

Host: Apple M4, Metal. Fixture: `fixtures/raw/nikon-nef.NEF`, active 7378x4924,
L2 output 1845x1231. Unmodified native revision-2 defaults are active.
Command (exit 0 for every run):

    cargo test -p tessera-ffi --release --test develop bench_slider_latency -- --ignored --nocapture --test-threads=1

Final implementation, milliseconds:

| Evidence | GPU first | GPU tone median | GPU tone p90 | GPU tone max | GPU WB median | CPU tone median | CPU WB median |
|---|---:|---:|---:|---:|---:|---:|---:|
| benchmark-final.log | 698 | 10.4 | 15.7 | 24.3 | 46 | 419.9 | 521 |
| benchmark-final-repeat.log | 517 | 4.8 | 5.6 | 6.1 | 23 | 426.7 | 502 |
| benchmark-final-repeat-2.log | 376 | 4.2 | 4.9 | 5.5 | 21 | 416.9 | 479 |

Two consecutive final-code runs meet the M2-06 median acceptance targets:
tone <=5 ms and WB <=40 ms. The slower first final-code run is retained above,
not discarded. These measurements are wall-clock develop presentation including
surface completion and histogram readback, with 40 tone edits and six WB edits.
They demonstrate the median targets, not a worst-case latency guarantee. Host
load was not controlled, so no specific cause is asserted for the slower run.
The earlier pre-precision-fix run (`benchmark.log`) was 4.4 ms tone / 23 ms WB.

Also ran the separate CPU-readable resident benchmark, exit 0:

    cargo test -p pipeline-gpu --release --test resident bench_nef_level2_resident -- --ignored --nocapture --test-threads=1

Evidence: `resident-benchmark.log`. Median of five: GPU first 341.890 ms,
tone 6.490 ms, WB 23.663 ms, versus CPU 736.698 / 480.540 / 560.486 ms.
This path includes a full pixel readback and does NOT meet the 5 ms tone target;
it is not the IOSurface interactive path used for M2-06 acceptance. Every render
submitted once, and warm tone/WB edits uploaded zero source tiles. Cold resident
payload allocations were 493805904 bytes; retained cache after WB was 447320788
bytes, within the configured 512 MiB budget. This is not a whole-process bound.

## Scope

All modifications are under the user allowlist. `git diff --check` passes.
`CARGO_TARGET_DIR` remained `/Users/rutmehta/.cache/tessera-target/M2-06b` for all
Cargo commands, and no worktree `target/` was created. No commit or push made.
X-Trans/extended-control hybrid barriers remain out of scope, not silently
claimed as a universal resident path.
