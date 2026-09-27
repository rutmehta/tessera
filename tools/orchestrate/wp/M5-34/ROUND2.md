# M5-34 Round 2: implementation and measured status

Current status: **PASS for the revised Fit/L2 median rule**, verified in three
fresh runs plus the exact correctness gate. See [RETRY2.md](RETRY2.md) for all
new measurements and p95 caveats. No idle-host result is claimed.

The earlier attempt below failed repeatable performance acceptance: one pair
met all median limits, but its final repeat missed the dashed median (and typing
at nice-20). Those measurements remain unchanged for provenance. No commit or
push was made.

## Retained changes

- `render/mod.rs`: separate premultiplied tile collection from the public planar
  conversion. `render_level_rgba` now fuses straight-alpha conversion with final
  interleaving. This avoids a second complete viewport allocation/copy and
  retains the exact reciprocal-then-multiply alpha arithmetic.
- `render/live.rs`: hash prepared geometry's exact binary coordinates and contour
  structure instead of serializing large dashed outlines as decimal JSON.
  Paint is still included separately in source identity. Fill rule, closed bits,
  contour lengths, anchor/control coordinates all participate. Runtime-only keys.
- `tests/live_frame.rs`: exact-bit comparison with the original planar frame
  assembly at U8/U16/F32, odd canvas edges, fractional alpha, HDR/negative values,
  live edits and L0–L3. A separate inside-dashed regression checks one geometry
  preparation across multiple tiles and levels, coverage/source cache hits on
  repeat, and exact warm/cold frame parity after a handle edit.
- `tests/live_latency.rs`: retain the real 5472×3648 photo and 43 sequential edits
  at L2/L3, with separate typing, fill-only polygon handle, and inside-dashed
  polygon handle cases. The dashed case uses width 24, dash lengths [32,16],
  inside alignment, default butt caps/miter joins, plus solid fill. The handle
  changes actual path coordinates, not just a layer translation. Cold photo mip
  generation is outside the timer. Document apply through completed interleaved
  CPU RGBA frame remains timed.

Round 1 already prepares fill/stroke/dash geometry once per model/transform,
shares that immutable geometry across levels, and caches raster coverage/source
per level/tile. The added regression verifies this stronger cross-level geometry
reuse. No per-tile dash-generation path was added. The existing `live_tile` hook
is still available to M5-31. Resident upload/damage scheduling is unchanged.

## Profile and rejected experiment

`evidence/round2-baseline.log` is the expanded workload before production edits.
Its L2 medians were 8.089ms typing, 17.915ms fill-only and 59.706ms inside-dashed.
The stage split measured roughly 3ms median in full-frame interleaving alone,
plus full-frame unpremultiplication in tile rendering, even when most root tiles
were hits. Counters showed no photo mip rebuilds after the cold frame.

`evidence/round2-sample.txt` is a real five-second `/usr/bin/sample` capture of
that release benchmark. The sampled stroke-preparation branch included 511
samples under `Stroke::outline`, with 470 under `Path::boolean`, including Kurbo
flattening and Boolean normalization. Coverage normalization and final frame
assembly were other visible costs. These sample counts are diagnostic, not
per-frame timings or a claim that all wall latency is CPU work.

An exact per-cubic flatten memo was tried. Its parity tests passed, but the
benchmark worsened (inside-dashed L2 median 120.184ms in that contended run).
The entire experiment was removed; no vector production edits remain.
`round2-cubic.log` / `round2-flatten-tests.log` are trial evidence, not final code.
No coarser tolerance, line substitution for degenerate cubics, changed Boolean
normalization, or approximate bitmap translation was retained.

## Acceptance rule and reproduction

Round 2 explicitly says pass means the stated medians at Fit/L2 for the ~1400px
viewport. The ignored test now prints and enforces median <16ms typing/fill and
<33ms inside-dashed at L2. It always logs p95 and explicitly prints missed p95
targets. `M5_34_STRICT_P95=1` additionally enforces the original L2 p95 targets
(<25ms typing/fill, <50ms dashed). L3 is diagnostic, not a substitute for L2.

