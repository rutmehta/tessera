# M5-34 evidence and integration notes

## Round 2 update

Current status: **PASS under the revised Round 2 Fit/L2 median rule**.
See [RETRY2.md](RETRY2.md): three fresh normal/nice-20/normal runs meet the
median targets, and the required correctness gate passes (318 tests).
P95 misses in some runs are reported honestly; no idle-host isolation is claimed.
The final normal repeat also met all original L2 p95 targets.

See [ROUND2.md](ROUND2.md) for the retained implementation, expanded fill-only
and inside-dashed benchmarks, actual sampling profile and earlier failed
measurements. All earlier evidence below remains historical, not a claim that
host contention or tail latency has been eliminated.

## Workload and reproduction

The ignored `crates/compositor/tests/live_latency.rs` benchmark uses a fully
populated 5472×3648 RGB photograph (19,961,856 pixels), U8 document storage,
explicit bundled Noto Sans, 274px type, and 43 sequential edits at each of L2
(1368×912) and L3 (684×456). A paragraph box keeps the typing visible, including
reflow. The shape case moves one polygon handle, with a solid fill and 9px stroke.
Each timer covers `Document::apply` through `render_level_rgba`, including final
interleaved CPU frame assembly. Photo decoding, font installation, and one cold
initial frame/mip build are outside the timer. No GPU is used.

Prepare a real decoded photograph of at least 19MP using Pillow:

```sh
python3 tools/orchestrate/wp/M5-34/prepare_photo.py /path/to/photo.png /tmp/m5-34-photo.rgb
M5_34_PHOTO_RGB=/tmp/m5-34-photo.rgb CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-34 cargo test -p compositor --release --test live_latency -- --ignored --nocapture
```

This run used the repository's Nikon D800 NEF fixture (the parent checkout's
`fixtures/raw/nikon-nef.NEF`), decoded with the vendored LibRaw 0.22.2 to
7378×4924 RGB8, then resized with Pillow Lanczos. `decode_fixture.c` is the
small decoder used; it links to the existing vendored LibRaw build archive.
The temporary archive used for decoding excluded the duplicate
`preprocessing_ph.o` definitions also present in `raw2image.o`; repository
LibRaw sources were not changed.

- NEF SHA256: `26d58c21ed3019af5a22ef6bc02f017b863aad9eafcf84252ed375cd495c0efb`
- Packed benchmark RGB SHA256: `f0146b3f71a44f9de81a68b54a3a2a621bb7b80e5685bca609d7bb28265ec3b4`
- Median: sorted sample 22/43. P95: nearest-rank sample 41/43.

The retry successfully read `origin/wp/B5-10:tools/orchestrate/wp/B5-10/NEEDS.md`.
Its app workload used Fit L1; this package's requested benchmark uses L2/L3.
The supplied 1072ms median / 1844ms p95 remains user-provided context, not a
baseline independently reproduced here.

## Implementation and correctness evidence

- The initial distant-text-edit regression failed with **16 full composites,
  16 source rasterizations** for one appended character across 16 tiles.
- Geometry preparation, paragraph layouts, shaped lines, positioned glyph
  outlines, sparse coverage and sparse layer source tiles now have reusable,
  bounded caches. Mask/paint/transform/font/depth/level inputs are retained.
- Ordered old/new geometry differences drive CPU partial updates. Unchanged
  tiles use the same downstream composite identity, including across undo.
- A regression asserts that appending an unkerned glyph prepares one new
  model, rasterizes one glyph coverage tile, and partially composites one tile.
- Independent legacy per-pixel vector clipping and full-tile source blending
  references check exact sample bits. Tests cover reflow, transforms, warp,
  deletion, movement, multiple depths/levels, masks, style halos, tiny cache
  budgets, font replacement, hidden layers/groups/clipping chains, and blank
  paragraph fallback metrics.
- A read-only independent review found hidden-layer font resolution and a
  missing blank-paragraph fallback metric in the cache key. Both were fixed
  with regressions. The blank-line bug was reproduced before the fix (cached
  baseline 12 versus expected 40).

A preliminary benchmark during concurrent builds failed the target. Profiling
then isolated a second root cause: the 9px stroke produced ~998 contour vertices,
which the vector rasterizer scanned for every pixel. Sharing the identical first
two clipping planes by column reduced a focused stroke-coverage sample from
326.459ms to 1.145ms. These are diagnostic component measurements, not frame
latency claims. Final isolated frame measurements and gate results are recorded
below after execution.

## Integration boundaries

No document/edit or resident code changed. CPU source caches are available
through the existing `live_tile` hook used by resident rendering. Resident
upload/damage scheduling still needs M5-31 integration; no GPU latency claim is
made. The retry implements halo-bounded root damage for styled live layers,
including nested ancestor effects. Neighborhood adjustments retain conservative
full-revision recomposition. Style source evaluation is still the existing
whole-source barrier; no style latency claim is made. Performance targets apply to the
specified warm CPU workload, not cold discovery/mips, arbitrary complex paints,
styles, or cache-thrashing scenes. See `crates/compositor/TEXT_VECTOR.md` for cache
budgets and placement/coverage limitations.

## Final correctness gates

- Parent independently ran the exact required command twice, outside the coding
  worker's sandbox, with **exit 0**:
  `cargo test -p compositor -p typography -p vector --release && cargo clippy -p compositor -p typography -p vector --all-targets -- -D warnings && cargo fmt --check`.
  The captured run reports **314 passed, 0 failed, 11 ignored**.
  Full evidence: [parent-gate.log](evidence/parent-gate.log).
- Earlier worker runs reported 46 Metal-adapter failures inside its sandbox.
  Those environment-only failures are superseded by the parent's passing gate.
  The earlier logs remain for provenance, not as the current gate result.
