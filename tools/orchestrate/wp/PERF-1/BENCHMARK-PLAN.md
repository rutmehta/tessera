# PERF-1 ignored benchmark draft — UNRUN

The ignored test is authored in `render::effects::perf1_tests::perf1_serial_style_reuse_benchmark`. Product source remains b64e5e00. Direct rustfmt and diff checks only; compilation/execution pending. The release guard uses an if/panic to avoid the known constant-assert Clippy lint.

## Comparison and observation boundary

Two actual timed paths use identical synthetic document and cold Compositor instances: candidate full-level style reuse versus its private serial full-level traversal with no StylePass. Both preserve the numerical source/style/composite path. Full-level premultiplied tile rendering is timed, including normal candidate pass teardown; fixture creation, Compositor construction/destruction, output digest/drop, straight-alpha interleaving, image encoding, app dispatch and disk I/O are excluded symmetrically. This tests engine repeated-style work, not end-to-end Export Flat latency.

Each path has one untimed parity run, one discarded warmup and three timed samples. Warmup warms code/allocator state; each render still constructs a new compositor and a new frame-local pass. Timed path order alternates by repetition. Every output is digested and dropped before timing its peer; no peer's output or compositor is resident during a timing interval. Full bitwise parity is performed separately before timing, where two outputs do coexist and must be excluded from timing memory interpretations. Timed-run digests are a cheap regression guard, not a substitute for that full comparison or an independent frozen oracle.

Both source-build and style-render counters are read before the assertion, for both paths on every render. The fixed one-layer fixture requires cached `(1,1)` and uncached `(number_of_output_tiles, number_of_output_tiles)`. Count acceptance is separate from elapsed time: there is no hardcoded speed ratio threshold. Print raw observations and medians, observed ratio, host metadata, exact revision, input geometry, tile grid, effect settings including defaults, global light, memory-cap interpretation and timing boundaries.

The candidate's uncached path is a contemporaneous control, **not frozen historical source**. Historical verification below remains required. Likewise the original 83,080 ms loaded app report is context, not the denominator for this benchmark.

## Fixture and bounded resource scope

The default `smoke` preset is 513x259 (six tiles). Explicit `14mp` selects exactly 4608x3072 (216 tiles) and shadow distance30/size40 plus glow size30, matching report canvas/radii. The alpha-banded raster with a hole is synthetic and independent of fonts or catalogs. It is not the report's Helvetica text/gradient scene; live forwarding already has a separate analytic fixture, and app/text reproduction remains Machine B's scope.

Settings are fixed: one layer, two effects, one level, three timing samples, one warmup, 64 MiB compositor cache, no fixture-size overrides. 14MP source plus two effect planes has predicted retained payload 679,477,248 bytes (648 MiB), below the default 1 GiB/256-entry style cap. Input and output each require approximately 216 MiB, in addition to scalar padded blur/morphology scratch, metadata, caches and allocator overhead. Untimed parity may retain another 216 MiB output. No RSS ceiling is claimed. Exact final retained capacities are handled by the candidate; if it unexpectedly falls back the work-count invariant fails rather than mislabeling the measurement a cache result. Explicit fallback performance is not measured by this fixture; separate zero/tiny-budget tests protect its pixels.

Even serial 14MP baseline is expensive: five complete uncached renders total (parity + warmup + three timed), each potentially evaluating the full style 216 times. The test bounds workload, **not wall-clock duration**; no credible runtime budget can be given before measurement. Run smoke first. Start 14MP only when the coordinator confirms the exclusive lane and enough time/memory. If operational time is insufficient, stop and record incomplete data; do not silently reduce repeats, radius, canvas or compare incomplete medians. Ignored status plus explicit preset prevents ordinary test runs accidentally executing 14MP. No runtime should launch during the current hold.

## Proposed execution after review and runtime release

Install the reviewed test only after budget/routing reviews finish. In a clean checkout, capture exact commit and dirty-state description, CPU model, RAM, OS version, power mode and contemporaneous host load/process activity in `TESSERA_PERF1_HOST` and `TESSERA_PERF1_REVISION`. Keep these facts in the preserved log; metadata is supplied by the operator, not inferred by the test. Use the same toolchain and RAYON_NUM_THREADS across runs. One test thread is necessary but does not prove an otherwise idle host.

Proposed command (not executed):

```sh
TESSERA_PERF1_PRESET=smoke TESSERA_PERF1_HOST='<recorded host/load>' TESSERA_PERF1_REVISION='<exact head + dirty state>' cargo test --locked --release -p compositor --lib render::effects::perf1_tests::perf1_serial_style_reuse_benchmark -- --ignored --exact --nocapture --test-threads=1
```

Then repeat with `TESSERA_PERF1_PRESET=14mp`, preserving all output. Do not overlap baseline/candidate processes. Recheck the external runtime lane between compilation and measurements; compile completion alone does not establish quiescence. Preserve compiler/toolchain, exit status and full command alongside log.

## Frozen preimplementation check with identical fixture

Use a separate reviewed test-only checkout at `e32e6320` (the immediate preimplementation parent with operation counters), not an old loaded-report binary and not a checkout containing `b64e5e00` production changes. Copy the fixture construction/settings verbatim from this draft. The baseline has no StylePass/private helper: use a test-only serial wrapper that obtains the level-zero tile grid, visits y then x, and calls existing `render_tile_premultiplied(&doc, TileCoord::new(0,x,y))` for each tile. Keep the entire coordinate/traversal/collection within the same timed boundary and allocate the same fresh 64 MiB Compositor before each timer. Do not copy candidate style_entry, source sampling keys, or any cache product code into this checkout.

Run the **one baseline path only** with identical parity-independent warmup/repeat counts, geometry, settings, build flags and host conditions; report it as the historical serial baseline. Existing baseline full-level traversal can be parallel, so using its public full-level entry unchanged would mix scheduling with reuse in a supposedly serial comparison. Optionally measure that original route separately, explicitly labeled, if the coordinator wants end-to-end old/new scheduling evidence.

Before timing, export baseline premultiplied tiles to a synthetic fixture artifact with explicit tile coordinates/layout/channel order and little-endian f32 bit patterns. On the same host, validate every candidate bit against this frozen artifact outside timing. For portable frozen numerical tests, capture selected blur/morphology plane/final-pixel values from the smaller fractional-alpha fixture in the separate pending oracle gate; use reviewed numerical tolerances. Do not treat this draft's current-control digest as frozen baseline provenance. Record artifact-producing commit, exact test patch/hash, toolchain, platform and command. The unmodified independent Gaussian impulse/morphology-band tests are also required after lane handoff.

Source review of this draft, compilation, historical artifact capture and all benchmark observations remain pending. No speedup or passing test evidence is claimed.

## Independent-review scope clarification

See BENCHMARK-SOURCE-REVIEW.md. This benchmark intentionally measures premultiplied tile rendering, not render_level_rgba, export encoding or UI latency. Three repetitions are an exploratory median, not the earlier planned five-run export qualification. The synthetic single-raster/no-gradient fixture does not reproduce the original text/gradient workload. The 14MP parity precheck retains two outputs; although dropped before timing, allocator/system high-water can influence trials. Report this caveat and use separately launched historical baseline/candidate runs for final performance claims; do not present this within-process control alone as final qualification. Frozen preimplementation parity and portable blur samples remain separate pending gates.
