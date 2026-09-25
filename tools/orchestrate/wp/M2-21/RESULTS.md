# M2-21 implementation and verification

## M2-21b: GPU export without viewport starvation, GPU lens/geometry

Main was merged first (M2-22 HDR presentation, M2-18b Adobe compat); the only
conflict was the `pipeline-gpu` re-export list (both `SurfaceFormat` and
`ExportResize` kept). engine-api is unchanged.

### Viewport starvation: diagnosis

The M2-21 admission lock never blocked the viewport. The viewport renders on
its own device and never took the lock, and before this change default recipes
did not reach the GPU export at all. The reported 121 s "no frame" timeout is a
race in the test itself, which assumes `commit` always re-renders (see "The
starvation test failure was a test race" below). It reproduces with no export
running. The lock is replaced anyway, as the brief requires.

### Design: priority-aware sharing (docs/08 §2)

- **No global lock.** `export::gpu` has no "one renderer at a time" mutex. All
  export callers share one device and queue.
- **Budget split.** The 512 MiB guard is now split into `VIEWPORT_RESERVE`
  (128 MiB) plus the export `BUDGET` (384 MiB). Each band reserves its planned
  share of `BUDGET` through a counting reservation, so concurrent exports
  interleave band by band instead of queuing whole exports. The per-transaction
  scratch cap is configurable (`ManagedRenderer::new_export_budgeted`, default
  512 MiB for existing callers).
- **Interactive pressure.** A new process-wide signal in the shared scheduler
  crate (`jobs::interactive_pending`, `jobs::yield_to_interactive`): every
  `ThreadPoolScheduler` counts its queued and running `Ui`/`Viewport` jobs,
  including after reprioritize, cancel and pool drop.
- **Yield points.** Export calls `yield_to_interactive` before every band. It
  also yields at every sensor-chunk checkpoint inside a band, whenever
  interactive work is pending: it submits its own work, waits for it, then
  waits. Resumption needs 50 ms without interactive work (`EXPORT_QUIET`). A
  single wait lasts at most 1 s (`EXPORT_MAX_YIELD`), so a viewport that never
  goes idle cannot stall export forever. Nothing is read back or published
  while yielding.
- **Deviation from the brief.** Bands are not submitted as `Priority::Export`
  jobs on the engine scheduler. `Engine::export_batch` is a blocking call made
  outside the pool, and each band borrows the decoded frame. Running bands on the
  engine's three workers would also take a worker from previews and masks. The
  pressure signal gives the ordering the brief asks for (export never starts or
  continues GPU work while viewport work is queued or running) with less
  machinery.

### GPU lens/geometry (default recipes stay on the GPU)

- **Sparse lens analysis.** Auto/CA estimation needs the demosaiced active area,
  but only at the ≤256-sample nearest-neighbour analysis grid.
  `pipeline_cpu::resolve_lens_sensor` develops each grid sample from a small CFA
  patch through the same highlight and demosaic operators. The patch margin
  accounts for halos and for edge folding by the CFA period. The analysis is
  bit-identical to the whole frame: a unit test covers Bayer and X-Trans, both
  highlight modes and sensor edges, and an opt-in test covers all five RAWs.
  It takes 0.18–0.31 s, against 0.47–1.34 s for the whole-frame reference.
- **`ResolvedLens::plan`** turns the resolved correction into plain GPU
  parameters. It returns None for what is not ported, and the caller then uses
  the CPU reference: guided/auto Upright, defringe, embedded per-channel warps,
  database CA on Bayer (never produced by export, which has no database), and
  more than 4 embedded opcodes.
- **Stage order matches the CPU reference** (`crates/pipeline-gpu/src/lens.wgsl`):
  - *Lateral CA* is not folded into the geometry pass, contrary to the brief.
    The reference resamples camera RGB before the camera/WB matrices, and
    per-channel resampling does not commute with that channel mixing. It runs as
    a sensor-frame bilinear per-channel radial scale on demosaiced tiles gathered
    with a halo of the largest displacement plus 2 (at most 32), before the
    matrices.
  - *Vignetting* (profile, embedded FixVignetteRadial, manual) is a
    per-pixel gain after WB, before Detail.
  - *Distortion + Transform + crop/straighten* is one composed inverse map
    with normalized Lanczos-3, after Effects and before Output. It covers manual
    k1, the Brown-Conrady sample with scale, odd terms and anisotropy, and green
    embedded WarpRectilinear. The f32 coordinate arithmetic follows
    `geometry_mapped`.
- **Banding.** Banding works in the mapped output frame. Each output band
  renders the input rows the map reads (sampled bounds plus Lanczos support),
  assembles them and remaps. Lens-plan stages are never memoized. Crop,
  straighten and Transform recipes now also stay on the GPU.

### Throughput work found while measuring

- **Sensor uploads.** Export transactions now reuse each uploaded sensor tile
  across its neighbours' halos. Before, a tile was uploaded once per dependent
  chunk, about 9×.
- **Parameter arena.** Dispatch parameters go into a 1 MiB mapped-at-creation
  arena. Before, each dispatch allocated its own buffer plus a staging copy, and
  a 256-row band has about 9–15k dispatches. This path is shared with the
  viewport.
- **Pipeline reuse.** Compiled pipelines are reused across bands
  (`ManagedRenderer::export_band`).
- **Tracing.** `TESSERA_EXPORT_TRACE=1` prints per-phase and per-band timings.

### The "starvation" test failure was a test race

`slow_interactive_frames_are_not_starved` failed in about half of full
develop-suite runs, including without the new export test. With
instrumentation, every failure had the same timing. Frames were fast, so all
40 drag edits rendered at the screen level during the burst (38–40 frames), and
the test drained the final frame. `commit` then correctly skips re-rendering
settings that are already drawn at the screen level, and `next_final` waited
120 s for a frame that is never sent. Faster frames make this more likely,
which explains why it appeared as GPU work got faster.

The test now accepts either a new final frame after commit (5 s) or the latest
final frame drained during the burst. The session behaviour is unchanged. It
then passed in 10 consecutive full develop-suite runs and in the final
workspace run.

### Measurements (Apple silicon, this machine)

The machine was shared with another work package's build (load average about
14 during the final runs). All figures are single runs.

