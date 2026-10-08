# ENG-10: fast Adobe-process exports and prints

Branch `wp/ENG-10`, rebased onto `origin/main` `ebe260a0` (ENG-9/9b merged).
Worker: Claude Opus 5.5. Source: REV-ENG-9 SHOULD-FIX 2 and the ENG-9b
follow-up. Synthetic fixtures only in tests; the benchmark reads the repo's
`fixtures/raw`. No board.json or Cargo.lock change, no new dependency.

## Summary

Adobe-process (imported Lightroom) exports and prints of a 16 MP RAW are
now 8-13x faster at full size and 10-13x faster at reduced sizes, use less
memory per render, and match Develop exactly instead of approximately.
**Corrected in ENG-10b (below):**
- In the app, file exports always render at scale 1 and then resize. A
  2048-px export is about 1.4 s per image, not the scale-2/4 figures.
- `export_pipeline` and its memory admission are CLI-only.
- Full-size output did change on CR3 and X-Trans when lens auto-calibration
  is selected.
Three things made them slow, and all three are fixed:

1. Export drew Adobe recipes with `pipeline_adobe` at full resolution and
   reduced afterwards, whatever the render scale. It now uses Develop's own
   renderer (`image_core::Renderer`, `AdobeStageOp` over the CPU operators)
   at the pyramid level of the render scale.
2. The Adobe compatibility barriers in that renderer (`AdobeStageOp::run_image`:
   Tone, Detail, Colour, Effects, the ToneExtra curve pass) walked the frame's
   tiles on one thread. They now run tiles in parallel with identical results.
   Develop itself (CPU and Metal sessions) gets the same speed-up.
3. The managed output transform (`pipeline_cpu::output_managed_linear`, the
   ICC conversion with gamut mapping) ran per pixel on one thread and also
   computed gamut warnings that file outputs throw away. It now runs in row
   bands on all cores, and file outputs skip the warnings. Pixels are
   unchanged. This also speeds up Native CPU exports, print and documents.

## Measurements (goal 1 and 5)

Benchmark: `crates/export/tests/eng10_adobe_bench.rs` (ignored; one
subprocess per case, so the peak RSS belongs to that case alone):

```text
cargo test --release -p export --test eng10_adobe_bench -- --ignored \
    --exact eng10_adobe_export_benchmark --nocapture
```

Knobs: `TESSERA_BENCH_FILE`, `ENG10_CASES`, `ENG10_PROCESS=native`,
`TESSERA_EXPORT_BACKEND`, and `TESSERA_EXPORT_TRACE=1` for phase times
(new phases: "Adobe Develop render", "output transform").

Recipe: Adobe PV6 with exposure +0.3, contrast 20, highlights -40,
shadows +30, whites 10, blacks -5, vibrance 15, saturation 5, default
sharpening. Cases:
- `export-s1`: full-size JPEG q90, render scale 1.
- `export-s4`: JPEG at render scale 4.
- `print-8x10`: `render_pixels_with_notes` into a 3000x2400 box (8x10 in at
  300 dpi), Display P3, glossy sharpening, scale from FFI `print_scale`
  binning (1 for these sensors).
- `print-4x6`: 1800x1200 box (4x6 in at 300 dpi), scale 2.
- `batch20`: 20 Web exports (long edge 2048, scale 2, screen sharpening)
  through `export_batch`.
- `batch20-full`: 20 full-size exports through `export_batch`.

Before is the ENG-9 tip `2fe4d42e` with only the benchmark file added,
built in a temporary worktree. Before and after ran interleaved, two rounds
each, on a 16-core machine with 48 GiB under other load (load average
5-25, given per row as the range over the four runs). Default Rayon and
renderer threads. Wall time is decode-excluded render, encode and commit;
peak RSS includes the decoded source (about 133 MiB for the ARW).

### Sony ARW, 4920x3276 (16 MP)

