# ENG-14 handoff: export effects map and uncounted device memory (REV-ENG-13)

Branch `wp/ENG-14`. Based on `wp/ENG-13` `f0359c12` and rebased onto
`origin/main` `f9798515` (batch 61), so it carries the three ENG-13
commits (replayed without conflicts) and the four ENG-14 commits. It
merges as batch 62. Worker: Claude Opus 5.5.

## Item table (finding -> code -> test)

| # | Finding (REV-ENG-13) | Code | Test |
|---|---|---|---|
| SF1 | The effects constants map (8 B/px of the whole frame) is uncounted and built once per band worker: 903 MiB device peak on the NEF | Ruling option (a). `Batch::effects_map` returns None for `export_float` transactions (bands and the tile fallback), so the inline WGSL path runs. The map stays for interactive renders. The test-only `ManagedRenderer::with_export_effects_map` forces it, with its own map cache. `GpuStats.effects_maps` counts maps built. | `pipeline-gpu/tests/export_effects.rs` (bands of changing shape, resized or not; tiles). Export `exports_use_the_inline_effects_path_bit_identical_to_the_map` covers synthetic Bayer and X-Trans, bands, 16-row bands and tiles, levels 0 and 1, resized or not. `five_fixture_exports_use_the_inline_effects_path_bit_identical_to_the_map` covers 5 fixtures x {full chain, Web full-res, Web pyramid} x {vignette, grain}. All require 0 maps built in production, at least 1 when forced, and **0 differing samples (bit for bit)**. |
| SF2 | Export memory still uncounted: readback staging, parameter arenas, resize offsets and taps, output LUT | The meter now counts every device allocation of a resident transaction. `host_buffer` counts each buffer and wgpu's staging copy of its queue write (parameters, resize params/offsets/taps, Detail and effects parameters, uploads); the staging bytes are released at a completed `checkpoint`. Payload uploads (`upload_buffer`: sensor rows, raw tiles, packed CFA) are refused by `fit` when buffer + staging do not fit. A parameter arena counts 1 MiB, plus 1 MiB for wgpu's mapped-at-creation staging copy. `read_now` staging is counted. The readback staging copy is counted in the published peak (`Meter::publish(counters, readback)`). Export subtracts the output's ICC, gamut and transfer tables (`GpuManagedOutput::device_bytes`, doubled for staging) from `BUDGET` before splitting it. Each worker reserves its part of them. The band planner is refit: `SENSOR_BYTES_PER_PIXEL` 26 -> 30 (upload staging) plus `PARAMS_BYTES` 2 MiB per band. | `pipeline-gpu/tests/export_device_peak.rs`: a single-test binary, because the device counter is process-wide. Two band workers recycle under shares of need + 1/64 while `MTLDevice.currentAllocatedSize` is sampled, and the device peak must be within the shares. RED: 174.5 > 168.7 MiB. Plus the ignored export diagnostic below. |
| SF2 measurement | Measure the reviewer's way, in the repo | `gpu_core::device_allocated_bytes` (objc2-metal `currentAllocatedSize` through `wgpu::Device::as_hal::<Metal>`, no new dependency), exposed as `GpuContext::device_allocated_bytes` / `idle_device_allocated_bytes` (doc-hidden diagnostics). | Export `five_fixture_device_peak_within_budget` is `#[ignore]` because the counter is process-wide. It runs 5 fixtures x 3 scales x {none, vignette -40, grain 40}, does a warm-up export, then samples every 0.3 ms during a second export, and requires peak minus the idle baseline <= `BUDGET`. Run it alone: `cargo test --release -p export --lib five_fixture_device_peak -- --ignored --test-threads=1`. |
| NIT 1 | Packed-CFA upload charged but never released | Comment in `resident_cfa.rs`: it is counted, never returns to `free`, and is conservative. | n/a |
| NIT 2 | `make_room` / idle "releases its memory at once" | The `Pool::idle` doc now says that dropping lets wgpu free the buffer once earlier submissions have completed. | n/a |
| NIT 3 | Per-band-renderer counters | `export_band` and `band_with` now document that every `stats()` counter is per band renderer. | `band_renderers_keep_their_own_statistics` (unchanged) |
| NIT 4 | Non-export recycle bound | Left as a design item, as the reviewer advised. | n/a |

## Why (a) works, and what it costs

