# M5-22 implementation

## Placement and integration

Retouching lives in `crates/filters`, reusing Raster, EngineResult, serde and the existing compositor evaluator. A new crate would add another boundary without removing the filters -> compositor dependency. No brush stroke engine changes are needed: Liquify consumes one resampled dab per `apply_brush` call, with brush density/pressure/rate and an inverse displacement mesh.

The compositor currently has a name-plus-JSON `SmartFilter` struct, not a closed effect enum. The existing extensible format is preserved. `CompositorFilters` recognizes:

- `liquify`: `{mesh, interpolation}`; interpolation defaults to `bilinear`.
- `content_aware_fill`: `{mask, fill}`.
- `content_aware_move` and `content_aware_extend`: `{mask, offset, fill, seam}`.
- `remove`: `{mask, remove}`. Replay uses explicit CPU/Auto fallback, never implicit model download. An explicit ONNX replay request without a supplied model fails cleanly.

Mask arrays are row-major canvas-sized finite coverage [0,1], compatible with selection/alpha channel raster conversion. Parameter structures reject unknown fields. Shared smart-filter opacity/blend/masks stay in the compositor. Native documents preserve the complete mesh and all controls without a format-version change. Tests verify native serialization and evaluation.

PSD cannot preserve these editable stages. Call `Document::rasterized_for_export` with a compositor configured with `CompositorFilters`, then export its returned proxy with the existing PSD API. It renders the complete visible document, returns a loss-of-editability note and does not mutate the native document. This explicitly flattens all layers rather than pretending Photoshop can replay Tessera filters. The legacy direct PSD exporter is not replaced; callers must use this explicit proxy path for retouched documents.

## Liquify

Version-1 serde mesh validates dimensions, counts, finite displacement and freeze values. Grid nodes lie at cell-size multiples, including a final partial cell. CPU rendering inverse-maps with bilinear or Catmull-Rom bicubic interpolation, clamped image borders, HDR RGB and original raster depth/channels. Parallel source packing and tile-local output avoid a full-image output clone. All edits preserve immutable input/history.

Brush tools: ForwardWarp, Reconstruct, Smooth, Twirl (clockwise), TwirlCounterClockwise, Pucker, Bloat, PushLeft, Freeze, Thaw. Reconstruct amount is brush pressure × rate × falloff × unfrozen coverage. Full pressure/rate at a hard-core node restores zero displacement. Freeze protects mesh editing, not rendering.

FaceAware uses YuNet's five pixel-space landmarks. Eye, nose, mouth and face-shape controls are face-local and scale/rotation-aware. Jaw/chin/forehead regions are documented heuristics inferred from eye/mouth geometry, not claimed dense landmarks. Eye controls currently affect both eyes. Apply separately for each detected face.

Metal uses an RG32Float displacement texture and explicit matching interpolation. CPU/GPU nonidentity parity is tested at 1e-4, including HDR, alpha, edges, thin rasters and tile boundaries. `GpuLiquify::prepare` returns a resident frame with validated mesh updates, stable output buffer, submit/wait and explicit readback. Downstream GPU consumption is exercised by a real compute stage. No silent CPU fallback.

## CAF and Remove

CAF performs deterministic PatchMatch propagation/random search with whole-footprint source exclusions. Sampling supports Auto, rectangular and custom mask areas. Rotation searches discrete angles, scale includes unit scale and endpoints, and mirror flips the horizontal source axis. None/Default/High/VeryHigh colour adaptation uses screened gradient-domain blending. Output can include a separate straight-RGBA paint layer; callers insert it in the layer tree.

Move fills the source selection, then gradient-blends the translated immutable source. Extend preserves the source. Overlap never feeds pasted pixels back into synthesis. Quality tests cover texture mean/variance, boundary seam error, valid donors, transforms, soft coverage, distant donors and deterministic replay.

`Remove` is an object-safe raster interface implemented by CpuPatchMatch and OnnxInpainter. Auto fallback reports the backend and reason; explicit ONNX errors do not silently fall back. The adapter has named image/mask inputs and output, stride-8 padding/cropping, bounded linear-sRGB conversion, dilation, shape validation, cancellation and alpha preservation. ONNX inference is tested with a real Identity graph, not fake inpainting weights. A local-only `remove/lama` registry slot documents big-lama / Apache-2.0 and an all-zero TODO hash sentinel. No model was downloaded. The loader returns missing weights for this sentinel.

