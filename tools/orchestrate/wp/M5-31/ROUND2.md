# M5-31 round 2 handoff

Status: implementation/correctness gate passes; strict cold resident performance gate remains unmet. Do not treat this handoff as round-2 acceptance.

## Implemented

- CPU and resident styles evaluate source alpha at the requested pyramid level and compute only a tile/viewport plus effect-radius halo. Geometry is scaled by `2^-L`; paint coordinates remain in native canvas units.
- The 16,777,216 alpha sample cap applies to the padded computed region, not full native canvas dimensions.
- CPU source tiles, cropped planes and descriptors use the existing byte-budgeted render cache. Cache identity includes document/layer namespace, source revision, style settings, global light, canvas/depth, level and tile. The bounded namespace registry contains no pixel data.
- Resident cache retains requested-level source/effect GPU buffers. No CPU fallback was introduced. One temporary source renderer is shared across a batch.
- Auxiliary plane reservations no longer allocate/upload/shadow CPU zero arrays. Metadata is a compact uploaded prefix; GPU-only ranges receive GPU buffer copies. Cumulative sizes are validated before retaining each pending style.
- Shared-file changes are small wiring hunks in resident/mod.rs. No document/edit/format/PSD code was modified.

## Semantics

Level-local evaluation deliberately replaces styling L0 then reducing. Nonlinear morphology is not interchangeable with mip reduction. Reference-crop tests therefore compare a region against a full-canvas reference evaluated at the SAME requested level. COMPOSITOR.md records this and the <=1e-4 composite tolerance.

## Verification executed

CARGO_TARGET_DIR remained /Volumes/betterSSD/tessera-cache/target/M5-31.

`cargo test -p compositor --release && cargo clippy -p compositor --all-targets -- -D warnings && cargo fmt --check`

Exit 0. Test totals: 262 passed, 0 failed, 12 ignored. Full output: round2-gate.log. `git diff --check` also passes.

Coverage includes per-effect CPU/GPU parity, L0/L2 tile-boundary crops, CPU L0/L1/L2 reference crops, cache reuse/partial eviction, metadata allocation limits, PSD-imported lfx2, and 20/50 MP nonempty viewport rendering at L0/L1/L2.

Independent read-only review identified delayed aggregate GPU allocation validation. A separate fix agent added early cumulative checks and tests; focused independent re-review passed. No commits were made.

## Benchmarks actually executed

Machine: Apple M4, not Machine B's M4 Max.

20 MP (5000x4000), five sparse styled layers (shadow, glow, bevel, satin, stroke), L1 1368x912 viewport, resident submission plus GPU wait:

- CPU cold: 405.943 ms
- CPU recomposite after clearing composites: 443.110 ms
- Resident cold: 175.719 ms
- Resident cached-plane full recomposite after specialization and output invalidation: 11.796 ms

The ignored benchmark asserts BOTH cold and warm thresholds. Its strict cold resident <100 ms assertion fails; this is preserved rather than relaxed. See round2-timing.log and tests/resident_styles_large.rs. Warm resident and CPU thresholds pass. The warm resident measurement dispatches work, not an idle frame.

Existing 3840x2160 / 20-layer benchmark, 256x256 L2 viewport:

- CPU cold tile: 294.991 ms; warm cached tile: 46.5 us
- Resident cold: 320.760 ms; unchanged idle call: 4.541 us
- Cold/warm dispatched blocks: 256 / 0
- Pixel parity passed. See round2-4k.log.

## Remaining limitations

- Cold resident latency is above the requested 100 ms target on this M4. Re-measure on M4 Max and/or optimize the cold source/effect pipeline before acceptance.
- Large documents render through bounded viewports; a single full-L0 resident output remains subject to existing device binding/allocation limits. No claim of unbounded full-canvas GPU allocation.
- Pathological permitted effect offsets can produce oversized region halos and fail the computed-region cap.
- CPU neighboring tiles share source tiles but retain separate cropped effect planes; overlapping halo arithmetic is not globally fused. Concurrent duplicate cold requests can duplicate computation, though published buffers remain byte-budgeted.
- Styles directly on adjustment/pass-through layers remain unsupported, matching CPU semantics. M5-14's unevaluated contour/jitter/texture controls remain unevaluated.