The map and inline paths call the same WGSL functions (`vignette_mask`,
`grain_value`, `effects_apply`). The map stores them as f32 and reads them
back exactly. Every comparison above showed 0 differing samples (also at the
RED commit, where export still built the map). Bands touch each pixel once,
so the inline path repeats no work: exports with vignette or grain got
faster (timings below).

## Planner refit (trace, five fixtures, `TESSERA_EXPORT_TRACE=1`)

Metering staging raised the bands' true "actual". Without the refit, 681 of
1321 traced bands exceeded their plan, with a largest actual/planned ratio of
**1.056**. They stayed within the share only through the 5% margin. With +4
B per sensor pixel and 2 MiB per band, the largest ratio over 1405 traced
bands is **0.989** and none is above 1, which matches ENG-13's 0.989. Bands
are slightly smaller: for example, CR3 Web full-res goes from 16 to 17 bands.
In the five-fixture tests, every band's `max(peak, live + readback)` is
within its share of 191.1 MiB (`(384 MiB - 1.7 MiB tables) / 2`). The worst
is 190.3 MiB peak and 191.1 MiB, and all exports stay on the band path.

## Device peak, before / after (MiB, `MTLDevice.currentAllocatedSize`)

Before is the RED commit `93e72f23` (`75de783a` before the rebase) (export behaviour of ENG-13). After is
the code tip. Each cell is the maximum over 3 alternating runs (before,
after, before, ...), measured as the second export of each configuration,
minus the idle baseline. `BUDGET` = 384 MiB.

| camera | scale | none | vignette | grain |
|---|---|---|---|---|
| CR3 | full chain | 378.2 / 357.8 | 622.3 / 357.8 | 622.2 / 364.5 |
| CR3 | Web full-res | 375.2 / 345.0 | 612.9 / 345.8 | 612.7 / 345.0 |
| CR3 | Web pyramid | 372.5 / 374.9 | 619.3 / 345.4 | 619.7 / 345.2 |
| ARW | full chain | 368.2 / 358.5 | 614.2 / 353.6 | 613.8 / 360.4 |
| ARW | Web full-res | 346.0 / 334.5 | 580.8 / 334.9 | 586.2 / 335.9 |
| ARW | Web pyramid | **392.5** / 373.3 | 453.8 / 355.1 | 444.8 / 357.1 |
| NEF | full chain | 373.2 / 349.7 | **903.0** / 354.8 | **904.0** / 349.7 |
| NEF | Web full-res | **388.7** / 371.3 | **889.9** / 365.8 | **889.2** / 366.1 |
| NEF | Web pyramid | 373.8 / 366.1 | 511.8 / 366.1 | 505.1 / 360.0 |
| RAF | full chain | 373.8 / 346.0 | 611.8 / 346.0 | 611.8 / 346.0 |
| RAF | Web full-res | **393.0** / 371.0 | 588.1 / 371.0 | 587.6 / 371.0 |
| RAF | Web pyramid | **392.6** / 366.3 | 453.4 / 357.1 | 444.4 / 357.1 |
| DNG | full chain | 366.1 / 358.2 | 641.9 / 356.1 | 641.9 / 342.5 |
| DNG | Web full-res | 360.7 / 382.9 | 634.8 / 382.9 | 634.8 / 382.9 |
| DNG | Web pyramid | **387.8** / 360.5 | 456.7 / 362.3 | 457.0 / 360.3 |

- Before: 35 of 45 configurations were over `BUDGET`, up to 904 MiB. After:
  none. The maximum over every after-run I made is **382.9 MiB**, against
  384. The reviewer's numbers are reproduced: 903 MiB for NEF + vignette,
  and 392 to 394 MiB without effects.
- Effects no longer cost memory: vignette and grain columns equal the
  no-effects column, within run-to-run spread.
- The remaining headroom is small (about 1 to 2 MiB in the worst cells).
  The peak sits near `BUDGET` by design: workers keep idle buffers up to
  their share and drop them only on demand. Anything not metered would show
  up here first.

## Export time, before / after (ms, median of 3 alternating runs)

These are the second-export wall times from the diagnostic (GPU export, CPU
encoding not included). Load average was about 8 to 13 with other lanes
running.

