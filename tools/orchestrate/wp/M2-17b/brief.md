# WP M2-17b — Interactive performance redesign for heavy operators (Opus)

Targets (docs/08): every slider < 16 ms at screen resolution; 1:1 region refinement < 100 ms. Current (tools/orchestrate/wp/M2-17/validation.md + presence-benchmark-results.md): Clarity 56 ms, Texture/Dehaze similar, NR and curves also over budget at L2 on the 36 MP NEF; the app hides this by dropping to L4–L6 during drags.
Read crates/pipeline-cpu (Texture/Clarity/Dehaze/NR/curves/grading/vignette implementations), crates/pipeline-gpu (WGSL kernels, batch.rs, resident.rs, the guided-filter passes), crates/image-core (graph, resident model, cache, mask hooks), crates/tessera-ffi/src/develop.rs (adaptive level), spikes/gpu-bench (timing harness).
Design and implement, with the CPU reference as oracle (docs/11 §1.3 tolerance; where an approximation is intentional at preview levels, document it and keep L0 exact):
1. **Multi-scale local contrast**: compute guided-filter guidance at 1/4 of the render level (Lightroom-style), upsample the low-frequency base bilinearly, and apply detail at full level; separable box sums via prefix sums or shared-memory tiles; one fused WGSL pass for Texture+Clarity+Dehaze.
2. **Per-image constant caches**: curve LUTs (1D, 4096 entries), grading/HSL LUTs (3D 33³) and vignette/grain maps rebuilt only when their params hash changes and kept GPU-resident.
3. **Fused output chain**: tone → colour → effects → output as one WGSL pass sampling the resident post-WB buffer; no intermediate readbacks; f16 storage where it does not violate tolerance.
4. **Noise reduction** at preview levels via a downsampled guided/bilateral pass; exact at L0.
5. **Adaptive level policy** in develop.rs: prefer L2 with a 12 ms budget, measure, and only drop levels when a measured frame exceeds 16 ms; expose the level and ms in the existing render readout.
Bench (ignored, prints): per-operator ms at L2 and L0 on all five fixtures before/after; slider p50/p90 in the develop session bench. Tests: tolerance gates per operator at L0; approximation bounds documented and tested at L2 (max abs err vs exact ≤ 4/255 display); goldens unchanged; determinism.
`cargo test --workspace --release`, clippy -D warnings on pipeline-cpu/pipeline-gpu/image-core/tessera-ffi, fmt. engine-api unchanged.