## Verification and performance interpretation

Required gate:

    cargo test -p filters -p brush -p compositor --release && cargo clippy -p filters -p brush -p compositor --all-targets -- -D warnings && cargo fmt --check

All Cargo commands retain CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-22.

Final parent-run required gate exited 0: 236 tests passed, 0 failed, 12 ignored across 41 reported suites (including doc tests); Clippy and workspace formatting passed. `gate.log` is the actual captured output. Existing vendored LibRaw C++ build warnings are present, but Rust Clippy completed with `-D warnings`. `git diff --check` passed and every changed/untracked path is in the supplied allowlist. No commits were made.

Explicit ignored performance tests:

    cargo test -q --release -p filters --test caf_bench -- --ignored --nocapture
    cargo test -q --release -p filters --test liquify_gpu benchmark_24mp -- --ignored --nocapture

The CAF benchmark uses a 6000x4000 synthetic periodic texture with a corrupted 512x512 hole and default controls. It asserts <2 s. Liquify asserts <300 ms CPU (both interpolations), and <25 ms first/max-of-ten changed-mesh resident GPU submit+wait wall time. Tests do not count an identity render or timestamp-only duration as a successful GPU budget.

Worker measurements after optimization: CAF 467–515 ms, CPU Liquify 107–202 ms; GPU resident first dispatch 12.173/23.053 ms (bilinear/bicubic), ten-frame maxima 11.435/18.928 ms. These were real runs but parent verification also observed contention-related failures: GPU bilinear maximum 39.258 ms, and CPU bicubic 319.851 ms in a subsequent run. Parent CAF was 1712.160 ms and passed. System inspection at that point showed multiple concurrent rustc processes and a busy WindowServer. Performance is therefore not yet a reliably verified pass on this shared host.

GPU timing excludes cold upload/preparation and explicit readback. Full Raster->GPU->Raster roundtrip is much slower (hundreds of milliseconds, occasionally >1 s under contention) and is not claimed to meet 25 ms. The resident API is required for interactive use. Benchmark assertions remain strict; timing failures have not been hidden or weakened.

## Retry verification

The retry ran the exact required gate again after the benchmark diagnostic change:
exit 0, 236 passed, 0 failed, 12 ignored. `gate.log` now contains this run.
The explicit CAF benchmark passed at 917.247 ms. The first Liquify benchmark
failed on bicubic CPU at 407.394 ms; bilinear CPU was 220.781 ms and resident
GPU first/max were 14.668/13.037 ms.

The benchmark now prints all measured stages before asserting CPU latency,
without relaxing any threshold. A second run (`performance-retry.log`) measured
bilinear CPU 269.851 ms, GPU first/max 14.518/13.077 ms; bicubic CPU 586.918 ms,
GPU first/max 25.825/21.337 ms. This is a performance FAIL despite the functional
gate passing. Concurrent unrelated rustc/clang processes and WindowServer were
observed consuming CPU. They were not stopped or modified. Contention is a
plausible contributor, not proof that the implementation meets the budget on
an idle host. Re-measure on an isolated host before claiming completion.

The model registry comment now names the intended big-lama / Apache-2.0
upstream explicitly. No weights were downloaded. No production algorithm was
changed in this retry and no commits were made.

## Latest verification

The subsequent verification reran the exact required gate with the external
target directory retained: exit 0, 236 passed, 0 failed, 12 ignored across
41 suites. Clippy with `-D warnings` and workspace formatting passed. The
updated `gate.log` records this run. No production changes were necessary.

Both explicit performance tests passed in this run (`performance-current.log`):

- CAF, 24 MP with a 512x512 hole: 643.913 ms (<2000 ms).
- CPU Liquify bilinear / bicubic: 184.682 / 222.294 ms (<300 ms).
- Resident GPU first dispatch bilinear / bicubic: 12.751 / 22.802 ms.
- Resident GPU maximum across ten changed-mesh frames: 11.320 / 16.248 ms
  (<25 ms for both modes).

These results establish a passing measured run, not immunity to the shared-host
contention documented above. GPU timing still excludes preparation and readback;
full Raster roundtrips measured 362.097 / 391.436 ms and are not a <25 ms path.
The existing implementation and benchmark thresholds were left unchanged.
`git diff --check` passed and an automated changed/untracked-path audit found
no paths outside the supplied allowlist. No model downloads or commits occurred.