| camera | scale | none | vignette | grain |
|---|---|---|---|---|
| CR3 | full chain | 92 / 85 | 116 / 92 | 110 / 89 |
| CR3 | Web full-res | 84 / 105 | 110 / 108 | 112 / 108 |
| CR3 | Web pyramid | 86 / 112 | 119 / 108 | 124 / 108 |
| ARW | full chain | 79 / 75 | 106 / 80 | 106 / 87 |
| ARW | Web full-res | 83 / 82 | 116 / 90 | 114 / 92 |
| ARW | Web pyramid | 54 / 52 | 59 / 55 | 60 / 55 |
| NEF | full chain | 146 / 144 | 214 / 161 | 223 / 174 |
| NEF | Web full-res | 194 / 185 | 260 / 200 | 264 / 210 |
| NEF | Web pyramid | 85 / 85 | 106 / 92 | 107 / 96 |
| RAF | full chain | 80 / 74 | 111 / 88 | 111 / 87 |
| RAF | Web full-res | 84 / 83 | 119 / 93 | 119 / 98 |
| RAF | Web pyramid | 52 / 56 | 60 / 59 | 60 / 58 |
| DNG | full chain | 91 / 86 | 117 / 100 | 115 / 96 |
| DNG | Web full-res | 89 / 102 | 132 / 115 | 132 / 109 |
| DNG | Web pyramid | 53 / 61 | 61 / 60 | 62 / 61 |
| **sum of 45** | | | | **4980 / 4419 (0.89x)** |

Vignette and grain exports are faster, because no full-frame map is built
per worker. For example, NEF full chain + vignette goes from 214 to 161 ms.
No-effects exports are unchanged within noise, except CR3 and DNG at Web
scale (84 to 105 ms, 86 to 112 ms, 89 to 102 ms). Those use slightly more,
smaller bands after the planner refit (CR3 Web: 16 to 17). The numbers are
noisy at this load. The Web pyramid cells differ by only a few ms in the
other cameras.

## Gates

All gates ran at the code tip `7c3783c0` (after the rebase onto
`origin/main` `f9798515`), after `cargo clean --release -p gpu-core -p
pipeline-gpu -p export`. Load average was 12 to 16, with other lanes
running.

| Gate | Result |
|---|---|
| `TESSERA_REQUIRE_RAW_FIXTURES=1 cargo test --release --workspace --no-fail-fast` | exit 0. 693 suites, **3576 passed, 0 failed, 101 ignored**. ENG-13 had 3571 / 691 / 100. The differences: +5 tests (2 in `export_effects`, 1 in `export_device_peak`, 2 export lib tests), +2 suites, and +1 ignored (the device-peak diagnostic). No RAW SKIPPED line; every SKIPPED line is an absent model weight or an opt-in private sample, as before. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift (only this HANDOFF was untracked) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 1 skipped, 0 failures; Swift Testing: 5 passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete |

Also run separately:
- `cargo test --release -p pipeline-gpu -p export`: 87 suites, 328
  passed, 0 failed.
- The ignored diagnostic `five_fixture_device_peak_within_budget`: passes.
  The maximum after the fix is 380.8, 382.1, 382.2 and 382.9 MiB in four
  separate runs.
- The synthetic `export_device_peak`: 175.3 / 170.8 / 175.3 / 174.5 MiB
  against 177.7 MiB of shares.
  - Its margin is about 2.4 MiB. The test sizes the shares from the
    metered need + 1/64, so it fails as soon as an unmetered allocation
    of a few MiB returns. That is intended, but watch it if it ever flakes
    under memory pressure.

## Commits (on top of the rebased ENG-13 commits)

- `93e72f23` test: export effects maps and true device peak (RED)
- `fe4421a2` fix: export builds no effects constants map (SHOULD-FIX 1)
- `5c6e5ed4` test: concurrent export bands' true device peak within their shares (RED)
- `7c3783c0` fix: meter every device allocation of an export band (SHOULD-FIX 2, NITS)
- this HANDOFF

The ENG-13 commits were replayed by the rebase as `cc43486a`, `10da2c1c`
and `ca41412e` (originally `46ff7a45`, `ee3369b1` and `f0359c12`).

## Not done / notes

- The `RECYCLE_BYTES` non-export bound is unchanged (REV-ENG-13 NIT 4,
  a design item).
- Interactive (viewport) transactions are metered the same way now, but
  nothing enforces a budget on them, as before.
- `with_export_effects_map` and the `GpuContext` device-counter accessors
  are `#[doc(hidden)]` public API, used only by tests and diagnostics.
- No Cargo.lock change. The device counter uses gpu-core's existing
  `objc2-metal` dependency.
