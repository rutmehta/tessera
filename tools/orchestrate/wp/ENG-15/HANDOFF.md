# ENG-15 handoff: GPU-declined export leak, CR3 Web export time (REV-ENG-14)

Branch `wp/ENG-15`, based on `wp/ENG-14` `a4cc94c7`, which already sits on
`origin/main` `4ba008f1` (batch 62): `git rebase origin/main` is a no-op.
It carries the four ENG-14 commits and merges as batch 63. Worker: Claude
Opus 5.5.

## Item table (finding -> code -> test)

| # | Finding (REV-ENG-14) | Code | Test |
|---|---|---|---|
| SF2 (priority) | Every GPU-declined export leaves ~1.8 MiB of upload staging on the device until the next GPU submission: unbounded in a headless batch of Texture/Clarity/Dehaze images | `export/src/gpu.rs`: (1) `render_with_options` decides the presence decline **before** `GpuManagedOutput::new` uploads its ICC/gamut/transfer tables, with the tile path's own band computation (factored out as `tile_band_rows`) at an upper bound of the job's budget, so anything declined there is declined later too. (2) A drop guard (`FlushUploads`) declared before the output is built calls the new `GpuContext::flush_uploads` (empty submission + non-blocking poll) on every later exit: bands, tiles, a decline from either, or an error. | `export/tests/gpu_declined_device.rs` (single-test binary: the Metal counter is process-wide). 2048x1024 synthetic Bayer, Clarity 30 / Dehaze 25 / Texture 20 x {no resize, long edge 1500}, twice: the idle device baseline (`export::idle_device_allocated_bytes`, doc-hidden) must stay within 64 KiB after every export. |
| SF1 | CR3 Web export +22% since ENG-14 (16 -> 19 bands) | `pipeline-gpu/src/resident.rs` `Batch::buffer`: export transactions take the smallest pooled buffer within 1/64 (`RECYCLE_SLACK`) of the requested size instead of an exact match. `GpuManagedOutput::bindings` accepts a source buffer at least the layout's size (was: exactly). | `pipeline-gpu/tests/export_recycling.rs`: one worker under a scratch of the largest band alone + 1/32, bands of 128 and 129 rows in both orders: interior bands allocate at most 2 more fresh buffers than bands of one height, every band is bit-identical to the same band rendered alone, and within the scratch. Plus `export/tests/web_export_timing.rs` (ignored harness, public API only). |
| NIT 1 | `Batch::count()` never refuses | Doc comment on `Batch::count`: parameters can push a transaction past its share once no idle buffer is left; this stays unreachable only through the planner's 5% margin and `PARAMS_BYTES`. | n/a |

## SF2: leak before / after

`gpu_declined_device`, idle device bytes after each declined export minus the
baseline taken after one warm-up export (MiB):

| | after exports 1..12 | baseline |
|---|---|---|
| RED `72252af1` | 1.81, 3.62, 5.44, 7.25, 9.06, 11.88, 13.69, 15.50, 17.31, 19.12, 20.94, 22.75 | 3.14 |
| fix `aba86043` | 0.00 x 12 | 1.08 |

- RED reproduces the reviewer's 1.8 MiB per export (+1 MiB once on the
  first Texture + resize export).
- With only the flush guard (the early decline disabled locally, not
  committed) the leak is gone too: one 1.00 MiB step on the sixth export,
  then flat through the twelfth. Both paths ended on the CPU (traced), so
  that step is the driver's, not an export allocation.
- With both, nothing remains: a presence export over one tile band now
  creates no output tables at all.

Audit of other early-return / decline paths:
- `render_with_options`: every return after the output is built is covered
  by the guard: the band planner's decline, a band's `Unsupported`, the
  tile path's declines, errors. Returns before it (non-CFA source, locals,
  denoise, lens plan None, enlargement, no device) upload nothing.
- `pipeline-gpu` `Batch`: host queue writes already flush on drop
  (`PendingUploads`). The parameter arena (mapped at creation) is never
  unmapped when a transaction is abandoned, so no copy is queued.