```sh
export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-34
export M5_34_PHOTO_RGB=/tmp/m5-34-photo.rgb
cargo test -p compositor --release --test live_latency -- --ignored --nocapture
# Optional original tail gate:
M5_34_STRICT_P95=1 cargo test -p compositor --release --test live_latency -- --ignored --nocapture
```

The existing packed-photo fixture was reused. Preparation and provenance are in
RESULTS.md. No new synthetic image was substituted. The measurement pairs below
ran the freshly built test executable directly, with RAYON_NUM_THREADS unset,
under `/usr/bin/time -p`, first normal priority, then `nice -n 20`. Other processes
were not killed, suspended or reprioritized. **Nice-20 lowers the benchmark's own
priority; it does not isolate it from competing work.** Genuine idle/isolation
acceptance has not been established.

## First final-source measurement pair

Same production code as the final repeat. The benchmark still enforced its old
strict tail rule at this point, so both processes exited 101 despite meeting all
Round 2 medians. Values are median / p95 in ms, 43 edits per cell.

| Case | Normal | Nice-20 |
|---|---:|---:|
| Typing L2 | 5.911 / 33.361 | 4.188 / 16.206 |
| Fill-only L2 | 6.600 / 30.428 | 6.837 / 36.154 |
| Inside-dashed L2 | 31.907 / 76.762 | 22.857 / 41.225 |
| Typing L3 | 1.473 / 12.128 | 1.708 / 12.009 |
| Fill-only L3 | 1.159 / 11.038 | 1.732 / 8.752 |
| Inside-dashed L3 | 15.048 / 38.457 | 12.889 / 41.917 |

Normal: load 40.46/64.55/88.09 before, real 4.04s, user 2.18s, system 0.17s.
Nice-20: load 39.06/63.86/87.71 before, real 3.17s, user 2.18s, system 0.16s.
Logs: `evidence/round2-final-normal.log`, `evidence/round2-final-nice20.log`.

## Final repeat with explicit Round 2 median gate

Both executions exited 101. The normal run missed the inside-dashed L2 median;
the nice-20 run missed both typing and inside-dashed L2 medians. p95 is reported
without trimming outliers or excusing misses as a pass.

| Case | Normal | Nice-20 |
|---|---:|---:|
| Typing L2 | 7.504 / 63.587 | 21.489 / 70.602 |
| Fill-only L2 | 11.772 / 66.261 | 7.835 / 72.585 |
| Inside-dashed L2 | 57.675 / 148.909 | 51.229 / 95.610 |
| Typing L3 | 2.166 / 20.299 | 1.040 / 57.605 |
| Fill-only L3 | 1.427 / 14.845 | 0.964 / 23.558 |
| Inside-dashed L3 | 26.332 / 102.160 | 18.272 / 133.484 |

Normal: load 43.23/56.66/81.90 before, real 7.37s, user 2.17s, system 0.14s.
Nice-20: load 47.10/57.05/81.74 before, real 7.38s, user 2.13s, system 0.13s.
Logs: `evidence/round2-acceptance-normal.log`, `evidence/round2-acceptance-nice20.log`.
These load changes prevent attributing the between-run timing differences solely
to the code changes. A passing repeatable isolated run is still needed.

## Required correctness gate

Executed the exact command personally after final code/test formatting:

```sh
cargo test -p compositor -p typography -p vector --release && cargo clippy -p compositor -p typography -p vector --all-targets -- -D warnings && cargo fmt --check
```

Exit 0, **318 passed, 0 failed, 11 ignored**, then clippy and fmt passed.
Evidence: `evidence/round2-final-gate.log`. The ignored latency test was run
separately as shown above. Existing exact text/vector parity tests passed.
All Cargo commands retained the externally configured target directory.
The pre-existing brief.md modification was left intact. No resident, styles,
effects, document/edit, or other disallowed source was modified.
