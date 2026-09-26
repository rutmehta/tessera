# tessera-bench

See [CONTRIBUTING](../../CONTRIBUTING.md#performance-benchmarks) for commands,
CI bootstrap and baseline review. Rust provides the CPU measurement worker and
launches the standard-library-only Python collector. `cargo test -p tessera-bench
--release` runs the Python contract tests too, so the required Rust test command
covers schema, adapters, comparison and CLI exit behavior.

## Version 1 fixture contract

| Bench | Fixture and measured work |
|---|---|
| index-search | Temporary on-disk catalog populated with 100,000 deterministic IDs. FTS `landscape` matches 10,000, limit 100. Times the public `Index::search`, verifies 100 returned IDs. Migration/population and warmup excluded. Inspired by the ignored index 100k bench; no facet timing here. |
| sidecar-roundtrip | Default recipe envelope plus selection XMP. Atomically write both (including fsync), read and validate both, compare round trips. The whole round trip is timed. |
| preview-pyramid | 8000×5625 RGB gradient/xor pattern. Each iteration uses a fresh key and builds Half/Quarter/Eighth JPEGs with `PreviewStore::ensure`. Includes source clone, resize, encode and disk I/O, not pattern generation. Checks Eighth exists. |
| pipeline-cpu-l3 | 2048×1536 scene-linear RGB pattern, default DevelopSettings, `render_scaled(..., 8)`, verified 256×192 result. This is the full-resolution CPU reference followed by downsampling to L3, not the cached CFA pyramid or a RAW decode. |
| export-web | Same RGB pattern, one image, JPEG q85, sRGB/default color settings, long edge 2048, Screen sharpening, render scale 1, orientation on. Includes render, encode and publication; excludes fixture construction. Each iteration uses a fresh name. Checks CPU actually used and output nonempty. |
| compositor-20-layer | 20 semi-transparent patterned RGBA8 layers, 1024×768, opacity .8, L0. CPU `Compositor::render_level`, composites cleared every iteration. Fixture/history creation excluded. Not the existing 100-layer 20 MP GPU document. |

Each CPU row is the median of five timed operations after one untimed warmup.
The collector forces four Rayon threads and serial execution. Production code's
other thread pools are not overridden. Correctness checks and result disposal
are included in these coarse-grained timings. No hard-coded wall-clock target
assertions are imported from the ignored tests. Existing ignored preview tests
have a 400 ms limit and existing index tests have 100 ms limits; those are not
portable correctness assertions on a shared CI host.

GPU suite (local Metal, separate baseline):

- `tessera-ffi/tests/develop.rs::bench_panel_latency`, Sony ARW, only tone
  exposure, only GPU. Uses the existing warmup and 16 retained drag frames.
  Records median and p90. Verifies Metal in the report and puts the adaptive
  drag level into the fixture ID: a level change cannot masquerade as a speedup.
- `export/tests/gpu_bench.rs::fixture_export_worker`, Sony ARW Web, GPU required,
  lens enabled and full-resolution development (`TESSERA_BENCH_WEB_SCALE=1`).
  Records one decode + render + encode elapsed time. GPU fallback is an error.
  It deliberately avoids the opt-in pyramid export path that failed the Web
  precision gate in M2-21c. This is a single cold measurement, not a median.
- `compositor/tests/bench.rs::resident_100_layers_20mp`, existing mixed-mode
  100-layer 5472×3648 fixture. Records warm full L2 median, 64² dab to L2 median,
  and full L0 median, including completion waits. Requires substantial RAM.

The Develop test discovers the first ARW under `fixtures/raw`, so preflight
requires that directory to contain exactly one ARW named `sony-arw.ARW`. No
fixtures are fetched implicitly. The runner strips inherited benchmark/backend
switches, rejects missing output/skips, logs child output, checks exit status,
and kills the child process group after 540 seconds per command. GPU command
budgets include compilation; CPU measurement excludes compilation. Build GPU
tests first on slow hosts if needed.

## Data and baseline review

Results are a JSON array of rows with exactly `bench`, `fixture`, `metric`,
`value`, `unit`, `backend`, `host`. Values must be finite and positive. Identity
is all fields except value, not array order. Duplicates, missing rows, changed
units/backend/fixtures/host are errors. Version fixture names when changing
input, settings, sample policy or timed boundaries.

Baseline envelope: `schema_version: 1`, `recorded_at` (UTC), `tolerances` and
`results`. Tolerances map metric names (or `bench/metric` overrides) to allowed
relative increases. Missing tolerances are errors. Equality at the threshold
passes. A baseline is never auto-updated by comparison, and recording cannot
be combined with an enabled gate. Recording preserves existing tolerances.
Raw measurement JSON is saved before baseline comparison, including regressions
and missing-baseline failures. Tables and raw subprocess logs are artifacts.

Use a quiet, stable machine; investigate repeatability, load, power state and
thermal throttling before accepting a new baseline. Hosted runners can vary
within a CPU model, so a host label alone is not proof of fixed hardware. The
workflow can target a dedicated runner via repository variables. Bootstrap on
that actual host and review/commit the artifact before expecting nightly gates
to pass. No CI baseline is invented from local measurements.

This is a curated regression suite, not proof that every docs/08 target is met.
In particular it does not test 1M-image searches, 45 MP RAW processing, 100-image
export throughput, app startup, memory limits or AI masks/denoise. M2-17b and
M2-21c measurements provide historical context, not interchangeable baselines.