**Slider during export.** New test `export_batch_does_not_starve_slider_drag`:
five full-size fixtures exported through `Engine::export_batch` while a develop
session drags exposure at 60 Hz at L2, 120 frames.

| Run | Render p50 / p90 / max | set→frame p90 | Export of 5 |
|---|---|---|---|
| Before (M2-21: default recipes fell back to CPU export) | 2.2 / 2.7 / 5.4 ms | 3.0 ms | 116.5 s |
| After, with yielding | 3.8–4.3 / 4.9–5.6 / 6–10 ms | 5.6–6.3 ms | 13.2–15.5 s (includes about 2 s paused for the drag) |
| After, yielding disabled (A/B, not committed) | 2.1–2.2 / 2.7–3.0 / 8–16 ms | 3.1–3.3 ms | 11.3–12.3 s |
| Same drag with no export (baseline in the test) | 2.3–4.5 / 2.7–5.6 / 3.5–8.3 ms | – | – |

All runs keep 120/120 frames at L2 and p90 well under the 16 ms target.
Yielding keeps slider frames inside the idle-baseline range. The A/B shows that
Metal already interleaves the viewport's queue with export work well. Yielding
mainly bounds the worst case (maximum 16 ms without it), and the higher p50 with
it tracks the idle GPU clock state rather than contention. The per-band trace
shows the band that overlapped the drag pausing at its checkpoints for the
drag's duration.

**Export timings** (`gpu_bench five_fixture_export_benchmark`, default recipe
with Auto lens profile and CA, isolated process, decode + render + encode, seconds).
Before, `requested=gpu` fell back to CPU (M2-21 benchmark-results.json).
After, `used_gpu=true` for all ten runs.

| Fixture | Web before → after | Full before → after |
|---|---:|---:|
| Canon CR3 (auto k1 distortion) | 19.37 → 5.50 | 19.63 → 5.77 |
| Sony ARW | 3.41 → 2.29 | 9.08 → 2.79 |
| Nikon NEF (36 MP) | 21.16 → 2.74 | 51.34 → 6.51 (4.32–4.39 at lower load) |
| Fuji RAF (X-Trans, auto k1) | 13.99 → 1.63 | 19.78 → 3.42 |
| DNG | 4.45 → 1.41 | 14.04 → 3.19 |

