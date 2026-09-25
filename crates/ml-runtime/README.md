# ml-runtime

Local ONNX Runtime scaffold, pinned to workspace `ort = 2.0.0-rc.13`.
The default ort binary distribution supplies ONNX Runtime/CoreML on macOS.
No engine-api changes are required.

## Registry

`ModelRegistry::open(manifest_path, cache_dir)` loads `models.toml`. Entries
contain id/version/task, dtype variant (`fp32`, `fp16`, `int8`), named input and
output tensor specs, SHA-256 and download URL. Tensor shapes are concrete probe
shapes, including for models whose ONNX spatial dimensions are dynamic.

`resolve(id)` only accepts an unambiguous id. `resolve_ref(&ModelRef)` pins an
exact version, and `ModelHandle::model_ref()` produces the engine-api recipe
reference. Give variants distinct IDs (e.g. `segment/subject-fp16`) or versions.
The registry never silently chooses a newer model or a different variant.

Models download only on explicit resolve/cache miss. HTTPS is supported;
`file:relative/path` resolves relative to the manifest and `file:/absolute/path`
is supported for offline deployment. Only verified bytes are atomically
published to the content-addressed cache. Every resolve rechecks SHA-256;
corruption is an error rather than silently redownloading. Manifest files are
trusted configuration, not an untrusted remote model discovery protocol.
No images or telemetry are uploaded.

## Sessions and partitions

`Session::load(path, SessionOptions::default())` tries CoreML on macOS with
`ComputeUnits::All` and `ModelFormat::MLProgram`, then CPU. Both options are
configurable. `SessionOptions::cpu()` explicitly selects CPU. An EP/model
initialization failure rebuilds on CPU and exposes `fallback_reason`.
Unsupported CoreML operators may be partitioned onto CPU without initialization
failing; the partition report, not the requested provider, is the authority.

Run representative inputs before `partition_report()`. It finalizes ORT's JSON
profiling and reads `Node` events' `args.provider`, deduplicating optimized node
names. CoreML fused subgraphs appear as one node (not the original Conv/Relu
names). This is measured execution-provider placement, not an assertion that
CoreML used the ANE rather than its CPU/GPU hardware. No verbose-log scraping is
needed on this ort version. The first report is a cached snapshot; later runs do
not extend it. A fresh session is required for another profiling window.

Profiling is enabled in this scaffold and temporary profiles are removed with
the session. Conditional branches not exercised by the probe are not covered.
Models with control flow need additional representative runs before reporting.
The guard refuses empty reports and any non-CoreML provider, including unknown
providers. Profiling does not expose a complete static assignment of unexecuted
branches.

`run` accepts one NCHW fp32 image buffer, automatically converts to fp16 when
the ONNX input requires it, and returns f32 output. Int8 variants may have float
I/O; quantization scale/zero point preprocessing is model-specific and is not
invented here. `probe` accepts multiple named fp32/fp16/int8 inputs for auditing.

## Tiles

`Tensor::from_tile` includes halo, preserves planar channel order, and accepts
f32/f16 planes without normalization or clamping. `to_tile` validates geometry
and emits either float format. `run_tiled` clips overlapping patches to the
image bounds, runs the model, and copies only the interiors. Use dynamic spatial
ONNX dimensions and a halo at least as large as the receptive radius. This path
requires shape-preserving local models; global attention/pooling, striding and
resizing models need separate stitching logic. It avoids padding the image
externally, preserving the model's own edge padding semantics.

## Verification

Fixtures (all below 1 KB) are generated reproducibly with the standard-library
Python script `python3 tools/make_ml_test_model.py`. Fixed fp32/fp16 fixtures have
1x3x64x64 I/O, a dense 3x3 convolution and ReLU. The dynamic fixture has identical
weights and symbolic height/width for the 300x300 tiling test. The scalar test
reference uses independent loops with fp32 tolerance 1e-4 / fp16 tolerance 1e-2;
tiled vs whole-image tolerance is 1e-5.

Keep CARGO_TARGET_DIR outside the repo on macOS:

    cargo test -p ml-runtime --release
    cargo clippy -p ml-runtime --all-targets -- -D warnings
    cargo fmt --check
    TESSERA_REQUIRE_COREML=1 cargo test -p ml-runtime --release --test guard
    cargo test -p ml-runtime --release --test guard -- --ignored --nocapture

`TESSERA_MODELS_MANIFEST` overrides the guard's default crate models.toml. Every
entry and exact version is resolved, probed and audited. The guard is deliberately
strict when enabled: missing models, no CoreML support, empty registry/report or
CPU nodes fail. By default it is disabled for non-CoreML CI hosts. The separate
macOS fixture test always demands a real CoreML node. The ignored benchmark
warms each provider, times 20 runs, and verifies CoreML placement; these tiny
fixtures measure invocation overhead more than accelerator throughput.
