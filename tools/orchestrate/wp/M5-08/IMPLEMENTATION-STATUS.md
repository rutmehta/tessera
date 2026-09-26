# M5-08: PARTIAL — full-L0 correctness and performance acceptance failed

## Implemented

- Structural WGSL generation with an eight-entry LRU, BLAKE3 structure key and full-key collision checks. Modes, feature flags, source kinds, group steps, adjustment kinds, depth and slab count participate. Parameters stay in GPU buffers. Oversized, failed or discontinuous structures use the existing interpreter.
- Viewport rendering in requested-level coordinates with a saturating margin and persistent per-block validity. Offscreen damage is retained through pans and full renders. Presentation/readback reject invalid regions. Full-level buffer allocation and source materialization remain.
- Explicit RGBA16F/display-linear and color-mgmt 33³ LUT presentation, including IOSurface entry point, unpremultiplication/repremultiplication, destination-space flattening and EDR headroom. The raw LUT contract matches pipeline-gpu output_lut, not the develop session's entire tone-map/proof pipeline. LUT upload currently occurs per presentation call.
- Shared-device GPU child rendering and smart-object bilinear resampling directly into planar GPU pages. Host f64 footprint generation preserves reference coordinate precision. No production CPU pixel resampling or readback. Child renderers are cached per child snapshot; children currently render whole levels.
- Smart page keys separately identify parent layer revision, child identity/revision and transform, fixing stale pages when the child's revision exceeds subsequent parent transform revisions.
- engine-api and compositor/src/format.rs unchanged. No commits or pushes.

## Verification

Executed with CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-08:

```
cargo test -p compositor -p gpu-core --release && cargo clippy -p compositor -p gpu-core --all-targets -- -D warnings && cargo fmt --check
```

Exit 0 in the current run: 79 passed, 0 failed, 6 ignored benchmarks/diagnostics. Exact output: `verification.log`. `git diff --check` also passed; programmatic allow-list verification found no out-of-scope changes.

Also executed the ignored original resident benchmark (exit 0) and strengthened before/after viewport benchmark with `TESSERA_BENCH_ASSERT=1` (exit 101). Both benchmarks' CPU gates are unconditional. Exact output: `bench-reference.log`, `bench-viewport.log`.

The retry added full-L0 CPU comparisons for both paths and viewport rendering into a fresh output allocation. Completing only invalid offscreen blocks must preserve the rendered viewport, and dispatched block counts must partition the whole level. All four cases now run before the benchmark reports accumulated correctness/timing failures. No tolerances were relaxed.

The gate includes per-mode/adjustment comparisons, float/U16/U8 chains, exact integer mips, dirty-frame equivalence, determinism across renderers/devices, specialization reuse/LRU/oversized fallback, viewport pan/offscreen edit validation, smart resample affine/mip/offset/large-coordinate cases and the high-child-revision transform/undo/redo regression, LUT/EDR tests, and end-to-end resident viewport presentation.

## Measured, not accepted, final timings (Apple M4)

| Case | Before (interpreter) | Enabled specialization, fallback |
|---|---:|---:|
| Full 20 MP L0, 100 mixed-mode layers, 9-run median | 204.862 ms | 203.217 ms |
| Full 3840×2160 L0 viewport, zero margin, 9-run median | 85.750 ms | 83.901 ms |
| Full L0 max absolute CPU error | 0.005570616 | 0.005570616 |

Original benchmark after mip/edit history: L0 median 210.9 ms, cold L2 1569 ms, 64² dab → L2 median 7.62 ms, L2 CPU max error 1.9848347e-5. Machine load was not isolated. Its correctness assertion passed; its optional timing assertions were not enabled.

## Why acceptance is not achieved

The earlier retry reproduced unrestricted specialization's L2 CPU gate failure at 0.08179048 (`specialization-repro.log`). The current run additionally tested unrestricted specialization after the accumulator rounding change: L0 84.302 ms, viewport 38.214 ms, full-L0 CPU error 0.053452015 (`rounding-specialized.log`). These timings are rejected. The temporary fallback bypass was removed; no production diagnostic override remains.

The current run isolated and fixed the prior interpreter maximum (0.017690986): a one-pixel, three-layer fixture (Linear Light → Pin Light → Hard Mix) reproduces it. Pin Light differed by one ulp, then Hard Mix amplified that to 0.29830068. Explicit separately rounded products in resident/doc.wgsl fix the regression without changing the reference, tie rules, or tolerances. `m5_08_hard_mix_boundary` is a new non-ignored regression, and the original pixel's 100-prefix trace now passes (`prefix-trace.log` before, `prefix-trace-fixed.log` after).

The remaining full-L0 error is 0.005570616 at tile (4,8), sample 258. The prefix trace locates the jump at layer 50's Divide: tiny positive versus negative red from earlier nonseparable operations becomes a 0.7764706 mismatch at prefix 90 (`prefix-trace-second.log`). Later layers reduce this to the failing final error. The fallback is not a correctness guarantee. The ignored benchmark still fails unconditionally on correctness.

Reproduce the remaining trace with `cargo test -p compositor --release --test bench m5_08_l0_prefix_trace -- --ignored --nocapture`; use `TESSERA_TRACE_PIXEL=1366,903` for the fixed Hard Mix pixel instead. The default residual pixel is (1026,2049). The new diagnostic preserves original layer IDs and coordinates for Dissolve.

The final implementation conservatively falls back for entire structures containing Dissolve, Darker Color, Lighter Color, Hard Mix, Threshold or Posterize. This includes the required mixed-mode benchmark. The tested continuous-mode specialization remains available, but this WP does not deliver <100 ms for the specified document or <8 ms for full 4K viewport recomposition.

Additional limitations: first structural compilation is synchronous, not background; viewport restriction does not avoid full level allocations/source uploads/mips; smart geometry is host-generated and child pools have separate budgets; renderer memory counters exclude retained child-renderer memory. RGBA16F IOSurface integration is wired through the existing import but no new live IOSurface allocation test was added.

RESULT: FAIL full-L0 CPU error 0.005570616 exceeds 0.002; L0 203.217 ms and 4K viewport 83.901 ms exceed the 100 ms / 8 ms targets.
