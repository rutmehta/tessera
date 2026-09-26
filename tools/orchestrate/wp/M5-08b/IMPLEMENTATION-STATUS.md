# M5-08b: L0 target and bit-exact GPU compositing met; 4K viewport target not met

## Root cause of the M5-08 failures

Metal compiles WGSL with fast math (approximate division/reciprocal,
reassociation, `a·b + c` contraction). Over 100 layers those one-ulp drifts
reach the discontinuities of Divide / Hard Mix / Darker-Lighter Colour and flip
pixels: 5.6e-3 with the interpreter, 0.053 with unrestricted specialization.
Specialization changed the compiler's choices, not the formulas.

## Implemented

- `gpu_core::precise_compute_pipeline` (crates/gpu-core/src/precise.rs): naga
  WGSL → MSL with explicit binding map (as wgpu-hal assigns it), no bounds
  checks / loop counters / workgroup zeroing, threadgroup memory moved into
  the kernel, `#pragma METAL fp math_mode(safe)` + `contract(off)`,
  `precise::sqrt`, and every `pdiv`/`pdiv3` compiled to a correctly rounded
  division (fast reciprocal, one Newton step, two exact-FMA corrections).
  Created through wgpu MSL passthrough; `GpuDevice` now requests
  `PASSTHROUGH_SHADERS`. Fallback without it: trusted WGSL (not bit-exact).
- All run-time f32 divisions in blend.wgsl, doc.wgsl and mip.wgsl go through
  `pdiv`/`pdiv3`. The document interpreter, specialized kernels, mip kernel
  and the per-tile port use the precise pipeline. 8-bit LUT staged in
  workgroup memory.
- Result: resident GPU == CPU bit for bit on the full 20 MP / 100-layer
  bench document (L0 and L2, interpreter and specialized) and on every chain
  and per-mode gate. This is the Divide fix: Divide is evaluated in the CPU's
  exact order and rounding; no epsilon was added, blend.rs is unchanged.
- Specialization: discontinuous-mode exclusion removed; every structure up to
  256 steps specializes. Compilation runs on worker threads (≤ 2 in flight,
  8-entry LRU keyed by BLAKE3 + full structure compare); the interpreter
  renders meanwhile, and since both are bit-identical the switch needs no
  re-render. `ResidentRenderer::wait_for_specializations` for tests/benches.
- Page pool: the first slab grows by GPU-copy reallocation (page numbers
  unchanged) up to the binding limit, raised in `gpu_core::limits` from 1 GiB
  to 2 GiB (the default budget). Kernels see one slab (an 8-way slab switch
  cost ~20%), and pool growth no longer changes the kernel key.
- Viewport-limited rendering: page tables resolve per tile on first need;
  a viewport frame interns, uploads and mips only tiles under the viewport
  (cold 3840×2160 L0 viewport: 1390 of 3390 pages, 188 ms vs 421 ms full).
  Unresolved offscreen tiles count as changed, so only the damage log keeps
  their blocks valid; history jumps invalidate them (tested).
- Tests: `gpu-core/tests/precise.rs` (division, reciprocal product,
  uncontracted sum, sqrt equal Rust over all 8-bit pairs + 2M random pairs);
  chain test now asserts exact equality; specialized vs interpreter vs second
  device bit-identical over the full chain scene (all modes);
  `viewport_resolves_only_visible_tiles`; `resident_per_mode` micro-bench.
- COMPOSITOR.md §2.2, §6, §9, §12.1–12.5 updated.

## Numbers (Apple M4, machine shared with other builds, load avg 14–42)

| | Before (M5-08) | After (M5-08b) | Target |
|---|---:|---:|---:|
| Full L0, 20 MP × 100 layers (m5_08 bench, median of 9) | 201.7 ms | **92.1 ms** | < 100 ✔ |
| Full L0 (resident_100 bench, median of 5) | 205.9–210.9 ms | **92.1 ms** (96.9 under load 28–42) | < 100 ✔ |
| Full 3840×2160 L0 viewport recomposite | 84.0 ms | **37.8 ms** | < 8 ✘ |
| Full-L0 max error vs CPU | 5.57e-3 | **0** | ≤ 2e-3 ✔ |
| L2 full recomposite | 13.6 ms | 6.4 ms | — |
| Cold open → first L2 | 537–1044 ms | 407–504 ms | < 1.5 s ✔ |
| 64² dab → L2 | 1.6–7.6 ms | 1.56 ms (3.8 under load) | < 16 ✔ |

Logs: `bench-before.log` (M5-08 state after merging main), `bench-after.log`,
`bench-resident-100.log`, `verification.log`, `pipeline-gpu-tests.log`.

## Verification

- `cargo test -p compositor -p gpu-core --release`: 81 passed, 0 failed,
  7 ignored (benchmarks/diagnostics).
- `cargo clippy -p compositor -p gpu-core --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean.
- `TESSERA_BENCH_ASSERT=1 … resident_100_layers_20mp`: passes (all asserts).
- `TESSERA_BENCH_ASSERT=1 … m5_08_structure_and_viewport`: every correctness
  gate passes at 0 error; fails only `performance viewport=true: 37.8 ms`.
- engine-api and apps/mac untouched.

## Not done

- 4K L0 viewport full recomposite < 8 ms. It is 8.3·10⁸ layer-pixel blends;
  8 ms needs 0.0096 ns each. The kernel runs at 0.045 ns; a minimal
  fast-math Multiply loop without page tables or exact rounding
  (`bench_micro::gpu_calibration`) runs at 0.029–0.033 ns = 25 ms for this
  viewport. Not reachable by full per-pixel recomposition on an M4;
  incremental frames (dabs, pans, idle) are far below 8 ms.
- Level output buffers are still allocated at full level size; smart-object
  children still render whole levels (parent smart pages are per visible
  tile); presentation LUT uploads per call (RGBA16F/LUT presentation and GPU
  smart resampling are as delivered in M5-08).
- Non-Metal fallback is not bit-exact. Tried and rejected: opaque-backdrop
  reciprocal skip (+5%), arithmetic 8-bit decode (+8%), `precise::divide`
  (+60%), `math_mode(relaxed)` (reassociates products; no faster once
  division is exact). Two pixels per thread not tried.

RESULT: PARTIAL: L0 < 100 ms and the 2e-3 gate (now 0) met, determinism
kept; the 4K viewport < 8 ms target is not met.