- All Cargo invocations used the required M5-34 target directory. Scope checking
  found only allowed changes. No commits or pushes were made.

## Acceptance measurements under shared-host contention

Independent parent rerun using the final source and normal Cargo invocation
also **failed**. Host load was **158.71 / 159.68 / 142.85** immediately before
execution. Each case has 43 edits, median / p95 in milliseconds:

| Workload | Parent run |
|---|---:|
| 274px typing L2 | 14.965 / 102.478 |
| Shape handle L2 | 66.524 / 110.997 |
| 274px typing L3 | 1.952 / 50.108 |
| Shape handle L3 | 6.982 / 90.953 |

Raw samples: [parent-latency.log](evidence/parent-latency.log).
Contention is observable but an idle-host pass is not established.

After our builds finished, the final release benchmark executable was run three
consecutive times with `RAYON_NUM_THREADS` explicitly unset. These measurements
include the production warm-frame scheduling policy and RGBA frame assembly.
All three runs **failed** the target. A host inspection immediately afterward
reported load averages **173.43 / 144.17 / 131.41**; an idle acceptance window
was requested. The low-load diagnostic below is not substituted for acceptance.

Each table cell is median / p95 in milliseconds; each case has 43 edits.

| Workload | Run 1 | Run 2 | Run 3 |
|---|---:|---:|---:|
| 274px typing L2 | 23.936 / 156.190 | 33.808 / 201.437 | 24.945 / 178.973 |
| Shape handle L2 | 59.555 / 246.273 | 76.371 / 286.067 | 120.766 / 249.329 |
| 274px typing L3 | 2.141 / 7.621 | 3.569 / 52.941 | 1.827 / 67.258 |
| Shape handle L3 | 7.215 / 128.898 | 36.683 / 143.860 | 14.362 / 144.157 |

Raw samples and counters: [run 1](evidence/contended-frame-1.log),
[run 2](evidence/contended-frame-2.log), [run 3](evidence/contended-frame-3.log).

An earlier single-worker diagnostic (`RAYON_NUM_THREADS=1`, before the
production warm-frame scheduling change) measured 4.911 / 6.893ms typing L2,
11.896 / 16.793ms shape L2, 1.857 / 3.038ms typing L3, and 4.265 / 6.270ms shape
L3. It motivated avoiding worker fanout for small warm live frames. It is
**diagnostic evidence only**, not final acceptance: [log](evidence/serial-diagnostic.log).

Until an idle-host final run passes, the <16ms median / <25ms p95 performance
requirement remains unverified. No resident/GPU performance result is claimed.

A subsequent `/usr/bin/time -p` run used **1.54s user + 0.17s system CPU** but
**7.57s real time** for the benchmark process; the contemporaneous host load
was **189.09 / 160.50 / 140.87**. This supports scheduling contention as a major
contributor to the failed wall-clock measurements, but does not establish the
required idle-host latency. [Wall/CPU diagnostic](evidence/wall-cpu-diagnostic.log).

## Retry: styled damage implementation and fresh verification

The styled-damage regression first failed because rendering a distant tile after
a shape edit raised `root_full` from 8 to 12. The fix extends live geometry damage
by each layer's finite style support, adding ancestor supports for nested styles.
Global light, properties, masks and ordinary raster revisions remain dependencies.
The regression now verifies distant reuse, shadow changes across a tile seam,
cold-frame exact parity and undo. A second test covers eight effect variants on
text inside a styled isolated group, fractional style scale, levels 0/1, light
changes, mask replacement and undo. No resident/styles/effects source was edited.

The exact required gate was run again after the final code changes, exit 0:
**316 passed, 0 failed, 11 ignored**, then clippy with `-D warnings` and fmt check.
See [retry-gate.log](evidence/retry-gate.log),
[style-red.log](evidence/style-red.log) and [style-green.log](evidence/style-green.log).
`git diff --check` and the allowed-path check also passed. No commits were made.

The real-photo acceptance benchmark was rerun without a Rayon override and still
**failed**. Host load immediately before it was **93.08 / 120.75 / 131.02**:

| Workload | Median / p95 (ms), 43 edits |
|---|---:|
| 274px typing L2 | 26.124 / 124.949 |
| Shape handle L2 | 86.094 / 162.312 |
| 274px typing L3 | 1.355 / 55.552 |
| Shape handle L3 | 9.559 / 121.866 |

Full samples: [retry-latency.log](evidence/retry-latency.log).
The performance acceptance requirement remains unmet, irrespective of contention.

## Current verification retry

No production code was changed in this retry. The exact required command was
run directly again with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-34`
and exited 0: **316 passed, 0 failed, 11 ignored**, followed by passing clippy
and fmt. See [current-gate.log](evidence/current-gate.log). `git diff --check`
and a check of modified/untracked paths against the allow-list passed.

The real-photo benchmark ran with `RAYON_NUM_THREADS` unset and exited 101:

| Workload | Median / p95 (ms), 43 edits |
|---|---:|
| 274px typing L2 | 26.008 / 92.236 |
| Shape handle L2 | 57.150 / 124.195 |
| 274px typing L3 | 1.360 / 34.030 |
| Shape handle L3 | 14.464 / 74.162 |

See [current-latency.log](evidence/current-latency.log). Load averages before
the benchmark were 54.13 / 109.58 / 126.78. A subsequent host check reported
61.55 / 101.37 / 122.39, 65 running processes, and 0.0% idle CPU. No unrelated
process was stopped or reprioritized. This is evidence of contention, not proof
that the performance requirement passes on an idle host. Final status remains
FAIL for performance acceptance; the correctness gate passes.
