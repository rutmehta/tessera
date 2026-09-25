# WP M3-01 — ML runtime scaffold (`ml-runtime`)

Read docs/09 §5 (model ops), docs/04 §5, docs/11 §1.7. Implement `crates/ml-runtime` (workspace member, currently a stub):
- `ModelRegistry`: manifest (`models.toml`) of models with id, version, task, input/output tensor specs, dtype variant (fp32/fp16/int8), sha256, download URL; local cache dir; `resolve(id) -> ModelHandle` that verifies the hash. Recipes will store `ModelRef { id, version }` (already in engine-api::id).
- `Session` over `ort` 2.0 rc (pinned in workspace) with execution-provider selection: CoreML (with the compute-units and MLProgram options) → CPU fallback; `Session::partition_report()` that lists which nodes were assigned to CoreML vs CPU (use ort's session/profiling APIs; if unavailable, run with `ORT` verbose logging captured and parse, and document).
- A `Tensor` helper converting between `engine_api::tile::Tile` planes and NCHW f32/f16 buffers, with tiling+halo for large images.
- Test model: generate a tiny ONNX file in the test (use the `onnx` protobuf via the `prost`-built types in `ort`, or check in a < 50 KB hand-made model created by a Python script in `tools/`): a 3×3 conv + relu on 1×3×64×64. Tests: session runs on CPU; on macOS the CoreML provider loads and the partition report shows ≥ 1 CoreML node; output matches a scalar reference within 1e-4 (fp32) / 1e-2 (fp16); tiled inference over a 300×300 input equals untiled within 1e-5.
- Bench (ignored): time 20 runs of the test model on CoreML vs CPU.
- CI guard: a test that fails if any registered model's partition report has CPU-assigned nodes when `TESSERA_REQUIRE_COREML=1`.
`cargo test -p ml-runtime --release`, clippy -D warnings, fmt. Do not modify engine-api.