| Case | Before: s | After: s | Speed-up | Before: peak MiB | After: peak MiB | Load |
| --- | --- | --- | --- | --- | --- | --- |
| export-s1 | 12.28-12.34 | 1.45-1.47 | 8.4x | 1224-1229 | 1070-1082 | 9-11 |
| export-s4 | 4.67-4.69 | 0.35-0.36 | 13x | 1182-1189 | 341-347 | 11-14 |
| print-8x10 (scale 1) | 11.51-12.56 | 1.43-1.46 | 8.3x | 1400-1404 | 1252-1260 | 14-17 |
| print-4x6 (scale 2) | 6.05-6.14 | 0.58-0.60 | 10x | 1241-1306 | 507 | 16-24 |
| batch20, Web | 65.8-70.9 (3.3-3.5 per image) | 7.2-7.4 (0.36-0.37) | 9.4x | 2421-2429 | 893-902 | 18-23 |
| batch20-full | 129.3-130.9 (6.5 per image) | 24.0 (1.20) | 5.4x | 2467 | 2578-2599 | 13-25 |

### Nikon NEF, 7378x4924 (36 MP)

| Case | Before: s | After: s | Speed-up | Before: peak MiB | After: peak MiB |
| --- | --- | --- | --- | --- | --- |
| export-s1 | 92.4-93.1 | 8.1-8.4 | 11x | 3525 | 2257-2269 |
| export-s4 | 37.1-37.7 | 0.73-0.75 | 50x | 3441-3446 | 650 |
| print-8x10 (scale 1) | 47.4-48.1 | 1.96-1.98 | 24x | 3515 | 1001 |
| print-4x6 (scale 2) | 36.6-37.4 | 0.78-1.29 | 30-47x | 2777-3466 | 667-670 |

The NEF print-8x10 box is smaller than the sensor, so the FFI binning picks
scale 2 there (3000x2002 out), which is why it is faster than export-s1.

### 24 MP (interpolated, not measured)

No 24 MP fixture exists. Interpolating by pixel count between the
measured 16 MP and 36 MP rows (so the 36 MP nonlinearity is included):
full-size export about 4 s (before about 45 s), scale-4 export about
0.5 s (before about 18 s), peak about 1.5 GiB (before about 2.1 GiB).

### Reference points

- Native GPU export of the same ARW (main's former, wrong-look path for
  these photos): 0.27-0.42 s (REV-ENG-9). The Adobe full-size export is now
  about 3.5x that rather than 30x.
- Develop's renderer alone, level 0, same recipe: Adobe on CPU 0.91-0.93 s,
  Adobe on the Metal backend 1.0-1.2 s, Native on CPU 6.0-7.5 s, Native on
  Metal 0.5 s. Level 2: Adobe 0.33 s CPU, 0.38-0.48 s Metal.
- Phase split of a full-size ARW export after the change
  (`TESSERA_EXPORT_TRACE=1`, load 22-24): render about 1.0 s, output
  transform 0.4-0.6 s (it was 8.6 s), JPEG encode and commit about 30 ms.

## Design

### Scaled prefix through Develop's renderer (goal 2)

`export::adobe_render::render` (new) draws an Adobe-process recipe with
`image_core::Renderer` at level `log2(render_scale)` and assembles the
`SceneLinear` tiles (display-referred linear Rec.2020, before the Output
stage) into the export frame as they arrive.
- Called from `ai_masks::render_develop`, so file export
  (`render_one_cancellable`), print and documents
  (`render_pixels_with_notes`) and HDR export (`hdr::render`) all use it.
- The renderer is private to the export:
  - tile cache budget 0, so no f16 checkpoints;
  - the export's own resources, installed as the Develop session installs
    them: mask rasters (`ready_hooks`, split out of `ready_masks`), retouch
    kernels, depth provider, neural denoiser (CFA capability when CFA
    denoise is selected, the post-demosaic adapter otherwise);
  - the export's cancellation token, through the new
    `Renderer::render_region_into`. That is `render_region_as` with a
    cancellation token and a tile sink; `render_region_as` now delegates
    to it.
- Sources:
  - RAW: the sensor plane is copied into the renderer's own `CfaImage`, as
    the GPU export already does.
  - RGB and stored-frame RGB (`StoredRgb`, LR-8n): copied into an
    `RgbSource`, rendered in the stored frame; the export orients
    afterwards.
  - External Smart Previews: the proxy is cloned.
- `render_scale` must be 1, 2, 4 or 8 for Adobe recipes, as on every other
  path that takes it (`pipeline_adobe` used to accept any positive integer).
