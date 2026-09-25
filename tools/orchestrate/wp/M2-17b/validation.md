# M2-17b validation

RESULT: PASS with documented deviations (see "Not done / deviations").
Gate and benchmark evidence are at the end of this file.

## Delivered

1. **Resident Texture/Clarity/Dehaze** (`pipeline-gpu/src/resident_tone.rs`,
   `presence.wgsl`, `image-core/src/resident_render.rs`). Presence no longer
   forces the non-resident whole-image CPU/GPU hybrid (`run_m2`): every level
   whose packed buffers fit 1 GiB renders resident, including the IOSurface
   path. Texture + Clarity run as two fused workgroup-tile kernels (guide →
   self-guided coefficients for all active scales; coefficient means →
   guided outputs → no-new-extrema combination written straight to the
   output). Dehaze runs separable min filters / box means with exact host
   order statistics cached per input identity (Dehaze drags never read back).
   Clipped normalisation and summation order match the CPU oracle.
2. **Whole-level resident rendering.** Whole-level requests (every Develop
   surface frame) run each stage once on a level-sized tile: memoized padded
   WB level and developed (post-Detail) level, one fused tone → curves →
   colour → effects → display dispatch sampling the resident developed
   buffer, one surface + histogram dispatch. No intermediate readbacks.
3. **Per-image constant caches.** GPU-resident vignette-mask / grain-value
   map keyed by the amount-independent parameters (published only after a
   successful transaction); exact Dehaze airlight/confidence cache; host
   curve/grading constants unchanged (exact per-pixel evaluation retained —
   see "not done").
4. **Detail/NR**: halo-free interior written directly, planar Y/Oklab
   decomposition, host-evaluated spatial weight tables (bit-identical to the
   CPU `kernel()` weights), inactive Detail shares its input.
5. **Adaptive level policy** (`tessera-ffi/src/develop.rs`): resident
   recipes drag at the screen level (no static pixel-budget offset), target
   `PREFERRED_BUDGET_MS = 12`, drop one level only after two consecutive
   measured frames exceed 16 ms (the first frame at a level, which refills
   that level's caches, is not counted — the old policy cascaded to L6 on
   such refills), and return finer when the finer level is predicted (×4
   pixels, smoothed) to fit 12 ms. `FrameInfo.level` and `render_ms` remain
   the render readout (no FFI record change).
6. **Infrastructure**: 2D dispatch grids for linear kernels (levels > 4.19
   MP), cross-transaction transient buffer recycling (≤ 1 GiB), device limits
   for 16 storage bindings / 32 KiB workgroup memory / 1 GiB buffers,
   `TESSERA_GPU_PROFILE=1` per-dispatch timestamps, level-0 output-demosaic
   checkpoint no longer duplicated in the cache (it re-decoded large frames
   on every Detail edit).
7. **Baseline bug found and fixed**: full-L0 presence on the 36 MP NEF
   panicked in the baseline (`level pixel readback 435951264 bytes > max
   buffer size 268435456`); it now renders resident.

## Tests added / changed

- `pipeline-gpu/tests/local_tone_resident.rs`: exact per-operator gate vs
  `tone_extra_image` (7 cases incl. ±Texture, ±Clarity, ±Dehaze, all three;
  max 4.8e-6, tolerance 1e-4), determinism, Dehaze statistics reuse (1
  submission on a Dehaze-only edit), L0 renderer parity for Bayer and X-Trans
  (≤ 2e-3 linear, ≤ 1 display code; max 2.9e-4 on X-Trans Dehaze+, the
  documented statistics conditioning), default previews exact, and the
  opt-in preview approximation bound (≤ 4/255 synthetic; ≤ 8/255 pinned on
  real fixtures — Nikon exceeds 4/255, so the approximation is off by
  default).
- `pipeline-gpu/tests/level_mode.rs`: whole-level output bit-identical to the
  per-tile resident path (L0/L1, scene-linear and display, heavy recipe incl.
  presence, sharpening+masking, NR, vignette, grain); Detail + effects-map
  CPU parity (≤ 5.3e-6 linear, 1 display code; effects map 1.2e-6); warm
  point edits = 1 fused dispatch; L0 Detail edits no longer re-decode.
- `pipeline-gpu/tests/resident_fusion.rs`: capability/dispatch expectations
  updated (presence resident; one fused dispatch per whole level; the first
  effects chain also builds the constants map).