Nikon full-size at lower load breaks down as: lens analysis 0.17 s, GPU bands
2.05 s (from 5.77 s before the upload/arena/pipeline fixes), JPEG encode +
commit 1.6–1.7 s, decode about 0.4 s.

**Batch** (`gpu_bench hundred_web_exports`, 20 × each fixture, Web preset,
pipelined render/encode): 219.5 s, with decode measured separately at
0.19 s/image. The M2-21 single-image Web times imply more than 1,000 s before.

**docs/08 targets.**
- 45 MP JPEG < 1.5 s: **not met** (36 MP best 4.3 s).
- 100 JPEGs < 40 s: **not met** (219.5 s, under load).

### Precision (docs/11 §1.3: ≤ 2e-3 linear, ≤ 1 8-bit code)

Five fixtures, scale 1, sRGB (`five_fixture_full_chain_tolerance`, log
`m2-21b-precision.log`):

| Fixture | Lens-off linear / codes | Default recipe linear / codes |
|---|---:|---:|
| Canon CR3 | 1.8e-6 / 1 | 8.7e-5 / 1 |
| Sony ARW | 1.8e-6 / 1 | 1.8e-6 / 1 |
| Nikon NEF | 5.7e-6 / 1 | 5.7e-6 / 1 |
| Fuji RAF | 3.9e-6 / 1 | 9.7e-4 / 1 |
| DNG | 8.5e-6 / 1 | 8.5e-6 / 1 |

Synthetic gates, run in the normal suite, whole frame and 1-byte-budget bands
(band seam < 1e-5):
- Auto lens, manual distortion, manual vignetting, crop + 3.5° straighten +
  distortion, and Transform: linear ≤ 3.5e-5, 1 code.
- A forced calibration with k1/k2 off-centre, red/blue lateral CA and profile
  vignetting: 7.5e-5, 1 code.

The Fuji default error (9.7e-4) comes from f32 versus f64 map coordinates.
Pixel positions near 5,000 carry about 5e-4 px of rounding.

### Verification

All run with `CARGO_TARGET_DIR=~/.cache/tessera-target/M2-21`:
- `cargo test --workspace --release --no-fail-fast`: exit 0, all 263 test
  binaries ok (`m2-21b-workspace-tests.log`). An earlier full run hit the
  starvation-test race (fixed above), the retirement test's size assumption
  (fixed below) and a single `ml-embed` HNSW failure that did not recur.
- `cargo clippy -p export -p image-core -p pipeline-gpu -p pipeline-cpu -p
  tessera-ffi -p jobs --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all --check` and `git diff --check`: clean.
- Opt-in tests, all passing:
  - `five_fixture_full_chain_tolerance`;
  - `sparse_lens_analysis_matches_reference_on_fixtures`;
  - `five_fixture_export_benchmark`;
  - `hundred_web_exports`.
- `export_retires_uploads_without_intermediate_readback` now uses a larger
  region. Reusing uploads keeps its old region under the 128 MiB retirement
  threshold.
- Several develop-suite runs failed exports and one WB slider test while the
  disk was nearly full (under 0.4 GB free). After freeing space, 10 consecutive
  runs passed.

### Unfinished / known limits

- **Performance targets are not met.** The resident sensor stage is
  tile-granular: 9–15k dispatches per 256-row band, 1–2 s of GPU per 24–36 MP
  image. The next step is band-granular sensor kernels (one gather,
  highlights and demosaic per band), plus two bands in flight so CPU encoding
  overlaps GPU execution. Full-size JPEG encode is single-threaded
  (1–2 s at 24–36 MP).
- **Bands are not scheduler jobs** (deviation explained above).
- **Lateral CA is not in the geometry pass** (deviation explained above).
- **Web preset with `render_scale` > 1.** The map and the vignette gain run on
  the level-L developed frame, while the reference maps at full resolution and
  then downsamples. This is not gated; the precision gate is at scale 1. Lateral
  CA stays exact, because it runs on the sensor frame.