- The only non-test `GpuManagedOutput::new` caller is export. `tessera-ffi`
  reaches GPU export only through the export crate; its own
  `create_buffer_init` (document render) is followed by a submit in the same
  function. Viewport/output-LUT uploads are followed by frame submissions.
- Noted, not changed: `export/src/depth.rs` `try_resident` opens a new
  `GpuContext` (its own device) per neural-denoise export, before checking
  that the method is Neural. A dropped device frees everything, so this is
  cost, not a leak.

## SF1: what the time was

`TESSERA_EXPORT_TRACE` + `TESSERA_GPU_PROFILE` on CR3 Web full-res at the
ENG-14 tip: **16 dispatches per band**, about 2 ms of GPU compute, not
~10k (the profile prints every dispatch; the 10k figure did not reproduce).
Bands took 8 ms each because every band allocated all 17 of its buffers
(~150 MiB) afresh (`fresh_buffers=17`, with 135 MiB recycled in): the
planner's bands alternate between 246 and 247 sensor rows, the two workers
take them in turn, and recycling matched sizes exactly, so it missed and
evicted. Fresh buffers cost Metal allocation and wgpu's zero fill. ENG-14's
three extra bands multiplied that.

With 1/64 slack, interior CR3 bands allocate 3 or 4 fresh buffers (the
sensor upload and parameter arena, which are always fresh). The two workers
start empty for every export, so their first bands still allocate.

Option (a) of the review (release upload staging at the band's first
submission, refit the sensor term to 26 B) was not done: a band normally
submits once, at its readback, so its first submission is its last; an
earlier one would need a blocking wait that also waits for the other
worker's band. With recycling fixed, 19 bands cost what 16 did. Device
peaks are unchanged, so the planner is not refit.

## Timing (ms, median of 8 after one warm-up, alternating ENG-13 / ENG-14 / tip)

`web_export_timing`, long edge 2048, render only (no encode). ENG-13 is
`origin/main` `4ba008f1` (tree of ENG-13's merge), ENG-14 is `a4cc94c7`,
tip is `0a83b465` (code identical to the final tip). Each process: decode,
warm-up, 8 timed exports. Three rounds; round 0 ran at load average 26-30
(other lanes building), rounds 1-2 at 17-28. Wall = `render_one`, GPU =
the trace's "GPU bands (incl. yields)".

| export | bands | ENG-13 wall (GPU) | ENG-14 wall (GPU) | tip wall (GPU) |
|---|---|---|---|---|
| CR3 Web full-res | 16 / 19 / 19 | 124.8 / 83.2 / 85.1 (93.6 / 55.6 / 56.8) | 136.6 / 104.3 / 104.3 (108.2 / 76.0 / 76.1) | 151.9 / 81.2 / 84.2 (113.2 / 53.8 / 56.5) |
| CR3 Web pyramid | 16 / 19 / 19 | 139.2 / 85.7 / 84.8 (107.3 / 57.9 / 55.5) | 162.2 / 108.3 / 105.4 (127.0 / 79.2 / 77.5) | 112.3 / 85.8 / 85.1 (77.8 / 56.9 / 56.7) |
| DNG Web full-res | 17 / 21 / 21 | 135.1 / 93.2 / 89.9 (107.7 / 67.2 / 64.4) | 154.8 / 102.2 / 99.8 (121.3 / 76.3 / 73.2) | 138.2 / 91.3 / 88.2 (111.2 / 65.2 / 61.8) |
| DNG Web pyramid | 7 / 8 / 8 | 76.7 / 58.1 / 55.5 (49.4 / 31.9 / 29.9) | 82.7 / 58.4 / 58.3 (48.3 / 33.2 / 32.8) | 74.5 / 57.0 / 54.8 (46.0 / 31.2 / 29.4) |
| NEF Web full-res | 43 / 43 / 43 | 276.4 / 190.3 / 185.4 (237.7 / 158.1 / 154.2) | 317.0 / 195.6 / 185.1 (281.9 / 163.2 / 153.3) | 219.8 / 164.2 / 158.6 (177.7 / 131.9 / 126.8) |
| NEF Web pyramid | 14 / 15 / 15 | 153.9 / 87.1 / 89.1 (107.0 / 55.0 / 56.8) | 144.8 / 87.6 / 91.7 (100.8 / 55.8 / 60.0) | 90.3 / 82.4 / 84.2 (58.8 / 50.6 / 52.6) |

Rounds 1-2 (quieter):
- CR3 Web full-res: ENG-14 +24% over ENG-13 (104.3 vs 84.2); tip 82.7,
  -1.8% vs ENG-13. Pyramid: ENG-14 +25%; tip 85.5 vs 85.3 (0%).
- DNG Web full-res: ENG-14 +10%; tip -2%. Pyramid: tip -1%.
- NEF Web full-res: tip -14% vs ENG-13 (161 vs 188; more bands of a
  steadier size recycle more). Pyramid: tip -5%.

The CR3 regression is recovered with 19 bands, still within budget.

## Device peak (MiB, `MTLDevice.currentAllocatedSize`)

- Ignored diagnostic `five_fixture_device_peak_within_budget`, run alone at
  the tip: passes, maximum **367.3 MiB** (CR3 Web full-res + grain) against
  `BUDGET` 384. ENG-14's runs reached 380.8 to 382.9. All 45 cells are in
  the gate log section below.
- Synthetic `export_device_peak` with the recycling fix: 174.5 MiB against
  177.7 MiB of shares (ENG-14: 170.8 to 175.3), and green in the gate.
- Five-fixture footprint tests (`TESSERA_REQUIRE_RAW_FIXTURES=1`, in the
  workspace gate): pass, no band over its share.

## Gates

All gates ran at the code tip `579a9a03` (rebased: `origin/main` is
`4ba008f1`, already the base), after `cargo clean --release -p pipeline-gpu
-p export`. Load average 10 to 17, other lanes running.

| Gate | Result |
|---|---|
| `TESSERA_REQUIRE_RAW_FIXTURES=1 cargo test --release --workspace --no-fail-fast` | exit 0. 696 result lines, **3578 passed, 0 failed, 102 ignored** (ENG-14: 693 / 3576 / 101): +2 tests (`gpu_declined_device`, `export_recycling`), +1 ignored (`web_export_timing`), +3 binaries. No RAW SKIPPED line; the SKIPPED lines are absent model weights or opt-in private samples, as before. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift (only this HANDOFF was untracked) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 1 skipped, 0 failures; Swift Testing: 5 passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete (149 s) |

Also run separately, before the gates, at the same code:
- `cargo test --release -p pipeline-gpu -p export -p image-core` with raw
  fixtures required: 494 passed, 0 failed, 22 ignored.
- The ignored `five_fixture_device_peak_within_budget`, alone
  (`cargo test --release -p export --lib five_fixture_device_peak -- --ignored --test-threads=1`):
  passes, maximum 367.3 MiB (above).

## Commits

- `72252af1` test: GPU-declined exports leave no device memory (RED)
- `aba86043` fix: GPU-declined exports leave no staging on the device
- `70fb0c0b` docs: `Batch::count` never refuses and relies on the planner's 5% margin
- `95d72a5c` test: export bands a row apart recycle like bands of one height (RED)
- `1c561671` perf: export bands recycle buffers up to 1/64 larger
- `0a83b465` test: opt-in Web export timing harness
- `579a9a03` test: calibrate the recycling test's interior window and allowance
  (after the RED commit; the RED code still fails the final form, 14 fresh
  buffers per band against at most 4)
- this HANDOFF

## Not done / notes

- Review option (a) (earlier staging release, planner refit to 26 B): not
  needed, see SF1. Option (b) (fewer dispatches): there are only 16 per band.
- The two band workers' pools start empty for every export, so each export's
  first bands still allocate afresh. Keeping buffers across exports would
  hold up to the export budget on the device between exports; not done.
- `RECYCLE_SLACK` applies to export transactions only; interactive renders
  keep exact matching.
