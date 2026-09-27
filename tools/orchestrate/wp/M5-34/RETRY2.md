# M5-34 Round 2 verification retry

Status: **PASS under the revised Round 2 Fit/L2 median acceptance rule.**
The required correctness gate and all three fresh median-gated benchmark runs
passed. This does not establish idle-host isolation or a universal p95 guarantee.
Earlier failed runs in ROUND2.md remain preserved as evidence of variability.

## Implementation reviewed and retained

This retry inherited the uncommitted Round 2 implementation and preserved it
without further production edits. The retained changes are:

- `render/mod.rs`: fuse unpremultiplication and interleaved CPU frame assembly,
  eliminating an extra full-viewport allocation/copy. The planar API and exact
  reciprocal/multiply arithmetic remain unchanged.
- `render/live.rs`: hash prepared path coordinates and contour structure in
  binary instead of formatting large stroke outlines as JSON. Paint remains an
  independent input to source identity.
- `tests/live_frame.rs`: exact-bit planar/RGBA parity across depths, levels,
  alpha and live edits; inside-dashed geometry preparation once across tiles
  and levels, warm cache reuse, and exact cold/warm frame parity.
- `tests/live_latency.rs`: real-photo typing, fill-only handle and inside-dashed
  handle workloads with 43 sequential edits per case at L2 and L3.

Round 1 already provides bounded paragraph/run/positioned-outline caches,
sparse source and coverage caches, and old/new geometry damage with style
halos. Dash/stroke preparation is model/transform cached and shared across
levels; raster coverage is keyed by level and tile. No resident files changed.
M5-31's existing `live_tile` hook and remaining resident damage scheduling work
are documented in TEXT_VECTOR.md.

The previous attempt's actual sampling profile and rejected flatten-cache
experiment remain documented in ROUND2.md. No new speedup is attributed to this
retry: the source was unchanged, while contemporaneous host load was lower.

## Fresh measurements

Same 5472×3648 real RGB photo fixture, SHA256 verified in this retry:
`f0146b3f71a44f9de81a68b54a3a2a621bb7b80e5685bca609d7bb28265ec3b4`.
Each sample measures document edit through a completed interleaved CPU RGBA
viewport frame. Initial font/photo/mip setup is outside the timer. No Rayon
thread override was used for the nice-20 or final normal repeat.

Values are median / p95 in milliseconds, 43 edits per cell:

| Case | Initial normal | Nice-20 | Normal repeat |
|---|---:|---:|---:|
| Typing L2 | 7.224 / 40.535 | 4.290 / 6.241 | 4.416 / 6.095 |
| Fill-only L2 | 6.591 / 31.110 | 5.954 / 33.395 | 5.491 / 8.195 |
| Inside-dashed L2 | 21.302 / 41.116 | 14.089 / 21.723 | 15.405 / 24.670 |
| Typing L3 | 1.596 / 22.326 | 1.640 / 2.397 | 1.921 / 4.380 |
| Fill-only L3 | 1.402 / 13.135 | 1.725 / 4.042 | 1.777 / 2.272 |
| Inside-dashed L3 | 24.974 / 88.636 | 10.398 / 28.412 | 7.359 / 11.530 |

All three benchmark processes exited 0 under the revised rule: L2 typing and
fill medians <16ms, inside-dashed median <33ms. L3 is diagnostic only.
The initial normal run missed the original typing/fill p95 targets; nice-20
missed fill p95. The final normal repeat met every original L2 p95 target too.
No samples were discarded. Raw samples and cache counters are retained in:

- `evidence/retry2-normal.log`
- `evidence/retry2-nice20.log`
- `evidence/retry2-repeat.log`

Host load before the initial run was 33.87 / 48.04 / 72.33, before nice-20
30.84 / 46.20 / 70.97, and before the normal repeat 29.09 / 45.58 / 70.60.
Nice-20 used `/usr/bin/time -p nice -n 20 cargo test ...`; it lowers the benchmark
process's priority and is **not isolation**. No unrelated process was stopped or
reprioritized. Nice-20 command wall/user/system was 3.20/2.35/0.36 seconds; the
normal repeat was 2.28/2.32/0.24 seconds (includes Cargo startup).

## Correctness and scope

Personally executed, with
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-34`:

```sh
cargo test -p compositor -p typography -p vector --release && cargo clippy -p compositor -p typography -p vector --all-targets -- -D warnings && cargo fmt --check
```

Exit 0: **318 passed, 0 failed, 11 ignored**, then clippy and fmt passed.
The ignored latency benchmark was separately executed as recorded above.
Full output is `evidence/retry2-gate.log`. LibRaw C++ warnings were emitted but
did not fail the gate. Existing exact text/vector parity tests passed.

`git diff --check` and the modified/untracked allowed-path check passed.
No source outside the allow-list was modified, no build output was created in
`./target`, and no commit or push was made. The pre-existing brief modification
was preserved. Only evidence/status documentation was added in this retry.
