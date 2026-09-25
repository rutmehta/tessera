# M3-06 Depth Anything V2 Small

## Provenance / license / export

Only Depth Anything V2 **Small** is registered. Upstream weights:
https://huggingface.co/depth-anything/Depth-Anything-V2-Small (Apache-2.0).
Base/Large are explicitly excluded (CC-BY-NC).

We consume the existing onnx-community export, not an undocumented local conversion:
https://huggingface.co/onnx-community/depth-anything-v2-small/tree/4472b7362082ad9968fee890ca0f1e5aca36b93d
Its card declares Apache-2.0 and identifies the upstream Small model.
Export: `onnx/model.onnx`, FP32, opset 14, 99,060,839 bytes.
SHA-256: `afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c`.
The SHA was checked against both Hugging Face LFS metadata and downloaded bytes.
The ONNX graph was inspected with onnx: input `pixel_values` [batch_size,3,height,width],
output `predicted_depth` [batch,14*floor(height/14),14*floor(width/14)].
No weights are committed. Re-exporting requires a new hash/version and validation.

Run `python3 tools/orchestrate/wp/M3-06/fetch_depth.py` from this worktree to fetch and
verify the registered bytes. This writes only a local ignored `.cache/` directory.
Rust ModelRegistry can also fetch missing weights on explicit DepthEstimator::load.
Inference itself never downloads.

## API and coordinates

`DepthEstimator::load(registry, options, DepthStore)` followed by `estimate(&RgbImage)`
returns a full-size `DepthMap`. Input is oriented sRGB RGB8. Resize is aspect-preserving,
long edge 518, rounded to multiples of 14, bicubic Catmull-Rom; channels use ImageNet
mean/std from the pinned preprocessor configuration. This bounded-long-edge policy
avoids unbounded allocations for panoramas. A very thin side has a 14-pixel minimum.
NCHW FP32 inference is min/max-normalized in f64 to avoid overflow. Constant prediction
maps to zero. The output is relative inverse depth, NOT meters: 1 near, 0 far.

Guided refinement reuses ml-segment's M2-08 guided-filter equations (linearized sRGB
luminance, radius 8, epsilon 1e-4, nearest upsample before refinement).
Cache keys include image pixels and dimensions, pinned model version and algorithm
revision. DepthStore reuses the atomic, checksummed, bounded lossless f32 MaskStore.
Corrupt/missing entries trigger inference. Cached output is checked for image dimensions.

IMPORTANT: call `DepthMap::near_to_far()` before supplying the plane to pipeline-cpu.
The mask/blur contracts are the opposite direction: 0 near, 1 far.
Pass it to `MaskOptions.depth` or `render_linear_scaled_with_depth`. The latter routes
it to both local depth masks and Effects-stage Lens Blur. It must match the RGB image
or RAW active-area crop before recipe Geometry. No engine-api changes.

See crates/pipeline-cpu/LENS_BLUR_M3.md for layered renderer options and approximations.
Cat-eye is an explicit reserved control (nonzero errors). Relight remains a future
consumer of this depth map; no normals or relighting are claimed.
GPU blur is deferred: existing GPU Effects is tile/point-oriented and has no supplied
depth-plane interface. Correct layered full-image coverage/compositing is not a
straightforward separable port; CPU is the reference, not a falsely equivalent GPU path.

## Validation

Required command executed successfully:
`cargo test -p ml-depth -p pipeline-cpu --release && cargo clippy -p ml-depth -p pipeline-cpu --all-targets -- -D warnings && cargo fmt --check`
CARGO_TARGET_DIR remained `/Users/rutmehta/.cache/tessera-target/M3-06`.
Vendored LibRaw emits existing C++ warnings; Rust Clippy passed with warnings denied.

Tests cover normalization/direction/nonfinite rejection, guided edge refinement,
versioned lossless cache round-trip, far-plane mask selection and render routing,
two-plane exact focus plus >50% background contrast reduction, aperture footprints,
normalized boundary colors, far-to-near compositing, boost and disabled identity.
Existing pipeline tests/goldens pass without modification; RAW fixture acceptance
remains ignored by its existing test configuration.

Actual downloaded model inference passed on synthetic RGB and a photograph obtained
from https://huggingface.co/datasets/huggingface/documentation-images/resolve/main/coco_sample.png
(converted to JPEG locally, not committed). Fixture smoke test checks dimensions and
valid outputs; it is not a quantitative depth accuracy benchmark.
Set TESSERA_DEPTH_FIXTURE to a local JPEG to repeat; unset skips only photograph input.
Missing cached model skips inference offline, without initiating a download in tests.
TESSERA_DEPTH_MODELS overrides the default SHA-addressed model cache.

`cargo test -p ml-depth --release --test model -- --nocapture` reports executed nodes.
Observed CPU-only run: 835 CPUExecutionProvider nodes.
`TESSERA_DEPTH_COREML=1 cargo test -p ml-depth --test model -- --nocapture` passed:
79 CPUExecutionProvider nodes and 34 CoreMLExecutionProvider fused subgraphs.
CoreML emitted unbounded-shape/rank diagnostics during partition compilation; this
is mixed execution, NOT a claim that the model is entirely on CoreML. Counts are
optimized execution nodes, not comparable raw graph-node totals across providers.