## Revalidation

The existing implementation was preserved. The timing test now also checks actual
cold/warm GPU dispatch, five evaluated style stacks, warm effect-plane reuse,
zero filter fallbacks, and the cache budget. No timing threshold was relaxed.

Four fresh-process runs passed (round2-timing.log and round2-timing-recheck-*.log):
cold resident 59.902–75.624 ms, CPU 201.424–261.333 ms. A subsequent rebuilt
benchmark failed at 212.620 ms resident cold, 208.043 ms CPU cold, 11.405 ms
resident warm (round2-timing-final.log). These are fresh renderer caches, not
cleared driver caches. Other builds were active on this host. Temporary timing
instrumentation measured pipeline compilation at 1.459 ms in a passing run
(round2-profile.log); it was removed and no root cause is asserted.

The required test/clippy/fmt command was rerun successfully after the benchmark
assertions were strengthened: 262 passed, 0 failed, 12 ignored; clippy and fmt
passed, GATE_EXIT=0. Final verification is recorded in
round2-revalidation-gate.log. `git diff --check` also passed. No commits were made.

RESULT: FAIL cold resident timing is intermittent; a measured 212.620 ms frame exceeds the 100 ms target on Apple M4.

## Expanded viewport verification (latest attempt)

Preserved the inherited round-2 implementation. Expanded
`twenty_and_fifty_megapixel_viewports_l0_l1_l2` from a 34x32 region to a
1368x912 viewport clipped to each level's canvas. It now asserts nonzero GPU
dispatch, five evaluated style stacks per level, bounded CPU/GPU caches and no
filter fallbacks. Both document sizes pass at L0/L1/L2, including the clipped
20MP L2 viewport. No performance threshold was relaxed.

The exact required command was executed twice after this test change, including
a final run after removing temporary instrumentation. Final gate exit was 0:
262 passed, 0 failed, 12 ignored; clippy and workspace fmt passed.
See `round2-viewport-gate.log`. `git diff --check` passes. The external target
directory remained `/Volumes/betterSSD/tessera-cache/target/M5-31`.

The first timing run and five additional fresh processes passed, with cold
resident times 47.561–79.757 ms (`round2-current-timing.log`,
`round2-baseline-*.log`). After the expanded correctness gate, three fresh
processes failed the unchanged 100 ms assertion:

| Run | CPU cold | Resident cold | Resident dispatched warm |
| --- | ---: | ---: | ---: |
| viewport-final-1 | 258.233 ms | 253.233 ms | 11.185 ms |
| viewport-final-2 | 163.975 ms | 129.152 ms | 25.815 ms |
| viewport-final-3 | 184.142 ms | 110.561 ms | 10.568 ms |

Temporary per-layer submission instrumentation produced passing runs instead
of reproducing the spike. It was removed; there is no claimed production
performance fix in this attempt. Another work package's resident GPU test and
other builds were observed active on this Apple M4 host. That is evidence of
possible measurement interference, not proof of the cause. Reliable timing
acceptance still needs an isolated GPU run (preferably on the requested M4 Max)
or further profiling of a failing frame. Do not select only passing samples.

The ignored 4K/20-layer L2 256x256 benchmark also passed pixel parity:
CPU cold 157.428 ms, resident cold 202.072 ms. Its warm resident call dispatched
zero blocks and is not a recomposite measurement. See `round2-viewport-4k.log`.

RESULT: FAIL cold resident timing remains intermittent (latest failing range 110.561–253.233 ms); correctness, large-view rendering and the required cargo gate pass.