- `tessera-ffi` `develop.rs` unit tests for the new policy.
- Benches (ignored): `interactive_performance.rs` (per-operator p50/p90 at L2
  and L0 on all five fixtures), `bench_panel_latency` (+ Texture, Clarity,
  Dehaze, luminance/colour NR, grain), `bench_detail_preview` (1:1 loupe cold
  pans and edits).

## Not done / deviations

- **Downsampled guidance** is implemented but opt-in
  (`RendererConfig::preview_approximations`, default false): the Nikon
  fixture measured 5–8/255 (1/2 grid) and 14/255 (1/4 grid) display error vs
  exact, over the 4/255 bound; Dehaze transmission on a 1/4 grid measured
  8/255 and was removed. Exact is fast enough (see benchmarks).
- **Preview-level downsampled NR** was not added: exact resident NR is
  inside the 12 ms target at L2 on every fixture.
- **Per-pixel curve (1D 4096) and grading/HSL (3D 33³) LUTs** were not
  adopted: exact evaluation costs < 1 ms per L2 frame and LUTs would break
  level-0 exactness.
- **f16 storage** not adopted (M2-17 measured 3.2e-3 linear on f16
  checkpoints, over the 2e-3 bound).
- **One fused Texture+Clarity+Dehaze pass** is not possible exactly: Dehaze's
  dark channel and guide read the presence output's neighbourhoods. Texture
  and Clarity share one fused pair of kernels; Dehaze is separate.
- Full-L0 frames above 16.7 MP (NEF, DNG) still use the per-tile resident
  path; L0 Detail edits on the 36 MP NEF exceed the 512 MiB resident cache
  (WB + Detail = 870 MB) and re-decode per edit. Develop's 1:1 path renders a
  window and is unaffected.

## Headline numbers (Apple M4, concurrent load; full tables in benchmark-results.md)

Resident frame p50 at L2, Nikon NEF (36 MP, 1845×1231 screen level), before →
after: tone 3.6 → 1.6 ms, curves 3.9 → 2.0, point curve 4.3 → 2.1,
Texture 233 → 6.9, Clarity 230 → 9.5, Dehaze 243 → 9.0, sharpening 16.2 →
7.6, luminance NR 17.7 → 8.9, colour NR 15.9 → 7.4, vibrance/HSL/grading
4.6–4.7 → 2.4, vignette 9.8 → 1.7, grain 10.5 → 1.7. Every operator is under
12 ms at L2 on all five fixtures (largest: NEF Clarity 9.5 ms).

Develop session (settings change → frame): every panel except crop/straighten
now settles at the screen level L2 on all five fixtures (before: Texture,
Clarity, Dehaze, sharpening and NR on the NEF/CR3 dragged at L4–L6). NEF L2
p50: Texture 7.0, Clarity 10.5, Dehaze 9.1, sharpening 8.4, luminance NR 11.6
ms; colour NR settled at L3 once (p90 43 ms includes one cold-level refill
under concurrent load).

1:1 loupe (1024² window at L0): cold pans 46–78 ms p50 (p90 ≤ 92 ms) for every
edit on every fixture, Texture/Clarity/Dehaze edits 9–26 ms; before, presence
loupes took 143–347 ms.

Full L0: presence 55–120 ms on CR3/RAF/ARW/DNG and 198–301 ms on the NEF
(before: 3–15 s CPU fallback; the NEF panicked). L0 Detail on the NEF
(tile path, 36 MP) is unchanged at ~1.1 s (resident-cache thrash, see
deviations).

## Gate

- `cargo fmt --check`: pass.
- `cargo clippy -p pipeline-cpu -p pipeline-gpu -p image-core -p tessera-ffi
  --all-targets -- -D warnings`: pass.
- `cargo test --workspace --release` (`workspace-test.log`, final code):
  749 passed, 2 failed, 20 ignored. The two failures are wall-clock
  assertions in crates this package does not touch or depend on through the
  changed paths — `previews` `raw_without_jpeg_is_rendered` (4.44 s vs 3 s)
  and `tessera-ffi/tests/fallback.rs:56` (3 s preview callback) — under load
  average ~17 from concurrent packages; both passed when rerun alone (2.2 s
  each). M2-17's validation records the same two flaky timeouts. The
  preceding full run on the code before the final policy/limit edits passed
  751/0/21.
- Engine-api unchanged; FFI changes limited to `develop.rs` (+ its tests).
