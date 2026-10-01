# B5-prof — release profiling report

Report-only work on `wp/B5-prof`; no product fixes. Reused measurements were collected September 29–30, 2026; follow-up October 1. **All reused app runs were measured under load** (1-minute load >8). These are diagnostic measurements, not a controlled regression gate.

## Scope, provenance and measurement definitions

The previous run notes identify `f28e2bdf` for batches 1/2 and `1d74771b` for the incomplete Camera Raw follow-up. The old packaged release metadata confirms `1d74771b`; it cannot prove the identity of an already-recorded earlier process. Earlier batch attribution is therefore notes-derived. Current checkout at restart: `68264c74`. The Camera Raw implementation changed since batch 1; its earlier numbers are explicitly historical to that build. Document compositor and Swift document paths are unchanged in the inspected diff; that does not prove identical whole-program behavior.

Live host: Apple M4 Max, 48 GiB RAM. Do not substitute the M4/24 GiB host from the original P01–P19 audit. Scratch root for reused data:

`/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof`

New scratch: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001`. Trace bundles remain there, outside Git. `reused-measurements.json` preserves raw log text, run loads, RSS and foreground checks; `reused-trace-summaries.json` preserves the exported symbol tables and trace-relative windows.

- Wall values use the median of three matching trials when available; **n is explicit where fewer trials or per-event medians are all that exist**. No missing trial is fabricated. Cold and warm data are not silently combined.
- **S** main span is a Time Profiler estimate: consecutive main-thread CPU samples with ≤3 ms gaps. It excludes blocked time and is not proof of a run-loop maximum or an <8 ms pass. **R** is the self-test's main run-loop observer (elapsed afterWaiting→beforeWaiting, plus stop-time remainder; scheduler delay and the final remainder can inflate it). FFI test-thread call durations are not AppKit main-thread spans.
- Memory is process-wide sampled RSS (100 ms), unless labeled footprint or incremental heap. It includes setup, history and other steps. It cannot be assigned exclusively to one filter. RSS, physical footprint and counting-allocator heap growth are different metrics.
- Self time is leaf sample weight summed across threads, in CPU ms; it can exceed wall time. Traces contain system symbols and optimizer-deduplicated frames. Source mapping below identifies a containing function/file where exact inlined closure identity is unavailable.
- Background delivery/presentation can be occlusion-limited. Engine `wait_idle`/listener completion is not input-to-photon. Log-arrival windows have buffering uncertainty; they are not signpost boundaries.

## Current release follow-up and consolidated summary

The requested release build succeeded and `Support/provenance.py` verified `68264c74c365d8e7a4c33ddaecebf934f9305b08`. Archive, bindings, source and packaged-binary hashes are in `release-provenance.json`. No product source was changed. Temporary engine measurement scaffolding was removed after use; its source and runners remain in scratch for reproduction.

Every new trial was measured under load. Some supplemental runs began above 900; timing variance and scheduler delay must not be mistaken for a clean code regression. Per-run load records are in `current-measurements.json`; Camera Raw app starts were 57.56 / 241.60 / 360.81, and engine starts 173.17 / 160.25 / 115.29. App runs include Time Profiler overhead. Engine wall/RSS runs are separate from the profiled hotspot runs.

| Scenario | Wall / latency | Main-thread max | Peak memory | Boundary / evidence |
|---|---:|---:|---:|---|
| 1. Saved 24 MP layered document, engine open → first listener frame | 310.01 ms; n=3 | N/A: Rust test, not AppKit | 901 MiB process RSS | prof_saved_layered_24mp; one process, cold first trial; excludes launch/UI presentation |
| 2. Brush on 5472 × 3648 (20 MP), tools self-test | stroke_points 1.62 ms; frame render 2.87 ms (median of 3 per-run medians) | 59.0 ms S, available brush windows | 2,572 MiB whole-test RSS | valid/tools-1..3; whole-stroke wall not instrumented |
| 3. 20 MP fill-only vector drag, reused release | 152.5 ms; 60 previews; median 15 frames | 21 ms S, 2 isolated trace windows | 4,915 MiB whole-test RSS | vector-1..3 in old scratch; source/build qualification below |
| 4. Camera Raw 24 MP — fit L2 preview, local settings | 228 ms; median of 3 warm-run medians | N/A: engine test thread | +220 MB heap; ≤7,074 MiB whole-process RSS | cr-bench-1..3; each warm-run median has n=3 |
| 4. Camera Raw 24 MP — 100% preview, local settings | 533 ms; median of 3 warm-run medians | N/A: engine test thread | +244 MB heap; ≤7,074 MiB whole-process RSS | cr-bench-1..3; each warm-run median has n=3 |
| 4. Camera Raw 24 MP — fit L2 preview, vignette (global) | 259 ms; median of 3 warm-run medians | N/A: engine test thread | +143 MB heap; ≤7,074 MiB whole-process RSS | cr-bench-1..3; each warm-run median has n=3 |
| 4. Camera Raw 24 MP — 100% preview, vignette (global) | 10,244 ms; median of 3 warm-run medians | N/A: engine test thread | +2,564 MB heap; ≤7,074 MiB whole-process RSS | cr-bench-1..3; each warm-run median has n=3 |
| 5. Gaussian r12 24 MP → completed frame | 723.20 ms; n=3 | N/A: engine test thread | 1,819 MiB isolated process RSS | prof_gaussian_isolated_24mp; two synthetic U8 layers |
| 5. Liquify commit 24 MP → completed frame | 84.90 ms; n=3 | N/A: engine test thread | 828 MiB isolated process RSS | prof_liquify_isolated_24mp; two synthetic U8 layers |
| 5. Camera Raw full apply, 24 MP app | 7,230 ms; n=3 | 78.0 ms S | 2,917 MiB whole-test RSS | camera-raw-1..3; real Canon U16 layer |
| 6. Export Flat UI, 18 MP smart-filter fixture, reused | 1,920 ms; n=3 first exports | 76.05 ms R; target <8 ms not met | 7,471 MiB entire-suite RSS | fperf-1..3; 14 MP styled variant 83,080 ms / 140.32 ms R |
| 6a. Export Flat 24 MP engine worker | 466.30 ms; median of 3 run medians | N/A; snapshot timing is not UI main max | 1,351 MiB process RSS | prof_export_flat_24mp-1..3; each run exports 3 times |
| 7. Paint undo / redo → frame, 24 MP engine | 0.96 / 0.90 ms; median of 3 run p50s | N/A: engine test thread | 561 MiB process RSS | prof_paint_undo_24mp-1..3; each run n=5 strokes |
| 7. Gaussian undo / redo → frame | 1.00 / 0.70 ms; n=3 | N/A: engine test thread | 1,819 MiB process RSS | prof_gaussian_isolated_24mp |
| 7. Liquify undo / redo → frame | 1.00 / 0.70 ms; n=3 | N/A: engine test thread | 828 MiB process RSS | prof_liquify_isolated_24mp |
| 8. Viewport / full-level, 24 MP, 4K | pan 12.50 / 2.37 ms; opacity 2.56 / 3.77 ms; median of 3 run p50s | N/A: engine test thread | 531 MiB process RSS | viewport-1..3; each mode n=30 pan + 30 opacity |
| 8a. Render live-state locks | max held 0.29 ms; max wait 1.64 ms | Not a main-thread span | — | viewport-1..3, render_records; 60 records/mode/run |

The native fixture contains two layers at 6000 × 4000. Camera Raw engine fit is 1500 × 1000 at L2; its 100% crop is 1600 × 1000. Local settings use clarity 20 plus exposure; the global case uses vignette −40 plus exposure. Saved-layer open here measures engine open to the first frame listener callback after attaching fit surfaces; the application's launch/open-to-present span remains unavailable. The 20 MP tools figures are per `stroke_points` update and render callback, not a stopwatch around the entire scripted gesture. AppKit main-thread maxima are unavailable for engine-only cases and are intentionally not replaced with the Rust test harness's main thread.

All three `--timing-output … --timing-selftest --develop-selftest` runs completed with zero dropped events. Their maximum instrumented photo-path main spans were 3.151 / 4.233 / 2.456 ms. Each run emitted 121 input events; drawable presentation was unavailable under background occlusion. These are supplementary photo-pipeline spans, not document or Export Flat main-loop bounds. The first timing series used a shared scratch RAW fixture; app directories were distinct.

The initial synthetic DNG lacked SampleFormat and WhiteLevel tags and was rejected. Those three failed `tools-*` attempts are excluded. Corrected `valid/tools-*` runs use an explicit 5472 × 3648 RGB16 DNG with both tags. The first corrected app self-test passed, but its Time Profiler finalization stalled: its RSS/log metadata was recovered and that trace is not treated as valid CPU evidence. Subsequent brush traces stop after the brush result, while the official harness completes the remaining correctness steps. Tools trial 3 reported zero app self-test failures but wrapper exit 1: the foreground changed from ChatGPT to loginwindow. Its timing is retained with that failed foreground-invariance check; neither endpoint was Tessera.

New method: the official `tools/orchestrate/wp/B5-selftest-window/run-background-selftest.sh <name>` launches with `open -g -n … --args --nonactivating --app-dir …`; a scratch wrapper attaches Time Profiler to the uniquely matched app-dir PID, records load before launch, samples RSS, and preserves foreground checks. No computer-use API or activation was used. Only launched processes are eligible for signalling. CLI profiling uses headless release tests and `xctrace record --template 'Time Profiler' --launch …`; callbacks use offscreen IOSurfaces.

Undo/redo profiling includes amplified repeated cycles to gather enough leaf samples. Those CPU hotspot tables are not single-action latency measurements; use the three-trial timing rows above. Tiny single undo/redo windows may contain fewer than five unique samples and cannot support an invented top five.

## Summary table — reused release evidence

| Scenario / boundary | Wall or latency, median of 3 unless stated | Main-thread max | Peak memory | Evidence / load (1 minute) |
|---|---:|---:|---:|---|
| 1. RAW 24 MP → Edit in Layers first frame | 1,150 ms (1,270 / 1,140 / 1,150) | 323 ms S, max over runs | 3,300 MiB RSS, whole test | doc24-a/b/c; 11.56 / 15.80 / 13.01 |
| 1a. 24 MP PNG open + first fit frame, engine | 251.8 ms (967.5 / 251.8 / 237.7) | N/A, test thread | 937 MiB max RSS; 1,827 MiB footprint | bench/prof_open_24mp; load 14.92; same process, cold first trial |
| 2. Brush, **18.1 MP** tools fixture (not requested 20 MP) | stroke_points 2.02 ms; frame render 2.24 ms (medians of per-run medians); whole-stroke wall unavailable | 50 ms S in available brush windows | 2,965 MiB RSS | tools-1/2/3; 38.54 / 40.19 / 22.19 |
| 2a. 24 MP synthetic paint, engine | stroke_points p50 0.95 ms; 900 calls, one run | FFI max 3.40 ms, not app main | 752 MiB RSS / 2,830 MiB footprint | bench/prof_paint_undo_24mp; load 14.79 |
| 3. 20 MP fill-only drag, step 372 | 152.5 ms median of per-run medians (148.7 / 152.5 / 164.5); 60 previews, 15 / 15 / 14 frames | 21 ms S in two isolated drag windows | 4,915 MiB RSS, whole vector test | vector-1/2/3; 10.86 / 23.83 / 13.52 |
| 4. Camera Raw 24 MP synthetic preview, B5-28 bench | fit local 220 ms; 100% local 497 ms; fit global 252 ms; 100% global 6,051 ms. Each warm median n=3 within one process | N/A, engine test thread | local fit +220 MB heap, 100% +244 MB; global 100% +2,564 MB | bench/cr24-b528.out; load provenance incomplete |
| 5. Gaussian 24 MP app apply | 1,100 ms (1,130 / 1,090 / 1,100) | 40 ms S | 3,897 MiB RSS, whole test | filter-1/2/3; 10.08 / 16.86 / 12.54 |
| 5a. Gaussian r12 24 MP engine call + completion | 778.2 ms (778.2 / 851.7 / 767.2) | N/A | shared filter process: 5,470 MiB RSS / 9,161 MiB footprint | bench/prof_filters_24mp |
| 5b. Camera Raw app full apply, pre-B5-28 | 8,030 ms (8,850 / 7,860 / 8,030) | 85 ms S | 5,025 MiB RSS, whole test | cr-1/2/3; 10.82 / 13.10 / 11.29; do not label current |
| 5c. Camera Raw 24 MP synthetic apply + completion, pre-B5-28 | 11,081.9 ms (11,081.9 / 11,052.1 / 11,576.5) | N/A | shared filter process above | bench/prof_filters_24mp |
| 5d. Liquify 24 MP synthetic commit | 78.5 ms (68.5 / 78.5 / 79.4), excludes begin 146.4 ms and brush 12.2 ms | N/A | shared process footprint at commit ≤8,994 MiB | bench/prof_filters_24mp; completion rounded to 0.0 ms |
| 6. Export Flat, background UI path, **18 MP smart filter** | 1,920 ms, first export per run (1,900 / 1,950 / 1,920) | 76.05 ms R (first-export maxima 74.45 / 73.19 / 76.05) | 7,471 MiB RSS for entire perf suite | fperf-1/2/3; 10.45 / 28.88 / 46.73; **<8 ms fails** |
| 6a. Export Flat, background UI path, styled 14 MP | 83,080 ms (81,790 / 83,080 / 85,260), first exports | 140.32 ms R | suite footprint up to 18,677 MiB | same fperf runs; size limited by style fixture |
| 6b. Export Flat, 24 MP engine snapshot + worker | 209.1 ms worker (241.7 / 209.1 / 205.8); begin rounds to 0.0 ms | N/A, not UI max | 1,271 MiB RSS / 2,032 MiB footprint | bench/prof_export_flat_24mp |
| 6c. Synchronous helper used by document self-test, 24 MP | log windows 4,916 ms incl. test pauses (not operation wall) | 2,153 ms S | doc24 whole-test RSS above | doc24 export windows; **not background UI exporter** |
| 7. Undo / redo paint, 24 MP engine | p50 0.6 / 0.9 ms over 5 strokes, one run | N/A; tools undo window includes pauses | 752 MiB RSS / 2,830 MiB footprint | bench/prof_paint_undo_24mp |
| 7a. Undo / redo filter, 24 MP engine | Gaussian 0.9 / 0.9 ms; Camera Raw 1.5 / 1.4 ms; Liquify 1.0 / 0.6 ms (each n=3) | N/A | shared filter process | bench/prof_filters_24mp |
| 8. Viewport / full-level 24 MP, 4K, engine | pan p50 7.76 / 0.93 ms; opacity p50 1.31 / 2.75 ms; one run ×30 each | N/A | 525 MiB RSS / 1,529 MiB footprint | bench/prof_viewport_vs_full_24mp; load 13.01 |
| 8a. Render live-state lock | held max 0.01 ms, wait rounds to 0.00 ms; 60 records per mode, one run | Not a UI span | — | document/render.rs record fields |

Step 372 is 39.7% lower latency than the supplied 253 ms baseline and delivers a median 15 rather than 9 frames. Because hardware/load/provenance are not matched, this is an observed comparison, not a causal B5-22 speedup claim. Full-level wins this cached-pan fixture while viewport wins opacity updates: do not extrapolate one pan result to large uncached or spatial-effect scenes.

The user-supplied previous Export Flat main-span range is 36–58 ms. The 74–76 ms first-export run-loop maxima here exceed both that range and the <8 ms target, but fixtures, load and measurement boundary are not matched, so this is not a controlled regression claim.

The failed `cr28-2` run is excluded from success medians: load 377.36, apply 906.43 s, two failures. `cr28-1` passed at load 234.89 with apply 10.92 s; there is no completed three-run post-B5-28 app median in the old scratchpad.

## Per-scenario sampled hotspots

Leaf sample weights below are self time, summed across threads in ms, not wall time. Current and reused evidence are labeled. Source mappings identify containing functions for inlined closures. System/dependency symbols remain explicit; an optimizer-deduplicated symbol cannot honestly be assigned a unique source function. See JSON for full callers and trace windows.

The brush interval includes concurrent RAW analysis. The viewport table includes fixture construction, which is not ranked as product work. Undo tables exclude Gaussian/paint setup using the harness's timestamped operation windows; system wait/dispatch symbols must not be interpreted as equivalent user-visible blocking durations.

### Saved layered 24 MP open, current engine; whole run

Evidence: `prof_saved_layered_24mp.summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/prof_saved_layered_24mp.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 330 | `HUF_decompress4X1_usingDTable_internal  [b5prof_followup-5d248d08350dc754]` | `zstd dependency huf_decompress.c:HUF_decompress4X1_usingDTable_internal (called from compositor format reader)` |
| 157 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |
| 143 | `<&<compositor::resident::ResidentRenderer>::materialize::{closure#0} as core::ops::function::FnMut<(&(u64, engine_api::tile::Tile),)>>::call_mut  [b5prof_followup-5d248d08350dc754]` | `crates/compositor/src/resident/mod.rs:ResidentRenderer::materialize` |
| 42 | `<compositor::format::Reader>::raster  [b5prof_followup-5d248d08350dc754]` | `crates/compositor/src/format.rs:Reader::raster` |
| 35 | `<fontdb::Database>::load_font_file_impl  [b5prof_followup-5d248d08350dc754]` | `fontdb dependency src/lib.rs:Database::load_font_file_impl` |