- The stage order is Develop's documented level order: Stages through
  White Balance run on full-resolution sensor tiles. The white-balanced
  frame is box-averaged to the level. Detail, Tone, Colour, locals,
  Effects and Geometry then run on level pixels (ENG-6). Smart Preview
  proxies keep the renderer's proxy route, which reduces after geometry.
- Level 0 is Develop's full-resolution path. It replaces
  `pipeline_adobe::render_linear_scaled(.., 1)`, which matches it within
  1e-4 linear on synthetic data
  (`compat_matches_standalone_with_and_without_dcp`). On the ENG-10
  fixtures the old path was already bit-identical for RAW and differed by
  6e-5 for RGB; now both are identical. **ENG-10b correction:** on the real
  CR3 and X-Trans RAF with lens auto-calibration selected, the old path
  differed from Develop by up to 18 and 96 levels, so those full-size
  exports changed toward Develop (see ENG-10b SF1).
- Native-process recipes, and external proxies that need Develop's
  resources, keep their previous path in `render_develop`.

### Parallel compatibility barriers (`image-core/src/adobe.rs`)

`for_each_tile` runs one scoped worker per core from a shared cursor,
polls cancellation before every tile and stops at the first error.
- Halo barriers (Detail) read every tile from the immutable input and
  write each tile once into a fresh frame. Only the copy-out takes the lock.
- Point barriers (Tone, Colour, Effects) do the same. An in-place version
  (`5023e4c1`) was measured and reverted (`d4a667ec`): reading under the
  frame lock left workers waiting on it (1879 against 375 mutex-wait
  samples), and the peak RSS did not fall.
- The ToneExtra curve pass stays in place under the lock. A separate frame
  there measured the same speed and about 190 MiB more peak memory.

### Parallel managed output (`pipeline-cpu/src/output.rs`)

- `output_with_transforms` splits the frame into contiguous row bands, one
  scoped thread each. Each band gets its own `color_mgmt::Transform`,
  resolved from the same context, because lcms transforms are `Send` but
  not `Sync`. Frames under 64 rows per worker use one thread.
- `output_managed_pixels` and `render_managed_scaled_pixels` skip the
  gamut-warning round trips (a Lab transform and a clipped destination
  round trip per pixel).
  - Used by: export `encode_output_profile` (Adobe and every
    resource-bearing export, print and documents) and `render_scaled_cpu`
    (Native CPU export).
  - Develop and display callers keep the warnings.

### Memory (goal 4)

- Per-render peak memory is lower everywhere (tables above). The old path's
  full-resolution f32 copies (`rgb.clone()` for Detail, the full-resolution
  prefix at every scale) are gone from exports. Scaled outputs now hold
  only level-size frames: a scale-4 ARW export peaks at 341-347 MiB instead
  of about 1.19 GiB.
- `export_pipeline` concurrency is now memory-aware for Adobe renders
  (`batch::pipeline_renders`). This is reached only from `tessera-cli`
  (ENG-10b SF2); the app exports one image at a time:
  - `adobe_render_bytes` estimates one render's host peak: the renderer's
    source copy, 96 MiB of demosaic chunk scratch, and 56 B per developed
    level pixel. This was fitted to the ARW, above the decoded source:
    the renderer alone peaks at 836 MiB at level 0 and 318 MiB at level 1,
    and a full-size export at 961 MiB.
  - Two renders run at once only while both fit one eighth of physical
    memory (`sysconf`; 1.5 GiB if it cannot be read).
  - Native admission is unchanged.
- The trade-off was measured on 20 full-size ARW exports (load 29-39):
  - one at a time: 34-40 s, 1.36-1.45 GiB peak;
  - paired: 26 s, 2.40-2.56 GiB peak.
- The resulting policy:
  - an 8 GiB machine renders full-size 16 and 24 MP Adobe exports one at
    a time;
  - a 48 GiB machine (this one) still pairs them, which is why
    batch20-full peaks at 2.6 GiB;
  - Web-size renders pair on both.

### GPU path (goal 3): investigated, not built

What already runs on the GPU:
- Under `AdobeStageOp`, the native stages go to the selected backend:
  decode, linearize, highlights, demosaic, lens, the matrices for proxies,
  geometry. The Metal backend `GpuStageOp` therefore already runs those for
  Adobe recipes in Develop.