- **Still on the CPU reference path:**
  - auto/guided Upright;
  - defringe;
  - embedded per-channel warps (DNG CA);
  - database CA on Bayer sensors;
  - more than 4 embedded opcodes;
  - Texture, Clarity or Dehaze when banding is needed;
  - local adjustments and denoise.
- **Band scratch estimate.** The estimate is a heuristic (256 B/px, or 384 B/px
  with a map). A band that exceeds its budget fails over to the CPU path. Band
  scratch measured 60–248 MiB on the fixtures, against a 384 MiB budget.

## M2-21 (earlier attempt, superseded where noted above)

Status: incomplete / FAIL acceptance. The latest required test command fails in the FFI preview suite; default-recipe GPU execution and performance acceptance also remain unresolved.

## Latest verification attempt

This attempt inspected the inherited implementation without changing its source code. It ran the exact required command with `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-21`. The release export and image-core suites passed, but `tessera-ffi` test `slow_interactive_frames_are_not_starved` failed at `crates/tessera-ffi/tests/develop.rs:182`: `no frame; failures: []`. The chained command exited 101 before reaching clippy or fmt.

The same preview test was then run alone with `cargo test -p tessera-ffi --release --test develop slow_interactive_frames_are_not_starved -- --exact` and failed again after 121.25 seconds. Log: `current-preview-check.log`. This reproduces the failure independently of the export tests, but does not establish its root cause or prove it predates the implementation. The preview test and develop implementation are outside this work package's permitted paths and were not changed.

Clippy was run separately with the required arguments and exited 0 (`current-clippy.log`). `cargo fmt --check` and `git diff --check` also passed. Native LibRaw warnings are not the failing gate. Earlier successful runs and benchmark numbers below are historical evidence, not results rerun by this attempt. No new benchmark or performance success is claimed.

RESULT: FAIL required release suite times out in FFI preview; default-recipe GPU execution and performance acceptance remain incomplete.

## Implemented

- Resident float managed output through image-core Renderer and pipeline-gpu, retaining the target profile LUT instead of display quantization.
- Horizontal export regions with renderer halo dependencies, GPU separable Lanczos-3 downsampling, and one pixel readback per completed band.
- Global GPU render admission (one renderer at a time), scratch allocation guard, and per-band renderer lifetimes.
- Owned RenderedExport and bounded render/encode overlap in batch and FFI exports, preserving transactional publication and cancellation.
- Explicit TESSERA_EXPORT_BACKEND=cpu fallback. Other values attempt GPU and retain reference fallback for unsupported requests. RenderedExport::used_gpu reports the actual path.
- Synthetic CPU/GPU, whole/band, resampling, float/readback, lens fallback, cancellation and ICC/XMP tests, plus opt-in five-fixture benchmarks and precision gate.

## Retry fix: retire sensor uploads without a pixel readback

The initial real-fixture precision test failed because the transaction accumulated fresh queue-upload buffers across every sensor dependency. Queue writes cannot safely reuse storage inside an unsubmitted encoder, even when the planner has dropped that tile. A smaller horizontal output band alone did not solve that accumulation.

ResidentBatch now exposes a default no-op checkpoint. After a sensor dependency chunk retires, export transactions with at least 128 MiB of tracked scratch submit their pending commands, wait for completion, and release retired buffers. Live intermediate images stay resident. This does not add a pixel readback, does not publish unfinished cache entries, and leaves the 512 MiB scratch guard in place. Non-export backends retain their existing scheduling.

The previously failing five-fixture full-chain precision gate now passes. A non-ignored pipeline-gpu regression test exercises multiple submissions with one pixel readback and tracked scratch below 512 MiB.

## CPU delegation and remaining limitations

Lens corrections are not implemented by the resident graph. Default recipes enable Auto lens correction and CA removal; these fall back rather than silently omit effects. Disabling those settings is an explicit recipe edit, never an automatic export change. All default-recipe benchmark requests still use CPU fallback.

