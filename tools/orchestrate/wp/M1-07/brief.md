# WP M1-07 — GPU ports of the M1 operators (`pipeline-gpu`)

Read docs/11 §1.2–1.3, spikes/gpu-bench (REPORT.md + the WGSL/timestamp code you can reuse), crates/pipeline-cpu (OPERATORS.md, every operator), crates/image-core (`StageOp` trait, `CpuStageOp`, tile grids, cache). Implement `crates/pipeline-gpu` (workspace stub):
- `GpuContext` (wgpu 30, Metal backend; device/queue; feature detection for TIMESTAMP_QUERY, f16 storage).
- `GpuStageOp` implementing image-core's `StageOp` for every stage pipeline-cpu implements today: linearize/highlight, demosaic (bilinear + MHC, RGGB/GRBG/GBRG/BGGR; X-Trans falls back to CPU op), camera profile matrix, white balance (CAT16), tone, display transform. Tiles are uploaded once as f32 storage buffers/textures and stages run as compute passes; read back only at the end of a tile's chain (batch several tiles per submission).
- Tolerance gate (docs/11 §1.3): for every operator, GPU vs CPU max abs error ≤ 1e-4 in linear and display output within 1 code value; tests over synthetic images and the Sony ARW fixture at level 3 (all five fixtures behind `PIPELINE_GPU_ALL_FIXTURES=1`).
- Bench (ignored): full level-2 tone-only re-render of each fixture on GPU vs CPU via image-core's Renderer; print ms.
- Deterministic per backend: rendering the same tile twice is bit-identical (test).
`cargo test -p pipeline-gpu --release`, clippy -D warnings, fmt. Do not modify engine-api; additive changes to image-core allowed if the trait needs a batch hook (document).