- The resident path refuses Adobe recipes (`resident_render` `is_adobe`).
- The export's resident band renderer (`gpu.rs`) accepts only
  `NATIVE_CURRENT`.

Measured, Develop's renderer on the ARW:
- With the native stages on Metal, the Adobe render is not faster: 1.0-1.2 s
  on Metal against 0.91-0.93 s on CPU at level 0, and 0.38-0.48 s against
  0.33 s at level 2.
- After this lane the remaining cost is the Adobe compatibility stages
  themselves. The top of a level-0 profile is:
  - Colour (Lab/HSL: `cbrtf`, `atan2f`, `sinf`/`cosf`);
  - `pipeline_adobe::curves::evaluate`;
  - `basic_tone` (`exp2f`);
  - Detail (`expf`);
  - tile copies.

Why the export does not use the Metal backend for Adobe recipes:
- Metal native stages are not bit-exact with the CPU ones. The
  pipeline-gpu renderer gate allows up to 0.005 linear on Bayer, because of
  its f16 checkpoints, and 2 display codes.
- So a Metal-backed Adobe export would leave the ENG-9 bound against CPU
  Develop (0.53 level), and break this lane's exactness.
- It would also be slower, as measured above.

A real GPU Adobe path means porting the compatibility operators to Metal
kernels behind a settings-only admission check, with CPU fallback for
everything not admitted:
- `basic_tone`, exposure/baseline;
- DCP `apply_camera`/`apply_exposure`/`apply_tone`/`apply_look`;
- ProPhoto curves with `default_tone`;
- `color_detail::color`;
- Detail;
- `tone_extra`;
- the Adobe Output stage.

Exact equality with the CPU fallback is not achievable for those
operators: Metal `exp2`/`pow`/`atan2`/`cbrt` are not libm-identical. So
the follow-up needs a ruling on a tolerance-based parity contract, like
the Native GPU gates. The ceiling is roughly the Native Metal render time,
0.5 s against 0.9 s at level 0 here. Recorded as the follow-up below rather
than started.

## Parity (goal 2, tests)

`crates/export/tests/eng10_level_parity.rs`:
- Sources: a 128x96 Bayer RAW with hard-edged saturated blocks, a 120x88
  linear Rec.2020 RGB image with colours outside sRGB, and the repo's
  external Smart Preview DNG.
- Recipe: Adobe PV6, saturation +40, contrast 35, highlights -30,
  shadows +25, sharpening 60.
- Every source at scales 1, 2, 4 and 8, with Perceptual and Clip mapping.
  Plus a gradient mask with exposure and a crop at scales 1, 2 and 4, with
  a clone retouch spot on the RAW.

The contract: Develop's `SceneLinear` frame at level `log2(s)`, put
through the export's own managed sRGB transform, must
- equal the print floats bit for bit;
- equal the 16-bit TIFF file within 0.5 code plus 1e-3;
- have the same frame size.

The test compares after the same output transform on both sides. That
isolates what ENG-10 changes (which renderer, which level) from the output
transform, which ENG-9 already measures against Develop's 8-bit Output
stage.

| Rows | Before (`2fe4d42e`) | After |
| --- | --- | --- |
| RAW, scale 2/4/8 | print 0.33-0.56 off, file up to 36653 codes | print 0, file at most 0.500 code |
| RGB, scale 2/4/8 | print 0.14-1.0 off, file up to 65534 codes | print 0, file at most 0.500 |
| RGB, scale 1 | print 6e-5 off, file up to 5 codes (pipeline_adobe against Develop) | print 0, file at most 0.500 |
| RAW, scale 1 | print 0, file at most 0.500 | unchanged |
| Proxy, all | print 0, file at most 0.500 (the proxy route already reduced after rendering) | unchanged |

ENG-9's `eng9_develop_parity.rs` is unchanged and passes. Every Adobe row
has the same max and mean to three decimals before and after. The only
change is the RGB HDR PQ row, 0.502 to 0.500 codes.

### Pre-existing finding: ENG-9's 0.53-level bound is marginal on saturated dark channels