Local adjustments, AI mask rendering/segmentation hooks, geometry, denoise, Adobe processing, RGB sources and unsupported resident requests retain the existing CPU/reference path. AI hooks remain intact. Presence operations requiring whole-frame statistics also fall back when bands are necessary. Upscaling remains on the existing path. Output orientation, output sharpening, metadata construction and encoding remain CPU work.

The scratch guard accounts for resident payload buffers and final readback, not every driver allocation or parameter buffer. No claim of a measured total-process/GPU peak below 512 MiB is made. Arbitrary image dimensions and unsupported settings can still fall back.

## Verification from this retry

The exact required chain was run successfully after the changes:

    cargo test -p export -p image-core -p tessera-ffi --release && cargo clippy -p export -p image-core -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check

Exit 0, log: retry-verification-3.log. CARGO_TARGET_DIR remained /Users/rutmehta/.cache/tessera-target/M2-21. LibRaw emitted native compiler warnings. Rust clippy passed with warnings denied.

Two earlier verification attempts failed intermittently in existing FFI tests: slow_interactive_frames_are_not_starved timed out awaiting a frame, then passed in isolation; print_renders_fit_the_box_in_the_chosen_colour_handling compared ICC bytes whose timestamp seconds differed (40 versus 41). Neither out-of-scope test was changed. Both passed in the final complete run. Logs: retry-verification.log, retry-develop.log, retry-verification-final.log.

Additional commands:

    cargo test -p pipeline-gpu --release --test export_float
    PIPELINE_RAW_FIXTURES="$PWD/tools/orchestrate/wp/M2-21/raw" cargo test -p export --release --lib five_fixture_full_chain_tolerance -- --ignored --nocapture

Both passed. Logs: retry-export-float.log, retry-precision.log. The precision command was also run before the fix and failed (retry-precision-red.log).

Full-chain precision with unsupported lens stages explicitly disabled:

| Fixture | Maximum linear error | Maximum 8-bit code error |
|---|---:|---:|
| Canon CR3 | 0.000001847744 | 1 |
| Sony ARW | 0.000001758337 | 1 |
| Nikon NEF | 0.000005722046 | 1 |
| Fuji RAF | 0.000003874302 | 1 |
| DNG | 0.00000846386 | 1 |

These satisfy the tested full-chain limits of 2e-3 linear and one 8-bit code. This is not a golden-suite DeltaE2000 measurement.

## Benchmarks

benchmark-results.json contains 40 newly measured records: five fixtures, Web/full JPEG, requested CPU/GPU, default/lens-disabled recipes. Times include RAW opening/decode, render and encode in isolated processes. These are single runs, not statistical or 100-image batch measurements. The benchmark prints used_gpu; requested=gpu alone does not demonstrate GPU use.

With lens correction explicitly disabled, all ten requested-GPU cases now complete on GPU:

| Fixture | Web CPU / GPU seconds | Full CPU / GPU seconds |
|---|---:|---:|
| Canon CR3 | 8.810 / 4.788 | 9.329 / 4.844 |
| Sony ARW | 3.298 / 2.054 | 9.285 / 3.546 |
| Nikon NEF | 15.092 / 4.924 | 50.095 / 8.286 |
| Fuji RAF | 3.742 / 2.400 | 8.776 / 5.297 |
| DNG | 4.446 / 2.964 | 14.084 / 5.880 |

Default full-size Nikon remained CPU fallback: CPU 65.930 s, requested GPU/fallback 51.335 s. The variance in these single runs is not evidence of GPU acceleration. No 45 MP under 1.5 s or 100-image under 40 s success is claimed.

Reproduce:

    PIPELINE_RAW_FIXTURES="$PWD/tools/orchestrate/wp/M2-21/raw" cargo test -p export --release --test gpu_bench five_fixture_export_benchmark -- --ignored --exact --nocapture
    PIPELINE_RAW_FIXTURES="$PWD/tools/orchestrate/wp/M2-21/raw" TESSERA_BENCH_LENS_OFF=1 cargo test -p export --release --test gpu_bench five_fixture_export_benchmark -- --ignored --exact --nocapture

Logs: retry-benchmark-default.log and retry-benchmark.log. RAW fixture files and logs are ignored. Engine-api was not modified. Changes are uncommitted and confined to allowed paths.