### 20 MP brush interval, current app; includes concurrent RAW preview/analysis

Evidence: `valid/tools-2/summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/valid/tools-2/run.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 183 | `<image_core::rgb::RgbSource>::from_linear_dng  [Tessera]` | `crates/image-core/src/rgb.rs:RgbSource::from_linear_dng` |
| 129 | `lens::ca::estimate_ca::{closure#1}  [Tessera]` | `crates/lens/src/ca.rs:estimate_ca` |
| 107 | `compositor::render::mip_exact::<u16>  [Tessera]` | `crates/compositor/src/render/mod.rs:mip_exact` |
| 101 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |
| 75 | `mach_msg2_trap  [libsystem_kernel.dylib]` | `system/framework symbol; source not in repository` |

### 20 MP vector fill-only step 372, reused

Evidence: `vector-2/drag372.json`; trace: `vector-2/run.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 579 | `<&<compositor::resident::ResidentRenderer>::materialize::{closure#0} as core::ops::function::FnMut<(&(u64, engine_api::tile::Tile),)>>::call_mut  [Tessera]` | `crates/compositor/src/resident/mod.rs:ResidentRenderer::materialize` |
| 434 | `<compositor::render::Compositor>::live_tile  [Tessera]` | `crates/compositor/src/render/live.rs:Compositor::live_tile` |
| 258 | `compositor::raster::tile_from_normalized  [Tessera]` | `crates/compositor/src/raster.rs:tile_from_normalized` |
| 176 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |
| 121 | `madvise  [libsystem_kernel.dylib]` | `system/framework symbol; source not in repository` |

### 24 MP Camera Raw preview + apply, reused B5-28 engine trace; mixed phases

Evidence: `bench/cr24-b528.summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/bench/cr24-b528.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 7,240 | `<pipeline_cpu::image::Image>::tile  [document_camera_raw_preview-6d80c9a9f4cf42df]` | `crates/pipeline-cpu/src/image.rs:Image::tile` |
| 4,861 | `filters::camera_raw::encode_channel  [document_camera_raw_preview-6d80c9a9f4cf42df]` | `crates/filters/src/camera_raw.rs:encode_channel` |
| 4,208 | `<pipeline_cpu::image::Image>::downsample_crop  [document_camera_raw_preview-6d80c9a9f4cf42df]` | `crates/pipeline-cpu/src/image.rs:Image::downsample_crop` |
| 3,615 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |
| 3,315 | `<core::iter::adapters::flatten::FlatMap<core::slice::iter::Iter<[f32; 4]>, core::iter::adapters::flatten::FlatMap<core::slice::iter::Iter<f32>, [u8; 4], filters  [document_camera_raw_preview-6d80c9a9f4cf42df]` | `crates/filters/src/camera_raw.rs:adapter output conversion (closure truncated)` |

### 24 MP Camera Raw apply, current app

Evidence: `camera-raw-2/summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/camera-raw-2/run.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 1,265 | `<pipeline_cpu::image::Image>::tile  [Tessera]` | `crates/pipeline-cpu/src/image.rs:Image::tile` |
| 996 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |
| 781 | `filters::camera_raw::encode_channel  [Tessera]` | `crates/filters/src/camera_raw.rs:encode_channel` |
| 668 | `<pipeline_cpu::image::Image>::downsample_crop  [Tessera]` | `crates/pipeline-cpu/src/image.rs:Image::downsample_crop` |
| 616 | `pipeline_cpu::map_rgb::<pipeline_cpu::color_detail::color::{closure#0}>  [Tessera]` | `crates/pipeline-cpu/src/color_detail.rs:color` |

### 24 MP Gaussian apply, current; union of three operation windows

Evidence: `gaussian-apply-to-frame.summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/prof_gaussian_isolated_24mp.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 10,520 | `filters::convolve  [b5prof_followup-5d248d08350dc754]` | `crates/filters/src/lib.rs:convolve` |
| 702 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |
| 662 | `tessera_ffi::document::filtering::read_raster  [b5prof_followup-5d248d08350dc754]` | `crates/tessera-ffi/src/document/filters.rs:read_raster` |
| 270 | `<filters::Buffer>::write  [b5prof_followup-5d248d08350dc754]` | `crates/filters/src/lib.rs:Buffer::write` |
| 254 | `<tessera_ffi::document::DocumentSession>::write_pixels  [b5prof_followup-5d248d08350dc754]` | `crates/tessera-ffi/src/document/filters.rs:DocumentSession::write_pixels` |

### 24 MP Liquify commit, current; union of three operation windows

Evidence: `liquify-commit-to-frame.summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/prof_liquify_isolated_24mp.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 368 | `filters::liquify::sample  [b5prof_followup-5d248d08350dc754]` | `crates/filters/src/liquify.rs:sample` |
| 238 | `std::sys::backtrace::__rust_begin_short_backtrace::<<filters::liquify::Mesh>::render::{closure#2}::{closure#0}, core::result::Result<alloc::vec::Vec<(u32, u32, engine_api::tile::Tile)>, engine_api::error::EngineError>>  [b5prof_followup-5d248d08350dc754]` | `crates/filters/src/liquify.rs:Mesh::render` |
| 213 | `<filters::liquify::Mesh>::displacement_at  [b5prof_followup-5d248d08350dc754]` | `crates/filters/src/liquify.rs:Mesh::displacement_at` |
| 149 | `std::sys::backtrace::__rust_begin_short_backtrace::<filters::liquify::read_source::{closure#0}::{closure#0}, core::result::Result<(), engine_api::error::EngineError>>  [b5prof_followup-5d248d08350dc754]` | `crates/filters/src/liquify.rs:read_source` |
| 76 | `compositor::raster::load_normalized  [b5prof_followup-5d248d08350dc754]` | `crates/compositor/src/raster.rs:load_normalized` |

### Export Flat UI, 18 MP smart filter, reused

Evidence: `fperf-2/exp-18.json`; trace: `fperf-2/run.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 1,389 | `filters::convolve  [Tessera]` | `crates/filters/src/lib.rs:convolve` |
| 594 | `pow  [libsystem_m.dylib]` | `system/framework symbol; source not in repository` |
| 402 | `<compositor::render::Compositor>::smart_tile::{closure#2}  [Tessera]` | `crates/compositor/src/render/mod.rs:Compositor::smart_tile` |
| 370 | `<std::hash::random::DefaultHasher as core::hash::Hasher>::write  [Tessera]` | `Rust std/hash:DefaultHasher::write; compositor/history callers` |
| 270 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |

### Export Flat UI, 14 MP styled document, reused

Evidence: `fperf-2/exp-styled.json`; trace: `fperf-2/run.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 622,858 | `compositor::render::styles::blur  [Tessera]` | `crates/compositor/src/render/styles.rs:blur` |
| 116,699 | `<compositor::raster::Raster>::pixel  [Tessera]` | `crates/compositor/src/raster.rs:Raster::pixel` |
| 28,381 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |
| 17,181 | `<compositor::raster::Raster>::edit_region::<compositor::render::styles::render::{closure#3}::{closure#0}>  [Tessera]` | `crates/compositor/src/render/styles.rs:render` |
| 9,806 | `<compositor::render::styles::Mask>::sample  [Tessera]` | `crates/compositor/src/render/styles.rs:Mask::sample` |

### Paint undo/redo, current; amplified 3 × 500 cycles, setup excluded

Evidence: `paint-undo-redo-500-cycles.summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/prof_paint_undo_repeated_24mp.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 463 | `mach_msg2_trap  [libsystem_kernel.dylib]` | `system/framework symbol; source not in repository` |
| 104 | `start_wqthread  [libsystem_pthread.dylib]` | `system/framework symbol; source not in repository` |
| 71 | `__psynch_cvwait  [libsystem_kernel.dylib]` | `system/framework symbol; source not in repository` |
| 52 | `<compositor::resident::ResidentRenderer>::raster_node  [b5prof_followup-5d248d08350dc754]` | `crates/compositor/src/resident/mod.rs:ResidentRenderer::raster_node` |
| 43 | `_platform_memset  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |

### Filter undo/redo, current; amplified 3 × 500 cycles, setup excluded

Evidence: `filter-undo-redo-500-cycles.summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/prof_filter_undo_repeated_24mp.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 711 | `mach_msg2_trap  [libsystem_kernel.dylib]` | `system/framework symbol; source not in repository` |
| 133 | `start_wqthread  [libsystem_pthread.dylib]` | `system/framework symbol; source not in repository` |
| 100 | `__psynch_cvwait  [libsystem_kernel.dylib]` | `system/framework symbol; source not in repository` |
| 97 | `<compositor::resident::ResidentRenderer>::raster_node  [b5prof_followup-5d248d08350dc754]` | `crates/compositor/src/resident/mod.rs:ResidentRenderer::raster_node` |
| 83 | `<deduplicated_symbol>  [b5prof_followup-5d248d08350dc754]` | `optimizer merged symbol; exact source unavailable` |

### Viewport/full-level 24 MP engine, current; whole benchmark includes fixture setup

Evidence: `viewport.summary.json`; trace: `/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/7f25c146-4b88-4a52-b274-0c8874eaaba3/scratchpad/prof/followup-20261001/viewport.trace`.

| Self sample ms | Symbol | File:function |
|---:|---|---|
| 93 | `mach_msg2_trap  [libsystem_kernel.dylib]` | `system/framework symbol; source not in repository` |
| 85 | `<&<compositor::resident::ResidentRenderer>::materialize::{closure#0} as core::ops::function::FnMut<(&(u64, engine_api::tile::Tile),)>>::call_mut  [b5prof_bench-3f93875fc26fc9cc]` | `crates/compositor/src/resident/mod.rs:ResidentRenderer::materialize` |
| 73 | `b5prof_bench::doc24  [b5prof_bench-3f93875fc26fc9cc]` | `profiling harness b5prof_bench.rs:doc24 (fixture setup; not a product fix)` |
| 37 | `AGX::BlitDispatchContext<AGX::HAL200::Encoders, AGX::HAL200::Classes, AGX::HAL200::ObjClasses>::checkDependentBlits(GPUVirtualAddressRange const&, GPUVirtualAdd  [AGXMetalG16X]` | `system/framework symbol; source not in repository` |
| 26 | `_platform_memmove  [libsystem_platform.dylib]` | `system/framework symbol; source not in repository` |

## Ranked candidate fixes (estimates, not measured gains)

| Rank | Mapping / owner | Candidate and evidence | Estimated benefit / verification |
|---:|---|---|---|
| 1 | P15, A | Cache style source/effect planes per revision and replace the expensive blur evaluation. Styled export spends >620,000 aggregate CPU ms in `styles::blur`; `Raster::pixel` adds >116,000 ms. | Aim for 5–10× on the styled fixture, 83 s toward 8–17 s, contingent on eliminating repeated work. Pixel parity and cache-memory bound required. |
| 2 | P19 + P11, A primary / B routing | Camera Raw whole-image preview fallback and image conversion: `Image::tile`, `downsample_crop`, encode/output serialization. Keep local preview locality, cache compatible full-image intermediates for global settings. | Current global 100% preview is 10.24 s versus 0.533 s local (old B5-28: 6.05 s versus 0.50 s); target 2–5× global-preview improvement, reduce multi-GB intermediates. Global semantics must remain exact. |
| 3 | P16 + new P20, B | Background export still has main run-loop spikes, despite a near-zero engine snapshot in the simple fixture. Profile SwiftUI invalidation/progress/model refresh and bound/coalesce work. Do not move already-background encoding again. | Reduce observed 73–76 ms first-export max toward <8 ms (about 90% reduction in worst busy span); throughput improvement unproven. Rerun with matched load and explicit main spans. |
| 4 | P13 + P19, A (B FFI routing support) | Vector drag still rematerializes/rasterizes tiles and uploads buffers: materialize 579 CPU ms, live_tile 434, tile conversion 258 in a ~2.1 s drag window. Preserve transformed fill tiles/dirty regions and residency. | Investigate 2× latency reduction, ~153 ms toward <80 ms and ≥25–30 frames/60 previews. Must retain pixel parity and dashed-stroke fallback. |
| 5 | P19, A | Gaussian `convolve` dominates apply (3,500 CPU ms within 1.09 s); optimize the existing separable loops with SIMD, reused scratch buffers, or an exact GPU route at the app boundary. | Plausible 2–4× apply improvement (1.1 s toward 0.3–0.6 s), subject to kernel/radius contract and transfer costs. |
| 6 | P14 + new P21, B | `DocumentController.reloadModel` / history byte accounting during edits: hasher and graph traversal prominent in document and paint traces. Cache history accounting by revision and narrow UI refresh. | Target lower CPU and main p95, potentially tens of ms on expensive refreshes; no isolated causal savings measured. |
| 7 | P01/P02, B | Complete scenario-specific spans and fixture control: saved-layer open, full-stroke wall, filter commit/undo, actual main occupancy. Keep release provenance and loaded-host labels with every trial. | Measurement confidence, not a runtime speedup. Avoid calling a sampled on-CPU cluster a main-thread maximum. |

P03–P12 (except P11 overlap), P17 and P18 are not reprioritized from these document-focused runs. Original acceptance criteria remain useful; this report does not claim the entire original audit is resolved. Existing short lock holds on the synthetic fixture lower the priority of another broad P14 rewrite; styled/filter contention still needs its own evidence.

## Coverage limitations

The current follow-up supplies three saved-layer engine opens, the exact 20 MP tools fixture, isolated engine Liquify and amplified undo/redo profiles, and three viewport/full trials. Exact application saved-layer open-to-present and whole-stroke wall spans remain unavailable. Engine-only phases have no AppKit main-thread measurement. The app spans cannot be inferred from Rust test-thread timings. The existing background exporter test uses 18 MP smart-filter and 14 MP styled documents, not a 24 MP styled fixture. These gaps are explicit; see follow-up evidence below for newly completed measurements.

## Verification and handoff

Release packaging and provenance verification succeeded. Current Camera Raw app self-tests: 3/3 zero failures. Corrected 20 MP tools app checks: 3/3 zero failures; trial 3 foreground-invariance check failed as documented above. Timing-span self-tests: 3/3 complete, zero dropped events. Engine measurement cases completed successfully; raw results and sample counts are preserved in JSON/scratch. The temporary `crates/tessera-ffi/tests/b5prof_followup.rs` was removed. The pre-existing untracked `b5prof_bench.rs` and profiling scripts were preserved and are excluded from the report-only commit.

Main-thread coverage remains partial: engine-only cases do not establish an AppKit main-thread maximum, sampled CPU clusters do not include all blocking, and occluded drawable callbacks do not establish input-to-display latency. No product fix or performance acceptance gate is claimed. No push is part of this handoff.