The first version of the level test (`92cfb7d2`, 8-bit rows against
Develop's Output stage) failed its 0.53-level bound at scale 1 on the
unchanged ENG-9 code, with this sharper fixture: RAW Perceptual 0.533,
RAW Clip 0.556.
- Where: pixels with one channel clipped and another dark (for example
  Develop 255/4/167 against export 255/3.40/167.4).
- Cause: the export's ICC transform (s15.16 matrix entries) and the Output
  stage's matrix differ by about 1e-5 relative. Next to a bright clipped
  channel, the steep sRGB toe turns that into 0.05-0.1 level. ENG-9's
  allowance for this is 0.03.
- This is independent of render scale and of ENG-10, so the level test was
  restructured (`5ce67e8d`) to compare through the same transform. No
  bound was changed.
- ENG-9's fixtures stay within 0.53, so its tests pass.
- Follow-up: the output-transform parity accounting (see below).

## Item table (goal → code → test)

| Goal / finding | Code | Test |
| --- | --- | --- |
| 1. Measure first | `export/tests/eng10_adobe_bench.rs` (ignored, per-case subprocess, peak RSS via `getrusage`); `TESSERA_EXPORT_TRACE` phases | numbers above |
| 2. Scaled prefix at the output level, Develop-level parity | `export/src/adobe_render.rs`; `ai_masks::render_develop` Adobe branch; `ready_hooks`; `Renderer::render_region_into` | `eng10_level_parity.rs` (4 tests; RED `92cfb7d2` and `5ce67e8d`, GREEN `f1072e75`) |
| 2. ENG-9 full-resolution parity not loosened | Level 0 is Develop's renderer | `eng9_develop_parity.rs` unchanged, green, same numbers |
| 3. GPU | Investigated (above); no GPU Adobe path, because exactness with CPU Develop is not achievable | `develop-gpu-L*` benchmark cases |
| 3. (CPU) single-threaded Adobe barriers | `AdobeStageOp::run_image` + `for_each_tile` | `image-core` `eng10_parallel_barriers_equal_the_serial_tile_loop` (bit equality: Detail across seams, Tone, Colour, ToneExtra; cancellation) |
| (found) Single-threaded managed output, 8.6 s of 14 s | `output_with_transforms`, `output_band` | `pipeline-cpu` `eng10_banded_output_transform_equals_one_pass` (bit equality and warnings, Perceptual and Clip, out-of-gamut input) |
| (found) Discarded gamut warnings computed per pixel | `output_managed_pixels`, `render_managed_scaled_pixels`; export callers | the same test checks the warning-free pixels |
| 4. Redundant full-resolution copies | Removed by 2 (no full-resolution prefix at scale > 1, no `pipeline_adobe` clones); tiles streamed into the frame | peak RSS above |
| 4. Memory-aware `export_pipeline` | `batch::pipeline_renders(_within)`, `adobe_render_bytes`, `adobe_pair_budget` | `export` `eng10_pipeline_pairs_adobe_renders_only_within_the_memory_budget` (RED `deac82e8`, GREEN `229591ee`) |
| 5. Before/after, 16 MP, 36 MP, 24 MP interpolated, 20-photo batch | — | tables above |

Test history notes:
- `deac82e8`'s 24 MP Native full-size row wrongly expected pairing, which
  the unchanged 16 Mi-pixel output rule never allows. `229591ee` corrects
  it and makes the machine's memory a parameter. The Adobe expectations
  did not change.
- The level test was restructured once, as explained above.

## Not done / follow-ups

- **GPU Adobe operators.** Metal kernels for the compatibility stages,
  behind admission with CPU fallback. This needs a ruling on a
  tolerance-based parity contract, because exact equality with CPU Develop
  is impossible for transcendental-heavy operators. Expected gain at most
  about 0.4 s per 16 MP full-size render.
- **Output-transform parity accounting (ENG-9).** On saturated content
  with a clipped bright channel and a dark one, export-against-Develop
  8-bit parity reaches 0.53-0.60 level at full resolution, on ENG-9's code
  as well. Either tighten the transform (for example the same matrix for
  well-behaved RGB profiles) or re-derive the allowance. This needs a
  ruling; nothing was changed.
- Further CPU headroom in the Adobe stages:
  - `Curve::is_identity` is evaluated per pixel inside
    `pipeline_adobe::curves`;
  - Colour runs Lab/HSL with libm transcendental functions.
  Neither was touched, to keep this lane's changes bit-exact and small.
- Native CPU Develop at level 0 (6-7.5 s on the ARW) has similar serial
  barriers on the Native side. Out of scope here.
- No "slow path" UI note was added: exports are now within a few times of
  the Native GPU path.

## Gates

Run on the final code tip `fb5080c0`, rebased onto `origin/main`
`ebe260a0`, after `cargo clean --release -p export -p image-core
-p pipeline-cpu -p pipeline-adobe`. Environment: `CARGO_BUILD_JOBS=5
RAYON_NUM_THREADS=5`.

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | 3518 passed, 0 failed, 109 ignored (exit 0). Run twice: once before the clippy style fix (`fb5080c0`), and again on the final code with the same result. Load average 8.7 at the start of the final run and 16.0 at the end. No wall-clock failures, no reruns |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean, after `fb5080c0` (`as_chunks_mut`, inline format arg) |
| `cargo fmt --all -- --check` | clean |
| `apps/mac/build-ffi.sh` | OK, no bindings drift (worktree clean apart from this HANDOFF) |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK (996 XCTest, 3 skipped, 0 failures) |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete |

## ENG-10b (REV-ENG-10 should-fix and ruling)

Binding review: `~/tessera-evidence/rulings/REV-ENG-10.out.md` (APPROVE
WITH SHOULD-FIX), plus the coordinator's ruling on the ENG-9 bound.

The branch was rebased onto `origin/main` `1c344175` (batch 58, ENG-7 lens
defaults). The rebase was clean.

Tests came first:
- `437846c1`: RED for SF3, then `deafa315` for the fix.
- `1c79ff8c`: RED for the ruling, then `94702212` for the bound.

| Item | Code | Test |
| --- | --- | --- |
| SF1: full-size output changed on CR3 and X-Trans; the HANDOFF and doc said it had not | Corrected the `adobe_render.rs` module doc and this HANDOFF; release note in `docs/RELEASE-NOTES.md` | `eng10b_real_fixture_parity.rs` |
| SF2: speed claims overstated what the app gets | Restated below; new benchmark cases `app-2048`, `app20-2048`, `app20-full`. The app's export scale policy is unchanged | benchmark |
| SF3: Adobe barriers ignored `RendererConfig.threads` | `AdobeStageOp::with_threads`, `StageOp::adobe_threads`, `Renderer::adobe_threads`. Every `AdobeStageOp` the renderer builds carries `config.threads` (`with_ops`, `for_process_version`, `prepare_dcp`'s DCP and baseline paths). `for_each_tile` uses at most that many workers, and runs on the calling thread for 1 | `image-core` `eng10b_barriers_honour_the_thread_cap`; `tests/adobe.rs` `eng10b_adobe_barriers_follow_renderer_threads` |
| Ruling: ENG-9 Adobe bound | `eng9_develop_parity.rs`: `ADOBE_MAX` 0.53 → 0.60, with the cause documented in the file header. The mean bound (0.35) and the Native bounds are unchanged | the sharp fixture test (`eng10b_sharp_saturated_raw_export_and_print_match_develop`) |
| Nit: per-pixel `put_pixel` | Tiles are copied into the frame row by row | `eng10_level_parity` stays bit-identical |
| Nit: barrier doc said "one per core" | Now "up to `threads` workers" | — |
| Nit: one extra lcms transform per band | Documented: cap the bands if a LUT-based ICC target ever appears. Not changed, since built-in matrix-shaper targets are cheap to resolve | — |
| Nit: ToneExtra copies under the mutex | Left as measured in ENG-10 (in place saves about 190 MiB) | — |

### SF1: what changed on real cameras

`eng10b_real_fixture_parity.rs` tests the Canon CR3 and the Fuji X-Trans
RAF:
- It uses the fixture helper: `SKIPPED` without `fixtures/raw`, and a
  failure when `TESSERA_REQUIRE_RAW_FIXTURES` is set.
- Each camera is printed at full size, Adobe PV6, through
  `render_pixels_with_notes`. The result must be bit-identical to Develop's
  level-0 `SceneLinear` frame put through the same managed sRGB transform.
- It runs with the default lens settings and with lens auto-calibration
  selected.
- Result: 0 differing samples in all four rows. The test takes about 50-65 s,
  mostly the X-Trans auto-calibration.

I compared the old `pipeline_adobe` path against Develop on the rebased
main, with the same recipe, through the same transform, in 8-bit sRGB
levels (a scratch check, not committed):

| Fixture | Default lens | Lens auto-calibration |
| --- | --- | --- |
| Canon CR3 (4000x4000) | identical | max 17.98, mean 0.104, 0.43% of samples > 1 level |
| Fuji X-Trans RAF (4896x3262) | identical | max 95.89, mean 0.099, 1.72% > 1 level |

- The reviewer measured these differences before ENG-7, when `Auto` fell
  back to auto-calibration. So ENG-9's file exports of those cameras did not
  match Develop.
- Since ENG-7 the default no longer auto-calibrates, so with default
  settings ENG-10 changes no full-size pixels on these cameras.
- With auto-calibration selected, ENG-10's output changed toward Develop.
  That is a parity fix, and it is recorded in the release notes.
- The ARW and DNG were already bit-identical (reviewer).

### SF2: what the app gets

- In the app, file exports always render at scale 1 and resize afterwards
  (FFI `Engine::export`, `tessera-ffi/src/export.rs` around line 1311). A
  reduced level is used only with `TESSERA_EXPORT_WEB_LEVEL=1`, because of
  the docs/11 §1.3 exactness gate. This lane did not change that policy;
  any change is a later product decision.
- `export_batch`, `export_pipeline` and the memory admission are reached
  only from `tessera-cli`. The app streams items one at a time.
- In the app, scale > 1 Adobe renders happen only in print (`print_scale`).
  There, print equals Develop's level render, the same contract Native
  print already follows. The reviewer measured somewhat stronger sharpening
  and edge contrast on small prints than ENG-9 produced. This is noted in
  the release notes.

In-app equivalents on the 16 MP ARW, after this change. Load average was
12-23 over two rounds, and the first round ran under rising load:

| Case | After | Before |
| --- | --- | --- |
| `app-2048` (Web export, long edge 2048, scale 1 + resize) | 1.41-2.43 s, peak 1148-1215 MiB | not re-measured. It needs the full-size render, which took 12.3 s before (`export-s1`), so it was at least that |
| `app20-2048` (20 in a row) | 1.38-2.28 s per image, peak 1171-1275 MiB | — |
| `app20-full` (20 full-size in a row) | 1.32-2.08 s per image, peak 1086-1260 MiB | at least 12.3 s per image |
| `export-s1` | 1.30-1.40 s | 12.28-12.34 s (ENG-10 table) |
| `print-4x6` (scale 2, in the app) | 0.36-0.41 s | 6.05-6.14 s |

In ENG-10's tables:
- `export-s4` and `batch20` (Web, scale 2) show library and CLI behaviour.
  They are not what an in-app export costs.
- `print-*` rows are in-app behaviour.
- `batch20-full` and the pairing trade-off are CLI-only.

### Ruling: ENG-9's Adobe bound is 0.60 level

- The coordinator accepted a justified max bound of 0.60 level, keeping the
  0.35 mean bound.
- Cause: the export's ICC transform uses the profile's matrix, stored as
  s15.16 fixed point, while Develop's Output stage uses the exact matrix.
  On a dark channel next to a clipped bright one, the sRGB toe (slope 12.92)
  magnifies the roughly 1e-5 relative difference to 0.05-0.1 level.
- The sharp 128x96 saturated RAW now runs in `eng9_develop_parity` for
  Perceptual and Clip, export and print. It measures 0.533 / 0.534 and
  0.556 / 0.555 level (means 0.17 and 0.10): over the old 0.53, under 0.60.
- Every other row is unchanged.

### ENG-10b gates

- Code tip: `9627e33f`, on `origin/main` `1c344175`.
- Before the run: `cargo clean --release -p export -p image-core
  -p pipeline-cpu -p pipeline-adobe`.
- Environment: `CARGO_BUILD_JOBS=5 RAYON_NUM_THREADS=5`.

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | 3552 passed, 0 failed, 109 ignored (exit 0). Load average 10.2 at the start and 26.7 at the end. No reruns |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `apps/mac/build-ffi.sh` | OK, no bindings drift (worktree clean) |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK (996 XCTest, 3 skipped, 0 failures) |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete |
